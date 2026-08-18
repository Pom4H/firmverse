use crate::bus::{HOST_FLASH_ADDR, HOST_FLASH_PROGRAM, XIP_BASE, XIP_SIZE};
use crate::discovery::DiscoveryBus;
use std::cell::Cell;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::fault::Fault;

pub const DMAC_BASE: u32 = 0x4001_0000;
const DMAC_END: u32 = DMAC_BASE + 0x400;
const CHANNELS: usize = 4;
const CH_STRIDE: u32 = 0x58;

const CH_SAR: u32 = 0x00;
const CH_DAR: u32 = 0x08;
const CH_LLP: u32 = 0x10;
const CH_CTL: u32 = 0x18;
const CH_CTL_H: u32 = 0x1C;
const CH_CFG: u32 = 0x40;
const CH_CFG_H: u32 = 0x44;

const RAW_TFR: u32 = 0x2C0;
const STATUS_TFR: u32 = 0x2E8;
const MASK_TFR: u32 = 0x310;
const CLEAR_TFR: u32 = 0x338;
const DMA_CFG: u32 = 0x398;
const CH_EN: u32 = 0x3A0;

const UART0_THR: u32 = 0x4000_4000;
const UART1_THR: u32 = 0x4000_9000;

#[derive(Clone, Copy, Debug, Default)]
struct Channel {
    sar: u32,
    dar: u32,
    llp: u32,
    ctl: u32,
    ctl_h: u32,
    cfg: u32,
    cfg_h: u32,
}

#[derive(Debug)]
struct DmaState {
    channels: [Channel; CHANNELS],
    raw_tfr: u32,
    status_tfr: u32,
    mask_tfr: u32,
    dma_cfg: u32,
    ch_en: u32,
}

impl Default for DmaState {
    fn default() -> Self {
        Self {
            channels: [Channel::default(); CHANNELS],
            raw_tfr: 0,
            status_tfr: 0,
            mask_tfr: 0,
            dma_cfg: 0,
            ch_en: 0,
        }
    }
}

/// Functional wrapper for the PHY6252 DesignWare-style DMAC.
///
/// Transfers are completed synchronously when ChEnReg enables a channel. This
/// preserves the guest-visible register contract used by the public SDK while
/// avoiding invented wall-clock DMA timing. Unsupported linked lists and
/// peripheral-to-peripheral transfers fault instead of silently succeeding.
pub struct DmaBus {
    inner: DiscoveryBus,
    state: DmaState,
    seen_transfer: Cell<bool>,
}

impl DmaBus {
    pub fn new(inner: DiscoveryBus) -> Self {
        Self {
            inner,
            state: DmaState::default(),
            seen_transfer: Cell::new(false),
        }
    }

    fn channel_reg(addr: u32) -> Option<(usize, u32)> {
        if !(DMAC_BASE..DMAC_BASE + CH_STRIDE * CHANNELS as u32).contains(&addr) {
            return None;
        }
        let off = addr - DMAC_BASE;
        let ch = (off / CH_STRIDE) as usize;
        Some((ch, off % CH_STRIDE))
    }

    fn dma_read32(&self, addr: u32) -> Option<u32> {
        if let Some((ch, reg)) = Self::channel_reg(addr) {
            let c = self.state.channels[ch];
            return match reg {
                CH_SAR => Some(c.sar),
                CH_DAR => Some(c.dar),
                CH_LLP => Some(c.llp),
                CH_CTL => Some(c.ctl),
                CH_CTL_H => Some(c.ctl_h),
                CH_CFG => Some(c.cfg),
                CH_CFG_H => Some(c.cfg_h),
                _ => None,
            };
        }
        match addr.wrapping_sub(DMAC_BASE) {
            RAW_TFR => Some(self.state.raw_tfr),
            STATUS_TFR => Some(self.state.status_tfr),
            MASK_TFR => Some(self.state.mask_tfr),
            DMA_CFG => Some(self.state.dma_cfg),
            CH_EN => Some(self.state.ch_en),
            _ => None,
        }
    }

