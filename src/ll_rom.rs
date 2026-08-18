use crate::mailbox;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

// Fetch addresses are Thumb entrypoints from the pinned PHY6252 ROM map.
const ROM_LL_CONN_ACTIVE: u32 = 0x0000_3010;
const ROM_LL_PROCESS_EVENT: u32 = 0x0000_59F0;
const ROM_LL_RESET0: u32 = 0x0000_6608;
const ROM_LL_SET_ADV_PARAM0: u32 = 0x0000_6A9C;
const ROM_LL_SET_DATA_LENGTH0: u32 = 0x0000_6E10;
const ROM_LL_DEQUEUE_CTRL_PKT: u32 = 0x0000_B8EC;
const ROM_LL_ENQUEUE_CTRL_PKT: u32 = 0x0000_B952;
const ROM_LL_REPLACE_CTRL_PKT: u32 = 0x0000_D5F4;

pub const LL_STATUS_SUCCESS: u32 = 0x00;
pub const LL_STATUS_ERROR_INACTIVE_CONNECTION: u32 = 0x02;
pub const LL_STATUS_ERROR_BAD_PARAMETER: u32 = 0x12;
const HOST_CONN_HANDLE: u16 = 0;
const PHY6252_MAX_CONNECTIONS: u16 = 3;
pub const MAX_DATA_OCTETS: u16 = 251;
pub const MAX_DATA_TIME_US: u16 = 2120;

thread_local! {
    static CTRL_QUEUES: RefCell<HashMap<u32, VecDeque<u8>>> = RefCell::new(HashMap::new());
}

/// Host-controller implementation of the small LL ABI surface used by legacy
/// PHY6252 applications. BlueZ owns over-the-air scheduling; this layer keeps
/// the guest-visible controller state coherent instead of pretending to run
/// the vendor radio scheduler.
pub fn handle(cpu: &mut Processor) -> bool {
    match cpu.get_pc() {
        ROM_LL_CONN_ACTIVE => conn_active(cpu),
        ROM_LL_PROCESS_EVENT => process_event(cpu),
        ROM_LL_RESET0 => reset0(cpu),
        ROM_LL_SET_ADV_PARAM0 => set_adv_param0(cpu),
        ROM_LL_SET_DATA_LENGTH0 => set_data_length0(cpu),
        ROM_LL_ENQUEUE_CTRL_PKT => enqueue_ctrl(cpu),
        ROM_LL_DEQUEUE_CTRL_PKT => dequeue_ctrl(cpu),
        ROM_LL_REPLACE_CTRL_PKT => replace_ctrl(cpu),
        _ => false,
    }
}

fn conn_active(cpu: &mut Processor) -> bool {
    let conn_id = cpu.get_r(Reg::R0) as u16;
    let status = connection_status(cpu, conn_id);
    cpu.set_r(Reg::R0, status);
    ret(cpu);
    true
}

fn process_event(cpu: &mut Processor) -> bool {
    let task = cpu.get_r(Reg::R0) as u8;
    let events = cpu.get_r(Reg::R1) as u16;
    // Radio scheduling, connection timing and RF IRQs live in BlueZ/the host
    // controller. Any LL OSAL events reaching the guest ROM task therefore
    // represent controller work already consumed at that boundary. Preserve
    // the public LL_ProcessEvent ABI by returning no unprocessed event bits.
    if events != 0 {
        eprintln!("BLE LL ProcessEvent task={task} consumed={events:#06x} host-controller");
    }
    cpu.set_r(Reg::R0, 0);
    ret(cpu);
    true
}

fn reset0(cpu: &mut Processor) -> bool {
    CTRL_QUEUES.with(|queues| queues.borrow_mut().clear());
    if mailbox::cccd(cpu, false).is_err() || mailbox::connect(cpu, false).is_err() {
        return false;
    }
    eprintln!("BLE LL Reset0 host-controller state");
    cpu.set_r(Reg::R0, LL_STATUS_SUCCESS);
    ret(cpu);
    true
}

fn set_adv_param0(cpu: &mut Processor) -> bool {
    let min = cpu.get_r(Reg::R0) as u16;
    let max = cpu.get_r(Reg::R1) as u16;
    let event_type = cpu.get_r(Reg::R2) as u8;
    let own_addr_type = cpu.get_r(Reg::R3) as u8;
    let sp = cpu.get_r(Reg::SP);
    let direct_addr_type = match cpu.read32(sp) { Ok(v) => v as u8, Err(_) => return false };
    let direct_addr = match cpu.read32(sp + 4) { Ok(v) => v, Err(_) => return false };
    let channel_map = match cpu.read32(sp + 8) { Ok(v) => v as u8, Err(_) => return false };
    let wl_policy = match cpu.read32(sp + 12) { Ok(v) => v as u8, Err(_) => return false };

    let directed = matches!(event_type, 1 | 4);
    let direct_ok = !directed || (direct_addr_type <= 1 && readable(cpu, direct_addr, 6));
    let status = if min < 0x20
        || max > 0x4000
        || min > max
        || event_type > 4
        || own_addr_type > 3
        || !direct_ok
        || channel_map == 0
        || channel_map & !0x07 != 0
        || wl_policy > 3
    {
        LL_STATUS_ERROR_BAD_PARAMETER
    } else {
        LL_STATUS_SUCCESS
    };
    eprintln!("BLE LL SetAdvParam0 interval={min}..{max} type={event_type} status={status:#04x}");
    cpu.set_r(Reg::R0, status);
    ret(cpu);
    true
}

