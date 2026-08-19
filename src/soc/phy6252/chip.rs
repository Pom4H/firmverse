//! One PHY6252 guest: load a HEX, apply host commands, step the Cortex-M0.

use crate::bus::{
    GpioBank, Phy6252Bus, ADC_CH_COUNT, GPIO_PIN_MASK, PWM_CHANNELS, ROM_END, SRAM_BASE, SRAM_SIZE,
    XIP_BASE, XIP_SIZE,
};
use crate::cmd::{scan_packet, ChipCmd};
use crate::discovery::DiscoveryBus;
use crate::hex::HexImage;
use crate::mailbox;
use crate::osal::HostOsal;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::fault::FaultTrapMode;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::core::reset::Reset;
use zmu_cortex_m::executor::Executor;
use zmu_cortex_m::Processor;

const VECTOR_MIRROR_BYTES: usize = 0xC0;
const CPU_THUNK_DISABLE_IRQ: u32 = 0x0000_00C0;
const CPU_THUNK_ENABLE_IRQ: u32 = 0x0000_00C4;
const BOOT_FLASH_BYTES: usize = 0xC8;
const ROM_DRV_DISABLE_IRQ: u32 = 0x0000_A974;
const ROM_DRV_ENABLE_IRQ: u32 = 0x0000_A99C;
const ROM_SPIF_READ_ID: u32 = 0x0001_7208;
const ROM_CLK_GET_PCLK: u32 = 0x0000_A5D0;
const PHY6252_G_HCLK: u32 = 0x1FFF_0874;

pub struct Chip {
    pub id: String,
    pub mac: [u8; 6],
    pub x: f64,
    pub y: f64,
    processor: Processor,
    gpio: Rc<RefCell<GpioBank>>,
    gpio_changed: Rc<RefCell<bool>>,
    uart_rx: Rc<RefCell<Vec<u8>>>,
    pwm: Rc<RefCell<[u32; PWM_CHANNELS]>>,
    pwm_changed: Rc<RefCell<bool>>,
    adc_mv: Rc<RefCell<[u16; ADC_CH_COUNT]>>,
    ext_in: Arc<AtomicU32>,
    pending_rx: VecDeque<Vec<u8>>,
    last_tx_seq: u32,
    uart_line: String,
    clock_ms: u32,
    cpu_rom_seen: u8,
    host_osal: HostOsal,
    pub insn: u64,
    pub hex_label: String,
    stop: Option<String>,
}

pub struct ChipDelta {
    pub uart_lines: Vec<String>,
    pub gpio: Option<(u32, u32)>,
    pub pwm: Option<[u32; PWM_CHANNELS]>,
    pub frames: Vec<Vec<u8>>,
}

pub enum Apply {
    Continue,
    Quit,
    Help,
}

