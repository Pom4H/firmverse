use crate::mailbox;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

// Fetch addresses are Thumb entrypoints from the pinned PHY6252 ROM map.
const ROM_LL_CONN_ACTIVE: u32 = 0x0000_3010;
const ROM_LL_DEQUEUE_CTRL_PKT: u32 = 0x0000_B8EC;
const ROM_LL_ENQUEUE_CTRL_PKT: u32 = 0x0000_B952;
const ROM_LL_REPLACE_CTRL_PKT: u32 = 0x0000_B9B8;

const LL_STATUS_SUCCESS: u32 = 0x00;
const LL_STATUS_ERROR_INACTIVE_CONNECTION: u32 = 0x02;
const LL_STATUS_ERROR_BAD_PARAMETER: u32 = 0x12;
const HOST_CONN_HANDLE: u16 = 0;
const PHY6252_MAX_CONNECTIONS: u16 = 3;

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
        ROM_LL_ENQUEUE_CTRL_PKT => enqueue_ctrl(cpu),
        ROM_LL_DEQUEUE_CTRL_PKT => dequeue_ctrl(cpu),
        ROM_LL_REPLACE_CTRL_PKT => replace_ctrl(cpu),
        _ => false,
    }
}

fn conn_active(cpu: &mut Processor) -> bool {
    let conn_id = cpu.get_r(Reg::R0) as u16;
    let status = if conn_id >= PHY6252_MAX_CONNECTIONS {
        LL_STATUS_ERROR_BAD_PARAMETER
    } else if conn_id == HOST_CONN_HANDLE && host_connected(cpu) {
        LL_STATUS_SUCCESS
    } else {
        LL_STATUS_ERROR_INACTIVE_CONNECTION
    };
    cpu.set_r(Reg::R0, status);
    ret(cpu);
    true
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
}
