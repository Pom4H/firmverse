use std::cell::RefCell;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_CB_TIMER_START: u32 = 0x0001_46A8;
const ROM_CB_TIMER_STOP: u32 = 0x0001_4710;
const ROM_CB_TIMER_UPDATE: u32 = 0x0001_4750;
const IDLE_BX_LR_ROM: u32 = 0x0000_A9C8;
const CONT_TRAP: u32 = 0x0000_00D0;
const CONT_MAGIC: u32 = 0x4342_544D; // "CBTM"

const SUCCESS: u32 = 0;
const FAILURE: u32 = 1;
const INVALID_TIMER_ID: u8 = 0xFF;
const MAX_TIMERS: usize = 16;

#[derive(Clone, Copy, Debug)]
struct CbTimer {
    id: u8,
    callback: u32,
    data: u32,
    deadline: u32,
}

thread_local! {
    static TIMERS: RefCell<Vec<CbTimer>> = const { RefCell::new(Vec::new()) };
}

/// Execute the PHY6252 OSAL callback-timer ABI. Deadlines are host-side, but
/// the callback itself executes as real guest Thumb code when the cooperative
/// scheduler is idle.
pub fn handle(cpu: &mut Processor) -> bool {
    if cpu.get_pc() == CONT_TRAP && cpu.get_r(Reg::R3) == CONT_MAGIC {
        cpu.set_r(Reg::R3, 0);
        cpu.set_pc(IDLE_BX_LR_ROM);
        return true;
    }

    match cpu.get_pc() {
        ROM_CB_TIMER_START => start(cpu),
        ROM_CB_TIMER_STOP => stop(cpu),
        ROM_CB_TIMER_UPDATE => update(cpu),
        IDLE_BX_LR_ROM => dispatch_due(cpu),
        _ => false,
    }
}

fn start(cpu: &mut Processor) -> bool {
    let callback = cpu.get_r(Reg::R0);
    let data = cpu.get_r(Reg::R1);
    let timeout = cpu.get_r(Reg::R2);
    let id_ptr = cpu.get_r(Reg::R3);
    if callback & 1 == 0 || id_ptr == 0 {
        cpu.set_r(Reg::R0, FAILURE);
        ret(cpu);
        return true;
    }

    let id = TIMERS.with(|timers| {
        let timers = timers.borrow();
        (0..MAX_TIMERS)
            .map(|candidate| candidate as u8)
            .find(|candidate| !timers.iter().any(|timer| timer.id == *candidate))
    });
    let Some(id) = id else {
        let _ = cpu.write8(id_ptr, INVALID_TIMER_ID);
        cpu.set_r(Reg::R0, FAILURE);
        ret(cpu);
        return true;
    };
    if cpu.write8(id_ptr, id).is_err() {
        return false;
    }
    let deadline = now_ms(cpu).wrapping_add(timeout);
    TIMERS.with(|timers| timers.borrow_mut().push(CbTimer { id, callback, data, deadline }));
    eprintln!("OSAL callback timer start id={id} timeout_ms={timeout} callback={callback:#010x}");
    cpu.set_r(Reg::R0, SUCCESS);
    ret(cpu);
    true
}

fn stop(cpu: &mut Processor) -> bool {
    let id = cpu.get_r(Reg::R0) as u8;
    let found = TIMERS.with(|timers| {
        let mut timers = timers.borrow_mut();
        let before = timers.len();
        timers.retain(|timer| timer.id != id);
        timers.len() != before
    });
    cpu.set_r(Reg::R0, if found { SUCCESS } else { FAILURE });
    ret(cpu);
    true
}

fn update(cpu: &mut Processor) -> bool {
    let id = cpu.get_r(Reg::R0) as u8;
    let timeout = cpu.get_r(Reg::R1);
    let deadline = now_ms(cpu).wrapping_add(timeout);
    let found = TIMERS.with(|timers| {
        let mut timers = timers.borrow_mut();
        if let Some(timer) = timers.iter_mut().find(|timer| timer.id == id) {
            timer.deadline = deadline;
            true
        } else {
            false
        }
    });
    cpu.set_r(Reg::R0, if found { SUCCESS } else { FAILURE });
    ret(cpu);
    true
}

fn dispatch_due(cpu: &mut Processor) -> bool {
    let now = now_ms(cpu);
    let due = TIMERS.with(|timers| {
        let mut timers = timers.borrow_mut();
        let pos = timers.iter().position(|timer| reached(now, timer.deadline))?;
        Some(timers.remove(pos))
    });
    let Some(timer) = due else { return false; };
    eprintln!("OSAL callback timer fire id={} callback={:#010x}", timer.id, timer.callback);
    cpu.set_r(Reg::R0, timer.data);
    cpu.set_r(Reg::R3, CONT_MAGIC);
    cpu.set_r(Reg::LR, CONT_TRAP | 1);
    cpu.set_pc(timer.callback & !1);
    true
}

fn now_ms(cpu: &Processor) -> u32 {
    (cpu.cycle_count / 16_000) as u32
}

fn reached(now: u32, deadline: u32) -> bool {
    now.wrapping_sub(deadline) < 0x8000_0000
}

fn ret(cpu: &mut Processor) {
    cpu.set_pc(cpu.get_r(Reg::LR) & !1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_ids_fit_public_invalid_sentinel() {
        assert!(MAX_TIMERS < INVALID_TIMER_ID as usize);
    }

    #[test]
    fn wrapped_deadlines_are_ordered() {
        assert!(!reached(9, 10));
        assert!(reached(10, 10));
        assert!(reached(1, 0xffff_fffe));
    }
}
