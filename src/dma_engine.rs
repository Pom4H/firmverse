use crate::bus::{HOST_FLASH_ADDR, HOST_FLASH_PROGRAM, XIP_BASE, XIP_SIZE};
use crate::flash_state;
use crate::silicon_regs::{
    dmac_channel_reg, DMAC_CFG, DMAC_CH_EN, DMAC_CLEAR_TFR, DMAC_RAW_TFR, DMAC_STATUS_TFR,
};
use std::sync::atomic::{AtomicBool, Ordering};
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::Processor;

const CH_SAR: u32 = 0x00;
const CH_DAR: u32 = 0x08;
const CH_LLP: u32 = 0x10;
const CH_CTL: u32 = 0x18;
const CH_CTL_H: u32 = 0x1C;

const UART0_THR: u32 = 0x4000_4000;
const UART1_THR: u32 = 0x4000_9000;
const DMA_CONTROLLER_ENABLE: u32 = 1;
const DMA_CHANNELS: u32 = 4;

static SEEN: AtomicBool = AtomicBool::new(false);

/// Service the exact AP_DMAC register contract exported by the public SDK.
///
/// The physical transfer is synchronous: after the guest sets ChEnReg the
/// bytes are moved before the next guest instruction and RawTfr/StatusTfr are
/// asserted. This intentionally models data-plane semantics, not bus timing.
pub fn service(cpu: &mut Processor) -> bool {
    if !clear_completed(cpu) {
        return false;
    }

    let ch_en = match cpu.read32(DMAC_CH_EN) {
        Ok(value) => value,
        Err(_) => return true,
    };
    if ch_en == 0 {
        return true;
    }

    let mut consumed = ch_en;
    for ch in 0..DMA_CHANNELS {
        let bit = 1u32 << ch;
        let write_enable = 1u32 << (ch + 8);
        if ch_en & write_enable == 0 {
            continue;
        }
        consumed &= !(write_enable | bit);
        if ch_en & bit == 0 {
            continue;
        }
        if !execute(cpu, ch) {
            cpu.running = false;
            return false;
        }
    }
    cpu.write32(DMAC_CH_EN, consumed).is_ok()
}

fn clear_completed(cpu: &mut Processor) -> bool {
    let clear = match cpu.read32(DMAC_CLEAR_TFR) {
        Ok(value) => value,
        Err(_) => return true,
    };
    if clear == 0 {
        return true;
    }
    let raw = cpu.read32(DMAC_RAW_TFR).unwrap_or(0) & !clear;
    let status = cpu.read32(DMAC_STATUS_TFR).unwrap_or(0) & !clear;
    cpu.write32(DMAC_RAW_TFR, raw).is_ok()
        && cpu.write32(DMAC_STATUS_TFR, status).is_ok()
        && cpu.write32(DMAC_CLEAR_TFR, 0).is_ok()
}

