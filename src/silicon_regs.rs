//! Exact PHY6252 MMIO registers whose state is deliberately modeled as R/W storage.
//!
//! Keep this file address-by-address. A register only belongs here after its identity and
//! reset value are established from the pinned SDK or real firmware. Behavioral peripherals
//! stay in their dedicated models instead of being hidden behind broad MMIO windows.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageReg {
    pub addr: u32,
    pub name: &'static str,
    pub reset: u32,
}

pub const IOMUX_ANALOG_IO_EN: u32 = 0x4000_3800;
pub const IOMUX_FULL_MUX0_EN: u32 = 0x4000_380C;
pub const IOMUX_GPIO_SEL1: u32 = 0x4000_381C;
pub const AP_CACHE_CTRL0: u32 = 0x4000_C000;
pub const AP_CACHE_CTRL1: u32 = 0x4000_C004;
pub const SPIF_CONFIG: u32 = 0x4000_C800;
pub const SPIF_FCMD: u32 = 0x4000_C890;
pub const SPIF_FCMD_ADDR: u32 = 0x4000_C894;
pub const SPIF_FCMD_RDDATA0: u32 = 0x4000_C8A0;
pub const SPIF_FCMD_RDDATA1: u32 = 0x4000_C8A4;
pub const SPIF_FCMD_WRDATA0: u32 = 0x4000_C8A8;
pub const SPIF_FCMD_WRDATA1: u32 = 0x4000_C8AC;
pub const SPIF_POLL_FSTATUS: u32 = 0x4000_C8B0;
pub const PCRM_CLKHF_CTL0: u32 = 0x4000_F040;
pub const PCRM_CLKHF_CTL1: u32 = 0x4000_F044;

const STORAGE_REGS: &[StorageReg] = &[
    StorageReg { addr: IOMUX_ANALOG_IO_EN, name: "IOMUX.Analog_IO_en", reset: 0 },
    StorageReg { addr: IOMUX_FULL_MUX0_EN, name: "IOMUX.full_mux0_en", reset: 0 },
    StorageReg { addr: IOMUX_GPIO_SEL1, name: "IOMUX.gpio_sel[1]", reset: 0 },
    StorageReg { addr: AP_CACHE_CTRL0, name: "CACHE.CTRL0", reset: 0 },
    StorageReg { addr: AP_CACHE_CTRL1, name: "CACHE.CTRL1", reset: 0 },
    // The SDK's SPIF_STATUS_WAIT_IDLE waits for CONFIG.bit31 and FCMD.bit1 to indicate
    // an idle controller. The host NOR backend completes commands synchronously, so these
    // reset values represent an immediately-ready controller while preserving command data.
    StorageReg { addr: SPIF_CONFIG, name: "SPIF.CONFIG", reset: 0x8000_0000 },
    StorageReg { addr: SPIF_FCMD, name: "SPIF.FCMD", reset: 0 },
    StorageReg { addr: SPIF_FCMD_ADDR, name: "SPIF.FCMD_ADDR", reset: 0 },
    StorageReg { addr: SPIF_FCMD_RDDATA0, name: "SPIF.FCMD_RDDATA[0]", reset: 0 },
    StorageReg { addr: SPIF_FCMD_RDDATA1, name: "SPIF.FCMD_RDDATA[1]", reset: 0 },
    StorageReg { addr: SPIF_FCMD_WRDATA0, name: "SPIF.FCMD_WRDATA[0]", reset: 0 },
    StorageReg { addr: SPIF_FCMD_WRDATA1, name: "SPIF.FCMD_WRDATA[1]", reset: 0 },
    StorageReg { addr: SPIF_POLL_FSTATUS, name: "SPIF.POLL_FSTATUS", reset: 0 },
    StorageReg { addr: PCRM_CLKHF_CTL0, name: "PCRM.CLKHF_CTL0", reset: 0 },
    StorageReg { addr: PCRM_CLKHF_CTL1, name: "PCRM.CLKHF_CTL1", reset: 0 },
];

pub fn storage_reg(addr: u32) -> Option<StorageReg> {
    let aligned = addr & !3;
    STORAGE_REGS.iter().copied().find(|reg| reg.addr == aligned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_iomux_registers_are_exact() {
        assert_eq!(storage_reg(IOMUX_ANALOG_IO_EN).unwrap().reset, 0);
        assert_eq!(storage_reg(IOMUX_FULL_MUX0_EN).unwrap().reset, 0);
        assert_eq!(storage_reg(IOMUX_GPIO_SEL1).unwrap().reset, 0);
        assert!(storage_reg(0x4000_3818).is_none());
        assert!(storage_reg(0x4000_3820).is_none());
    }

    #[test]
    fn cache_control_registers_are_exact_not_a_window() {
        assert_eq!(storage_reg(AP_CACHE_CTRL0).unwrap().reset, 0);
        assert_eq!(storage_reg(AP_CACHE_CTRL1).unwrap().reset, 0);
        assert!(storage_reg(0x4000_C008).is_none());
    }

    #[test]
    fn spi_flash_command_registers_are_exact_and_idle() {
        assert_eq!(storage_reg(SPIF_CONFIG).unwrap().reset & 0x8000_0000, 0x8000_0000);
        assert_eq!(storage_reg(SPIF_FCMD).unwrap().reset & 0x2, 0);
        assert_eq!(storage_reg(SPIF_FCMD_RDDATA0).unwrap().reset, 0);
        assert!(storage_reg(0x4000_C89C).is_none());
    }

    #[test]
    fn high_frequency_clock_controls_are_exact() {
        assert_eq!(storage_reg(PCRM_CLKHF_CTL0).unwrap().reset, 0);
        assert_eq!(storage_reg(PCRM_CLKHF_CTL1).unwrap().reset, 0);
        assert!(storage_reg(0x4000_F048).is_none());
    }
}