impl Chip {
    pub fn load(
        id: String,
        hex: &Path,
        strict: bool,
        mac: [u8; 6],
        x: f64,
        y: f64,
    ) -> Result<Self, String> {
        let image = HexImage::load(hex).map_err(|e| format!("{}: {e}", hex.display()))?;
        let label = hex
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| hex.display().to_string());
        Self::from_image(id, label, image, strict, mac, x, y)
    }

    /// Build the exact same PHY6252 runtime from Intel HEX text already in
    /// memory. Browser/WASM callers use this instead of inventing a virtual
    /// filesystem around `load`.
    pub fn load_text(
        id: String,
        label: String,
        text: &str,
        strict: bool,
        mac: [u8; 6],
        x: f64,
        y: f64,
    ) -> Result<Self, String> {
        let image = HexImage::parse(text).map_err(|e| format!("{label}: {e}"))?;
        Self::from_image(id, label, image, strict, mac, x, y)
    }

    fn from_image(
        id: String,
        hex_label: String,
        image: HexImage,
        strict: bool,
        mac: [u8; 6],
        x: f64,
        y: f64,
    ) -> Result<Self, String> {
        let mut sram = vec![0u8; SRAM_SIZE];
        let mut xip = vec![0u8; XIP_SIZE];
        image.fill(SRAM_BASE, &mut sram);
        image.fill(XIP_BASE, &mut xip);

        let (vector_base, vectors) = locate_vector_table(&sram)?;
        let boot_flash = build_boot_flash(&vectors);
        let device = Phy6252Bus::new(sram, xip);
        let gpio = Rc::clone(&device.gpio);
        let gpio_changed = Rc::clone(&device.gpio_changed);
        let uart_rx = Rc::clone(&device.uart_rx);
        let pwm = Rc::clone(&device.pwm);
        let pwm_changed = Rc::clone(&device.pwm_changed);
        let adc_mv = Rc::clone(&device.adc_mv);
        let device = DiscoveryBus::new(device, strict);
        let sp = u32::from_le_bytes(vectors[0..4].try_into().unwrap());
        let reset = u32::from_le_bytes(vectors[4..8].try_into().unwrap());
        eprintln!("hex {hex_label}");
        eprintln!(
            "node {id} mac={mac} Vectors={vector_base:#010x} bytes={:#x} SP={sp:#010x} Reset={reset:#010x}",
            vectors.len(),
            mac = format_mac(&mac),
        );
        if strict {
            eprintln!("MMIO discovery: strict");
        }

        let mut processor = Processor::new();
        processor.fault_trap_mode(FaultTrapMode::hardfault());
        processor.device(Some(Box::new(device)));
        processor.flash_memory(boot_flash.len(), &boot_flash);
        processor
            .reset()
            .map_err(|fault| format!("reset failed: {fault}"))?;
        mailbox::plant_magic(&mut processor).map_err(|fault| format!("mailbox plant {fault}"))?;

        Ok(Self {
            hex_label,
            id,
            mac,
            x,
            y,
            processor,
            gpio,
            gpio_changed,
            uart_rx,
            pwm,
            pwm_changed,
            adc_mv,
            ext_in: Arc::new(AtomicU32::new(0)),
            pending_rx: VecDeque::new(),
            last_tx_seq: 0,
            uart_line: String::new(),
            clock_ms: 0,
            cpu_rom_seen: 0,
            host_osal: HostOsal::new(),
            insn: 0,
            stop: None,
        })
    }

    pub fn stopped(&self) -> Option<&str> {
        self.stop.as_deref()
    }

    pub fn gpio_bank(&self) -> Rc<RefCell<GpioBank>> {
        Rc::clone(&self.gpio)
    }

    pub fn apply(&mut self, cmd: ChipCmd) -> Result<Apply, String> {
        match cmd {
            ChipCmd::Quit => Ok(Apply::Quit),
            ChipCmd::Help => Ok(Apply::Help),
            ChipCmd::In(value) => {
                self.ext_in.store(value & GPIO_PIN_MASK, Ordering::Relaxed);
                Ok(Apply::Continue)
            }
            ChipCmd::Pin { bit, high } => {
                let mask = 1u32 << bit;
                let cur = self.ext_in.load(Ordering::Relaxed);
                let next = if high { cur | mask } else { cur & !mask };
                self.ext_in.store(next & GPIO_PIN_MASK, Ordering::Relaxed);
                Ok(Apply::Continue)
            }
            ChipCmd::Write(bytes) => {
                self.pending_rx.push_back(bytes);
                Ok(Apply::Continue)
            }
            ChipCmd::Scan { addr, rssi } => {
                self.pending_rx.push_back(scan_packet(&addr, rssi, false));
                Ok(Apply::Continue)
            }
            ChipCmd::Gone { addr } => {
                self.pending_rx.push_back(scan_packet(&addr, 0, true));
                Ok(Apply::Continue)
            }
            ChipCmd::Connect => mailbox::connect(&mut self.processor, true)
                .map_err(|f| format!("{f}"))
                .map(|()| Apply::Continue),
            ChipCmd::Disconnect => mailbox::connect(&mut self.processor, false)
                .map_err(|f| format!("{f}"))
                .map(|()| Apply::Continue),
            ChipCmd::Cccd(on) => mailbox::cccd(&mut self.processor, on)
                .map_err(|f| format!("{f}"))
                .map(|()| Apply::Continue),
            ChipCmd::Tick(ms) => {
                self.clock_ms = self.clock_ms.wrapping_add(ms);
                mailbox::set_tick(&mut self.processor, self.clock_ms)
                    .map_err(|f| format!("{f}"))
                    .map(|()| Apply::Continue)
            }
            ChipCmd::Adc(pads) => {
                let mut adc = self.adc_mv.borrow_mut();
                adc[7] = pads[0];
                adc[6] = pads[1];
                adc[4] = pads[2];
                adc[3] = pads[3];
                Ok(Apply::Continue)
            }
        }
    }

    pub fn tick(&mut self, burst: u32, max_insns: u64, live_clock: bool) -> ChipDelta {
        let mut delta = ChipDelta {
            uart_lines: Vec::new(),
            gpio: None,
            pwm: None,
            frames: Vec::new(),
        };
        if self.stop.is_some() {
            return delta;
        }
        apply_ext(&self.gpio, &self.ext_in);
        self.redirect();
        if let Some(trap) = self.processor.take_pending_fault_trap() {
            self.stop = Some(format!("fault {trap:?}"));
            delta.gpio = Some(gpio_pair(&self.gpio));
            return delta;
        }
        if !self.processor.running {
            self.stop = Some("halt".into());
            delta.gpio = Some(gpio_pair(&self.gpio));
            return delta;
        }
        if self.processor.sleeping {
            self.processor.sleeping = false;
        }
        if let Some(pkt) = self.pending_rx.pop_front() {
            if let Err(fault) = mailbox::write_rx(&mut self.processor, &pkt) {
                eprintln!("err {fault}");
            }
        }
        for _ in 0..burst {
            self.redirect();
            let pc = self.processor.get_pc();
            let lr = self.processor.get_r(Reg::LR);
            let r0 = self.processor.get_r(Reg::R0);
            let r1 = self.processor.get_r(Reg::R1);
            let r2 = self.processor.get_r(Reg::R2);
            let r3 = self.processor.get_r(Reg::R3);
            self.processor.step();
            self.insn += 1;
            if let Some(trap) = self.processor.take_pending_fault_trap() {
                eprintln!(
                    "CPU fault pc={pc:#010x} lr={lr:#010x} r0={r0:#010x} r1={r1:#010x} r2={r2:#010x} r3={r3:#010x} trap={trap:?}"
                );
                self.stop = Some(format!("fault {trap:?} at pc={pc:#010x} lr={lr:#010x}"));
                delta.gpio = Some(gpio_pair(&self.gpio));
                break;
            }
            if self.insn >= max_insns {
                self.stop = Some("max instructions".into());
                break;
            }
        }
        self.collect(&mut delta);
        if live_clock {
            self.clock_ms = self.clock_ms.wrapping_add(1);
            let _ = mailbox::set_tick(&mut self.processor, self.clock_ms);
        }
        delta
    }

    pub fn pc_lr_msp(&mut self) -> (u32, u32, u32) {
        (
            self.processor.get_pc(),
            self.processor.lr,
            self.processor.msp,
        )
    }

    fn collect(&mut self, delta: &mut ChipDelta) {
        let mut buf = self.uart_rx.borrow_mut();
        if !buf.is_empty() {
            let bytes = buf.split_off(0);
            drop(buf);
            for byte in bytes {
                if byte == b'\n' {
                    delta.uart_lines.push(std::mem::take(&mut self.uart_line));
                } else if (32..127).contains(&byte) {
                    self.uart_line.push(char::from(byte));
                }
            }
        }
        if take_flag(&self.gpio_changed) {
            delta.gpio = Some(gpio_pair(&self.gpio));
        }
        if take_flag(&self.pwm_changed) {
            delta.pwm = Some(*self.pwm.borrow());
        }
        match mailbox::take_tx(&mut self.processor, &mut self.last_tx_seq) {
            Ok(Some(frame)) => delta.frames.push(frame),
            Ok(None) => {}
            Err(fault) => eprintln!("err mailbox {fault}"),
        }
    }

    fn redirect(&mut self) {
        redirect_cpu_rom_abi(
            &mut self.processor,
            &mut self.cpu_rom_seen,
            &mut self.host_osal,
        );
    }
}

