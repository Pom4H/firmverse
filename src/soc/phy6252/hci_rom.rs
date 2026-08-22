use crate::mailbox;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_HCI_GAP_TASK_REGISTER: u32 = 0x0000_175C;
const ROM_HCI_INIT: u32 = 0x0000_183C;
const ROM_HCI_L2CAP_TASK_REGISTER: u32 = 0x0000_1878;
const ROM_HCI_SMP_TASK_REGISTER: u32 = 0x0000_26C8;
const ROM_HCI_LE_READ_BUF_SIZE_CMD: u32 = 0x0000_1C28;
const ROM_HCI_LE_SET_ADV_DATA_CMD: u32 = 0x0000_1F4C;
const ROM_HCI_LE_SET_ADV_ENABLE_CMD: u32 = 0x0000_1F68;
const ROM_HCI_LE_SET_ADV_PARAM_CMD: u32 = 0x0000_1F84;
const ROM_HCI_LE_SET_SCAN_ENABLE_CMD: u32 = 0x0000_2218;
const ROM_HCI_LE_SET_SCAN_PARAM_CMD: u32 = 0x0000_2234;
const ROM_HCI_LE_SET_SCAN_RSP_DATA_CMD: u32 = 0x0000_2254;
const ROM_HCI_READ_BDADDR_CMD: u32 = 0x0000_2550;
const ROM_HCI_SEND_DATA_PKT: u32 = 0x0000_27E8;
const ROM_OSAL_MEM_ALLOC: u32 = 0x0001_4B3C;
const ROM_OSAL_MSG_ALLOC: u32 = 0x0001_4D1C;
const ROM_OSAL_MSG_SEND: u32 = 0x0001_4F58;

const HCI_TASK_ID: u32 = 0x1FFF_090C;
const HCI_GAP_TASK_ID: u32 = 0x1FFF_090E;
const HCI_L2CAP_TASK_ID: u32 = 0x1FFF_090F;
const HCI_SMP_TASK_ID: u32 = 0x1FFF_0910;

const GUEST_SRAM_BASE: u32 = 0x1FFF_0000;
const GUEST_SRAM_END: u32 = 0x2000_0000;
const GUEST_XIP_BASE: u32 = 0x1100_0000;
const GUEST_XIP_END: u32 = 0x1104_0000;
const GATT_ATTRIBUTE_BYTES: u32 = 16;
const GATT_WRITE_PERMISSIONS: u8 = 0xAA;

const RADIO_STATUS_SHADOW: u32 = mailbox::BASE + 0x300;
const RX_SEQ_SHADOW: u32 = mailbox::BASE + 0x304;
const RX_HANDLE_SHADOW: u32 = mailbox::BASE + 0x308;
const TX_CCCD_HANDLE_SHADOW: u32 = mailbox::BASE + 0x30C;
const PENDING_HANDLE_SHADOW: u32 = mailbox::BASE + 0x310;
const PENDING_LEN_SHADOW: u32 = mailbox::BASE + 0x314;
const RX_MSG_SHADOW: u32 = mailbox::BASE + 0x318;
const PENDING_BYTES: u32 = mailbox::BASE + 0x320;

const IDLE_BX_LR_ROM: u32 = 0x0000_A9C8;
const CONT_TRAP: u32 = 0x0000_00C2;
const CONT_MAGIC: u32 = 0x4843_4921;
const STAGE_BDADDR_ALLOC: u32 = 1;
const STAGE_BUF_SIZE_ALLOC: u32 = 2;
const STAGE_SEND_DONE: u32 = 3;
const STAGE_CONN_ALLOC: u32 = 4;
const STAGE_DISCONN_ALLOC: u32 = 5;
const STAGE_RX_MSG_ALLOC: u32 = 6;
const STAGE_RX_DATA_ALLOC: u32 = 7;
const STAGE_STATUS_FLAG: u32 = 0x8000_0000;

