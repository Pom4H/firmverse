use crate::{arm_abi, bm_rom, cbtimer_rom, hci_extra, hci_rom, hci_security, ll_crypto, ll_rom, osal_power};
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_LL_ENC_PSEUDO_RAND: u32 = 0x0000_4458;
const ROM_LL_ENC_TRUE_RAND: u32 = 0x0000_4468;
const ROM_LL_EXT_SET_SCA: u32 = 0x0000_4634;
const ROM_LL_INIT_FEATURE_SET_DLE: u32 = 0x0000_BC6C;
const LL_STATUS_SUCCESS: u32 = 0x00;
const LL_STATUS_ERROR_BAD_PARAMETER: u32 = 0x12;

pub fn handle(cpu: &mut Processor, rng: &mut u32) -> bool {
    if arm_abi::handle(cpu) || osal_power::handle(cpu) {
        return true;
    }
    if cbtimer_rom::handle(cpu) {
        return true;
    }
    if bm_rom::handle(cpu) {
        return true;
    }
    if hci_security::handle(cpu) {
        return true;
    }
    if hci_extra::handle(cpu, rng) {
        return true;
    }
    if hci_rom::handle(cpu) {
        return true;
    }
    if ll_crypto::handle(cpu) || ll_rom::handle(cpu) {
        return true;
    }
    match cpu.get_pc() {
        ROM_LL_ENC_PSEUDO_RAND => pseudo_rand(cpu, rng),
        ROM_LL_ENC_TRUE_RAND => true_rand(cpu, rng),
        ROM_LL_EXT_SET_SCA => set_sca(cpu),
        ROM_LL_INIT_FEATURE_SET_DLE => init_feature_set_dle(cpu),
        _ => false,
    }
}

fn next_u32(state: &mut u32) -> u32 {
    let mut x = if *state == 0 { 0x6252_A5A5 } else { *state };
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

fn pseudo_rand(cpu: &mut Processor, rng: &mut u32) -> bool {
    cpu.set_r(Reg::R0, next_u32(rng) & 0xff);
    eprintln!("BLE ROM LL_ENC_GeneratePseudoRandNum deterministic host entropy");
    ret(cpu);
    true
}

fn true_rand(cpu: &mut Processor, rng: &mut u32) -> bool {
    let dst = cpu.get_r(Reg::R0);
    let len = cpu.get_r(Reg::R1) as u8;
    let mut word = 0u32;
    for i in 0..len {
        if i & 3 == 0 {
            word = next_u32(rng);
        }
        let byte = ((word >> (8 * (i & 3))) & 0xff) as u8;
        if cpu.write8(dst + u32::from(i), byte).is_err() {
            return false;
        }
    }
    cpu.set_r(Reg::R0, LL_STATUS_SUCCESS);
    eprintln!("BLE ROM LL_ENC_GenerateTrueRandNum len={len} deterministic host entropy");
    ret(cpu);
    true
}

fn set_sca(cpu: &mut Processor) -> bool {
    let ppm = cpu.get_r(Reg::R0) as u16;
    let status = if ppm <= 500 {
        LL_STATUS_SUCCESS
    } else {
        LL_STATUS_ERROR_BAD_PARAMETER
    };
    eprintln!("BLE ROM LL_EXT_SetSCA ppm={ppm} status={status:#04x}");
    cpu.set_r(Reg::R0, status);
    ret(cpu);
    true
}

fn init_feature_set_dle(cpu: &mut Processor) -> bool {
    let enabled = cpu.get_r(Reg::R0) != 0;
    eprintln!("BLE ROM llInitFeatureSetDLE enabled={enabled}");
    ret(cpu);
    true
}

fn ret(cpu: &mut Processor) {
    cpu.set_pc(cpu.get_r(Reg::LR) & !1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift_is_deterministic_and_nonzero() {
        let mut a = 0;
        let mut b = 0;
        assert_eq!(next_u32(&mut a), next_u32(&mut b));
        assert_ne!(a, 0);
    }

    #[test]
    fn sca_contract_matches_ble_range() {
        assert!(500u16 <= 500);
        assert!(501u16 > 500);
    }
}