pub fn format_mac(mac: &[u8; 6]) -> String {
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

pub fn mac_from_id(id: &str) -> [u8; 6] {
    let mut mac = [0x02, 0x62, 0x52, 0, 0, 0];
    let bytes = id.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        mac[3 + i % 3] ^= b.wrapping_mul(17u8.wrapping_add(i as u8));
    }
    mac[5] ^= bytes.len() as u8;
    mac
}

fn gpio_pair(gpio: &Rc<RefCell<GpioBank>>) -> (u32, u32) {
    let bank = gpio.borrow();
    (bank.dr, bank.ddr)
}

fn apply_ext(gpio: &Rc<RefCell<GpioBank>>, ext_in: &Arc<AtomicU32>) {
    let value = ext_in.load(Ordering::Relaxed) & GPIO_PIN_MASK;
    let mut bank = gpio.borrow_mut();
    if bank.ext != value {
        bank.ext = value;
    }
}

fn take_flag(flag: &Rc<RefCell<bool>>) -> bool {
    let mut changed = flag.borrow_mut();
    let value = *changed;
    *changed = false;
    value
}

fn redirect_cpu_rom_abi(processor: &mut Processor, seen: &mut u8, host_osal: &mut HostOsal) {
    if host_osal.handle(processor) {
        return;
    }
    let pc = processor.get_pc();
    if pc == ROM_CLK_GET_PCLK {
        let pclk = match processor.read32(PHY6252_G_HCLK) {
            Ok(value) if value != 0 => value,
            Ok(_) => {
                if *seen & 16 == 0 {
                    eprintln!(
                        "ROM CPU strict clk_get_pclk entry={pc:#010x} -- g_hclk is zero before clock init"
                    );
                    *seen |= 16;
                }
                return;
            }
            Err(fault) => {
                eprintln!(
                    "ROM CPU strict clk_get_pclk entry={pc:#010x} -- cannot read g_hclk: {fault}"
                );
                return;
            }
        };
        if *seen & 32 == 0 {
            eprintln!(
                "ROM CPU shim clk_get_pclk entry={pc:#010x} behavior=g_hclk/default-divider pclk={pclk}Hz"
            );
            *seen |= 32;
        }
        processor.set_r(Reg::R0, pclk);
        let lr = processor.get_r(Reg::LR);
        processor.set_pc(lr & !1);
        return;
    }
    if pc == ROM_SPIF_READ_ID {
        let pid_ptr = processor.get_r(Reg::R0);
        if pid_ptr != 0 {
            if *seen & 4 == 0 {
                eprintln!(
                    "ROM CPU strict spif_read_id entry={pc:#010x} pid={pid_ptr:#010x} -- flash identity profile not configured"
                );
                *seen |= 4;
            }
            return;
        }
        if *seen & 8 == 0 {
            eprintln!(
                "ROM CPU shim spif_read_id entry={pc:#010x} behavior=NULL-probe-success (no JEDEC ID invented)"
            );
            *seen |= 8;
        }
        processor.set_r(Reg::R0, 0);
        let lr = processor.get_r(Reg::LR);
        processor.set_pc(lr & !1);
        return;
    }

    let (thunk, bit, name, behavior) = match pc {
        ROM_DRV_DISABLE_IRQ => (
            CPU_THUNK_DISABLE_IRQ,
            1u8,
            "drv_disable_irq",
            "CPSID i / PRIMASK=1",
        ),
        ROM_DRV_ENABLE_IRQ => (
            CPU_THUNK_ENABLE_IRQ,
            2u8,
            "drv_enable_irq",
            "CPSIE i / PRIMASK=0",
        ),
        _ => return,
    };
    if *seen & bit == 0 {
        eprintln!("ROM CPU shim {name} entry={pc:#010x} behavior={behavior}");
        *seen |= bit;
    }
    processor.set_pc(thunk);
}