const HCI_DATA_EVENT: u8 = 0x90;
const HCI_GAP_EVENT_EVENT: u8 = 0x91;
const HCI_COMMAND_COMPLETE_EVENT_CODE: u8 = 0x0E;
const HCI_DISCONNECTION_COMPLETE_EVENT_CODE: u8 = 0x05;
const HCI_LE_EVENT_CODE: u8 = 0x3E;
const HCI_BLE_CONNECTION_COMPLETE_EVENT: u8 = 0x01;
const HCI_PB_FIRST_HOST_PKT: u8 = 0x02;
const L2CAP_CID_ATT: u16 = 0x0004;
const ATT_WRITE_CMD: u8 = 0x52;
const GATT_CLIENT_CHAR_CFG_UUID: [u8; 2] = [0x29, 0x02];

const HCI_SUCCESS: u32 = 0;
const HCI_ERROR_MEM_CAP_EXCEEDED: u32 = 0x07;
const HCI_ERROR_INVALID_PARAMS: u32 = 0x12;
const OPCODE_READ_BDADDR: u16 = 0x1009;
const OPCODE_LE_READ_BUF_SIZE: u16 = 0x2002;
const OPCODE_LE_SET_ADV_PARAM: u16 = 0x2006;
const OPCODE_LE_SET_ADV_DATA: u16 = 0x2008;
const OPCODE_LE_SET_SCAN_RSP_DATA: u16 = 0x2009;
const OPCODE_LE_SET_ADV_ENABLE: u16 = 0x200A;
const OPCODE_LE_SET_SCAN_PARAM: u16 = 0x200B;
const OPCODE_LE_SET_SCAN_ENABLE: u16 = 0x200C;
const CMD_COMPLETE_BYTES: u32 = 12;
const HCI_DATA_EVENT_BYTES: u32 = 12;
const CONN_COMPLETE_BYTES: u32 = 22;
const DISCONN_COMPLETE_BYTES: u32 = 8;

#[derive(Clone, Copy)]
struct GuestAttribute {
    addr: u32,
    handle: u16,
}

pub fn handle(cpu: &mut Processor) -> bool {
    if poll_host_radio(cpu) {
        return false;
    }
    if poll_host_cccd(cpu) {
        return false;
    }
    if poll_host_rx(cpu) {
        return false;
    }
    match cpu.get_pc() {
        ROM_HCI_INIT => init_hci(cpu),
        ROM_HCI_GAP_TASK_REGISTER => register(cpu, HCI_GAP_TASK_ID, "GAP"),
        ROM_HCI_L2CAP_TASK_REGISTER => register(cpu, HCI_L2CAP_TASK_ID, "L2CAP"),
        ROM_HCI_SMP_TASK_REGISTER => register(cpu, HCI_SMP_TASK_ID, "SMP"),
        ROM_HCI_READ_BDADDR_CMD => begin_event(cpu, CMD_COMPLETE_BYTES + 7, STAGE_BDADDR_ALLOC),
        ROM_HCI_LE_READ_BUF_SIZE_CMD => {
            begin_event(cpu, CMD_COMPLETE_BYTES + 6, STAGE_BUF_SIZE_ALLOC)
        }
        ROM_HCI_LE_SET_ADV_DATA_CMD => set_payload_data(cpu, OPCODE_LE_SET_ADV_DATA, "AdvData"),
        ROM_HCI_LE_SET_SCAN_RSP_DATA_CMD => {
            set_payload_data(cpu, OPCODE_LE_SET_SCAN_RSP_DATA, "ScanRspData")
        }
        ROM_HCI_LE_SET_ADV_ENABLE_CMD => set_adv_enable(cpu),
        ROM_HCI_LE_SET_ADV_PARAM_CMD => set_adv_params(cpu),
        ROM_HCI_LE_SET_SCAN_ENABLE_CMD => set_scan_enable(cpu),
        ROM_HCI_LE_SET_SCAN_PARAM_CMD => set_scan_params(cpu),
        ROM_HCI_SEND_DATA_PKT => send_data_pkt(cpu),
        CONT_TRAP if cpu.get_r(Reg::R2) == CONT_MAGIC => continue_event(cpu),
        _ => false,
    }
}

