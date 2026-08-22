//! PHY62x2 UART bootloader image layout (pvvx `rdwr_phy62x2` compatible).
#![allow(clippy::chunks_exact_to_as_chunks)]

const SRAM_WINDOW: u32 = 0x1FFF_0000;
const FLASH_WINDOW: u32 = 0x1100_0000;
const MAX_FLASH: u32 = 0x20_0000;
const HEADER_FLASH_OFF: u32 = 0x2000;
const SRAM_STORE_OFF: u32 = 0x5000;
const HEADER_PREFIX: usize = 0x100;
const HEADER_DESCRIPTOR_BYTES: usize = 16;
const HEX_RECORD_BYTES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub load_addr: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashPart {
    pub flash_off: u32,
    pub load_addr: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashImage {
    pub start: u32,
    pub parts: Vec<FlashPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexFile {
    pub segments: Vec<Segment>,
    pub entry: Option<u32>,
}

pub fn parse_intel_hex(text: &str) -> Result<HexFile, String> {
    let mut bytes: Vec<(u32, u8)> = Vec::new();
    let mut ext = 0u32;
    let mut entry = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with(':') {
            return Err(format!("not an Intel HEX line: {line}"));
        }
        let data = decode_hex_line(line)?;
        let count = data[0] as usize;
        let addr = u16::from_be_bytes([data[1], data[2]]) as u32;
        let kind = data[3];
        match kind {
            0 => {
                let base = ext.wrapping_add(addr);
                for (i, value) in data[4..4 + count].iter().enumerate() {
                    bytes.push((base + i as u32, *value));
                }
            }
            1 => break,
            2 => ext = u32::from(u16::from_be_bytes([data[4], data[5]])) << 4,
            4 => ext = u32::from(u16::from_be_bytes([data[4], data[5]])) << 16,
            3 => {
                if count >= 4 {
                    entry = Some(
                        (u32::from(u16::from_be_bytes([data[4], data[5]])) << 4)
                            .wrapping_add(u32::from(u16::from_be_bytes([data[6], data[7]]))),
                    );
                }
            }
            5 => {
                if count >= 4 {
                    entry = Some(u32::from_be_bytes([data[4], data[5], data[6], data[7]]));
                }
            }
            other => return Err(format!("unsupported HEX record {other}")),
        }
    }
    Ok(HexFile {
        segments: coalesce(bytes),
        entry,
    })
}

fn decode_hex_line(line: &str) -> Result<Vec<u8>, String> {
    if line.len() < 11 || !(line.len() - 1).is_multiple_of(2) {
        return Err("truncated HEX line".into());
    }
    let mut out = Vec::with_capacity((line.len() - 1) / 2);
    let chars = &line.as_bytes()[1..];
    for chunk in chars.chunks_exact(2) {
        let hi = hex_digit(chunk[0]).ok_or("invalid HEX")?;
        let lo = hex_digit(chunk[1]).ok_or("invalid HEX")?;
        out.push((hi << 4) | lo);
    }
    let sum: u8 = out.iter().fold(0u8, |a, b| a.wrapping_add(*b));
    if sum != 0 {
        return Err("HEX checksum mismatch".into());
    }
    Ok(out)
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn coalesce(mut bytes: Vec<(u32, u8)>) -> Vec<Segment> {
    bytes.sort_by_key(|(addr, _)| *addr);
    let mut segs: Vec<Segment> = Vec::new();
    for (addr, value) in bytes {
        if let Some(last) = segs.last_mut() {
            let end = last.load_addr + last.data.len() as u32;
            if addr == end {
                last.data.push(value);
                continue;
            }
            if addr < end {
                let off = (addr - last.load_addr) as usize;
                last.data[off] = value;
                continue;
            }
        }
        segs.push(Segment {
            load_addr: addr,
            data: vec![value],
        });
    }
    segs
}

fn is_sram(addr: u32) -> bool {
    addr & SRAM_WINDOW == SRAM_WINDOW
}

fn flash_offset(addr: u32) -> Option<u32> {
    if addr & !(MAX_FLASH - 1) == FLASH_WINDOW {
        Some(addr & (MAX_FLASH - 1))
    } else {
        None
    }
}

pub fn build_flash_image(segments: &[Segment], start: Option<u32>) -> Result<FlashImage, String> {
    let payload: Vec<&Segment> = segments.iter().filter(|s| !s.data.is_empty()).collect();
    if payload.is_empty() {
        return Err("HEX image has no loadable bytes".into());
    }

    let start = start.unwrap_or_else(|| infer_start(&payload));
    let mut flash_min = MAX_FLASH - 1;
    let mut flash_max = 0u32;
    let mut sram_bytes = 0u32;
    for seg in &payload {
        if is_sram(seg.load_addr) {
            sram_bytes += seg.data.len() as u32;
        } else if let Some(off) = flash_offset(seg.load_addr) {
            flash_min = flash_min.min(off);
            flash_max = flash_max.max(off + seg.data.len() as u32);
        } else {
            return Err(format!("unsupported load address {:#010x}", seg.load_addr));
        }
    }

    let mut store = SRAM_STORE_OFF;
    if store + sram_bytes >= flash_min {
        store = (flash_max + 3) & !3;
    }

    let mut header = vec![0xFFu8; HEADER_PREFIX];
    header[0..4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    header[8..12].copy_from_slice(&start.to_le_bytes());

    let mut parts = Vec::new();
    for seg in payload {
        let flash_off = if is_sram(seg.load_addr) {
            let off = store;
            store += (seg.data.len() as u32 + 3) & !3;
            off
        } else {
            flash_offset(seg.load_addr)
                .ok_or_else(|| format!("unsupported load address {:#010x}", seg.load_addr))?
        };
        header.extend_from_slice(&flash_off.to_le_bytes());
        header.extend_from_slice(&(seg.data.len() as u32).to_le_bytes());
        header.extend_from_slice(&seg.load_addr.to_le_bytes());
        header.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        parts.push(FlashPart {
            flash_off,
            load_addr: seg.load_addr,
            data: seg.data.clone(),
        });
    }

    let mut out = vec![FlashPart {
        flash_off: HEADER_FLASH_OFF,
        load_addr: 0,
        data: header,
    }];
    out.extend(parts);
    Ok(FlashImage { start, parts: out })
}

/// Reconstruct the image that the PHY62xx boot path would load using only the
/// bytes currently stored in external NOR. This deliberately does not accept a
/// `FlashImage`: callers can prove that erase/program/checksum/reset produced a
/// self-contained bootable image rather than accidentally reusing the source
/// Intel HEX.
pub fn programmed_flash_to_hex(flash: &[u8]) -> Result<HexFile, String> {
    let header_off = HEADER_FLASH_OFF as usize;
    let prefix_end = header_off
        .checked_add(HEADER_PREFIX)
        .ok_or("boot header range overflow")?;
    if prefix_end > flash.len() {
        return Err("NOR is too small to contain PHY62xx boot header".into());
    }

    let header = &flash[header_off..];
    let count = read_u32_le(header, 0)? as usize;
    if count == 0 {
        return Err("PHY62xx boot header contains zero segments".into());
    }
    if count > 128 {
        return Err(format!("unreasonable PHY62xx boot segment count: {count}"));
    }
    let start = read_u32_le(header, 8)?;
    let descriptor_end = HEADER_PREFIX
        .checked_add(
            count
                .checked_mul(HEADER_DESCRIPTOR_BYTES)
                .ok_or("boot descriptor count overflow")?,
        )
        .ok_or("boot descriptor range overflow")?;
    if descriptor_end > header.len() {
        return Err("truncated PHY62xx boot descriptors".into());
    }

    let mut segments = Vec::with_capacity(count);
    for index in 0..count {
        let base = HEADER_PREFIX + index * HEADER_DESCRIPTOR_BYTES;
        let flash_off = read_u32_le(header, base)?;
        let len = read_u32_le(header, base + 4)? as usize;
        let load_addr = read_u32_le(header, base + 8)?;
        if len == 0 {
            return Err(format!("PHY62xx boot segment {index} has zero length"));
        }
        if !is_sram(load_addr) && flash_offset(load_addr).is_none() {
            return Err(format!(
                "PHY62xx boot segment {index} has unsupported load address {load_addr:#010x}"
            ));
        }
        let data_start = flash_off as usize;
        let data_end = data_start
            .checked_add(len)
            .ok_or_else(|| format!("PHY62xx boot segment {index} range overflow"))?;
        if data_end > flash.len() {
            return Err(format!(
                "PHY62xx boot segment {index} points outside NOR: {data_start:#x}..{data_end:#x}"
            ));
        }
        segments.push(Segment {
            load_addr,
            data: flash[data_start..data_end].to_vec(),
        });
    }

    Ok(HexFile {
        segments,
        entry: Some(start),
    })
}

/// Encode a logical firmware image as canonical Intel HEX. This is primarily
/// used to hand the image reconstructed from programmed NOR to the normal
/// Firmverse execution path, preserving one emulator loader instead of adding a
/// second test-only boot mechanism.
pub fn encode_intel_hex(image: &HexFile) -> Result<String, String> {
    if image.segments.is_empty() {
        return Err("cannot encode empty Intel HEX image".into());
    }
    let mut out = String::new();
    let mut current_upper = None;

    for segment in &image.segments {
        if segment.data.is_empty() {
            continue;
        }
        let mut offset = 0usize;
        while offset < segment.data.len() {
            let addr = segment
                .load_addr
                .checked_add(offset as u32)
                .ok_or("Intel HEX address overflow")?;
            let upper = (addr >> 16) as u16;
            if current_upper != Some(upper) {
                push_hex_record(&mut out, 0, 4, &upper.to_be_bytes())?;
                current_upper = Some(upper);
            }

            let low = (addr & 0xFFFF) as u16;
            let until_boundary = 0x1_0000usize - low as usize;
            let len = HEX_RECORD_BYTES
                .min(segment.data.len() - offset)
                .min(until_boundary);
            push_hex_record(&mut out, low, 0, &segment.data[offset..offset + len])?;
            offset += len;
        }
    }

    if let Some(entry) = image.entry {
        push_hex_record(&mut out, 0, 5, &entry.to_be_bytes())?;
    }
    push_hex_record(&mut out, 0, 1, &[])?;
    Ok(out)
}

fn read_u32_le(bytes: &[u8], off: usize) -> Result<u32, String> {
    let end = off.checked_add(4).ok_or("u32 range overflow")?;
    let value = bytes
        .get(off..end)
        .ok_or("truncated PHY62xx boot header field")?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

fn push_hex_record(out: &mut String, addr: u16, kind: u8, data: &[u8]) -> Result<(), String> {
    let len = u8::try_from(data.len()).map_err(|_| "Intel HEX record too large")?;
    let [addr_hi, addr_lo] = addr.to_be_bytes();
    let mut sum = len
        .wrapping_add(addr_hi)
        .wrapping_add(addr_lo)
        .wrapping_add(kind);
    out.push(':');
    push_hex_byte(out, len);
    push_hex_byte(out, addr_hi);
    push_hex_byte(out, addr_lo);
    push_hex_byte(out, kind);
    for byte in data {
        sum = sum.wrapping_add(*byte);
        push_hex_byte(out, *byte);
    }
    push_hex_byte(out, 0u8.wrapping_sub(sum));
    out.push('\n');
    Ok(())
}

fn push_hex_byte(out: &mut String, value: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(HEX[(value >> 4) as usize] as char);
    out.push(HEX[(value & 0x0F) as usize] as char);
}

fn infer_start(segments: &[&Segment]) -> u32 {
    segments
        .iter()
        .filter(|s| is_sram(s.load_addr))
        .map(|s| s.load_addr)
        .min()
        .unwrap_or(0x1FFF_1838)
}

pub fn pad_cpbin_chunk(data: &[u8]) -> Vec<u8> {
    const BLK: usize = 0x2000;
    if data.len() > 0x1000 && data.len() < BLK {
        let mut out = data.to_vec();
        out.resize(BLK, 0xFF);
        out
    } else {
        data.to_vec()
    }
}

pub fn chunk_is_erased(data: &[u8]) -> bool {
    data.iter().all(|b| *b == 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sram_hex_gets_boot_header_and_payload_slot() {
        let hex = "\
:020000041FFFDC
:080000001000FF1F0500FF1FA7
:00000001FF
";
        let parsed = parse_intel_hex(hex).unwrap();
        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].load_addr, 0x1FFF_0000);
        let image = build_flash_image(&parsed.segments, parsed.entry).unwrap();
        assert_eq!(image.start, 0x1FFF_0000);
        assert_eq!(image.parts[0].flash_off, 0x2000);
        assert_eq!(&image.parts[0].data[0..4], &1u32.to_le_bytes());
        assert_eq!(&image.parts[0].data[8..12], &0x1FFF_0000u32.to_le_bytes());
        assert_eq!(image.parts[1].flash_off, 0x5000);
        assert_eq!(image.parts[1].load_addr, 0x1FFF_0000);
        assert_eq!(image.parts[1].data.len(), 8);
    }

    #[test]
    fn cpbin_tail_padding() {
        let small = vec![1u8; 0x1001];
        let padded = pad_cpbin_chunk(&small);
        assert_eq!(padded.len(), 0x2000);
        assert_eq!(padded[0x1001], 0xFF);
        assert!(chunk_is_erased(&[0xFF; 16]));
        assert!(!chunk_is_erased(&[0xFF, 0x00]));
    }

    #[test]
    fn xip_hex_uses_start_linear_address() {
        let hex = "\
:020000041102E7
:10000000B0B9FF1F09000211024981F3088808F006
:0400000511020009DB
:00000001FF
";
        let parsed = parse_intel_hex(hex).unwrap();
        assert_eq!(parsed.entry, Some(0x1102_0009));
        assert_eq!(parsed.segments[0].load_addr, 0x1102_0000);
        let image = build_flash_image(&parsed.segments, parsed.entry).unwrap();
        assert_eq!(image.start, 0x1102_0009);
        assert_eq!(image.parts[1].flash_off, 0x0002_0000);
    }

    #[test]
    fn programmed_nor_reconstructs_boot_image_without_source_hex() {
        let source = HexFile {
            segments: vec![
                Segment {
                    load_addr: 0x1102_0000,
                    data: (0u8..64).collect(),
                },
                Segment {
                    load_addr: 0x1FFF_0000,
                    data: vec![0x10, 0x00, 0xFF, 0x1F, 0xE1, 0x19, 0xFF, 0x1F],
                },
            ],
            entry: Some(0x1FFF_19E1),
        };
        let plan = build_flash_image(&source.segments, source.entry).unwrap();
        let mut nor = vec![0xFF; 256 * 1024];
        for part in &plan.parts {
            let start = part.flash_off as usize;
            let end = start + part.data.len();
            nor[start..end].copy_from_slice(&part.data);
        }

        let reconstructed = programmed_flash_to_hex(&nor).unwrap();
        assert_eq!(reconstructed, source);

        let encoded = encode_intel_hex(&reconstructed).unwrap();
        assert_eq!(parse_intel_hex(&encoded).unwrap(), source);
    }

    #[test]
    fn programmed_nor_rejects_descriptor_outside_flash() {
        let source = HexFile {
            segments: vec![Segment {
                load_addr: 0x1FFF_0000,
                data: vec![1, 2, 3, 4],
            }],
            entry: Some(0x1FFF_0001),
        };
        let plan = build_flash_image(&source.segments, source.entry).unwrap();
        let mut nor = vec![0xFF; 0x8000];
        for part in &plan.parts {
            let start = part.flash_off as usize;
            let end = start + part.data.len();
            nor[start..end].copy_from_slice(&part.data);
        }
        let descriptor = HEADER_FLASH_OFF as usize + HEADER_PREFIX;
        nor[descriptor..descriptor + 4].copy_from_slice(&0xFFFF_F000u32.to_le_bytes());
        let error = programmed_flash_to_hex(&nor).unwrap_err();
        assert!(error.contains("outside NOR"));
    }
}