fn build_boot_flash(vectors: &[u8]) -> Vec<u8> {
    let mut flash = vec![0u8; BOOT_FLASH_BYTES];
    let vector_len = vectors.len().min(VECTOR_MIRROR_BYTES);
    flash[..vector_len].copy_from_slice(&vectors[..vector_len]);
    flash[CPU_THUNK_DISABLE_IRQ as usize..CPU_THUNK_DISABLE_IRQ as usize + 4]
        .copy_from_slice(&[0x72, 0xB6, 0x70, 0x47]);
    flash[CPU_THUNK_ENABLE_IRQ as usize..CPU_THUNK_ENABLE_IRQ as usize + 4]
        .copy_from_slice(&[0x62, 0xB6, 0x70, 0x47]);
    flash
}

pub(crate) fn locate_vector_table(sram: &[u8]) -> Result<(u32, Vec<u8>), String> {
    if vector_pair_plausible(sram, 0) {
        return Ok((SRAM_BASE, vector_table(sram, 0)));
    }

    let mut best: Option<(u32, usize, u32)> = None;
    for offset in (0..sram.len().saturating_sub(8)).step_by(4) {
        if !vector_pair_plausible(sram, offset) {
            continue;
        }
        let score = vector_score(sram, offset);
        if best.is_none_or(|(_, _, best_score)| score > best_score) {
            best = Some((SRAM_BASE + offset as u32, offset, score));
        }
    }

    let Some((base, offset, _)) = best else {
        let first = vector_pair(sram, 0);
        let sp = u32::from_le_bytes(first[0..4].try_into().unwrap());
        let reset = u32::from_le_bytes(first[4..8].try_into().unwrap());
        return Err(format!(
            "no plausible Cortex-M vector table in PHY6252 SRAM (first SP={sp:#010x} Reset={reset:#010x})"
        ));
    };
    Ok((base, vector_table(sram, offset)))
}

