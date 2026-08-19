use crate::mailbox;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

// PHY6252 ROM entrypoints from the pinned public ROM symbol map.
const ROM_HCI_COMMAND_COMPLETE_EVENT: u32 = 0x0000_1174;
const ROM_HCI_DISCONNECT_CMD: u32 = 0x0000_128C;
const ROM_HCI_HOST_NUM_COMPLETED_PKT_CMD: u32 = 0x0000_17E0;
const ROM_HCI_LE_ADD_DEV_TO_RESOLVING_LIST_CMD: u32 = 0x0000_18A0;
const ROM_HCI_LE_ADD_WHITE_LIST_CMD: u32 = 0x0000_18BC;
const ROM_HCI_LE_CLEAR_WHITE_LIST_CMD: u32 = 0x0000_1934;
const ROM_HCI_LE_RAND_CMD: u32 = 0x0000_1BD8;
const ROM_HCI_READ_RSSI_CMD: u32 = 0x0000_2624;

const ROM_OSAL_MSG_ALLOC: u32 = 0x0001_4D1C;
const ROM_OSAL_MSG_SEND: u32 = 0x0001_4F58;

const HCI_GAP_TASK_ID: u32 = 0x1FFF_090E;
const CONT_TRAP: u32 = 0x0000_00CC;
const CONT_MAGIC: u32 = 0x4843_4558; // "HCEX"
const STAGE_COMPLETE_ALLOC: u32 = 1;
const STAGE_STATUS_ALLOC: u32 = 2;
const STAGE_SEND_DONE: u32 = 3;

const SHADOW_OPCODE: u32 = mailbox::BASE + 0x500;
const SHADOW_PARAM_LEN: u32 = mailbox::BASE + 0x504;
const SHADOW_PARAMS: u32 = mailbox::BASE + 0x508;
const MAX_PARAMS: usize = 64;

const HCI_GAP_EVENT_EVENT: u8 = 0x91;
const HCI_COMMAND_COMPLETE_EVENT_CODE: u8 = 0x0E;
const HCI_COMMAND_STATUS_EVENT_CODE: u8 = 0x0F;
const HCI_SUCCESS: u8 = 0x00;
const HCI_ERROR_UNKNOWN_CONN_HANDLE: u8 = 0x02;
const HCI_ERROR_INVALID_PARAMS: u8 = 0x12;
const HCI_RSSI_NOT_AVAILABLE: u8 = 0x7F;

const OPCODE_DISCONNECT: u16 = 0x0406;
const OPCODE_HOST_NUM_COMPLETED_PACKETS: u16 = 0x0C35;
const OPCODE_READ_RSSI: u16 = 0x1405;
const OPCODE_LE_CLEAR_WHITE_LIST: u16 = 0x2010;
const OPCODE_LE_ADD_WHITE_LIST: u16 = 0x2011;
const OPCODE_LE_RAND: u16 = 0x2018;
const OPCODE_LE_ADD_DEVICE_TO_RESOLVING_LIST: u16 = 0x2027;

const CMD_COMPLETE_BYTES: u32 = 12;
const CMD_STATUS_BYTES: u32 = 6;

pub fn handle(cpu: &mut Processor, rng: &mut u32) -> bool {
    match cpu.get_pc() {
        ROM_HCI_COMMAND_COMPLETE_EVENT => command_complete_event(cpu),
        ROM_HCI_DISCONNECT_CMD => disconnect_cmd(cpu),
        ROM_HCI_HOST_NUM_COMPLETED_PKT_CMD => host_num_completed_pkt_cmd(cpu),
        ROM_HCI_LE_ADD_DEV_TO_RESOLVING_LIST_CMD => add_resolving_list_cmd(cpu),
        ROM_HCI_LE_ADD_WHITE_LIST_CMD => add_white_list_cmd(cpu),
        ROM_HCI_LE_CLEAR_WHITE_LIST_CMD => status_complete(cpu, OPCODE_LE_CLEAR_WHITE_LIST),
        ROM_HCI_LE_RAND_CMD => rand_cmd(cpu, rng),
        ROM_HCI_READ_RSSI_CMD => read_rssi_cmd(cpu),
        CONT_TRAP if cpu.get_r(Reg::R2) == CONT_MAGIC => continue_event(cpu),
        _ => false,
    }
}