fn set_data_length0(cpu: &mut Processor) -> bool {
    let handle = cpu.get_r(Reg::R0) as u16;
    let octets = cpu.get_r(Reg::R1) as u16;
    let time = cpu.get_r(Reg::R2) as u16;
    let status = data_length_status(cpu, handle, octets, time);
    eprintln!("BLE LL SetDataLengh0 handle={handle} octets={octets} time={time} status={status:#04x}");
    cpu.set_r(Reg::R0, status);
    ret(cpu);
    true
}

pub fn data_length_status(cpu: &mut Processor, handle: u16, octets: u16, time: u16) -> u32 {
    let conn = connection_status(cpu, handle);
    if conn != LL_STATUS_SUCCESS {
        conn
    } else if !(27..=MAX_DATA_OCTETS).contains(&octets) || !(328..=MAX_DATA_TIME_US).contains(&time) {
        LL_STATUS_ERROR_BAD_PARAMETER
    } else {
        LL_STATUS_SUCCESS
    }
}

fn connection_status(cpu: &mut Processor, conn_id: u16) -> u32 {
    if conn_id >= PHY6252_MAX_CONNECTIONS {
        LL_STATUS_ERROR_BAD_PARAMETER
    } else if conn_id == HOST_CONN_HANDLE && host_connected(cpu) {
        LL_STATUS_SUCCESS
    } else {
        LL_STATUS_ERROR_INACTIVE_CONNECTION
    }
}

fn enqueue_ctrl(cpu: &mut Processor) -> bool {
    let conn_ptr = cpu.get_r(Reg::R0);
    let ctrl_type = cpu.get_r(Reg::R1) as u8;
    if conn_ptr == 0 {
        ret(cpu);
        return true;
    }
    CTRL_QUEUES.with(|queues| {
        queues.borrow_mut().entry(conn_ptr).or_default().push_back(ctrl_type);
    });
    eprintln!("BLE LL control enqueue conn={conn_ptr:#010x} type={ctrl_type:#04x}");
    ret(cpu);
    true
}

fn dequeue_ctrl(cpu: &mut Processor) -> bool {
    let conn_ptr = cpu.get_r(Reg::R0);
    if conn_ptr != 0 {
        let removed = CTRL_QUEUES.with(|queues| {
            let mut queues = queues.borrow_mut();
            let item = queues.get_mut(&conn_ptr).and_then(VecDeque::pop_front);
            if queues.get(&conn_ptr).is_some_and(VecDeque::is_empty) {
                queues.remove(&conn_ptr);
            }
            item
        });
        if let Some(ctrl_type) = removed {
            eprintln!("BLE LL control dequeue conn={conn_ptr:#010x} type={ctrl_type:#04x}");
        }
    }
    ret(cpu);
    true
}

fn replace_ctrl(cpu: &mut Processor) -> bool {
    let conn_ptr = cpu.get_r(Reg::R0);
    let ctrl_type = cpu.get_r(Reg::R1) as u8;
    if conn_ptr != 0 {
        CTRL_QUEUES.with(|queues| {
            let mut queues = queues.borrow_mut();
            let queue = queues.entry(conn_ptr).or_default();
            if let Some(front) = queue.front_mut() {
                *front = ctrl_type;
            } else {
                queue.push_back(ctrl_type);
            }
        });
        eprintln!("BLE LL control replace conn={conn_ptr:#010x} type={ctrl_type:#04x}");
    }
    ret(cpu);
    true
}

fn readable(cpu: &mut Processor, ptr: u32, len: u32) -> bool {
    ptr != 0 && (0..len).all(|i| cpu.read8(ptr.wrapping_add(i)).is_ok())
}

fn host_connected(cpu: &mut Processor) -> bool {
    mailbox::status(cpu)
        .map(|status| status & mailbox::STATUS_CONNECTED != 0)
        .unwrap_or(false)
}

fn ret(cpu: &mut Processor) {
    cpu.set_pc(cpu.get_r(Reg::LR) & !1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_ll_status_contract_is_preserved() {
        assert_eq!(LL_STATUS_SUCCESS, 0x00);
        assert_eq!(LL_STATUS_ERROR_INACTIVE_CONNECTION, 0x02);
        assert_eq!(LL_STATUS_ERROR_BAD_PARAMETER, 0x12);
    }

    #[test]
    fn host_controller_exposes_one_handle_inside_phy6252_range() {
        assert_eq!(HOST_CONN_HANDLE, 0);
        assert!(HOST_CONN_HANDLE < PHY6252_MAX_CONNECTIONS);
    }

    #[test]
    fn data_length_limits_match_host_acl_path() {
        assert_eq!(MAX_DATA_OCTETS, 251);
        assert_eq!(MAX_DATA_TIME_US, 2120);
    }
}