    fn dma_write32(&mut self, addr: u32, value: u32) -> Option<Result<(), Fault>> {
        if let Some((ch, reg)) = Self::channel_reg(addr) {
            let c = &mut self.state.channels[ch];
            match reg {
                CH_SAR => c.sar = value,
                CH_DAR => c.dar = value,
                CH_LLP => c.llp = value,
                CH_CTL => c.ctl = value,
                CH_CTL_H => c.ctl_h = value,
                CH_CFG => c.cfg = value,
                CH_CFG_H => c.cfg_h = value,
                _ => return None,
            }
            return Some(Ok(()));
        }

        let off = addr.wrapping_sub(DMAC_BASE);
        match off {
            MASK_TFR => {
                // DW_apb_dmac write-enable convention: upper byte selects which
                // low mask bits are updated.
                for ch in 0..CHANNELS {
                    let bit = 1u32 << ch;
                    let we = 1u32 << (ch + 8);
                    if value & we != 0 {
                        if value & bit != 0 { self.state.mask_tfr |= bit; }
                        else { self.state.mask_tfr &= !bit; }
                    }
                }
                Some(Ok(()))
            }
            CLEAR_TFR => {
                self.state.raw_tfr &= !value;
                self.state.status_tfr &= !value;
                Some(Ok(()))
            }
            DMA_CFG => {
                self.state.dma_cfg = value & 1;
                Some(Ok(()))
            }
            CH_EN => {
                let mut starts = Vec::new();
                for ch in 0..CHANNELS {
                    let bit = 1u32 << ch;
                    let we = 1u32 << (ch + 8);
                    if value & we == 0 { continue; }
                    if value & bit != 0 {
                        self.state.ch_en |= bit;
                        starts.push(ch);
                    } else {
                        self.state.ch_en &= !bit;
                    }
                }
                for ch in starts {
                    if let Err(fault) = self.execute(ch) {
                        return Some(Err(fault));
                    }
                }
                Some(Ok(()))
            }
            _ => None,
        }
    }

    fn execute(&mut self, ch: usize) -> Result<(), Fault> {
        if self.state.dma_cfg & 1 == 0 {
            eprintln!("DMAC strict channel {ch} started while controller disabled");
            return Err(Fault::DAccViol);
        }
        let c = self.state.channels[ch];
        if c.llp != 0 {
            eprintln!("DMAC strict channel {ch} linked-list pointer={:#010x} unsupported", c.llp);
            return Err(Fault::DAccViol);
        }

        let items = (c.ctl_h & 0x7FF) as usize;
        let src_width = width_bytes((c.ctl >> 4) & 0x7)?;
        let dst_width = width_bytes((c.ctl >> 1) & 0x7)?;
        let src_inc = (c.ctl >> 9) & 0x3;
        let dst_inc = (c.ctl >> 7) & 0x3;
        let transfer_type = (c.ctl >> 20) & 0x7;

        if items == 0 || src_width != dst_width || !matches!(transfer_type, 0 | 1 | 2) {
            eprintln!(
                "DMAC strict channel {ch} unsupported items={items} src_width={src_width} dst_width={dst_width} type={transfer_type}"
            );
            return Err(Fault::DAccViol);
        }
        if !matches!(src_inc, 0 | 1 | 2) || !matches!(dst_inc, 0 | 1 | 2) {
            return Err(Fault::DAccViol);
        }
        if transfer_type == 1 && !matches!(c.dar, UART0_THR | UART1_THR) {
            eprintln!("DMAC strict channel {ch} M2P destination={:#010x} unsupported", c.dar);
            return Err(Fault::DAccViol);
        }
        if transfer_type == 2 {
            eprintln!("DMAC strict channel {ch} P2M source={:#010x} unsupported", c.sar);
            return Err(Fault::DAccViol);
        }

        let mut src = c.sar;
        let mut dst = c.dar;
        for _ in 0..items {
            for byte_off in 0..src_width {
                let byte = self.inner.read8(src.wrapping_add(byte_off as u32))?;
                self.write_destination(dst.wrapping_add(byte_off as u32), byte)?;
            }
            src = step_addr(src, src_inc, src_width)?;
            dst = step_addr(dst, dst_inc, dst_width)?;
        }

        let bit = 1u32 << ch;
        self.state.ch_en &= !bit;
        self.state.raw_tfr |= bit;
        self.state.status_tfr |= bit;
        if !self.seen_transfer.replace(true) {
            eprintln!("DMAC functional synchronous transfer engine active");
        }
        eprintln!(
            "DMAC ch={ch} complete items={items} width={src_width} src={:#010x} dst={:#010x} type={transfer_type}",
            c.sar, c.dar
        );
        Ok(())
    }