fn command_complete_event(cpu: &mut Processor) -> bool {
    let opcode = cpu.get_r(Reg::R0) as u16;
    let len = (cpu.get_r(Reg::R1) as usize).min(MAX_PARAMS);
    let src = cpu.get_r(Reg::R2);
    if len != 0 && src == 0 {
        ret(cpu, HCI_ERROR_INVALID_PARAMS as u32);
        return true;
    }
    let mut params = Vec::with_capacity(len);
    for i in 0..len {
        let Ok(byte) = cpu.read8(src.wrapping_add(i as u32)) else {
            return false;
        };
        params.push(byte);
    }
    stage_complete(cpu, opcode, &params)
}

fn disconnect_cmd(cpu: &mut Processor) -> bool {
    let conn_handle = cpu.get_r(Reg::R0) as u16;
    let reason = cpu.get_r(Reg::R1) as u8;
    let status = mailbox::status(cpu).unwrap_or(0);
    if conn_handle != 0 || status & mailbox::STATUS_CONNECTED == 0 {
        return stage_status(cpu, HCI_ERROR_UNKNOWN_CONN_HANDLE, OPCODE_DISCONNECT);
    }
    // The host controller owns the radio link. Clearing the mailbox link bit
    // lets the existing HCI link-state bridge emit DisconnectionComplete after
    // this command-status event has returned to the guest scheduler.
    if mailbox::connect(cpu, false).is_err() {
        return false;
    }
    eprintln!("BLE HCI Disconnect handle={conn_handle} reason={reason:#04x}");
    stage_status(cpu, HCI_SUCCESS, OPCODE_DISCONNECT)
}

fn host_num_completed_pkt_cmd(cpu: &mut Processor) -> bool {
    let count = cpu.get_r(Reg::R0) as u8;
    let handles = cpu.get_r(Reg::R1);
    let completed = cpu.get_r(Reg::R2);
    if count != 0 && (handles == 0 || completed == 0) {
        ret(cpu, HCI_ERROR_INVALID_PARAMS as u32);
        return true;
    }
    // This host-to-controller flow-control command has no Command Complete
    // event. Validate that guest arrays are readable, then consume the credits.
    for i in 0..u32::from(count) {
        if cpu.read16(handles + i * 2).is_err() || cpu.read16(completed + i * 2).is_err() {
            return false;
        }
    }
    eprintln!("BLE HCI HostNumCompletedPackets handles={count}");
    ret(cpu, HCI_SUCCESS as u32);
    true
}

fn add_white_list_cmd(cpu: &mut Processor) -> bool {
    let addr_type = cpu.get_r(Reg::R0) as u8;
    let addr = cpu.get_r(Reg::R1);
    if addr_type > 1 || !readable(cpu, addr, 6) {
        ret(cpu, HCI_ERROR_INVALID_PARAMS as u32);
        return true;
    }
    eprintln!("BLE HCI LE_AddWhiteList addr_type={addr_type}");
    status_complete(cpu, OPCODE_LE_ADD_WHITE_LIST)
}

fn add_resolving_list_cmd(cpu: &mut Processor) -> bool {
    let addr_type = cpu.get_r(Reg::R0) as u8;
    let addr = cpu.get_r(Reg::R1);
    let peer_irk = cpu.get_r(Reg::R2);
    let local_irk = cpu.get_r(Reg::R3);
    if addr_type > 1
        || !readable(cpu, addr, 6)
        || !readable(cpu, peer_irk, 16)
        || !readable(cpu, local_irk, 16)
    {
        ret(cpu, HCI_ERROR_INVALID_PARAMS as u32);
        return true;
    }
    eprintln!("BLE HCI LE_AddDeviceToResolvingList addr_type={addr_type}");
    status_complete(cpu, OPCODE_LE_ADD_DEVICE_TO_RESOLVING_LIST)
}

