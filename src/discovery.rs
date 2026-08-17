use crate::bus::{
    Phy6252Bus, ADC_CH_BASE, MMIO_BASE, MMIO_END, PWM_CHANNELS, ROM_END,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::fault::Fault;

const GPIO_BASE: u32 = 0x4000_8000;
const UART0_BASE: u32 = 0x4000_4000;
const UART1_BASE: u32 = 0x4000_9000;
const PWM_BASE: u32 = 0x4000_E000;
const VECTOR_MIRROR_BYTES: u32 = 8;
const THUMB_BX_LR: u16 = 0x4770;

// Emulator-private MMIO cells used only by tiny ROM ABI thunks. Real firmware never sees
// these addresses directly: the thunk bridges Cortex-M argument registers into DiscoveryBus
// state without teaching the CPU executor about PHY6252 vendor functions.
const EMU_SLEEP_ALLOWED: u32 = 0x5000_FF00;
const EMU_SLEEP_MODE: u32 = 0x5000_FF04;

const TIM_CURRENT: [u32; 6] = [
    0x4000_1004,
    0x4000_1018,
    0x4000_102C,
    0x4000_1040,
    0x4000_1054,
    0x4000_1068,
];

// Registers deliberately accepted as inert read/write storage. Keep this list exact:
// broad peripheral ranges would hide the next silicon behavior that real firmware needs.
// The watchdog-startup entries come from the pinned PHY62XX SDK 3.1.2 drivers.
const KNOWN_STUB_REGS: &[(u32, &str)] = &[
    (0x4000_0000, "PCR.SW_RESET0"),
    (0x4000_000C, "PCR.SW_RESET2"),
    (0x4000_0014, "PCR.SW_CLK1"),
    (0x4000_2000, "WDT.CR"),
    (0x4000_2004, "WDT.TORR"),
    (0x4000_200C, "WDT.CRR"),
    (0x4000_2014, "WDT.EOI"),
    (0x4000_5000, "I2C0.IC_CON"),
    (0x4000_6000, "SPI0"),
    (0x4000_F000, "AON.PMCTL0"),
    (0x4000_F03C, "PCRM.CLKSEL"),
];

struct RomShim {
    entry: u32,
    name: &'static str,
    behavior: &'static str,
    code: &'static [u8],
}

// BX LR, padded so a 32-bit instruction fetch at the entry is still fully backed.
const DRV_IRQ_INIT_CODE: &[u8] = &[0x70, 0x47, 0x70, 0x47];

// enableSleep():
//   movs r0, #1
//   ldr  r1, [pc, #4]   ; literal at entry + 8
//   str  r0, [r1]
//   bx   lr
//   .word EMU_SLEEP_ALLOWED
const ENABLE_SLEEP_CODE: &[u8] = &[
    0x01, 0x20, 0x01, 0x49, 0x08, 0x60, 0x70, 0x47, 0x00, 0xFF, 0x00, 0x50,
];

// disableSleep(): same thunk, writing zero.
const DISABLE_SLEEP_CODE: &[u8] = &[
    0x00, 0x20, 0x01, 0x49, 0x08, 0x60, 0x70, 0x47, 0x00, 0xFF, 0x00, 0x50,
];

// setSleepMode(Sleep_Mode mode): preserve r0, store it in the emulator power-state cell.
//   ldr  r1, [pc, #4]   ; literal at entry + 8
//   str  r0, [r1]
//   bx   lr
//   nop
//   .word EMU_SLEEP_MODE
const SET_SLEEP_MODE_CODE: &[u8] = &[
    0x01, 0x49, 0x08, 0x60, 0x70, 0x47, 0x00, 0xBF, 0x04, 0xFF, 0x00, 0x50,
];

// Exact ROM ABI entry points observed in PHY62XX SDK 3.1.2 / Test-DPLS.
// Addresses are fetch addresses (Thumb symbol address with bit 0 cleared).
const ROM_SHIMS: &[RomShim] = &[
    RomShim {
        entry: 0x0000_A9C8,
        name: "drv_irq_init",
        behavior: "noop-return",
        code: DRV_IRQ_INIT_CODE,
    },
    RomShim {
        entry: 0x0000_A920,
        name: "disableSleep",
        behavior: "sleep-allowed=false",
        code: DISABLE_SLEEP_CODE,
    },
    RomShim {
        entry: 0x0000_AEAC,
        name: "enableSleep",
        behavior: "sleep-allowed=true",
        code: ENABLE_SLEEP_CODE,
    },
    RomShim {
        entry: 0x0001_6B44,
        name: "setSleepMode",
        behavior: "sleep-mode=r0",
        code: SET_SLEEP_MODE_CODE,
    },
];

pub struct DiscoveryBus {
    inner: Phy6252Bus,
    strict: bool,
    sparse_mmio: RefCell<HashMap<u32, u32>>,
    seen_unknown: RefCell<HashSet<u32>>,
    seen_rom: RefCell<HashSet<u32>>,
    seen_shims: RefCell<HashSet<u32>>,
    sleep_allowed: Cell<bool>,
    sleep_mode: Cell<u32>,
}

impl DiscoveryBus {
    pub fn new(inner: Phy6252Bus, strict: bool) -> Self {
        Self {
            inner,
            strict,
            sparse_mmio: RefCell::new(HashMap::new()),
            seen_unknown: RefCell::new(HashSet::new()),
            seen_rom: RefCell::new(HashSet::new()),
            seen_shims: RefCell::new(HashSet::new()),
            sleep_allowed: Cell::new(false),
            sleep_mode: Cell::new(0),
        }
    }

    pub fn sleep_allowed(&self) -> bool {
        self.sleep_allowed.get()
    }

    pub fn sleep_mode(&self) -> u32 {
        self.sleep_mode.get()
    }

    fn is_mmio(addr: u32) -> bool {
        (MMIO_BASE..MMIO_END).contains(&addr)
    }

    fn is_unmodeled_rom(addr: u32) -> bool {
        // zmu owns the mirrored SP/reset pair at 0..8. Everything after that is vendor ROM,
        // which the emulator does not have. In strict discovery, stop at the first access
        // instead of executing the old all-zero ROM placeholder until 0x0002_0000.
        (VECTOR_MIRROR_BYTES..ROM_END).contains(&addr)
    }

    fn rom_shim_for_addr(addr: u32) -> Option<(&'static RomShim, usize)> {
        ROM_SHIMS.iter().find_map(|shim| {
            let offset = addr.checked_sub(shim.entry)? as usize;
            (offset < shim.code.len()).then_some((shim, offset))
        })
    }

    fn rom_shim_byte(&self, addr: u32) -> Option<u8> {
        let (shim, offset) = Self::rom_shim_for_addr(addr)?;
        if self.seen_shims.borrow_mut().insert(shim.entry) {
            eprintln!(
                "ROM shim {} entry={:#010x} behavior={}",
                shim.name, shim.entry, shim.behavior
            );
        }
        Some(shim.code[offset])
    }

    fn rom_shim_read(&self, addr: u32, width: usize) -> Option<u32> {
        let mut value = 0u32;
        for i in 0..width {
            value |= u32::from(self.rom_shim_byte(addr + i as u32)?) << (i * 8);
        }
        Some(value)
    }

    fn gpio_known(addr: u32, write: bool) -> bool {
        let aligned = addr & !3;
        match aligned.wrapping_sub(GPIO_BASE) {
            0x00 | 0x04 | 0x08 => true,
            0x50 => !write,
            _ => false,
        }
    }

    fn uart_read_known(addr: u32) -> bool {
        let aligned = addr & !3;
        [UART0_BASE, UART1_BASE].iter().any(|base| {
            matches!(aligned.wrapping_sub(*base), 0x08 | 0x14 | 0x7C | 0x80 | 0x84)
        })
    }

    fn uart_write_known(addr: u32) -> bool {
        let aligned = addr & !3;
        aligned == UART0_BASE || aligned == UART1_BASE
    }

    fn adc_read_known(addr: u32) -> bool {
        let aligned = addr & !3;
        aligned >= ADC_CH_BASE && aligned < ADC_CH_BASE + 9 * 4
    }

    fn pwm_write_known(addr: u32) -> bool {
        let aligned = addr & !3;
        (0..PWM_CHANNELS as u32).any(|ch| aligned == PWM_BASE + ch * 16 + 8)
    }

    fn timer_read_known(addr: u32) -> bool {
        TIM_CURRENT.contains(&(addr & !3))
    }

    fn functional_read(addr: u32) -> bool {
        Self::gpio_known(addr, false)
            || Self::uart_read_known(addr)
            || Self::adc_read_known(addr)
            || Self::timer_read_known(addr)
    }

    fn functional_write(addr: u32) -> bool {
        Self::gpio_known(addr, true) || Self::uart_write_known(addr) || Self::pwm_write_known(addr)
    }

    fn known_stub(addr: u32) -> Option<&'static str> {
        let aligned = addr & !3;
        KNOWN_STUB_REGS
            .iter()
            .find_map(|(reg, name)| (*reg == aligned).then_some(*name))
    }

    fn emu_control_read(&self, addr: u32) -> Option<u32> {
        match addr & !3 {
            EMU_SLEEP_ALLOWED => Some(u32::from(self.sleep_allowed.get())),
            EMU_SLEEP_MODE => Some(self.sleep_mode.get()),
            _ => None,
        }
    }

    fn emu_control_write(&self, addr: u32, value: u32) -> bool {
        match addr & !3 {
            EMU_SLEEP_ALLOWED => {
                let new_value = value != 0;
                if self.sleep_allowed.replace(new_value) != new_value {
                    eprintln!("PWR sleep_allowed={new_value}");
                }
                true
            }
            EMU_SLEEP_MODE => {
                let old = self.sleep_mode.replace(value);
                if old != value {
                    let name = match value {
                        0 => "MCU_SLEEP_MODE",
                        1 => "SYSTEM_SLEEP_MODE",
                        2 => "SYSTEM_OFF_MODE",
                        _ => "UNKNOWN",
                    };
                    eprintln!("PWR sleep_mode={value} ({name})");
                }
                true
            }
            _ => false,
        }
    }

    fn sparse_read(&self, addr: u32) -> u32 {
        *self
            .sparse_mmio
            .borrow()
            .get(&(addr & !3))
            .unwrap_or(&0xFFFF_FFFF)
    }

    fn sparse_write(&self, addr: u32, value: u32, width: u32) {
        let aligned = addr & !3;
        let shift = (addr & 3) * 8;
        let bits = width * 8;
        let mask = if bits >= 32 {
            0xFFFF_FFFF
        } else {
            ((1u32 << bits) - 1) << shift
        };
        let mut mmio = self.sparse_mmio.borrow_mut();
        let current = *mmio.get(&aligned).unwrap_or(&0xFFFF_FFFF);
        mmio.insert(aligned, (current & !mask) | ((value << shift) & mask));
    }

    fn unknown(&self, op: &str, addr: u32) -> Result<(), Fault> {
        let aligned = addr & !3;
        let first = self.seen_unknown.borrow_mut().insert(aligned);
        if first {
            if self.strict {
                eprintln!("MMIO unknown {op} addr={addr:#010x} aligned={aligned:#010x} -- strict fault");
            } else {
                eprintln!("MMIO unknown {op} addr={addr:#010x} aligned={aligned:#010x} -- sparse stub");
            }
        }
        if self.strict {
            Err(Fault::DAccViol)
        } else {
            Ok(())
        }
    }

    fn rom_unknown<T>(&self, op: &str, addr: u32) -> Result<T, Fault> {
        let first = self.seen_rom.borrow_mut().insert(addr & !1);
        if first {
            eprintln!(
                "ROM unknown {op} addr={addr:#010x} -- vendor ROM image/ABI not modeled; strict fault"
            );
        }
        Err(Fault::DAccViol)
    }

    fn read_fallback(&self, op: &str, addr: u32) -> Result<u32, Fault> {
        if Self::known_stub(addr).is_some() {
            return Ok(self.sparse_read(addr));
        }
        self.unknown(op, addr)?;
        Ok(self.sparse_read(addr))
    }

    fn write_fallback(&self, op: &str, addr: u32, value: u32, width: u32) -> Result<(), Fault> {
        if Self::known_stub(addr).is_none() {
            self.unknown(op, addr)?;
        }
        self.sparse_write(addr, value, width);
        Ok(())
    }
}

