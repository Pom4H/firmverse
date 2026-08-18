use crate::bus::{HOST_FLASH_ADDR, HOST_FLASH_ERASE, HOST_FLASH_PROGRAM, XIP_BASE, XIP_SIZE};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::Processor;

const MAGIC: &[u8; 8] = b"PHY6252F";
const VERSION: u32 = 1;
const HEADER_BYTES: usize = 8 + 4 + 4 + 8;
const FLASH_SECTOR: usize = 4096;

#[derive(Default)]
struct State {
    initialized: bool,
    path: Option<PathBuf>,
    baseline_hash: u64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Restore an explicitly configured raw NOR image before the guest executes its
/// first instruction. The image is accepted only for the exact baseline HEX so
/// a stale snapshot cannot silently replace firmware code.
pub fn ensure_loaded(cpu: &mut Processor) -> bool {
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.initialized {
            return true;
        }
        state.initialized = true;
        let Some(path) = std::env::var_os("PHY6252_FLASH_STATE").map(PathBuf::from) else {
            return true;
        };

        let Some(baseline) = read_xip(cpu) else {
            eprintln!("FLASH state: cannot read baseline XIP");
            return false;
        };
        state.baseline_hash = fingerprint(&baseline);
        state.path = Some(path.clone());

        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("FLASH state: new {}", path.display());
                return true;
            }
            Err(err) => {
                eprintln!("FLASH state: read {}: {err}", path.display());
                return false;
            }
        };
        let Some(snapshot) = decode(&bytes, state.baseline_hash) else {
            eprintln!("FLASH state: ignored incompatible {}", path.display());
            return true;
        };
        if !restore_xip(cpu, snapshot) {
            eprintln!("FLASH state: restore failed {}", path.display());
            return false;
        }
        eprintln!(
            "FLASH state: restored {} bytes from {}",
            XIP_SIZE,
            path.display()
        );
        true
    })
}

/// Persist the complete physical NOR image after a confirmed program/erase
/// transaction. This deliberately sits below SNV/filesystem formats.
pub fn persist(cpu: &mut Processor) -> bool {
    if !ensure_loaded(cpu) {
        return false;
    }
    STATE.with(|slot| {
        let state = slot.borrow();
        let Some(path) = state.path.as_deref() else {
            return true;
        };
        let Some(snapshot) = read_xip(cpu) else {
            eprintln!("FLASH state: cannot snapshot XIP");
            return false;
        };
        let bytes = encode(&snapshot, state.baseline_hash);
        match atomic_write(path, &bytes) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("FLASH state: write {}: {err}", path.display());
                false
            }
        }
    })
}

fn read_xip(cpu: &mut Processor) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(XIP_SIZE);
    for offset in 0..XIP_SIZE {
        out.push(cpu.read8(XIP_BASE + offset as u32).ok()?);
    }
    Some(out)
}

fn restore_xip(cpu: &mut Processor, snapshot: &[u8]) -> bool {
    if snapshot.len() != XIP_SIZE {
        return false;
    }
    for offset in (0..XIP_SIZE).step_by(FLASH_SECTOR) {
        if cpu.write32(HOST_FLASH_ADDR, offset as u32).is_err()
            || cpu.write32(HOST_FLASH_ERASE, 1).is_err()
        {
            return false;
        }
    }
    if cpu.write32(HOST_FLASH_ADDR, 0).is_err() {
        return false;
    }
    for byte in snapshot.iter().copied() {
        if cpu.write32(HOST_FLASH_PROGRAM, u32::from(byte)).is_err() {
            return false;
        }
    }
    true
}

fn encode(snapshot: &[u8], baseline_hash: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_BYTES + snapshot.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(snapshot.len() as u32).to_le_bytes());
    out.extend_from_slice(&baseline_hash.to_le_bytes());
    out.extend_from_slice(snapshot);
    out
}

fn decode(bytes: &[u8], baseline_hash: u64) -> Option<&[u8]> {
    if bytes.len() != HEADER_BYTES + XIP_SIZE || &bytes[..8] != MAGIC {
        return None;
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let size = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    let hash = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
    if version != VERSION || size != XIP_SIZE || hash != baseline_hash {
        return None;
    }
    Some(&bytes[HEADER_BYTES..])
}

fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrip_requires_matching_firmware_hash() {
        let flash = vec![0xa5; XIP_SIZE];
        let hash = fingerprint(&vec![0xff; XIP_SIZE]);
        let encoded = encode(&flash, hash);
        assert_eq!(decode(&encoded, hash), Some(flash.as_slice()));
        assert!(decode(&encoded, hash ^ 1).is_none());
    }

    #[test]
    fn state_header_is_small_and_versioned() {
        assert_eq!(HEADER_BYTES, 24);
        assert_eq!(MAGIC, b"PHY6252F");
        assert_eq!(VERSION, 1);
    }
}