fn rand_cmd(cpu: &mut Processor, rng: &mut u32) -> bool {
    let mut params = [0u8; 9];
    params[0] = HCI_SUCCESS;
    let mut word = 0u32;
    for i in 0..8 {
        if i & 3 == 0 {
            word = next_u32(rng);
        }
        params[1 + i] = ((word >> (8 * (i & 3))) & 0xFF) as u8;
    }
    eprintln!("BLE HCI LE_Rand deterministic host entropy");
    stage_complete(cpu, OPCODE_LE_RAND, &params)
}

fn read_rssi_cmd(cpu: &mut Processor) -> bool {
    let conn_handle = cpu.get_r(Reg::R0) as u16;
    let connected = mailbox::status(cpu)
        .map(|status| status & mailbox::STATUS_CONNECTED != 0)
        .unwrap_or(false);
    let status = if connected && conn_handle == 0 {
        HCI_SUCCESS
    } else {
        HCI_ERROR_UNKNOWN_CONN_HANDLE
    };
    let rssi = if status == HCI_SUCCESS {
        (-42i8) as u8
    } else {
        HCI_RSSI_NOT_AVAILABLE
    };
    let params = [status, conn_handle as u8, (conn_handle >> 8) as u8, rssi];
    eprintln!("BLE HCI ReadRSSI handle={conn_handle} status={status:#04x}");
    stage_complete(cpu, OPCODE_READ_RSSI, &params)
}

fn status_complete(cpu: &mut Processor, opcode: u16) -> bool {
    stage_complete(cpu, opcode, &[HCI_SUCCESS])
}

fn stage_complete(cpu: &mut Processor, opcode: u16, params: &[u8]) -> bool {
    let len = params.len().min(MAX_PARAMS);
    if cpu.write16(SHADOW_OPCODE, opcode).is_err()
        || cpu.write32(SHADOW_PARAM_LEN, len as u32).is_err()
    {
        return false;
    }
    for (i, byte) in params.iter().take(len).copied().enumerate() {
        if cpu.write8(SHADOW_PARAMS + i as u32, byte).is_err() {
            return false;
        }
    }
    begin(cpu, CMD_COMPLETE_BYTES + len as u32, STAGE_COMPLETE_ALLOC)
}

fn stage_status(cpu: &mut Processor, status: u8, opcode: u16) -> bool {
    if cpu.write16(SHADOW_OPCODE, opcode).is_err() || cpu.write8(SHADOW_PARAMS, status).is_err() {
        return false;
    }
    begin(cpu, CMD_STATUS_BYTES, STAGE_STATUS_ALLOC)
}

fn begin(cpu: &mut Processor, bytes: u32, stage: u32) -> bool {
    cpu.set_r(Reg::R12, cpu.get_r(Reg::LR));
    cpu.set_r(Reg::R2, CONT_MAGIC);
    cpu.set_r(Reg::R3, stage);
    cpu.set_r(Reg::R0, bytes);
    cpu.set_r(Reg::LR, CONT_TRAP | 1);
    cpu.set_pc(ROM_OSAL_MSG_ALLOC);
    false
}

fn continue_event(cpu: &mut Processor) -> bool {
    match cpu.get_r(Reg::R3) {
        STAGE_COMPLETE_ALLOC => finish_complete(cpu),
        STAGE_STATUS_ALLOC => finish_status(cpu),
        STAGE_SEND_DONE => finish_send(cpu),
        _ => false,
    }
}

fn finish_complete(cpu: &mut Processor) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 {
        return finish_alloc_failure(cpu);
    }
    let opcode = match cpu.read16(SHADOW_OPCODE) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let len = match cpu.read32(SHADOW_PARAM_LEN) {
        Ok(v) => (v as usize).min(MAX_PARAMS),
        Err(_) => return false,
    };
    let param_ptr = msg + CMD_COMPLETE_BYTES;
    if cpu.write8(msg, HCI_GAP_EVENT_EVENT).is_err()
        || cpu
            .write8(msg + 1, HCI_COMMAND_COMPLETE_EVENT_CODE)
            .is_err()
        || cpu.write8(msg + 2, 1).is_err()
        || cpu.write8(msg + 3, 0).is_err()
        || cpu.write16(msg + 4, opcode).is_err()
        || cpu.write16(msg + 6, 0).is_err()
        || cpu.write32(msg + 8, param_ptr).is_err()
    {
        return false;
    }
    for i in 0..len {
        let byte = match cpu.read8(SHADOW_PARAMS + i as u32) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if cpu.write8(param_ptr + i as u32, byte).is_err() {
            return false;
        }
    }
    route_to_gap(cpu, msg)
}