impl Bus for DiscoveryBus {
    fn read32(&mut self, addr: u32) -> Result<u32, Fault> {
        if let Some(value) = self.rom_shim_read(addr, 4) {
            return Ok(value);
        }
        if let Some(value) = self.emu_control_read(addr) {
            return Ok(value);
        }
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("read32", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_read(addr) {
            return self.inner.read32(addr);
        }
        self.read_fallback("read32", addr)
    }

    fn read16(&self, addr: u32) -> Result<u16, Fault> {
        if let Some(value) = self.rom_shim_read(addr, 2) {
            return Ok(value as u16);
        }
        if let Some(value) = self.emu_control_read(addr) {
            return Ok((value >> ((addr & 3) * 8)) as u16);
        }
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("read16", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_read(addr) {
            return self.inner.read16(addr);
        }
        let word = self.read_fallback("read16", addr)?;
        Ok((word >> ((addr & 3) * 8)) as u16)
    }

    fn read8(&self, addr: u32) -> Result<u8, Fault> {
        if let Some(value) = self.rom_shim_byte(addr) {
            return Ok(value);
        }
        if let Some(value) = self.emu_control_read(addr) {
            return Ok((value >> ((addr & 3) * 8)) as u8);
        }
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("read8", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_read(addr) {
            return self.inner.read8(addr);
        }
        let word = self.read_fallback("read8", addr)?;
        Ok((word >> ((addr & 3) * 8)) as u8)
    }

    fn write32(&mut self, addr: u32, value: u32) -> Result<(), Fault> {
        if self.emu_control_write(addr, value) {
            return Ok(());
        }
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("write32", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_write(addr) {
            return self.inner.write32(addr, value);
        }
        self.write_fallback("write32", addr, value, 4)
    }

    fn write16(&mut self, addr: u32, value: u16) -> Result<(), Fault> {
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("write16", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_write(addr) {
            return self.inner.write16(addr, value);
        }
        self.write_fallback("write16", addr, u32::from(value), 2)
    }

    fn write8(&mut self, addr: u32, value: u8) -> Result<(), Fault> {
        if self.strict && Self::is_unmodeled_rom(addr) {
            return self.rom_unknown("write8", addr);
        }
        if !Self::is_mmio(addr) || Self::functional_write(addr) {
            return self.inner.write8(addr, value);
        }
        self.write_fallback("write8", addr, u32::from(value), 1)
    }

    fn in_range(&self, addr: u32) -> bool {
        self.inner.in_range(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{SRAM_SIZE, XIP_SIZE};

    fn bus(strict: bool) -> DiscoveryBus {
        DiscoveryBus::new(
            Phy6252Bus::new(vec![0; SRAM_SIZE], vec![0; XIP_SIZE]),
            strict,
        )
    }

    #[test]
    fn permissive_unknown_mmio_is_sparse_and_does_not_alias() {
        let mut bus = bus(false);
        let a = 0x4001_0000;
        let b = 0x4001_1000; // These aliased in the old `% 1024` backing store.

        bus.write32(a, 0x1122_3344).unwrap();
        bus.write32(b, 0xAABB_CCDD).unwrap();

        assert_eq!(bus.read32(a).unwrap(), 0x1122_3344);
        assert_eq!(bus.read32(b).unwrap(), 0xAABB_CCDD);
    }

    #[test]
    fn sparse_mmio_preserves_partial_writes() {
        let mut bus = bus(false);
        let addr = 0x4001_2000;

        bus.write32(addr, 0x1122_3344).unwrap();
        bus.write8(addr + 1, 0xAA).unwrap();
        bus.write16(addr + 2, 0xBEEF).unwrap();

        assert_eq!(bus.read32(addr).unwrap(), 0xBEEF_AA44);
    }

    #[test]
    fn strict_mode_faults_on_unmodeled_register() {
        let mut bus = bus(true);
        assert!(matches!(
            bus.write32(0x4001_0000, 1),
            Err(Fault::DAccViol)
        ));
    }

    #[test]
    fn strict_mode_accepts_explicit_watchdog_startup_regs() {
        let mut bus = bus(true);
        for addr in [
            0x4000_F03C,
            0x4000_0014,
            0x4000_0000,
            0x4000_000C,
            0x4000_2000,
            0x4000_2004,
            0x4000_200C,
            0x4000_2014,
        ] {
            bus.write32(addr, 0x5555).unwrap();
            assert_eq!(bus.read32(addr).unwrap(), 0x5555);
        }
    }

    #[test]
    fn strict_mode_stops_at_first_vendor_rom_access() {
        let bus = bus(true);
        assert!(matches!(bus.read16(0x0000_1000), Err(Fault::DAccViol)));
    }

    #[test]
    fn drv_irq_init_shim_is_a_thumb_noop_return() {
        let bus = bus(true);
        assert_eq!(bus.read16(0x0000_A9C8).unwrap(), THUMB_BX_LR);
        assert!(matches!(bus.read16(0x0000_A9CC), Err(Fault::DAccViol)));
    }

    #[test]
    fn sleep_rom_thunks_encode_real_state_updates() {
        let mut bus = bus(true);

        assert_eq!(bus.read16(0x0000_AEAC).unwrap(), 0x2001); // movs r0,#1
        assert_eq!(bus.read16(0x0000_AEAE).unwrap(), 0x4901); // ldr r1,literal
        assert_eq!(bus.read16(0x0000_AEB0).unwrap(), 0x6008); // str r0,[r1]
        assert_eq!(bus.read16(0x0000_AEB2).unwrap(), THUMB_BX_LR);
        assert_eq!(bus.read32(0x0000_AEB4).unwrap(), EMU_SLEEP_ALLOWED);

        assert_eq!(bus.read16(0x0001_6B44).unwrap(), 0x4901);
        assert_eq!(bus.read16(0x0001_6B46).unwrap(), 0x6008);
        assert_eq!(bus.read16(0x0001_6B48).unwrap(), THUMB_BX_LR);
        assert_eq!(bus.read32(0x0001_6B4C).unwrap(), EMU_SLEEP_MODE);
    }

    #[test]
    fn emulator_power_cells_track_sleep_policy() {
        let mut bus = bus(true);
        assert!(!bus.sleep_allowed());
        assert_eq!(bus.sleep_mode(), 0);

        bus.write32(EMU_SLEEP_ALLOWED, 1).unwrap();
        bus.write32(EMU_SLEEP_MODE, 1).unwrap();
        assert!(bus.sleep_allowed());
        assert_eq!(bus.sleep_mode(), 1);

        bus.write32(EMU_SLEEP_ALLOWED, 0).unwrap();
        assert!(!bus.sleep_allowed());
    }

    #[test]
    fn vector_mirror_is_not_treated_as_unknown_rom() {
        let mut bus = bus(true);
        assert!(bus.read32(0).is_ok());
    }
}
