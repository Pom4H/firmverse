use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_LL_ENC_PSEUDO_RAND: u32 = 0x0000_4458;
const ROM_LL_ENC_TRUE_RAND: u32 = 0x0000_4468;

pub fn handle(cpu: &mut Processor, rng: &mut u32) -> bool {
    match cpu.get_pc() {
        ROM_LL_ENC_PSEUDO_RAND => pseudo_rand(cpu, rng),
        ROM_LL_ENC_TRUE_RAND => true_rand(cpu, rng),
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
    // LL status codes map success to zero. This is deterministic test entropy,
    // deliberately not a cryptographic RNG supplied to production firmware.
    cpu.set_r(Reg::R0, 0);
    eprintln!("BLE ROM LL_ENC_GenerateTrueRandNum len={len} deterministic host entropy");
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
}