fn init_hci(cpu: &mut Processor) -> bool {
    /* The mailbox is flash-like backing memory and starts erased (0xFF), while
     * these words are emulator-owned runtime caches. Leaving them erased makes
     * bit 0 look like an already-connected host and injects a fake disconnect
     * before the guest ever enabled advertising. It also turns cached ATT
     * handles into 0xFFFF. Initialize the cache explicitly at the HCI lifecycle
     * boundary; do not infer runtime state from erased bytes. */
    let rx_seq = cpu.read32(mailbox::BASE + mailbox::RX_SEQ).unwrap_or(0);
    for (addr, value) in [
        (RADIO_STATUS_SHADOW, 0),
        (RX_SEQ_SHADOW, rx_seq),
        (RX_HANDLE_SHADOW, 0),
        (TX_CCCD_HANDLE_SHADOW, 0),
        (PENDING_HANDLE_SHADOW, 0),
        (PENDING_LEN_SHADOW, 0),
        (RX_MSG_SHADOW, 0),
    ] {
        if cpu.write32(addr, value).is_err() {
            return false;
        }
    }
    register(cpu, HCI_TASK_ID, "HCI")
}

fn poll_host_radio(cpu: &mut Processor) -> bool {
    if !host_idle(cpu) {
        return false;
    }
    let gap_task = match cpu.read8(HCI_GAP_TASK_ID) {
        Ok(v) if v > 0 && v < 64 => v,
        _ => return false,
    };
    let status = match mailbox::status(cpu) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let seen = cpu.read32(RADIO_STATUS_SHADOW).unwrap_or(0);
    let connected = status & mailbox::STATUS_CONNECTED != 0;
    let was_connected = seen & mailbox::STATUS_CONNECTED != 0;
    if connected == was_connected {
        return false;
    }
    if cpu.write32(RADIO_STATUS_SHADOW, status).is_err() {
        return false;
    }
    let stage = if connected {
        STAGE_CONN_ALLOC
    } else {
        STAGE_DISCONN_ALLOC
    };
    let bytes = if connected {
        CONN_COMPLETE_BYTES
    } else {
        DISCONN_COMPLETE_BYTES
    };
    eprintln!(
        "BLE host radio {} -> guest GAP task={gap_task}",
        if connected { "connect" } else { "disconnect" }
    );
    begin_async_event(cpu, bytes, stage);
    true
}

fn poll_host_cccd(cpu: &mut Processor) -> bool {
    if !host_idle(cpu) {
        return false;
    }
    let status = match mailbox::status(cpu) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if status & mailbox::STATUS_CONNECTED == 0 {
        return false;
    }
    let seen = cpu.read32(RADIO_STATUS_SHADOW).unwrap_or(0);
    let notify = status & mailbox::STATUS_NOTIFY != 0;
    let was_notify = seen & mailbox::STATUS_NOTIFY != 0;
    if notify == was_notify {
        return false;
    }
    if cpu.write32(RADIO_STATUS_SHADOW, status).is_err() {
        return false;
    }
    let Some(handle) = resolve_tx_cccd_handle(cpu) else {
        eprintln!("BLE guest TX CCCD not found for configured runtime UUID");
        return false;
    };
    let value = if notify { [1u8, 0] } else { [0u8, 0] };
    eprintln!(
        "BLE host {} -> guest ATT CCCD handle={handle:#06x}",
        if notify { "subscribe" } else { "unsubscribe" }
    );
    stage_att_write(cpu, handle, &value)
}

fn poll_host_rx(cpu: &mut Processor) -> bool {
    if !host_idle(cpu) {
        return false;
    }
    let status = match mailbox::status(cpu) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if status & mailbox::STATUS_CONNECTED == 0 {
        return false;
    }
    let task = match cpu.read8(HCI_L2CAP_TASK_ID) {
        Ok(v) if v < 64 => v,
        _ => return false,
    };
    let seq = match cpu.read32(mailbox::BASE + mailbox::RX_SEQ) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let seen = cpu.read32(RX_SEQ_SHADOW).unwrap_or(0);
    if seq == seen {
        return false;
    }
    if cpu.write32(RX_SEQ_SHADOW, seq).is_err() {
        return false;
    }
    let Some(handle) = resolve_rx_handle(cpu) else {
        eprintln!("BLE guest RX attribute not found for configured runtime UUID");
        return false;
    };
    let len = match cpu.read32(mailbox::BASE + mailbox::RX_LEN) {
        Ok(v) => (v as usize).min(mailbox::PAYLOAD),
        Err(_) => return false,
    };
    let mut value = Vec::with_capacity(len);
    for i in 0..len {
        match cpu.read8(mailbox::BASE + mailbox::RX_BYTES + i as u32) {
            Ok(byte) => value.push(byte),
            Err(_) => return false,
        }
    }
    eprintln!("BLE host RX -> guest L2CAP task={task} ATT handle={handle:#06x} bytes={len}");
    stage_att_write(cpu, handle, &value)
}

