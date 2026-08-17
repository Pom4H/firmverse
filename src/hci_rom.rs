use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_HCI_GAP_TASK_REGISTER: u32 = 0x0000_175C;
const ROM_HCI_INIT: u32 = 0x0000_183C;
const ROM_HCI_L2CAP_TASK_REGISTER: u32 = 0x0000_1878;
const ROM_HCI_SMP_TASK_REGISTER: u32 = 0x0000_26C8;
const ROM_HCI_READ_BDADDR_CMD: u32 = 0x0000_2550;
const ROM_HCI_LE_READ_BUF_SIZE_CMD: u32 = 0x0000_1C28;
const ROM_OSAL_MSG_ALLOC: u32 = 0x0001_4D1C;
const ROM_OSAL_MSG_SEND: u32 = 0x0001_4F58;

const HCI_TASK_ID: u32 = 0x1FFF_090C;
const HCI_GAP_TASK_ID: u32 = 0x1FFF_090E;
const HCI_L2CAP_TASK_ID: u32 = 0x1FFF_090F;
const HCI_SMP_TASK_ID: u32 = 0x1FFF_0910;

// 0xC2 is the existing BX LR halfword in the boot-flash drv_disable_irq thunk.
// With LR=0xC3 it executes one harmless self-loop, giving the emulator another
// host-dispatch tick without touching strict MMIO or inventing executable RAM.
const CONT_TRAP: u32 = 0x0000_00C2;
const STAGE_BDADDR_ALLOC: u32 = 1;
const STAGE_BUF_SIZE_ALLOC: u32 = 2;
const STAGE_SEND_DONE: u32 = 3;

const HCI_GAP_EVENT_EVENT: u8 = 0x91;
const HCI_COMMAND_COMPLETE_EVENT_CODE: u8 = 0x0E;
const HCI_SUCCESS: u32 = 0;
const HCI_ERROR_MEM_CAP_EXCEEDED: u32 = 0x07;
const OPCODE_READ_BDADDR: u16 = 0x1009;
const OPCODE_LE_READ_BUF_SIZE: u16 = 0x2002;
const CMD_COMPLETE_BYTES: u32 = 12;

pub fn handle(cpu: &mut Processor) -> bool {
    match cpu.get_pc() {
        ROM_HCI_INIT => register(cpu, HCI_TASK_ID, "HCI"),
        ROM_HCI_GAP_TASK_REGISTER => register(cpu, HCI_GAP_TASK_ID, "GAP"),
        ROM_HCI_L2CAP_TASK_REGISTER => register(cpu, HCI_L2CAP_TASK_ID, "L2CAP"),
        ROM_HCI_SMP_TASK_REGISTER => register(cpu, HCI_SMP_TASK_ID, "SMP"),
        ROM_HCI_READ_BDADDR_CMD => begin_event(cpu, CMD_COMPLETE_BYTES + 7, STAGE_BDADDR_ALLOC),
        ROM_HCI_LE_READ_BUF_SIZE_CMD => begin_event(cpu, CMD_COMPLETE_BYTES + 6, STAGE_BUF_SIZE_ALLOC),
        CONT_TRAP => match cpu.get_r(Reg::R3) {
            STAGE_BDADDR_ALLOC => finish_bdaddr_alloc(cpu),
            STAGE_BUF_SIZE_ALLOC => finish_buf_size_alloc(cpu),
            STAGE_SEND_DONE => finish_send(cpu),
            stage => {
                eprintln!("BLE strict unknown HCI continuation stage={stage}");
                false
            }
        },
        _ => false,
    }
}

fn register(cpu: &mut Processor, slot: u32, name: &str) -> bool {
    let task = cpu.get_r(Reg::R0) as u8;
    if cpu.write8(slot, task).is_err() {
        return false;
    }
    eprintln!("BLE HCI ROM register {name} task={task}");
    ret(cpu);
    true
}

fn begin_event(cpu: &mut Processor, bytes: u32, stage: u32) -> bool {
    // R12 and R3 are caller-saved under AAPCS. Host OSAL shims deliberately leave them
    // untouched, so they carry the original return address and continuation stage.
    cpu.set_r(Reg::R12, cpu.get_r(Reg::LR));
    cpu.set_r(Reg::R3, stage);
    cpu.set_r(Reg::R0, bytes);
    cpu.set_r(Reg::LR, CONT_TRAP | 1);
    cpu.set_pc(ROM_OSAL_MSG_ALLOC);
    // Return false intentionally: HostOsal::handle sees the rewritten PC in the same
    // dispatch call and executes msg_allocate before Cortex-M fetches vendor ROM.
    false
}

