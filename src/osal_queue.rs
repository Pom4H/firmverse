use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

// Thumb entrypoints from the pinned public PHY6252 ROM symbol map.
const ROM_MSG_DEQUEUE: u32 = 0x0001_4D64;
const ROM_MSG_ENQUEUE: u32 = 0x0001_4D90;
const ROM_MSG_ENQUEUE_MAX: u32 = 0x0001_4DC2;
const ROM_MSG_EXTRACT: u32 = 0x0001_4E6C;
const ROM_MSG_PUSH: u32 = 0x0001_4ED0;

// osal_msg_hdr_t is eight bytes on Cortex-M0: next pointer, uint16 len,
// uint8 dest_id and one byte of alignment. Queue links store payload pointers.
const MSG_HDR: u32 = 8;
const TRUE: u32 = 1;
const FALSE: u32 = 0;
const MAX_WALK: usize = 4096;

pub fn handle(cpu: &mut Processor) -> bool {
    match cpu.get_pc() {
        ROM_MSG_DEQUEUE => dequeue_call(cpu),
        ROM_MSG_ENQUEUE => enqueue_call(cpu),
        ROM_MSG_ENQUEUE_MAX => enqueue_max_call(cpu),
        ROM_MSG_EXTRACT => extract_call(cpu),
        ROM_MSG_PUSH => push_call(cpu),
        _ => false,
    }
}

fn header(payload: u32) -> Option<u32> {
    payload.checked_sub(MSG_HDR)
}

fn next(cpu: &mut Processor, payload: u32) -> Option<u32> {
    cpu.read32(header(payload)?).ok()
}

fn set_next(cpu: &mut Processor, payload: u32, value: u32) -> bool {
    let Some(hdr) = header(payload) else {
        return false;
    };
    cpu.write32(hdr, value).is_ok()
}

fn head(cpu: &mut Processor, q_ptr: u32) -> Option<u32> {
    if q_ptr == 0 {
        return None;
    }
    cpu.read32(q_ptr).ok()
}

fn set_head(cpu: &mut Processor, q_ptr: u32, value: u32) -> bool {
    q_ptr != 0 && cpu.write32(q_ptr, value).is_ok()
}

fn enqueue(cpu: &mut Processor, q_ptr: u32, msg: u32) -> bool {
    if q_ptr == 0 || msg < MSG_HDR || !set_next(cpu, msg, 0) {
        return false;
    }
    let Some(first) = head(cpu, q_ptr) else {
        return false;
    };
    if first == 0 {
        return set_head(cpu, q_ptr, msg);
    }
    let mut cur = first;
    for _ in 0..MAX_WALK {
        let Some(n) = next(cpu, cur) else {
            return false;
        };
        if n == 0 {
            return set_next(cpu, cur, msg);
        }
        if n == cur {
            return false;
        }
        cur = n;
    }
    false
}

fn queue_len(cpu: &mut Processor, q_ptr: u32, limit: usize) -> Option<usize> {
    let mut cur = head(cpu, q_ptr)?;
    let mut count = 0usize;
    while cur != 0 {
        count += 1;
        if count >= limit {
            return Some(count);
        }
        let n = next(cpu, cur)?;
        if n == cur {
            return None;
        }
        cur = n;
    }
    Some(count)
}

fn dequeue_call(cpu: &mut Processor) -> bool {
    let q_ptr = cpu.get_r(Reg::R0);
    let Some(first) = head(cpu, q_ptr) else {
        return false;
    };
    if first == 0 {
        cpu.set_r(Reg::R0, 0);
        ret(cpu);
        return true;
    }
    let Some(n) = next(cpu, first) else {
        return false;
    };
    if !set_head(cpu, q_ptr, n) || !set_next(cpu, first, 0) {
        return false;
    }
    cpu.set_r(Reg::R0, first);
    ret(cpu);
    true
}

fn enqueue_call(cpu: &mut Processor) -> bool {
    let q_ptr = cpu.get_r(Reg::R0);
    let msg = cpu.get_r(Reg::R1);
    if !enqueue(cpu, q_ptr, msg) {
        return false;
    }
    ret(cpu);
    true
}

fn enqueue_max_call(cpu: &mut Processor) -> bool {
    let q_ptr = cpu.get_r(Reg::R0);
    let msg = cpu.get_r(Reg::R1);
    let max = cpu.get_r(Reg::R2) as usize;
    let allowed = if max == 0 {
        true
    } else {
        match queue_len(cpu, q_ptr, max) {
            Some(count) => count < max,
            None => return false,
        }
    };
    if allowed && !enqueue(cpu, q_ptr, msg) {
        return false;
    }
    cpu.set_r(Reg::R0, if allowed { TRUE } else { FALSE });
    ret(cpu);
    true
}

fn extract_call(cpu: &mut Processor) -> bool {
    let q_ptr = cpu.get_r(Reg::R0);
    let msg = cpu.get_r(Reg::R1);
    let prev = cpu.get_r(Reg::R2);
    if q_ptr == 0 || msg < MSG_HDR {
        return false;
    }
    let Some(first) = head(cpu, q_ptr) else {
        return false;
    };
    let Some(n) = next(cpu, msg) else {
        return false;
    };
    let ok = if first == msg {
        set_head(cpu, q_ptr, n)
    } else if prev >= MSG_HDR && next(cpu, prev) == Some(msg) {
        set_next(cpu, prev, n)
    } else {
        false
    };
    if !ok || !set_next(cpu, msg, 0) {
        return false;
    }
    ret(cpu);
    true
}

fn push_call(cpu: &mut Processor) -> bool {
    let q_ptr = cpu.get_r(Reg::R0);
    let msg = cpu.get_r(Reg::R1);
    if q_ptr == 0 || msg < MSG_HDR {
        return false;
    }
    let Some(first) = head(cpu, q_ptr) else {
        return false;
    };
    if !set_next(cpu, msg, first) || !set_head(cpu, q_ptr, msg) {
        return false;
    }
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
    fn queue_rom_entries_match_public_map() {
        assert_eq!(ROM_MSG_DEQUEUE, 0x0001_4D64);
        assert_eq!(ROM_MSG_ENQUEUE, 0x0001_4D90);
        assert_eq!(ROM_MSG_ENQUEUE_MAX, 0x0001_4DC2);
        assert_eq!(ROM_MSG_EXTRACT, 0x0001_4E6C);
        assert_eq!(ROM_MSG_PUSH, 0x0001_4ED0);
    }

    #[test]
    fn message_header_matches_arm_osal_layout() {
        assert_eq!(MSG_HDR, 8);
    }
}
