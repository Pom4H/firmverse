use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_PWRMGR_DEVICE: u32 = 0x0001_4FA4;
const ROM_PWRMGR_INIT: u32 = 0x0001_4FB0;
const ROM_PWRMGR_POWERCONSERVE: u32 = 0x0001_4FC0;
const ROM_PWRMGR_POWERCONSERVE0: u32 = 0x0001_4FD8;
const ROM_PWRMGR_TASK_STATE: u32 = 0x0001_50F8;

// Fixed PHY6252 ROM/SDK global from the public ROM symbol map.
const PWRMGR_ATTRIBUTE: u32 = 0x1FFF_08E8;
const TASK_STATE_OFF: u32 = 0;
const NEXT_TIMEOUT_OFF: u32 = 2;
const ACC_SLEEP_OFF: u32 = 4;
const DEVICE_OFF: u32 = 6;

const PWRMGR_ALWAYS_ON: u8 = 0;
const PWRMGR_BATTERY: u8 = 1;
const PWRMGR_CONSERVE: u8 = 0;
const PWRMGR_HOLD: u8 = 1;
const SUCCESS: u32 = 0;
const FAILURE: u32 = 1;

pub fn handle(cpu: &mut Processor) -> bool {
    match cpu.get_pc() {
        ROM_PWRMGR_DEVICE => device(cpu),
        ROM_PWRMGR_INIT => init(cpu),
        ROM_PWRMGR_POWERCONSERVE | ROM_PWRMGR_POWERCONSERVE0 => powerconserve(cpu),
        ROM_PWRMGR_TASK_STATE => task_state(cpu),
        _ => false,
    }
}

fn init(cpu: &mut Processor) -> bool {
    if cpu.write16(PWRMGR_ATTRIBUTE + TASK_STATE_OFF, 0).is_err()
        || cpu.write16(PWRMGR_ATTRIBUTE + NEXT_TIMEOUT_OFF, 0).is_err()
        || cpu.write16(PWRMGR_ATTRIBUTE + ACC_SLEEP_OFF, 0).is_err()
        || cpu.write8(PWRMGR_ATTRIBUTE + DEVICE_OFF, PWRMGR_ALWAYS_ON).is_err()
    {
        return false;
    }
    eprintln!("OSAL power manager initialized ALWAYS_ON");
    ret(cpu);
    true
}

fn device(cpu: &mut Processor) -> bool {
    let mode = cpu.get_r(Reg::R0) as u8;
    if mode != PWRMGR_ALWAYS_ON && mode != PWRMGR_BATTERY {
        eprintln!("OSAL strict invalid pwrmgr_device={mode}");
        return false;
    }
    if cpu.write8(PWRMGR_ATTRIBUTE + DEVICE_OFF, mode).is_err() {
        return false;
    }
    eprintln!("OSAL power manager device={}", if mode == PWRMGR_BATTERY { "BATTERY" } else { "ALWAYS_ON" });
    ret(cpu);
    true
}

fn task_state(cpu: &mut Processor) -> bool {
    let task = cpu.get_r(Reg::R0) as u8;
    let state = cpu.get_r(Reg::R1) as u8;
    if task >= 16 || (state != PWRMGR_CONSERVE && state != PWRMGR_HOLD) {
        cpu.set_r(Reg::R0, FAILURE);
        ret(cpu);
        return true;
    }
    let mut votes = match cpu.read16(PWRMGR_ATTRIBUTE + TASK_STATE_OFF) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let bit = 1u16 << task;
    if state == PWRMGR_HOLD {
        votes |= bit;
    } else {
        votes &= !bit;
    }
    if cpu.write16(PWRMGR_ATTRIBUTE + TASK_STATE_OFF, votes).is_err() {
        return false;
    }
    cpu.set_r(Reg::R0, SUCCESS);
    ret(cpu);
    true
}

fn powerconserve(cpu: &mut Processor) -> bool {
    // PHY6252 ROM normally enters light/deep sleep here when all tasks vote CONSERVE.
    // The emulator advances timers from emulated CPU cycles and keeps the host process
    // responsive, so sleeping the host CPU would be the wrong observable behavior.
    // Attribute state remains guest-visible and accurate; the sleep action is a no-op.
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
    fn public_power_modes_are_binary() {
        assert_eq!(PWRMGR_ALWAYS_ON, 0);
        assert_eq!(PWRMGR_BATTERY, 1);
        assert_eq!(PWRMGR_CONSERVE, 0);
        assert_eq!(PWRMGR_HOLD, 1);
    }
}