fn write_common(cpu: &mut Processor, msg: u32, opcode: u16, ret_len: u32) -> bool {
    let ret_ptr = msg + CMD_COMPLETE_BYTES;
    cpu.write8(msg, HCI_GAP_EVENT_EVENT).is_ok()
        && cpu.write8(msg + 1, HCI_COMMAND_COMPLETE_EVENT_CODE).is_ok()
        && cpu.write8(msg + 2, 1).is_ok()
        && cpu.write8(msg + 3, 0).is_ok()
        && cpu.write16(msg + 4, opcode).is_ok()
        && cpu.write16(msg + 6, 0).is_ok()
        && cpu.write32(msg + 8, ret_ptr).is_ok()
        && (0..ret_len).all(|i| cpu.write8(ret_ptr + i, 0).is_ok())
}

fn route_to_gap(cpu: &mut Processor, msg: u32) -> bool {
    let task = match cpu.read8(HCI_GAP_TASK_ID) {
        Ok(v) if v != 0xFF => v,
        _ => {
            eprintln!("BLE strict HCI GAP task is not registered");
            return true;
        }
    };
    cpu.set_r(Reg::R3, STAGE_SEND_DONE);
    cpu.set_r(Reg::R0, u32::from(task));
    cpu.set_r(Reg::R1, msg);
    cpu.set_r(Reg::LR, CONT_TRAP | 1);
    cpu.set_pc(ROM_OSAL_MSG_SEND);
    // Same handoff rule as begin_event: let HostOsal consume msg_send immediately.
    false
}

fn finish_bdaddr_alloc(cpu: &mut Processor) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 {
        return finish_failed_alloc(cpu);
    }
    if !write_common(cpu, msg, OPCODE_READ_BDADDR, 7) {
        return false;
    }
    let ret_ptr = msg + CMD_COMPLETE_BYTES;
    // Stable, emulator-local public address. HCI transmits BDADDR least-significant octet first.
    let addr = [0x01u8, 0x00, 0x00, 0x25, 0x62, 0x52];
    if cpu.write8(ret_ptr, 0).is_err() {
        return false;
    }
    for (i, byte) in addr.into_iter().enumerate() {
        if cpu.write8(ret_ptr + 1 + i as u32, byte).is_err() {
            return false;
        }
    }
    eprintln!("BLE HCI ReadBDADDR -> 52:62:25:00:00:01");
    route_to_gap(cpu, msg)
}

fn finish_buf_size_alloc(cpu: &mut Processor) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 {
        return finish_failed_alloc(cpu);
    }
    if !write_common(cpu, msg, OPCODE_LE_READ_BUF_SIZE, 6) {
        return false;
    }
    let ret_ptr = msg + CMD_COMPLETE_BYTES;
    if cpu.write8(ret_ptr, 0).is_err()
        || cpu.write8(ret_ptr + 1, 0).is_err()
        || cpu.write16(ret_ptr + 2, 251).is_err()
        || cpu.write8(ret_ptr + 4, 12).is_err()
        || cpu.write8(ret_ptr + 5, 0).is_err()
    {
        return false;
    }
    eprintln!("BLE HCI LE_ReadBufSize len=251 packets=12");
    route_to_gap(cpu, msg)
}

fn finish_failed_alloc(cpu: &mut Processor) -> bool {
    cpu.set_r(Reg::R0, HCI_ERROR_MEM_CAP_EXCEEDED);
    cpu.set_pc(cpu.get_r(Reg::R12) & !1);
    true
}

fn finish_send(cpu: &mut Processor) -> bool {
    let send_status = cpu.get_r(Reg::R0);
    cpu.set_r(Reg::R0, if send_status == 0 { HCI_SUCCESS } else { send_status });
    cpu.set_pc(cpu.get_r(Reg::R12) & !1);
    true
}

fn ret(cpu: &mut Processor) {
    cpu.set_pc(cpu.get_r(Reg::LR) & !1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_startup_opcodes_are_correct() {
        assert_eq!(OPCODE_READ_BDADDR, 0x1009);
        assert_eq!(OPCODE_LE_READ_BUF_SIZE, 0x2002);
    }

    #[test]
    fn command_complete_layout_is_arm32_compatible() {
        assert_eq!(CMD_COMPLETE_BYTES, 12);
    }

    #[test]
    fn continuation_trap_is_existing_boot_flash_bx_lr() {
        assert_eq!(CONT_TRAP, 0xC2);
    }
}