fn host_idle(cpu: &mut Processor) -> bool {
    cpu.get_pc() == IDLE_BX_LR_ROM && cpu.get_r(Reg::R2) != CONT_MAGIC
}

fn stage_att_write(cpu: &mut Processor, handle: u16, value: &[u8]) -> bool {
    let len = value.len().min(mailbox::PAYLOAD);
    if cpu
        .write32(PENDING_HANDLE_SHADOW, u32::from(handle))
        .is_err()
        || cpu.write32(PENDING_LEN_SHADOW, len as u32).is_err()
    {
        return false;
    }
    for (index, byte) in value.iter().take(len).copied().enumerate() {
        if cpu.write8(PENDING_BYTES + index as u32, byte).is_err() {
            return false;
        }
    }
    begin_async_event(cpu, HCI_DATA_EVENT_BYTES, STAGE_RX_MSG_ALLOC);
    true
}

fn resolve_rx_handle(cpu: &mut Processor) -> Option<u16> {
    if let Ok(cached) = cpu.read32(RX_HANDLE_SHADOW) {
        let handle = cached as u16;
        if handle != 0 {
            return Some(handle);
        }
    }
    let uuid = std::env::var("PHY6252_GUEST_RX_UUID").ok()?;
    let target = parse_uuid_bytes(&uuid)?;
    let attr = find_guest_attribute(cpu, &target, true)?;
    let _ = cpu.write32(RX_HANDLE_SHADOW, u32::from(attr.handle));
    eprintln!(
        "BLE guest writable attribute resolved handle={:#06x}",
        attr.handle
    );
    Some(attr.handle)
}

fn resolve_tx_cccd_handle(cpu: &mut Processor) -> Option<u16> {
    if let Ok(cached) = cpu.read32(TX_CCCD_HANDLE_SHADOW) {
        let handle = cached as u16;
        if handle != 0 {
            return Some(handle);
        }
    }
    let uuid = std::env::var("PHY6252_GUEST_TX_UUID").ok()?;
    let target = parse_uuid_bytes(&uuid)?;
    let value_attr = find_guest_attribute(cpu, &target, false)?;
    let descriptor = find_cccd_after(cpu, value_attr)?;
    let _ = cpu.write32(TX_CCCD_HANDLE_SHADOW, u32::from(descriptor.handle));
    eprintln!("BLE guest CCCD resolved handle={:#06x}", descriptor.handle);
    Some(descriptor.handle)
}

fn find_guest_attribute(
    cpu: &mut Processor,
    target: &[u8],
    require_write: bool,
) -> Option<GuestAttribute> {
    let mut addr = GUEST_SRAM_BASE;
    while addr + GATT_ATTRIBUTE_BYTES <= GUEST_SRAM_END {
        if cpu.read8(addr).ok()? == target.len() as u8 {
            let uuid_ptr = cpu.read32(addr + 4).ok()?;
            let permissions = cpu.read8(addr + 8).ok()?;
            let handle = cpu.read16(addr + 10).ok()?;
            let value_ptr = cpu.read32(addr + 12).ok()?;
            if handle != 0
                && value_ptr != 0
                && (!require_write || permissions & GATT_WRITE_PERMISSIONS != 0)
                && guest_data_ptr(uuid_ptr, target.len())
                && uuid_matches(cpu, uuid_ptr, target)
            {
                return Some(GuestAttribute { addr, handle });
            }
        }
        addr += 4;
    }
    None
}

