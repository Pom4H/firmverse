use crate::bus::{
    GpioBank, Phy6252Bus, ADC_CH_COUNT, GPIO_PIN_MASK, PWM_CHANNELS, ROM_END, SRAM_BASE, SRAM_SIZE,
    XIP_BASE, XIP_SIZE,
};
use crate::cmd::{gpio_silk, ChipCmd, HELP};
use crate::discovery::DiscoveryBus;
use crate::hex::HexImage;
use crate::mailbox;
use std::cell::RefCell;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use zmu_cortex_m::core::fault::FaultTrapMode;
use zmu_cortex_m::core::register::BaseReg;
use zmu_cortex_m::core::reset::Reset;
use zmu_cortex_m::executor::Executor;
use zmu_cortex_m::Processor;

const VECTOR_MIRROR_BYTES: usize = 0xC0;
const CPU_THUNK_DISABLE_IRQ: u32 = 0x0000_00C0;
const CPU_THUNK_ENABLE_IRQ: u32 = 0x0000_00C4;
const BOOT_FLASH_BYTES: usize = 0xC8;
const ROM_DRV_DISABLE_IRQ: u32 = 0x0000_A974;
const ROM_DRV_ENABLE_IRQ: u32 = 0x0000_A99C;

pub struct RunOpts {
    pub hex: PathBuf,
    pub live: bool,
    pub raw: bool,
    pub strict_mmio: bool,
    pub max_insns: u64,
}

pub fn default_hex() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("PHY6252_HEX") {
        return Ok(PathBuf::from(path));
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("firmware/kit-demo.hex");
    if dir.is_file() {
        return Ok(dir);
    }
    let mut built = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    built.push("firmware/build/kit-demo.hex");
    if built.is_file() {
        return Ok(built);
    }
    Err("pass a .hex or set PHY6252_HEX".into())
}

pub fn run(opts: RunOpts) -> Result<ExitCode, String> {
    let hex_path = opts.hex;
    let live = opts.live;
    let raw = opts.raw;
    let strict_mmio = opts.strict_mmio;
    let max_insns = opts.max_insns;

    let image = HexImage::load(&hex_path).map_err(|e| format!("{}: {e}", hex_path.display()))?;
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
    let device = DiscoveryBus::new(device, strict_mmio);
    let sp = u32::from_le_bytes(vectors[0..4].try_into().unwrap());
    let reset = u32::from_le_bytes(vectors[4..8].try_into().unwrap());
    eprintln!("hex {}", hex_path.display());
    eprintln!(
        "Vectors={vector_base:#010x} bytes={:#x} SP={sp:#010x} Reset={reset:#010x}",
        vectors.len()
    );
    if strict_mmio {
        eprintln!("MMIO discovery: strict");
    }

    let ext_in = Arc::new(AtomicU32::new(0));
    let cmd_rx = if live {
        Some(spawn_cmd_reader())
    } else {
        None
    };

    let mut processor = Processor::new();
    processor.fault_trap_mode(FaultTrapMode::hardfault());
    processor.device(Some(Box::new(device)));
    // zmu owns Cortex-M exception-vector lookup at address zero. Mirror the complete
    // SDK vector block and append two tiny CPU-local ROM ABI thunks. The thunks execute
    // real CPSID/CPSIE instructions so PRIMASK semantics remain zmu/Cortex-M0 semantics.
    processor.flash_memory(boot_flash.len(), &boot_flash);
    processor
        .reset()
        .map_err(|fault| format!("reset failed: {fault}"))?;
    mailbox::plant_magic(&mut processor).map_err(|fault| format!("mailbox plant {fault}"))?;

    let mut last_tx_seq = 0u32;
    let mut uart_line = String::new();
    let mut clock_ms = 0u32;
    let mut cpu_rom_seen = 0u8;

    if live {
        if raw {
            println!("READY");
            println!("ADV name=PB03FKIT service=6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001");
            emit_gpio(&gpio, true);
            emit_pwm(&pwm);
        } else {
            eprintln!("{}", HELP.trim_end());
            eprintln!("ready  {}", hex_label(&hex_path));
        }
    }

    let burst: u32 = if live { 8_000 } else { 1 };
    let mut insn: u64 = 0;
    while insn < max_insns {
        apply_ext(&gpio, &ext_in);
        redirect_cpu_rom_abi(&mut processor, &mut cpu_rom_seen);
        if let Some(trap) = processor.take_pending_fault_trap() {
            return report_stop(&mut processor, insn, live, raw, &gpio, &format!("fault {trap:?}"));
        }
        if !processor.running {
            return report_stop(&mut processor, insn, live, raw, &gpio, "halt");
        }
        if processor.sleeping {
            processor.sleeping = false;
        }

        if live {
            if let Some(rx) = cmd_rx.as_ref() {
                if drain_cmds(rx, &ext_in, &mut processor, &adc_mv, &mut clock_ms, raw) {
                    return report_stop(&mut processor, insn, live, raw, &gpio, "quit");
                }
            }
        }

        for _ in 0..burst {
            redirect_cpu_rom_abi(&mut processor, &mut cpu_rom_seen);
            processor.step();
            insn += 1;
            if insn >= max_insns {
                break;
            }
        }

        flush_uart(&uart_rx, &mut uart_line, raw);
        if take_flag(&gpio_changed) {
            emit_gpio(&gpio, raw);
        }
        if take_flag(&pwm_changed) && (!live || clock_ms % 50 == 0) {
            if raw {
                emit_pwm(&pwm);
            }
        }
        emit_frames(&mut processor, &mut last_tx_seq, raw);

        if live {
            clock_ms = clock_ms.wrapping_add(1);
            let _ = mailbox::set_tick(&mut processor, clock_ms);
            let _ = io::stdout().flush();
            thread::sleep(Duration::from_millis(1));
        }
    }

    report_stop(&mut processor, insn, live, raw, &gpio, "max instructions")
}

