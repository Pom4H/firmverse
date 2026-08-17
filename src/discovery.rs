use crate::bus::{
    Phy6252Bus, ADC_CH_BASE, MMIO_BASE, MMIO_END, PWM_CHANNELS, ROM_END,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::fault::Fault;

const GPIO_BASE: u32 = 0x4000_8000;
const UART0_BASE: u32 = 0x4000_4000;
const UART1_BASE: u32 = 0x4000_9000;
const PWM_BASE: u32 = 0x4000_E000;
const VECTOR_MIRROR_BYTES: u32 = 8;
const THUMB_BX_LR: u16 = 0x4770;

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

// Exact ROM ABI entry points that are intentionally replaced in the emulator.
// drv_irq_init is a bootstrap IRQ-table initializer. Until IRQ delivery itself is modeled,
// the host-side behavior is deliberately a no-op return instead of pretending the vendor ROM
// body is present. The symbol is Thumb 0x0000_a9c9; the fetch address is 0x0000_a9c8.
const ROM_NOOP_SHIMS: &[(u32, &str)] = &[(0x0000_A9C8, "drv_irq_init")];

pub struct DiscoveryBus {
    inner: Phy6252Bus,
    strict: bool,
    sparse_mmio: RefCell<HashMap<u32, u32>>,
    seen_unknown: RefCell<HashSet<u32>>,
    seen_rom: RefCell<HashSet<u32>>,
    seen_shims: RefCell<HashSet<u32>>,
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
        }
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

    fn rom_noop_shim(addr: u32) -> Option<&'static str> {
        ROM_NOOP_SHIMS
            .iter()
            .find_map(|(entry, name)| (*entry == (addr & !1)).then_some(*name))
    }

    fn rom_shim_read16(&self, addr: u32) -> Option<u16> {
        let entry = addr & !1;
        let name = Self::rom_noop_shim(entry)?;
        if self.seen_shims.borrow_mut().insert(entry) {
            eprintln!("ROM shim {name} entry={entry:#010x} behavior=noop-return");
        }
        Some(THUMB_BX_LR)
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
        if Self::rom_noop_shim(addr).is_some() {
            let lo = u32::from(self.rom_shim_read16(addr).unwrap());
            return Ok(lo | (u32::from(THUMB_BX_LR) << 16));
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
        if let Some(value) = self.rom_shim_read16(addr) {
            return Ok(value);
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
        if let Some(_) = Self::rom_noop_shim(addr) {
            let bytes = THUMB_BX_LR.to_le_bytes();
            return Ok(bytes[(addr & 1) as usize]);
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
    fn explicit_rom_shim_is_a_thumb_noop_return() {
        let bus = bus(true);
        assert_eq!(bus.read16(0x0000_A9C8).unwrap(), THUMB_BX_LR);
        assert!(matches!(bus.read16(0x0000_A9CA), Err(Fault::DAccViol)));
    }

    #[test]
    fn vector_mirror_is_not_treated_as_unknown_rom() {
        let mut bus = bus(true);
        assert!(bus.read32(0).is_ok());
    }
}