fn find_cccd_after(cpu: &mut Processor, value_attr: GuestAttribute) -> Option<GuestAttribute> {
    let mut addr = value_attr.addr + GATT_ATTRIBUTE_BYTES;
    for _ in 0..8 {
        if addr + GATT_ATTRIBUTE_BYTES > GUEST_SRAM_END {
            break;
        }
        let len = cpu.read8(addr).ok()? as usize;
        let uuid_ptr = cpu.read32(addr + 4).ok()?;
        let permissions = cpu.read8(addr + 8).ok()?;
        let handle = cpu.read16(addr + 10).ok()?;
        if handle != 0 && handle > value_attr.handle {
            if len == GATT_CLIENT_CHAR_CFG_UUID.len()
                && permissions & GATT_WRITE_PERMISSIONS != 0
                && guest_data_ptr(uuid_ptr, len)
                && uuid_matches(cpu, uuid_ptr, &GATT_CLIENT_CHAR_CFG_UUID)
            {
                return Some(GuestAttribute { addr, handle });
            }
            if len == 16 {
                break;
            }
        }
        addr += GATT_ATTRIBUTE_BYTES;
    }
    None
}

fn guest_data_ptr(ptr: u32, len: usize) -> bool {
    let end = ptr.saturating_add(len as u32);
    (ptr >= GUEST_SRAM_BASE && end <= GUEST_SRAM_END)
        || (ptr >= GUEST_XIP_BASE && end <= GUEST_XIP_END)
}

fn uuid_matches(cpu: &mut Processor, ptr: u32, target: &[u8]) -> bool {
    let direct = target
        .iter()
        .enumerate()
        .all(|(i, expected)| cpu.read8(ptr + i as u32).ok() == Some(*expected));
    if direct {
        return true;
    }
    target
        .iter()
        .rev()
        .enumerate()
        .all(|(i, expected)| cpu.read8(ptr + i as u32).ok() == Some(*expected))
}

