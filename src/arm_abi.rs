use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const AEABI_MEMCPY: u32 = 0x0000_0E80;
const AEABI_MEMSET: u32 = 0x0000_0EA4;
const AEABI_MEMCLR: u32 = 0x0000_0EB2;
const C_MEMSET: u32 = 0x0000_0EB6;
const C_STRLEN: u32 = 0x0000_0EC8;
const C_STRCMP: u32 = 0x0000_0ED6;
const C_MEMCMP: u32 = 0x0000_0EF2;
const C_STRNCMP: u32 = 0x0000_0F0C;
const AEABI_UREAD4: u32 = 0x0000_0F74;

pub fn handle(cpu: &mut Processor) -> bool {
    match cpu.get_pc() {
        AEABI_MEMCPY => memcpy(cpu),
        AEABI_MEMSET => aeabi_memset(cpu),
        AEABI_MEMCLR => memclr(cpu),
        C_MEMSET => c_memset(cpu),
        C_STRLEN => strlen(cpu),
        C_STRCMP => strcmp(cpu, None),
        C_MEMCMP => memcmp(cpu),
        C_STRNCMP => strcmp(cpu, Some(cpu.get_r(Reg::R2))),
        AEABI_UREAD4 => uread4(cpu),
        _ => false,
    }
}

fn copy_bytes(cpu: &mut Processor, dst: u32, src: u32, len: u32) -> bool {
    let mut bytes = Vec::with_capacity(len as usize);
    for i in 0..len {
        let Ok(byte) = cpu.read8(src.wrapping_add(i)) else { return false; };
        bytes.push(byte);
    }
    for (i, byte) in bytes.into_iter().enumerate() {
        if cpu.write8(dst.wrapping_add(i as u32), byte).is_err() { return false; }
    }
    true
}

fn fill(cpu: &mut Processor, dst: u32, len: u32, value: u8) -> bool {
    for i in 0..len {
        if cpu.write8(dst.wrapping_add(i), value).is_err() { return false; }
    }
    true
}

fn memcpy(cpu: &mut Processor) -> bool {
    let dst = cpu.get_r(Reg::R0);
    if !copy_bytes(cpu, dst, cpu.get_r(Reg::R1), cpu.get_r(Reg::R2)) { return false; }
    cpu.set_r(Reg::R0, dst);
    ret(cpu);
    true
}

fn aeabi_memset(cpu: &mut Processor) -> bool {
    let dst = cpu.get_r(Reg::R0);
    if !fill(cpu, dst, cpu.get_r(Reg::R1), cpu.get_r(Reg::R2) as u8) { return false; }
    cpu.set_r(Reg::R0, dst);
    ret(cpu);
    true
}

fn memclr(cpu: &mut Processor) -> bool {
    let dst = cpu.get_r(Reg::R0);
    if !fill(cpu, dst, cpu.get_r(Reg::R1), 0) { return false; }
    cpu.set_r(Reg::R0, dst);
    ret(cpu);
    true
}

fn c_memset(cpu: &mut Processor) -> bool {
    let dst = cpu.get_r(Reg::R0);
    if !fill(cpu, dst, cpu.get_r(Reg::R2), cpu.get_r(Reg::R1) as u8) { return false; }
    cpu.set_r(Reg::R0, dst);
    ret(cpu);
    true
}

fn strlen(cpu: &mut Processor) -> bool {
    let ptr = cpu.get_r(Reg::R0);
    let mut len = 0u32;
    loop {
        match cpu.read8(ptr.wrapping_add(len)) {
            Ok(0) => break,
            Ok(_) if len < 0x0010_0000 => len += 1,
            _ => return false,
        }
    }
    cpu.set_r(Reg::R0, len);
    ret(cpu);
    true
}

fn strcmp(cpu: &mut Processor, limit: Option<u32>) -> bool {
    let a = cpu.get_r(Reg::R0);
    let b = cpu.get_r(Reg::R1);
    let mut i = 0u32;
    loop {
        if limit.is_some_and(|n| i >= n) {
            cpu.set_r(Reg::R0, 0);
            ret(cpu);
            return true;
        }
        let av = match cpu.read8(a.wrapping_add(i)) { Ok(v) => v, Err(_) => return false };
        let bv = match cpu.read8(b.wrapping_add(i)) { Ok(v) => v, Err(_) => return false };
        if av != bv || av == 0 {
            cpu.set_r(Reg::R0, (av as i32 - bv as i32) as u32);
            ret(cpu);
            return true;
        }
        i = i.wrapping_add(1);
    }
}

fn memcmp(cpu: &mut Processor) -> bool {
    let a = cpu.get_r(Reg::R0);
    let b = cpu.get_r(Reg::R1);
    let len = cpu.get_r(Reg::R2);
    for i in 0..len {
        let av = match cpu.read8(a.wrapping_add(i)) { Ok(v) => v, Err(_) => return false };
        let bv = match cpu.read8(b.wrapping_add(i)) { Ok(v) => v, Err(_) => return false };
        if av != bv {
            cpu.set_r(Reg::R0, (av as i32 - bv as i32) as u32);
            ret(cpu);
            return true;
        }
    }
    cpu.set_r(Reg::R0, 0);
    ret(cpu);
    true
}

fn uread4(cpu: &mut Processor) -> bool {
    let ptr = cpu.get_r(Reg::R0);
    let mut value = 0u32;
    for i in 0..4u32 {
        let byte = match cpu.read8(ptr.wrapping_add(i)) { Ok(v) => v, Err(_) => return false };
        value |= u32::from(byte) << (8 * i);
    }
    cpu.set_r(Reg::R0, value);
    ret(cpu);
    true
}

fn ret(cpu: &mut Processor) {
    cpu.set_pc(cpu.get_r(Reg::LR) & !1);
}

#[cfg(test)]
mod tests {
    #[test]
    fn byte_order_for_uread4_is_little_endian() {
        assert_eq!(u32::from_le_bytes([1, 2, 3, 4]), 0x0403_0201);
    }
}
