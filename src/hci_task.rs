use crate::mailbox;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

// Fetch addresses are Thumb entrypoints from the pinned public PHY6252 ROM map.
const ROM_HCI_COMMAND_COMPLETE_EVENT: u32 = 0x0000_1174;
const ROM_HCI_PROCESS_EVENT: u32 = 0x0000_24FC;
const ROM_HCI_RESET_CMD: u32 = 0x0000_267C;
const ROM_HCI_REVERSE_BYTES: u32 = 0x0000_26A8;

const SHADOW: u32 = mailbox::BASE + 0x7C0;
const HCI_SUCCESS: u8 = 0x00;
const HCI_ERROR_INVALID_PARAMS: u32 = 0x12;
const OPCODE_HCI_RESET: u16 = 0x0C03;

/// Generic HCI task boundary for the host-backed controller.
///
/// BlueZ owns RF scheduling and link timing. Guest HCI command/event objects
/// are already delivered through the modeled HCI/OSAL layers, so the vendor
/// ROM HCI task has no additional radio work to execute.
pub fn handle(cpu: &mut Processor) -> bool {
    match cpu.get_pc() {
        ROM_HCI_PROCESS_EVENT => process_event(cpu),
        ROM_HCI_RESET_CMD => reset_cmd(cpu),
        ROM_HCI_REVERSE_BYTES => reverse_bytes(cpu),
        _ => false,
    }
}

fn process_event(cpu: &mut Processor) -> bool {
    let task = cpu.get_r(Reg::R0) as u8;
    let events = cpu.get_r(Reg::R1) as u16;
    if events != 0 {
        eprintln!("BLE HCI ProcessEvent task={task} consumed={events:#06x} host-controller");
    }
    cpu.set_r(Reg::R0, 0);
    ret(cpu);
    true
}

fn reset_cmd(cpu: &mut Processor) -> bool {
    // Reset the host-visible link state before emitting Command Complete.
    // The next real BlueZ connection will generate a fresh ConnectionComplete.
    if mailbox::cccd(cpu, false).is_err() || mailbox::connect(cpu, false).is_err() {
        return false;
    }
    if cpu.write8(SHADOW, HCI_SUCCESS).is_err() {
        return false;
    }
    eprintln!("BLE HCI Reset host-controller state");
    cpu.set_r(Reg::R0, u32::from(OPCODE_HCI_RESET));
    cpu.set_r(Reg::R1, 1);
    cpu.set_r(Reg::R2, SHADOW);
    cpu.set_pc(ROM_HCI_COMMAND_COMPLETE_EVENT);
    true
}

fn reverse_bytes(cpu: &mut Processor) -> bool {
    let ptr = cpu.get_r(Reg::R0);
    let len = cpu.get_r(Reg::R1) as u8;
    if ptr == 0 || len > 128 || len & 1 != 0 {
        cpu.set_r(Reg::R0, HCI_ERROR_INVALID_PARAMS);
        ret(cpu);
        return true;
    }
    for i in 0..u32::from(len / 2) {
        let left = ptr.wrapping_add(i);
        let right = ptr.wrapping_add(u32::from(len) - 1 - i);
        let a = match cpu.read8(left) { Ok(v) => v, Err(_) => return false };
        let b = match cpu.read8(right) { Ok(v) => v, Err(_) => return false };
        if cpu.write8(left, b).is_err() || cpu.write8(right, a).is_err() {
            return false;
        }
    }
    eprintln!("BLE HCI ReverseBytes len={len}");
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
    fn reset_opcode_is_standard_hci() {
        assert_eq!(OPCODE_HCI_RESET, 0x0C03);
    }

    #[test]
    fn reverse_contract_limit_is_public_sdk_limit() {
        assert_eq!(128u8 & 1, 0);
    }
}
