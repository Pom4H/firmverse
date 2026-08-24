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
pub const IOMUX_PAD_PS0: u32 = 0x4000_3844;
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
pub const PCRM_ANA_CTL: u32 = 0x4000_F048;
pub const PCRM_ADC_CTL0: u32 = 0x4000_F06C;
pub const PCRM_ADC_CTL1: u32 = 0x4000_F070;
pub const PCRM_ADC_CTL2: u32 = 0x4000_F074;
pub const PCRM_ADC_CTL3: u32 = 0x4000_F078;
pub const PCRM_ADC_CTL4: u32 = 0x4000_F07C;

pub const DMAC_BASE: u32 = 0x4001_0000;
pub const DMAC_CH_STRIDE: u32 = 0x58;
pub const DMAC_RAW_TFR: u32 = DMAC_BASE + 0x2C0;
pub const DMAC_STATUS_TFR: u32 = DMAC_BASE + 0x2E8;
pub const DMAC_MASK_TFR: u32 = DMAC_BASE + 0x310;
pub const DMAC_CLEAR_TFR: u32 = DMAC_BASE + 0x338;
pub const DMAC_CFG: u32 = DMAC_BASE + 0x398;
pub const DMAC_CH_EN: u32 = DMAC_BASE + 0x3A0;

const fn dma_ch(ch: u32, reg: u32) -> u32 {
    DMAC_BASE + ch * DMAC_CH_STRIDE + reg
}

const STORAGE_REGS: &[StorageReg] = &[
    StorageReg {
        addr: IOMUX_ANALOG_IO_EN,
        name: "IOMUX.Analog_IO_en",
        reset: 0,
    },
    StorageReg {
        addr: IOMUX_FULL_MUX0_EN,
        name: "IOMUX.full_mux0_en",
        reset: 0,
    },
    StorageReg {
        addr: IOMUX_GPIO_SEL1,
        name: "IOMUX.gpio_sel[1]",
        reset: 0,
    },
    StorageReg {
        addr: IOMUX_PAD_PS0,
        name: "IOMUX.pad_ps0",
        reset: 0,
    },
    StorageReg {
        addr: AP_CACHE_CTRL0,
        name: "CACHE.CTRL0",
        reset: 0,
    },
    StorageReg {
        addr: AP_CACHE_CTRL1,
        name: "CACHE.CTRL1",
        reset: 0,
    },
    // The SDK's SPIF_STATUS_WAIT_IDLE waits for CONFIG.bit31 and FCMD.bit1 to indicate
    // an idle controller. The host NOR backend completes commands synchronously, so these
    // reset values represent an immediately-ready controller while preserving command data.
    StorageReg {
        addr: SPIF_CONFIG,
        name: "SPIF.CONFIG",
        reset: 0x8000_0000,
    },
    StorageReg {
        addr: SPIF_FCMD,
        name: "SPIF.FCMD",
        reset: 0,
    },
    StorageReg {
        addr: SPIF_FCMD_ADDR,
        name: "SPIF.FCMD_ADDR",
        reset: 0,
    },
    StorageReg {
        addr: SPIF_FCMD_RDDATA0,
        name: "SPIF.FCMD_RDDATA[0]",
        reset: 0,
    },
    StorageReg {
        addr: SPIF_FCMD_RDDATA1,
        name: "SPIF.FCMD_RDDATA[1]",
        reset: 0,
    },
    StorageReg {
        addr: SPIF_FCMD_WRDATA0,
        name: "SPIF.FCMD_WRDATA[0]",
        reset: 0,
    },
    StorageReg {
        addr: SPIF_FCMD_WRDATA1,
        name: "SPIF.FCMD_WRDATA[1]",
        reset: 0,
    },
    StorageReg {
        addr: SPIF_POLL_FSTATUS,
        name: "SPIF.POLL_FSTATUS",
        reset: 0,
    },
    StorageReg {
        addr: PCRM_CLKHF_CTL0,
        name: "PCRM.CLKHF_CTL0",
        reset: 0,
    },
    StorageReg {
        addr: PCRM_CLKHF_CTL1,
        name: "PCRM.CLKHF_CTL1",
        reset: 0,
    },
    StorageReg {
        addr: PCRM_ANA_CTL,
        name: "PCRM.ANA_CTL",
        reset: 0,
    },
    StorageReg {
        addr: PCRM_ADC_CTL0,
        name: "PCRM.ADC_CTL0",
        reset: 0,
    },
    StorageReg {
        addr: PCRM_ADC_CTL1,
        name: "PCRM.ADC_CTL1",
        reset: 0,
    },
    StorageReg {
        addr: PCRM_ADC_CTL2,
        name: "PCRM.ADC_CTL2",
        reset: 0,
    },
    StorageReg {
        addr: PCRM_ADC_CTL3,
        name: "PCRM.ADC_CTL3",
        reset: 0,
    },
    StorageReg {
        addr: PCRM_ADC_CTL4,
        name: "PCRM.ADC_CTL4",
        reset: 0,
    },
    // Public SDK AP_DMA_CH_CFG(n), AP_DMA_INT and AP_DMA_MISC registers used by
    // hal_dma_config_channel/start/stop/wait. Only identified words are exposed;
    // LLP remains visible so the behavioral model can reject non-zero linked lists.
    StorageReg {
        addr: dma_ch(0, 0x00),
        name: "DMAC.CH0.SAR",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(0, 0x08),
        name: "DMAC.CH0.DAR",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(0, 0x10),
        name: "DMAC.CH0.LLP",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(0, 0x18),
        name: "DMAC.CH0.CTL",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(0, 0x1C),
        name: "DMAC.CH0.CTL_H",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(0, 0x40),
        name: "DMAC.CH0.CFG",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(0, 0x44),
        name: "DMAC.CH0.CFG_H",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(1, 0x00),
        name: "DMAC.CH1.SAR",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(1, 0x08),
        name: "DMAC.CH1.DAR",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(1, 0x10),
        name: "DMAC.CH1.LLP",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(1, 0x18),
        name: "DMAC.CH1.CTL",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(1, 0x1C),
        name: "DMAC.CH1.CTL_H",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(1, 0x40),
        name: "DMAC.CH1.CFG",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(1, 0x44),
        name: "DMAC.CH1.CFG_H",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(2, 0x00),
        name: "DMAC.CH2.SAR",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(2, 0x08),
        name: "DMAC.CH2.DAR",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(2, 0x10),
        name: "DMAC.CH2.LLP",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(2, 0x18),
        name: "DMAC.CH2.CTL",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(2, 0x1C),
        name: "DMAC.CH2.CTL_H",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(2, 0x40),
        name: "DMAC.CH2.CFG",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(2, 0x44),
        name: "DMAC.CH2.CFG_H",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(3, 0x00),
        name: "DMAC.CH3.SAR",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(3, 0x08),
        name: "DMAC.CH3.DAR",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(3, 0x10),
        name: "DMAC.CH3.LLP",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(3, 0x18),
        name: "DMAC.CH3.CTL",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(3, 0x1C),
        name: "DMAC.CH3.CTL_H",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(3, 0x40),
        name: "DMAC.CH3.CFG",
        reset: 0,
    },
    StorageReg {
        addr: dma_ch(3, 0x44),
        name: "DMAC.CH3.CFG_H",
        reset: 0,
    },
    StorageReg {
        addr: DMAC_RAW_TFR,
        name: "DMAC.RawTfr",
        reset: 0,
    },
    StorageReg {
        addr: DMAC_STATUS_TFR,
        name: "DMAC.StatusTfr",
        reset: 0,
    },
    StorageReg {
        addr: DMAC_MASK_TFR,
        name: "DMAC.MaskTfr",
        reset: 0,
    },
    StorageReg {
        addr: DMAC_CLEAR_TFR,
        name: "DMAC.ClearTfr",
        reset: 0,
    },
    StorageReg {
        addr: DMAC_CFG,
        name: "DMAC.DmaCfgReg",
        reset: 0,
    },
    StorageReg {
        addr: DMAC_CH_EN,
        name: "DMAC.ChEnReg",
        reset: 0,
    },
];

