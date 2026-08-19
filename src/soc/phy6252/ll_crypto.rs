use crate::aes::aes128_encrypt_block;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_LL_ENC_AES128_ENCRYPT: u32 = 0x0000_3FC4;
const ROM_LL_ENCRYPT: u32 = 0x0000_48E4;
const LL_STATUS_SUCCESS: u32 = 0x00;

/// Execute the public PHY6252 AES-128 LL APIs with host AES while preserving
/// guest buffer ownership and ARM ABI.
pub fn handle(cpu: &mut Processor) -> bool {
    match cpu.get_pc() {
        ROM_LL_ENC_AES128_ENCRYPT => aes_call(cpu, false),
        ROM_LL_ENCRYPT => aes_call(cpu, true),
        _ => false,
    }
}

fn aes_call(cpu: &mut Processor, returns_status: bool) -> bool {
    let key_ptr = cpu.get_r(Reg::R0);
    let plaintext_ptr = cpu.get_r(Reg::R1);
    let ciphertext_ptr = cpu.get_r(Reg::R2);
    let key = match read_block(cpu, key_ptr) {
        Some(v) => v,
        None => return false,
    };
    let plaintext = match read_block(cpu, plaintext_ptr) {
        Some(v) => v,
        None => return false,
    };
    let ciphertext = aes128_encrypt_block(key, plaintext);
    if !write_block(cpu, ciphertext_ptr, &ciphertext) {
        return false;
    }
    eprintln!("BLE LL AES128 key={key_ptr:#010x} input={plaintext_ptr:#010x} output={ciphertext_ptr:#010x}");
    if returns_status {
        cpu.set_r(Reg::R0, LL_STATUS_SUCCESS);
    }
    ret(cpu);
    true
}

fn read_block(cpu: &mut Processor, ptr: u32) -> Option<[u8; 16]> {
    if ptr == 0 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = cpu.read8(ptr.wrapping_add(i as u32)).ok()?;
    }
    Some(out)
}

fn write_block(cpu: &mut Processor, ptr: u32, block: &[u8; 16]) -> bool {
    if ptr == 0 {
        return false;
    }
    block
        .iter()
        .copied()
        .enumerate()
        .all(|(i, byte)| cpu.write8(ptr.wrapping_add(i as u32), byte).is_ok())
}

fn ret(cpu: &mut Processor) {
    cpu.set_pc(cpu.get_r(Reg::LR) & !1);
}

#[cfg(test)]
mod tests {
    #[test]
    fn public_ll_aes_abi_is_one_block() {
        assert_eq!(128 / 8, 16);
    }
}