fn finish_status(cpu: &mut Processor) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 {
        return finish_alloc_failure(cpu);
    }
    let opcode = match cpu.read16(SHADOW_OPCODE) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let status = match cpu.read8(SHADOW_PARAMS) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if cpu.write8(msg, HCI_GAP_EVENT_EVENT).is_err()
        || cpu.write8(msg + 1, HCI_COMMAND_STATUS_EVENT_CODE).is_err()
        || cpu.write8(msg + 2, status).is_err()
        || cpu.write8(msg + 3, 1).is_err()
        || cpu.write16(msg + 4, opcode).is_err()
    {
        return false;
    }
    route_to_gap(cpu, msg)
}

fn route_to_gap(cpu: &mut Processor, msg: u32) -> bool {
    let task = match cpu.read8(HCI_GAP_TASK_ID) {
        Ok(v) if v < 64 => v,
        _ => {
            // HCI_CommandCompleteEvent can be used before GAP registration.
            cpu.set_r(Reg::R0, HCI_SUCCESS as u32);
            cpu.set_pc(cpu.get_r(Reg::R12) & !1);
            return true;
        }
    };
    cpu.set_r(Reg::R2, CONT_MAGIC);
    cpu.set_r(Reg::R3, STAGE_SEND_DONE);
    cpu.set_r(Reg::R0, u32::from(task));
    cpu.set_r(Reg::R1, msg);
    cpu.set_r(Reg::LR, CONT_TRAP | 1);
    cpu.set_pc(ROM_OSAL_MSG_SEND);
    false
}

fn finish_alloc_failure(cpu: &mut Processor) -> bool {
    cpu.set_r(Reg::R0, 0x07);
    cpu.set_pc(cpu.get_r(Reg::R12) & !1);
    true
}

fn finish_send(cpu: &mut Processor) -> bool {
    let status = cpu.get_r(Reg::R0);
    cpu.set_r(
        Reg::R0,
        if status == 0 {
            HCI_SUCCESS as u32
        } else {
            status
        },
    );
    cpu.set_r(Reg::R2, 0);
    cpu.set_r(Reg::R3, 0);
    cpu.set_pc(cpu.get_r(Reg::R12) & !1);
    true
}

fn readable(cpu: &mut Processor, ptr: u32, len: u32) -> bool {
    if ptr == 0 {
        return false;
    }
    (0..len).all(|i| cpu.read8(ptr.wrapping_add(i)).is_ok())
}

fn next_u32(state: &mut u32) -> u32 {
    let mut x = if *state == 0 { 0x6252_A5A5 } else { *state };
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

fn ret(cpu: &mut Processor, r0: u32) {
    cpu.set_r(Reg::R0, r0);
    cpu.set_pc(cpu.get_r(Reg::LR) & !1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_firmware_hci_opcodes_are_standard() {
        assert_eq!(OPCODE_DISCONNECT, 0x0406);
        assert_eq!(OPCODE_HOST_NUM_COMPLETED_PACKETS, 0x0C35);
        assert_eq!(OPCODE_READ_RSSI, 0x1405);
        assert_eq!(OPCODE_LE_CLEAR_WHITE_LIST, 0x2010);
        assert_eq!(OPCODE_LE_ADD_WHITE_LIST, 0x2011);
        assert_eq!(OPCODE_LE_RAND, 0x2018);
        assert_eq!(OPCODE_LE_ADD_DEVICE_TO_RESOLVING_LIST, 0x2027);
    }

    #[test]
    fn deterministic_rng_progresses() {
        let mut state = 0;
        let first = next_u32(&mut state);
        let second = next_u32(&mut state);
        assert_ne!(first, 0);
        assert_ne!(first, second);
    }
}