fn vector_table(sram: &[u8], offset: usize) -> Vec<u8> {
    let available = sram.len().saturating_sub(offset);
    let len = VECTOR_MIRROR_BYTES.min(available);
    sram[offset..offset + len].to_vec()
}

fn vector_pair(sram: &[u8], offset: usize) -> [u8; 8] {
    let mut out = [0u8; 8];
    if let Some(bytes) = sram.get(offset..offset + 8) {
        out.copy_from_slice(bytes);
    }
    out
}

fn vector_pair_plausible(sram: &[u8], offset: usize) -> bool {
    let pair = vector_pair(sram, offset);
    let sp = u32::from_le_bytes(pair[0..4].try_into().unwrap());
    let reset = u32::from_le_bytes(pair[4..8].try_into().unwrap());
    let sram_end = SRAM_BASE + SRAM_SIZE as u32;
    let sp_ok = sp >= SRAM_BASE && sp <= sram_end && sp & 3 == 0;
    sp_ok && reset & 1 == 1 && executable_address(reset & !1)
}

fn vector_score(sram: &[u8], offset: usize) -> u32 {
    let mut score = 16;
    for index in 2..16 {
        let at = offset + index * 4;
        let Some(bytes) = sram.get(at..at + 4) else {
            break;
        };
        let value = u32::from_le_bytes(bytes.try_into().unwrap());
        if value == 0 {
            score += 1;
        } else if value & 1 == 1 && executable_address(value & !1) {
            score += 2;
        }
    }
    score
}

fn executable_address(addr: u32) -> bool {
    addr < ROM_END
        || (SRAM_BASE..SRAM_BASE + SRAM_SIZE as u32).contains(&addr)
        || (XIP_BASE..XIP_BASE + XIP_SIZE as u32).contains(&addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_demo_vectors_at_sram_base() {
        let mut sram = vec![0u8; SRAM_SIZE];
        sram[0..4].copy_from_slice(&(SRAM_BASE + 0x8000).to_le_bytes());
        sram[4..8].copy_from_slice(&(SRAM_BASE + 0x101).to_le_bytes());
        sram[12..16].copy_from_slice(&(SRAM_BASE + 0x121).to_le_bytes());
        let (base, vectors) = locate_vector_table(&sram).unwrap();
        assert_eq!(base, SRAM_BASE);
        assert_eq!(vectors.len(), VECTOR_MIRROR_BYTES);
        assert_eq!(
            u32::from_le_bytes(vectors[12..16].try_into().unwrap()),
            SRAM_BASE + 0x121
        );
    }

    #[test]
    fn finds_sdk_vectors_after_jump_table_and_mirrors_exceptions() {
        let mut sram = vec![0u8; SRAM_SIZE];
        let offset = 0x1838usize;
        sram[offset..offset + 4].copy_from_slice(&0x1FFF_9000u32.to_le_bytes());
        sram[offset + 4..offset + 8].copy_from_slice(&0x1FFF_19E1u32.to_le_bytes());
        sram[offset + 8..offset + 12].copy_from_slice(&0x0000_8481u32.to_le_bytes());
        sram[offset + 12..offset + 16].copy_from_slice(&0x0000_28F1u32.to_le_bytes());
        sram[offset + 0xBC..offset + 0xC0].copy_from_slice(&0x1FFF_2223u32.to_le_bytes());
        let (base, vectors) = locate_vector_table(&sram).unwrap();
        assert_eq!(base, SRAM_BASE + offset as u32);
        assert_eq!(vectors.len(), VECTOR_MIRROR_BYTES);
        assert_eq!(
            u32::from_le_bytes(vectors[4..8].try_into().unwrap()),
            0x1FFF_19E1
        );
        assert_eq!(
            u32::from_le_bytes(vectors[12..16].try_into().unwrap()),
            0x0000_28F1
        );
        assert_eq!(
            u32::from_le_bytes(vectors[0xBC..0xC0].try_into().unwrap()),
            0x1FFF_2223
        );
    }

    #[test]
    fn boot_flash_contains_real_cortex_m0_irq_mask_thunks() {
        let vectors = vec![0u8; VECTOR_MIRROR_BYTES];
        let flash = build_boot_flash(&vectors);
        assert_eq!(&flash[0xC0..0xC4], &[0x72, 0xB6, 0x70, 0x47]);
        assert_eq!(&flash[0xC4..0xC8], &[0x62, 0xB6, 0x70, 0x47]);
    }

    #[test]
    fn node_ids_get_distinct_local_macs() {
        assert_ne!(mac_from_id("a"), mac_from_id("b"));
        assert_eq!(mac_from_id("lamp")[0] & 0x02, 0x02);
    }
}