fn execute(cpu: &mut Processor, ch: u32) -> bool {
    let controller = cpu.read32(DMAC_CFG).unwrap_or(0);
    if controller & DMA_CONTROLLER_ENABLE == 0 {
        eprintln!("DMAC strict ch={ch}: controller disabled");
        return false;
    }

    let sar = match reg(cpu, ch, CH_SAR) {
        Some(v) => v,
        None => return false,
    };
    let dar = match reg(cpu, ch, CH_DAR) {
        Some(v) => v,
        None => return false,
    };
    let llp = match reg(cpu, ch, CH_LLP) {
        Some(v) => v,
        None => return false,
    };
    let ctl = match reg(cpu, ch, CH_CTL) {
        Some(v) => v,
        None => return false,
    };
    let ctl_h = match reg(cpu, ch, CH_CTL_H) {
        Some(v) => v,
        None => return false,
    };

    if llp != 0 {
        eprintln!("DMAC strict ch={ch}: LLP={llp:#010x} is not modeled");
        return false;
    }

    let items = (ctl_h & 0x7FF) as usize;
    let src_width = match width_bytes((ctl >> 4) & 7) {
        Some(v) => v,
        None => return unsupported(ch, ctl, "source width"),
    };
    let dst_width = match width_bytes((ctl >> 1) & 7) {
        Some(v) => v,
        None => return unsupported(ch, ctl, "destination width"),
    };
    let src_inc = (ctl >> 9) & 3;
    let dst_inc = (ctl >> 7) & 3;
    let transfer_type = (ctl >> 20) & 7;

    if items == 0 || src_width != dst_width {
        return unsupported(ch, ctl, "zero transfer or width conversion");
    }
    if !matches!(src_inc, 0 | 1 | 2) || !matches!(dst_inc, 0 | 1 | 2) {
        return unsupported(ch, ctl, "increment mode");
    }
    match transfer_type {
        0 => {}
        1 if matches!(dar, UART0_THR | UART1_THR) => {}
        _ => return unsupported(ch, ctl, "transfer type/peripheral"),
    }

    let mut src = sar;
    let mut dst = dar;
    let mut touched_flash = false;
    for _ in 0..items {
        for byte_index in 0..src_width {
            let byte = match cpu.read8(src.wrapping_add(byte_index as u32)) {
                Ok(v) => v,
                Err(fault) => {
                    eprintln!(
                        "DMAC strict ch={ch}: read {:#010x}: {fault}",
                        src.wrapping_add(byte_index as u32)
                    );
                    return false;
                }
            };
            let at = dst.wrapping_add(byte_index as u32);
            if (XIP_BASE..XIP_BASE + XIP_SIZE as u32).contains(&at) {
                touched_flash = true;
                if cpu.write32(HOST_FLASH_ADDR, at - XIP_BASE).is_err()
                    || cpu.write32(HOST_FLASH_PROGRAM, u32::from(byte)).is_err()
                {
                    return false;
                }
            } else if let Err(fault) = cpu.write8(at, byte) {
                eprintln!("DMAC strict ch={ch}: write {at:#010x}: {fault}");
                return false;
            }
        }
        src = match step(src, src_inc, src_width) {
            Some(v) => v,
            None => return false,
        };
        dst = match step(dst, dst_inc, dst_width) {
            Some(v) => v,
            None => return false,
        };
    }

    let bit = 1u32 << ch;
    let raw = cpu.read32(DMAC_RAW_TFR).unwrap_or(0) | bit;
    let status = cpu.read32(DMAC_STATUS_TFR).unwrap_or(0) | bit;
    if cpu.write32(DMAC_RAW_TFR, raw).is_err() || cpu.write32(DMAC_STATUS_TFR, status).is_err() {
        return false;
    }
    if touched_flash && !flash_state::persist(cpu) {
        return false;
    }

    if !SEEN.swap(true, Ordering::Relaxed) {
        eprintln!("DMAC functional synchronous engine enabled");
    }
    eprintln!(
        "DMAC ch={ch} complete items={items} width={src_width} src={sar:#010x} dst={dar:#010x} type={transfer_type}"
    );
    true
}

fn reg(cpu: &mut Processor, ch: u32, off: u32) -> Option<u32> {
    cpu.read32(dmac_channel_reg(ch, off)?).ok()
}

fn width_bytes(code: u32) -> Option<usize> {
    match code {
        0 => Some(1),
        1 => Some(2),
        2 => Some(4),
        3 => Some(8),
        4 => Some(16),
        5 => Some(32),
        _ => None,
    }
}

fn step(addr: u32, mode: u32, width: usize) -> Option<u32> {
    match mode {
        0 => Some(addr.wrapping_add(width as u32)),
        1 => Some(addr.wrapping_sub(width as u32)),
        2 => Some(addr),
        _ => None,
    }
}

fn unsupported(ch: u32, ctl: u32, what: &str) -> bool {
    eprintln!("DMAC strict ch={ch}: unsupported {what} ctl={ctl:#010x}");
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dma_width_encoding_matches_public_sdk() {
        assert_eq!(width_bytes(0), Some(1));
        assert_eq!(width_bytes(1), Some(2));
        assert_eq!(width_bytes(2), Some(4));
        assert_eq!(width_bytes(7), None);
    }

    #[test]
    fn address_increment_modes_match_dma_header() {
        assert_eq!(step(0x100, 0, 4), Some(0x104));
        assert_eq!(step(0x100, 1, 4), Some(0xFC));
        assert_eq!(step(0x100, 2, 4), Some(0x100));
        assert_eq!(step(0x100, 3, 4), None);
    }
}