fn redirect_cpu_rom_abi(processor: &mut Processor, seen: &mut u8) {
    let pc = processor.get_pc();
    let (thunk, bit, name, behavior) = match pc {
        ROM_DRV_DISABLE_IRQ => (CPU_THUNK_DISABLE_IRQ, 1u8, "drv_disable_irq", "CPSID i / PRIMASK=1"),
        ROM_DRV_ENABLE_IRQ => (CPU_THUNK_ENABLE_IRQ, 2u8, "drv_enable_irq", "CPSIE i / PRIMASK=0"),
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
    // Thumb: CPSID i = 0xB672, BX LR = 0x4770.
    flash[CPU_THUNK_DISABLE_IRQ as usize..CPU_THUNK_DISABLE_IRQ as usize + 4]
        .copy_from_slice(&[0x72, 0xB6, 0x70, 0x47]);
    // Thumb: CPSIE i = 0xB662, BX LR = 0x4770.
    flash[CPU_THUNK_ENABLE_IRQ as usize..CPU_THUNK_ENABLE_IRQ as usize + 4]
        .copy_from_slice(&[0x62, 0xB6, 0x70, 0x47]);
    flash
}

fn locate_vector_table(sram: &[u8]) -> Result<(u32, Vec<u8>), String> {
    if vector_pair_plausible(sram, 0) {
        return Ok((SRAM_BASE, vector_table(sram, 0)));
    }

    let mut best: Option<(u32, usize, u32)> = None;
    for offset in (0..sram.len().saturating_sub(8)).step_by(4) {
        if !vector_pair_plausible(sram, offset) {
            continue;
        }
        let score = vector_score(sram, offset);
        if best.map_or(true, |(_, _, best_score)| score > best_score) {
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

fn hex_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn drain_cmds(
    rx: &Receiver<ChipCmd>,
    ext_in: &Arc<AtomicU32>,
    processor: &mut Processor,
    adc_mv: &Rc<RefCell<[u16; ADC_CH_COUNT]>>,
    clock_ms: &mut u32,
    raw: bool,
) -> bool {
    while let Ok(cmd) = rx.try_recv() {
        if matches!(cmd, ChipCmd::Quit) {
            return true;
        }
        if matches!(cmd, ChipCmd::Help) {
            if !raw {
                eprintln!("{}", HELP.trim_end());
            }
            continue;
        }
        let result = match cmd {
            ChipCmd::In(value) => {
                ext_in.store(value & GPIO_PIN_MASK, Ordering::Relaxed);
                Ok(())
            }
            ChipCmd::Pin { bit, high } => {
                let mask = 1u32 << bit;
                let cur = ext_in.load(Ordering::Relaxed);
                let next = if high { cur | mask } else { cur & !mask };
                ext_in.store(next & GPIO_PIN_MASK, Ordering::Relaxed);
                Ok(())
            }
            ChipCmd::Write(bytes) => mailbox::write_rx(processor, &bytes),
            ChipCmd::Connect => mailbox::connect(processor, true),
            ChipCmd::Disconnect => mailbox::connect(processor, false),
            ChipCmd::Cccd(on) => mailbox::cccd(processor, on),
            ChipCmd::Tick(ms) => {
                *clock_ms = clock_ms.wrapping_add(ms);
                mailbox::set_tick(processor, *clock_ms)
            }
            ChipCmd::Adc(pads) => {
                let mut adc = adc_mv.borrow_mut();
                adc[7] = pads[0];
                adc[6] = pads[1];
                adc[4] = pads[2];
                adc[3] = pads[3];
                Ok(())
            }
            ChipCmd::Help | ChipCmd::Quit => Ok(()),
        };
        if let Err(fault) = result {
            eprintln!("err {fault}");
        }
    }
    false
}

fn spawn_cmd_reader() -> Receiver<ChipCmd> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            match crate::cmd::parse_line(line.trim()) {
                Ok(Some(cmd)) => {
                    if tx.send(cmd).is_err() {
                        break;
                    }
                }
                Ok(None) => {}
                Err(err) => eprintln!("{err}"),
            }
        }
    });
    rx
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

fn emit_frames(processor: &mut Processor, last_tx_seq: &mut u32, raw: bool) {
    match mailbox::take_tx(processor, last_tx_seq) {
        Ok(Some(frame)) => {
            if raw {
                print!("FRAME ");
                for byte in &frame {
                    print!("{byte:02X}");
                }
                println!();
            } else {
                print!("att ");
                for byte in &frame {
                    print!("{byte:02x}");
                }
                println!();
            }
        }
        Ok(None) => {}
        Err(fault) => eprintln!("err mailbox {fault}"),
    }
}

fn emit_gpio(gpio: &Rc<RefCell<GpioBank>>, raw: bool) {
    let bank = gpio.borrow();
    if raw {
        println!("GPIO {:08x} {:08x}", bank.dr, bank.ddr);
    } else {
        println!("gpio {}", gpio_silk(bank.dr, bank.ddr));
    }
}

fn emit_pwm(pwm: &Rc<RefCell<[u32; PWM_CHANNELS]>>) {
    let duty = pwm.borrow();
    print!("PWM");
    for value in duty.iter() {
        print!(" {value:04x}");
    }
    println!();
}

fn flush_uart(uart_rx: &Rc<RefCell<Vec<u8>>>, line: &mut String, raw: bool) {
    let mut buf = uart_rx.borrow_mut();
    if buf.is_empty() {
        return;
    }
    let bytes = buf.split_off(0);
    drop(buf);
    for byte in bytes {
        if byte == b'\n' {
            if raw {
                println!("UART {line}");
            } else {
                println!("uart {line}");
            }
            line.clear();
        } else if byte >= 32 && byte < 127 {
            line.push(char::from(byte));
        }
    }
}

fn report_stop(
    processor: &mut Processor,
    insns: u64,
    live: bool,
    raw: bool,
    gpio: &Rc<RefCell<GpioBank>>,
    reason: &str,
) -> Result<ExitCode, String> {
    emit_gpio(gpio, raw);
    if live {
        if raw {
            println!("STOP {reason}");
        } else {
            eprintln!("stop {reason}");
        }
    } else {
        println!("stop: {reason}");
    }
    eprintln!(
        "insns={insns} pc={:#010x} lr={:#010x} msp={:#010x}",
        processor.get_pc(),
        processor.lr,
        processor.msp
    );
    Ok(ExitCode::from(if reason.starts_with("fault") { 2 } else { 0 }))
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
        assert_eq!(u32::from_le_bytes(vectors[12..16].try_into().unwrap()), SRAM_BASE + 0x121);
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
        assert_eq!(u32::from_le_bytes(vectors[4..8].try_into().unwrap()), 0x1FFF_19E1);
        assert_eq!(u32::from_le_bytes(vectors[12..16].try_into().unwrap()), 0x0000_28F1);
        assert_eq!(u32::from_le_bytes(vectors[0xBC..0xC0].try_into().unwrap()), 0x1FFF_2223);
    }

    #[test]
    fn boot_flash_contains_real_cortex_m0_irq_mask_thunks() {
        let vectors = vec![0u8; VECTOR_MIRROR_BYTES];
        let flash = build_boot_flash(&vectors);
        assert_eq!(&flash[0xC0..0xC4], &[0x72, 0xB6, 0x70, 0x47]);
        assert_eq!(&flash[0xC4..0xC8], &[0x62, 0xB6, 0x70, 0x47]);
    }
}