pub fn dmac_channel_reg(ch: u32, reg: u32) -> Option<u32> {
    (ch < 4).then_some(dma_ch(ch, reg))
}

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
        assert_eq!(storage_reg(IOMUX_PAD_PS0).unwrap().reset, 0);
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
        assert_eq!(
            storage_reg(SPIF_CONFIG).unwrap().reset & 0x8000_0000,
            0x8000_0000
        );
        assert_eq!(storage_reg(SPIF_FCMD).unwrap().reset & 0x2, 0);
        assert_eq!(storage_reg(SPIF_FCMD_RDDATA0).unwrap().reset, 0);
        assert!(storage_reg(0x4000_C89C).is_none());
    }

    #[test]
    fn high_frequency_clock_controls_are_exact() {
        assert_eq!(storage_reg(PCRM_CLKHF_CTL0).unwrap().reset, 0);
        assert_eq!(storage_reg(PCRM_CLKHF_CTL1).unwrap().reset, 0);
        assert_eq!(storage_reg(PCRM_ANA_CTL).unwrap().reset, 0);
        assert!(storage_reg(0x4000_F04C).is_none());
    }

    #[test]
    fn adc_power_and_channel_controls_match_sdk_layout() {
        for addr in [
            PCRM_ADC_CTL0,
            PCRM_ADC_CTL1,
            PCRM_ADC_CTL2,
            PCRM_ADC_CTL3,
            PCRM_ADC_CTL4,
        ] {
            assert_eq!(storage_reg(addr).unwrap().reset, 0);
        }
        assert!(storage_reg(0x4000_F068).is_none());
        assert!(storage_reg(0x4000_F080).is_none());
    }

    #[test]
    fn dmac_registers_match_public_sdk_layout() {
        assert_eq!(DMAC_BASE, 0x4001_0000);
        assert_eq!(DMAC_CH_STRIDE, 0x58);
        assert_eq!(dmac_channel_reg(0, 0x18), Some(0x4001_0018));
        assert_eq!(dmac_channel_reg(3, 0x44), Some(0x4001_014C));
        assert_eq!(DMAC_RAW_TFR, 0x4001_02C0);
        assert_eq!(DMAC_CLEAR_TFR, 0x4001_0338);
        assert_eq!(DMAC_CH_EN, 0x4001_03A0);
        assert!(storage_reg(DMAC_CH_EN).is_some());
        assert!(storage_reg(DMAC_BASE + 0x04).is_none());
    }
}