    fn write_destination(&mut self, addr: u32, byte: u8) -> Result<(), Fault> {
        if (XIP_BASE..XIP_BASE + XIP_SIZE as u32).contains(&addr) {
            self.inner.write32(HOST_FLASH_ADDR, addr - XIP_BASE)?;
            self.inner.write32(HOST_FLASH_PROGRAM, u32::from(byte))
        } else {
            self.inner.write8(addr, byte)
        }
    }
}

fn width_bytes(code: u32) -> Result<usize, Fault> {
    match code {
        0 => Ok(1),
        1 => Ok(2),
        2 => Ok(4),
        3 => Ok(8),
        4 => Ok(16),
        5 => Ok(32),
        _ => Err(Fault::DAccViol),
    }
}

fn step_addr(addr: u32, inc: u32, width: usize) -> Result<u32, Fault> {
    match inc {
        0 => Ok(addr.wrapping_add(width as u32)),
        1 => Ok(addr.wrapping_sub(width as u32)),
        2 => Ok(addr),
        _ => Err(Fault::DAccViol),
    }
}

impl Bus for DmaBus {
    fn read32(&mut self, addr: u32) -> Result<u32, Fault> {
        if (DMAC_BASE..DMAC_END).contains(&addr) {
            return self.dma_read32(addr).ok_or(Fault::DAccViol);
        }
        self.inner.read32(addr)
    }

    fn read16(&self, addr: u32) -> Result<u16, Fault> {
        if (DMAC_BASE..DMAC_END).contains(&addr) {
            return Err(Fault::DAccViol);
        }
        self.inner.read16(addr)
    }

    fn read8(&self, addr: u32) -> Result<u8, Fault> {
        if (DMAC_BASE..DMAC_END).contains(&addr) {
            return Err(Fault::DAccViol);
        }
        self.inner.read8(addr)
    }

    fn write32(&mut self, addr: u32, value: u32) -> Result<(), Fault> {
        if (DMAC_BASE..DMAC_END).contains(&addr) {
            return self.dma_write32(addr, value).unwrap_or(Err(Fault::DAccViol));
        }
        self.inner.write32(addr, value)
    }

    fn write16(&mut self, addr: u32, value: u16) -> Result<(), Fault> {
        if (DMAC_BASE..DMAC_END).contains(&addr) {
            return Err(Fault::DAccViol);
        }
        self.inner.write16(addr, value)
    }

    fn write8(&mut self, addr: u32, value: u8) -> Result<(), Fault> {
        if (DMAC_BASE..DMAC_END).contains(&addr) {
            return Err(Fault::DAccViol);
        }
        self.inner.write8(addr, value)
    }

    fn in_range(&self, addr: u32) -> bool {
        (DMAC_BASE..DMAC_END).contains(&addr) || self.inner.in_range(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_sdk_register_offsets_are_stable() {
        assert_eq!(DMAC_BASE, 0x4001_0000);
        assert_eq!(RAW_TFR, 0x2C0);
        assert_eq!(CLEAR_TFR, 0x338);
        assert_eq!(CH_EN, 0x3A0);
        assert_eq!(CH_STRIDE, 0x58);
    }

    #[test]
    fn width_codes_match_public_dma_header() {
        assert_eq!(width_bytes(0).unwrap(), 1);
        assert_eq!(width_bytes(1).unwrap(), 2);
        assert_eq!(width_bytes(2).unwrap(), 4);
        assert!(width_bytes(7).is_err());
    }

    #[test]
    fn increment_modes_are_explicit() {
        assert_eq!(step_addr(0x100, 0, 4).unwrap(), 0x104);
        assert_eq!(step_addr(0x100, 1, 4).unwrap(), 0xFC);
        assert_eq!(step_addr(0x100, 2, 4).unwrap(), 0x100);
        assert!(step_addr(0x100, 3, 4).is_err());
    }
}
