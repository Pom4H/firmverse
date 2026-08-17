//! Radio mailbox in Cortex-M SRAM at `0x20000000` (zmu's RAM window).

use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::fault::Fault;
use zmu_cortex_m::Processor;

pub const BASE: u32 = 0x2000_0000;
pub const MAGIC_VALUE: u32 = 0x5048_5932; // "PHY2"
pub const MAGIC: u32 = 0;
pub const STATUS: u32 = 4;
pub const RX_SEQ: u32 = 8;
pub const RX_LEN: u32 = 12;
pub const RX_BYTES: u32 = 16;
pub const TX_SEQ: u32 = 272;
pub const TX_LEN: u32 = 276;
pub const TX_BYTES: u32 = 280;
pub const TICK_MS: u32 = 536;
pub const PAYLOAD: usize = 256;
pub const STATUS_CONNECTED: u32 = 1;
pub const STATUS_NOTIFY: u32 = 2;

fn write_u32(processor: &mut Processor, off: u32, value: u32) -> Result<(), Fault> {
    processor.write32(BASE + off, value)
}

fn read_u32(processor: &mut Processor, off: u32) -> Result<u32, Fault> {
    processor.read32(BASE + off)
}

pub fn plant_magic(processor: &mut Processor) -> Result<(), Fault> {
    write_u32(processor, STATUS, 0)?;
    write_u32(processor, RX_SEQ, 0)?;
    write_u32(processor, TX_SEQ, 0)?;
    write_u32(processor, TICK_MS, 0)?;
    write_u32(processor, MAGIC, MAGIC_VALUE)
}

pub fn set_tick(processor: &mut Processor, ms: u32) -> Result<(), Fault> {
    write_u32(processor, TICK_MS, ms)
}

pub fn connect(processor: &mut Processor, on: bool) -> Result<(), Fault> {
    let mut status = read_u32(processor, STATUS)?;
    if on {
        status |= STATUS_CONNECTED;
    } else {
        status &= !(STATUS_CONNECTED | STATUS_NOTIFY);
    }
    write_u32(processor, STATUS, status)
}

pub fn cccd(processor: &mut Processor, enable: bool) -> Result<(), Fault> {
    let mut status = read_u32(processor, STATUS)?;
    if enable {
        status |= STATUS_NOTIFY;
    } else {
        status &= !STATUS_NOTIFY;
    }
    write_u32(processor, STATUS, status)
}

pub fn status(processor: &mut Processor) -> Result<u32, Fault> {
    read_u32(processor, STATUS)
}

pub fn write_rx(processor: &mut Processor, payload: &[u8]) -> Result<(), Fault> {
    let n = payload.len().min(PAYLOAD);
    write_u32(processor, RX_LEN, n as u32)?;
    for (i, byte) in payload.iter().take(n).enumerate() {
        processor.write8(BASE + RX_BYTES + i as u32, *byte)?;
    }
    let seq = read_u32(processor, RX_SEQ)?.wrapping_add(1);
    write_u32(processor, RX_SEQ, seq)
}

pub fn emit_tx(processor: &mut Processor, payload: &[u8]) -> Result<(), Fault> {
    let n = payload.len().min(PAYLOAD);
    write_u32(processor, TX_LEN, n as u32)?;
    for (i, byte) in payload.iter().take(n).enumerate() {
        processor.write8(BASE + TX_BYTES + i as u32, *byte)?;
    }
    let seq = read_u32(processor, TX_SEQ)?.wrapping_add(1);
    write_u32(processor, TX_SEQ, seq)
}

pub fn take_tx(processor: &mut Processor, last_seq: &mut u32) -> Result<Option<Vec<u8>>, Fault> {
    let seq = read_u32(processor, TX_SEQ)?;
    if seq == *last_seq {
        return Ok(None);
    }
    *last_seq = seq;
    let n = read_u32(processor, TX_LEN)? as usize;
    if n == 0 || n > PAYLOAD {
        return Ok(Some(Vec::new()));
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(processor.read8(BASE + TX_BYTES + i as u32)?);
    }
    Ok(Some(out))
}