fn parse_uuid_bytes(input: &str) -> Option<Vec<u8>> {
    let compact: Vec<u8> = input
        .bytes()
        .filter(|byte| byte.is_ascii_hexdigit())
        .collect();
    if !matches!(compact.len(), 4 | 32) {
        return None;
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    for pair in compact.chunks_exact(2) {
        out.push((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?);
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn continue_event(cpu: &mut Processor) -> bool {
    let stage = cpu.get_r(Reg::R3);
    match stage {
        STAGE_BDADDR_ALLOC => finish_bdaddr_alloc(cpu),
        STAGE_BUF_SIZE_ALLOC => finish_buf_size_alloc(cpu),
        STAGE_SEND_DONE => finish_send(cpu),
        STAGE_CONN_ALLOC => finish_conn_alloc(cpu),
        STAGE_DISCONN_ALLOC => finish_disconn_alloc(cpu),
        STAGE_RX_MSG_ALLOC => finish_rx_msg_alloc(cpu),
        STAGE_RX_DATA_ALLOC => finish_rx_data_alloc(cpu),
        _ if stage & STAGE_STATUS_FLAG != 0 => finish_status_alloc(cpu, (stage & 0xFFFF) as u16),
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

fn set_payload_data(cpu: &mut Processor, opcode: u16, label: &str) -> bool {
    let len = cpu.get_r(Reg::R0) as u8;
    let ptr = cpu.get_r(Reg::R1);
    if len > 31 || (len != 0 && ptr == 0) {
        return immediate_error(cpu);
    }
    eprintln!("BLE HCI LE_Set{label} len={len}");
    begin_status_event(cpu, opcode)
}

fn set_adv_enable(cpu: &mut Processor) -> bool {
    let enabled = cpu.get_r(Reg::R0) as u8;
    if enabled > 1 {
        return immediate_error(cpu);
    }
    eprintln!("BLE HCI LE_SetAdvEnable enabled={enabled}");
    begin_status_event(cpu, OPCODE_LE_SET_ADV_ENABLE)
}

fn set_adv_params(cpu: &mut Processor) -> bool {
    let min = cpu.get_r(Reg::R0) as u16;
    let max = cpu.get_r(Reg::R1) as u16;
    let adv_type = cpu.get_r(Reg::R2) as u8;
    let own_addr_type = cpu.get_r(Reg::R3) as u8;
    if min > max || min < 0x20 || max > 0x4000 || adv_type > 4 || own_addr_type > 1 {
        return immediate_error(cpu);
    }
    eprintln!(
        "BLE HCI LE_SetAdvParam interval={min}..{max} type={adv_type} own_addr={own_addr_type}"
    );
    begin_status_event(cpu, OPCODE_LE_SET_ADV_PARAM)
}

fn set_scan_params(cpu: &mut Processor) -> bool {
    let scan_type = cpu.get_r(Reg::R0) as u8;
    let interval = cpu.get_r(Reg::R1) as u16;
    let window = cpu.get_r(Reg::R2) as u16;
    let own_addr_type = cpu.get_r(Reg::R3) as u8;
    let wl_policy = match cpu.read32(cpu.get_r(Reg::SP)) {
        Ok(value) => value as u8,
        Err(_) => return false,
    };
    if scan_type > 1
        || !(4..=0x4000).contains(&interval)
        || !(4..=interval).contains(&window)
        || own_addr_type > 3
        || wl_policy > 3
    {
        return immediate_error(cpu);
    }
    eprintln!(
        "BLE HCI LE_SetScanParam type={scan_type} interval={interval} window={window} own_addr={own_addr_type} policy={wl_policy}"
    );
    begin_status_event(cpu, OPCODE_LE_SET_SCAN_PARAM)
}

fn set_scan_enable(cpu: &mut Processor) -> bool {
    let enabled = cpu.get_r(Reg::R0) as u8;
    let filter_duplicates = cpu.get_r(Reg::R1) as u8;
    if enabled > 1 || filter_duplicates > 1 {
        return immediate_error(cpu);
    }
    eprintln!("BLE HCI LE_SetScanEnable enabled={enabled} filter_duplicates={filter_duplicates}");
    begin_status_event(cpu, OPCODE_LE_SET_SCAN_ENABLE)
}

fn send_data_pkt(cpu: &mut Processor) -> bool {
    let len = cpu.get_r(Reg::R2) as usize;
    let ptr = cpu.get_r(Reg::R3);
    let mut data = Vec::with_capacity(len);
    for i in 0..len {
        match cpu.read8(ptr.wrapping_add(i as u32)) {
            Ok(v) => data.push(v),
            Err(_) => return false,
        }
    }
    if let Some(value) = att_notification_value(&data) {
        let _ = mailbox::emit_tx(cpu, value);
        eprintln!("BLE HCI ACL TX ATT notification bytes={}", value.len());
    } else {
        eprintln!("BLE HCI ACL TX bytes={len}");
    }
    cpu.set_r(Reg::R0, HCI_SUCCESS);
    ret(cpu);
    true
}

fn att_notification_value(data: &[u8]) -> Option<&[u8]> {
    if data.len() >= 7 && data[2] == 0x04 && data[3] == 0x00 && matches!(data[4], 0x1B | 0x1D) {
        return Some(&data[7..]);
    }
    if data.len() >= 3 && matches!(data[0], 0x1B | 0x1D) {
        return Some(&data[3..]);
    }
    None
}

fn immediate_error(cpu: &mut Processor) -> bool {
    cpu.set_r(Reg::R0, HCI_ERROR_INVALID_PARAMS);
    ret(cpu);
    true
}

fn begin_status_event(cpu: &mut Processor, opcode: u16) -> bool {
    begin_event(
        cpu,
        CMD_COMPLETE_BYTES + 1,
        STAGE_STATUS_FLAG | u32::from(opcode),
    )
}

fn begin_event(cpu: &mut Processor, bytes: u32, stage: u32) -> bool {
    cpu.set_r(Reg::R12, cpu.get_r(Reg::LR));
    begin_event_common(cpu, bytes, stage)
}

fn begin_async_event(cpu: &mut Processor, bytes: u32, stage: u32) -> bool {
    let resume = cpu.get_pc() | 1;
    cpu.set_r(Reg::R12, resume);
    begin_event_common(cpu, bytes, stage)
}

fn begin_event_common(cpu: &mut Processor, bytes: u32, stage: u32) -> bool {
    cpu.set_r(Reg::R2, CONT_MAGIC);
    cpu.set_r(Reg::R3, stage);
    cpu.set_r(Reg::R0, bytes);
    cpu.set_r(Reg::LR, CONT_TRAP | 1);
    cpu.set_pc(ROM_OSAL_MSG_ALLOC);
    false
}

fn finish_rx_msg_alloc(cpu: &mut Processor) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 {
        return finish_failed_alloc(cpu);
    }
    if cpu.write32(RX_MSG_SHADOW, msg).is_err() {
        return false;
    }
    let value_len = match cpu.read32(PENDING_LEN_SHADOW) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let l2cap_len = value_len.saturating_add(7);
    cpu.set_r(Reg::R2, CONT_MAGIC);
    cpu.set_r(Reg::R3, STAGE_RX_DATA_ALLOC);
    cpu.set_r(Reg::R0, l2cap_len);
    cpu.set_r(Reg::LR, CONT_TRAP | 1);
    cpu.set_pc(ROM_OSAL_MEM_ALLOC);
    false
}

fn finish_rx_data_alloc(cpu: &mut Processor) -> bool {
    let data = cpu.get_r(Reg::R0);
    if data == 0 {
        return finish_failed_alloc(cpu);
    }
    let msg = match cpu.read32(RX_MSG_SHADOW) {
        Ok(v) if v != 0 => v,
        _ => return false,
    };
    let handle = match cpu.read32(PENDING_HANDLE_SHADOW) {
        Ok(v) => v as u16,
        Err(_) => return false,
    };
    let value_len = match cpu.read32(PENDING_LEN_SHADOW) {
        Ok(v) => v as usize,
        Err(_) => return false,
    };
    let att_len = value_len + 3;
    let l2cap_len = att_len + 4;

    if cpu.write8(msg, HCI_DATA_EVENT).is_err()
        || cpu.write8(msg + 1, 0).is_err()
        || cpu.write16(msg + 2, 0).is_err()
        || cpu.write8(msg + 4, HCI_PB_FIRST_HOST_PKT).is_err()
        || cpu.write8(msg + 5, 0).is_err()
        || cpu.write16(msg + 6, l2cap_len as u16).is_err()
        || cpu.write32(msg + 8, data).is_err()
        || cpu.write16(data, att_len as u16).is_err()
        || cpu.write16(data + 2, L2CAP_CID_ATT).is_err()
        || cpu.write8(data + 4, ATT_WRITE_CMD).is_err()
        || cpu.write16(data + 5, handle).is_err()
    {
        return false;
    }
    for i in 0..value_len {
        let byte = match cpu.read8(PENDING_BYTES + i as u32) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if cpu.write8(data + 7 + i as u32, byte).is_err() {
            return false;
        }
    }
    eprintln!("BLE host ATT write injected handle={handle:#06x} value_bytes={value_len}");
    route_to_l2cap(cpu, msg)
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
        _ => return true,
    };
    route_message(cpu, task, msg)
}

fn route_to_l2cap(cpu: &mut Processor, msg: u32) -> bool {
    let task = match cpu.read8(HCI_L2CAP_TASK_ID) {
        Ok(v) if v != 0xFF => v,
        _ => return true,
    };
    route_message(cpu, task, msg)
}

fn route_message(cpu: &mut Processor, task: u8, msg: u32) -> bool {
    cpu.set_r(Reg::R2, CONT_MAGIC);
    cpu.set_r(Reg::R3, STAGE_SEND_DONE);
    cpu.set_r(Reg::R0, u32::from(task));
    cpu.set_r(Reg::R1, msg);
    cpu.set_r(Reg::LR, CONT_TRAP | 1);
    cpu.set_pc(ROM_OSAL_MSG_SEND);
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

fn finish_conn_alloc(cpu: &mut Processor) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 {
        return finish_failed_alloc(cpu);
    }
    let peer = [0x06u8, 0x05, 0x04, 0x03, 0x02, 0x01];
    if cpu.write8(msg, HCI_GAP_EVENT_EVENT).is_err()
        || cpu.write8(msg + 1, HCI_LE_EVENT_CODE).is_err()
        || cpu
            .write8(msg + 2, HCI_BLE_CONNECTION_COMPLETE_EVENT)
            .is_err()
        || cpu.write8(msg + 3, 0).is_err()
        || cpu.write16(msg + 4, 0).is_err()
        || cpu.write8(msg + 6, 1).is_err()
        || cpu.write8(msg + 7, 0).is_err()
    {
        return false;
    }
    for (i, byte) in peer.into_iter().enumerate() {
        if cpu.write8(msg + 8 + i as u32, byte).is_err() {
            return false;
        }
    }
    if cpu.write16(msg + 14, 24).is_err()
        || cpu.write16(msg + 16, 0).is_err()
        || cpu.write16(msg + 18, 200).is_err()
        || cpu.write8(msg + 20, 0).is_err()
        || cpu.write8(msg + 21, 0).is_err()
    {
        return false;
    }
    eprintln!("BLE HCI LE ConnectionComplete handle=0 interval=30ms");
    route_to_gap(cpu, msg)
}

fn finish_disconn_alloc(cpu: &mut Processor) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 {
        return finish_failed_alloc(cpu);
    }
    if cpu.write8(msg, HCI_GAP_EVENT_EVENT).is_err()
        || cpu
            .write8(msg + 1, HCI_DISCONNECTION_COMPLETE_EVENT_CODE)
            .is_err()
        || cpu.write8(msg + 2, 0).is_err()
        || cpu.write8(msg + 3, 0).is_err()
        || cpu.write16(msg + 4, 0).is_err()
        || cpu.write8(msg + 6, 0x13).is_err()
        || cpu.write8(msg + 7, 0).is_err()
    {
        return false;
    }
    eprintln!("BLE HCI DisconnectionComplete handle=0 reason=0x13");
    route_to_gap(cpu, msg)
}

fn finish_status_alloc(cpu: &mut Processor, opcode: u16) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 {
        return finish_failed_alloc(cpu);
    }
    if !write_common(cpu, msg, opcode, 1) {
        return false;
    }
    if cpu
        .write8(msg + CMD_COMPLETE_BYTES, HCI_SUCCESS as u8)
        .is_err()
    {
        return false;
    }
    eprintln!("BLE HCI CommandComplete opcode={opcode:#06x} status=0");
    route_to_gap(cpu, msg)
}

fn finish_failed_alloc(cpu: &mut Processor) -> bool {
    cpu.set_r(Reg::R0, HCI_ERROR_MEM_CAP_EXCEEDED);
    cpu.set_pc(cpu.get_r(Reg::R12) & !1);
    true
}

fn finish_send(cpu: &mut Processor) -> bool {
    let send_status = cpu.get_r(Reg::R0);
    cpu.set_r(
        Reg::R0,
        if send_status == 0 {
            HCI_SUCCESS
        } else {
            send_status
        },
    );
    cpu.set_r(Reg::R2, 0);
    cpu.set_r(Reg::R3, 0);
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
        assert_eq!(OPCODE_LE_SET_ADV_PARAM, 0x2006);
        assert_eq!(OPCODE_LE_SET_ADV_DATA, 0x2008);
        assert_eq!(OPCODE_LE_SET_SCAN_RSP_DATA, 0x2009);
        assert_eq!(OPCODE_LE_SET_ADV_ENABLE, 0x200A);
        assert_eq!(OPCODE_LE_SET_SCAN_PARAM, 0x200B);
        assert_eq!(OPCODE_LE_SET_SCAN_ENABLE, 0x200C);
    }

    #[test]
    fn connection_event_layout_is_arm32_sdk_layout() {
        assert_eq!(CONN_COMPLETE_BYTES, 22);
        assert_eq!(DISCONN_COMPLETE_BYTES, 8);
        assert_eq!(HCI_DATA_EVENT_BYTES, 12);
    }

    #[test]
    fn notification_parser_accepts_l2cap_att() {
        let pdu = [3, 0, 4, 0, 0x1B, 1, 0, 0xAA];
        assert_eq!(att_notification_value(&pdu), Some(&[0xAA][..]));
    }

    #[test]
    fn canonical_uuid_parser_accepts_128_and_16_bit() {
        assert_eq!(
            parse_uuid_bytes("00112233-4455-6677-8899-AABBCCDDEEFF")
                .unwrap()
                .len(),
            16
        );
        assert_eq!(parse_uuid_bytes("2902"), Some(vec![0x29, 0x02]));
        assert!(parse_uuid_bytes("bad").is_none());
    }
}
