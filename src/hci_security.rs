use crate::aes::aes128_encrypt_block;
use crate::mailbox;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

// Thumb entrypoints from the pinned public PHY6252 ROM symbol map.
const ROM_HCI_LE_ENCRYPT_CMD: u32 = 0x0000_1A88;
const ROM_HCI_LE_LTK_REQ_NEG_REPLY_CMD: u32 = 0x0000_1B08;
const ROM_HCI_LE_LTK_REQ_REPLY_CMD: u32 = 0x0000_1B30;
const ROM_HCI_LE_START_ENCRYPT_CMD: u32 = 0x0000_22E0;
const ROM_OSAL_MSG_ALLOC: u32 = 0x0001_4D1C;
const ROM_OSAL_MSG_SEND: u32 = 0x0001_4F58;

const HCI_SMP_TASK_ID: u32 = 0x1FFF_0910;
const CONT_TRAP: u32 = 0x0000_00D2;
const CONT_MAGIC: u32 = 0x5345_4355; // "SECU"

const STAGE_COMPLETE_ALLOC: u32 = 1;
const STAGE_COMPLETE_SEND: u32 = 2;
const STAGE_STATUS_ALLOC: u32 = 3;
const STAGE_STATUS_SEND: u32 = 4;
const STAGE_ENC_CHANGE_ALLOC: u32 = 5;
const STAGE_ENC_CHANGE_SEND: u32 = 6;

const SHADOW_OPCODE: u32 = mailbox::BASE + 0x700;
const SHADOW_PARAM_LEN: u32 = mailbox::BASE + 0x704;
const SHADOW_PARAMS: u32 = mailbox::BASE + 0x708;
const SHADOW_STATUS: u32 = mailbox::BASE + 0x730;
const SHADOW_FOLLOWUP: u32 = mailbox::BASE + 0x734;
const SHADOW_ENCRYPTED: u32 = mailbox::BASE + 0x738;
const MAX_PARAMS: usize = 32;

const HCI_SMP_EVENT_EVENT: u8 = 0x92;
const HCI_COMMAND_COMPLETE_EVENT_CODE: u8 = 0x0E;
const HCI_COMMAND_STATUS_EVENT_CODE: u8 = 0x0F;
const HCI_ENCRYPTION_CHANGE_EVENT_CODE: u8 = 0x08;
const HCI_SUCCESS: u8 = 0x00;
const HCI_ERROR_UNKNOWN_CONN_HANDLE: u8 = 0x02;
const HCI_ERROR_MEM_CAP_EXCEEDED: u8 = 0x07;
const HCI_ERROR_INVALID_PARAMS: u8 = 0x12;

const OPCODE_LE_ENCRYPT: u16 = 0x2017;
const OPCODE_LE_START_ENCRYPTION: u16 = 0x2019;
const OPCODE_LE_LTK_REQ_REPLY: u16 = 0x201A;
const OPCODE_LE_LTK_REQ_NEG_REPLY: u16 = 0x201B;

const CMD_COMPLETE_BYTES: u32 = 12;
const CMD_STATUS_BYTES: u32 = 6;
const ENC_CHANGE_BYTES: u32 = 8;

pub fn handle(cpu: &mut Processor) -> bool {
    match cpu.get_pc() {
        ROM_HCI_LE_ENCRYPT_CMD => encrypt_cmd(cpu),
        ROM_HCI_LE_START_ENCRYPT_CMD => start_encrypt_cmd(cpu),
        ROM_HCI_LE_LTK_REQ_REPLY_CMD => ltk_reply_cmd(cpu),
        ROM_HCI_LE_LTK_REQ_NEG_REPLY_CMD => ltk_neg_reply_cmd(cpu),
        CONT_TRAP if cpu.get_r(Reg::R2) == CONT_MAGIC => continue_event(cpu),
        _ => false,
    }
}

fn encrypt_cmd(cpu: &mut Processor) -> bool {
    let key_ptr = cpu.get_r(Reg::R0);
    let plaintext_ptr = cpu.get_r(Reg::R1);
    let Some(key) = read_block(cpu, key_ptr) else { return immediate(cpu, HCI_ERROR_INVALID_PARAMS); };
    let Some(plaintext) = read_block(cpu, plaintext_ptr) else { return immediate(cpu, HCI_ERROR_INVALID_PARAMS); };
    let encrypted = aes128_encrypt_block(key, plaintext);
    let mut params = [0u8; 17];
    params[0] = HCI_SUCCESS;
    params[1..].copy_from_slice(&encrypted);
    eprintln!("BLE HCI LE_Encrypt AES-128 command-complete -> SMP");
    begin_complete(cpu, OPCODE_LE_ENCRYPT, &params)
}

fn start_encrypt_cmd(cpu: &mut Processor) -> bool {
    let conn_handle = cpu.get_r(Reg::R0) as u16;
    let random = cpu.get_r(Reg::R1);
    let enc_div = cpu.get_r(Reg::R2);
    let ltk = cpu.get_r(Reg::R3);
    if conn_handle != 0 || !host_connected(cpu) {
        return immediate(cpu, HCI_ERROR_UNKNOWN_CONN_HANDLE);
    }
    if !readable(cpu, random, 8) || !readable(cpu, enc_div, 2) || !readable(cpu, ltk, 16) {
        return immediate(cpu, HCI_ERROR_INVALID_PARAMS);
    }
    let _ = cpu.write32(SHADOW_ENCRYPTED, 1);
    eprintln!("BLE HCI LE_StartEncryption handle=0 accepted -> SMP encryption-change");
    begin_status(cpu, OPCODE_LE_START_ENCRYPTION, HCI_SUCCESS, true)
}

fn ltk_reply_cmd(cpu: &mut Processor) -> bool {
    let conn_handle = cpu.get_r(Reg::R0) as u16;
    let ltk = cpu.get_r(Reg::R1);
    let status = if conn_handle != 0 || !host_connected(cpu) {
        HCI_ERROR_UNKNOWN_CONN_HANDLE
    } else if !readable(cpu, ltk, 16) {
        HCI_ERROR_INVALID_PARAMS
    } else {
        let _ = cpu.write32(SHADOW_ENCRYPTED, 1);
        HCI_SUCCESS
    };
    let params = [status, conn_handle as u8, (conn_handle >> 8) as u8];
    eprintln!("BLE HCI LE_LtkReqReply handle={conn_handle} status={status:#04x}");
    begin_complete(cpu, OPCODE_LE_LTK_REQ_REPLY, &params)
}

fn ltk_neg_reply_cmd(cpu: &mut Processor) -> bool {
    let conn_handle = cpu.get_r(Reg::R0) as u16;
    let status = if conn_handle == 0 && host_connected(cpu) {
        HCI_SUCCESS
    } else {
        HCI_ERROR_UNKNOWN_CONN_HANDLE
    };
    let params = [status, conn_handle as u8, (conn_handle >> 8) as u8];
    eprintln!("BLE HCI LE_LtkReqNegReply handle={conn_handle} status={status:#04x}");
    begin_complete(cpu, OPCODE_LE_LTK_REQ_NEG_REPLY, &params)
}

fn begin_complete(cpu: &mut Processor, opcode: u16, params: &[u8]) -> bool {
    let len = params.len().min(MAX_PARAMS);
    if cpu.write16(SHADOW_OPCODE, opcode).is_err()
        || cpu.write32(SHADOW_PARAM_LEN, len as u32).is_err()
        || cpu.write32(SHADOW_FOLLOWUP, 0).is_err()
    {
        return false;
    }
    for (i, byte) in params.iter().take(len).copied().enumerate() {
        if cpu.write8(SHADOW_PARAMS + i as u32, byte).is_err() { return false; }
    }
    begin_alloc(cpu, CMD_COMPLETE_BYTES + len as u32, STAGE_COMPLETE_ALLOC, true)
}

fn begin_status(cpu: &mut Processor, opcode: u16, status: u8, followup: bool) -> bool {
    let followup_word = if followup { 1 } else { 0 };
    if cpu.write16(SHADOW_OPCODE, opcode).is_err()
        || cpu.write8(SHADOW_STATUS, status).is_err()
        || cpu.write32(SHADOW_FOLLOWUP, followup_word).is_err()
    {
        return false;
    }
    begin_alloc(cpu, CMD_STATUS_BYTES, STAGE_STATUS_ALLOC, true)
}

fn begin_alloc(cpu: &mut Processor, bytes: u32, stage: u32, save_return: bool) -> bool {
    if save_return {
        cpu.set_r(Reg::R12, cpu.get_r(Reg::LR));
    }
    cpu.set_r(Reg::R2, CONT_MAGIC);
    cpu.set_r(Reg::R3, stage);
    cpu.set_r(Reg::R0, bytes);
    cpu.set_r(Reg::LR, CONT_TRAP | 1);
    cpu.set_pc(ROM_OSAL_MSG_ALLOC);
    false
}

fn continue_event(cpu: &mut Processor) -> bool {
    match cpu.get_r(Reg::R3) {
        STAGE_COMPLETE_ALLOC => finish_complete_alloc(cpu),
        STAGE_COMPLETE_SEND => finish_return(cpu),
        STAGE_STATUS_ALLOC => finish_status_alloc(cpu),
        STAGE_STATUS_SEND => finish_status_send(cpu),
        STAGE_ENC_CHANGE_ALLOC => finish_enc_change_alloc(cpu),
        STAGE_ENC_CHANGE_SEND => finish_return(cpu),
        _ => false,
    }
}

fn finish_complete_alloc(cpu: &mut Processor) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 { return finish_alloc_error(cpu); }
    let opcode = match cpu.read16(SHADOW_OPCODE) { Ok(v) => v, Err(_) => return false };
    let len = match cpu.read32(SHADOW_PARAM_LEN) { Ok(v) => v.min(MAX_PARAMS as u32), Err(_) => return false };
    let ret_ptr = msg + CMD_COMPLETE_BYTES;
    if cpu.write8(msg, HCI_SMP_EVENT_EVENT).is_err()
        || cpu.write8(msg + 1, HCI_COMMAND_COMPLETE_EVENT_CODE).is_err()
        || cpu.write8(msg + 2, 1).is_err()
        || cpu.write8(msg + 3, 0).is_err()
        || cpu.write16(msg + 4, opcode).is_err()
        || cpu.write16(msg + 6, 0).is_err()
        || cpu.write32(msg + 8, ret_ptr).is_err()
    {
        return false;
    }
    for i in 0..len {
        let byte = match cpu.read8(SHADOW_PARAMS + i) { Ok(v) => v, Err(_) => return false };
        if cpu.write8(ret_ptr + i, byte).is_err() { return false; }
    }
    route_smp(cpu, msg, STAGE_COMPLETE_SEND)
}

fn finish_status_alloc(cpu: &mut Processor) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 { return finish_alloc_error(cpu); }
    let opcode = match cpu.read16(SHADOW_OPCODE) { Ok(v) => v, Err(_) => return false };
    let status = match cpu.read8(SHADOW_STATUS) { Ok(v) => v, Err(_) => return false };
    if cpu.write8(msg, HCI_SMP_EVENT_EVENT).is_err()
        || cpu.write8(msg + 1, HCI_COMMAND_STATUS_EVENT_CODE).is_err()
        || cpu.write8(msg + 2, status).is_err()
        || cpu.write8(msg + 3, 1).is_err()
        || cpu.write16(msg + 4, opcode).is_err()
    {
        return false;
    }
    route_smp(cpu, msg, STAGE_STATUS_SEND)
}

fn finish_status_send(cpu: &mut Processor) -> bool {
    if cpu.get_r(Reg::R0) != 0 {
        return finish_return(cpu);
    }
    let followup = cpu.read32(SHADOW_FOLLOWUP).unwrap_or(0) != 0;
    if !followup {
        return finish_return(cpu);
    }
    begin_alloc(cpu, ENC_CHANGE_BYTES, STAGE_ENC_CHANGE_ALLOC, false)
}

fn finish_enc_change_alloc(cpu: &mut Processor) -> bool {
    let msg = cpu.get_r(Reg::R0);
    if msg == 0 { return finish_alloc_error(cpu); }
    // ARM32 layout of hciEvt_EncryptChange_t:
    // hdr{event,status}, BLEEventCode, pad, connHandle, reason, encEnable.
    if cpu.write8(msg, HCI_SMP_EVENT_EVENT).is_err()
        || cpu.write8(msg + 1, HCI_ENCRYPTION_CHANGE_EVENT_CODE).is_err()
        || cpu.write8(msg + 2, 0).is_err()
        || cpu.write8(msg + 3, 0).is_err()
        || cpu.write16(msg + 4, 0).is_err()
        || cpu.write8(msg + 6, HCI_SUCCESS).is_err()
        || cpu.write8(msg + 7, 1).is_err()
    {
        return false;
    }
    eprintln!("BLE HCI EncryptionChange handle=0 enabled=1 -> SMP");
    route_smp(cpu, msg, STAGE_ENC_CHANGE_SEND)
}

fn route_smp(cpu: &mut Processor, msg: u32, stage: u32) -> bool {
    let task = match cpu.read8(HCI_SMP_TASK_ID) {
        Ok(v) if v < 64 => v,
        _ => return finish_return(cpu),
    };
    cpu.set_r(Reg::R2, CONT_MAGIC);
    cpu.set_r(Reg::R3, stage);
    cpu.set_r(Reg::R0, u32::from(task));
    cpu.set_r(Reg::R1, msg);
    cpu.set_r(Reg::LR, CONT_TRAP | 1);
    cpu.set_pc(ROM_OSAL_MSG_SEND);
    false
}

fn finish_alloc_error(cpu: &mut Processor) -> bool {
    cpu.set_r(Reg::R0, HCI_ERROR_MEM_CAP_EXCEEDED as u32);
    cpu.set_r(Reg::R2, 0);
    cpu.set_r(Reg::R3, 0);
    cpu.set_pc(cpu.get_r(Reg::R12) & !1);
    true
}

fn finish_return(cpu: &mut Processor) -> bool {
    let send_status = cpu.get_r(Reg::R0);
    cpu.set_r(Reg::R0, if send_status == 0 { HCI_SUCCESS as u32 } else { send_status });
    cpu.set_r(Reg::R2, 0);
    cpu.set_r(Reg::R3, 0);
    cpu.set_pc(cpu.get_r(Reg::R12) & !1);
    true
}

fn immediate(cpu: &mut Processor, status: u8) -> bool {
    cpu.set_r(Reg::R0, status as u32);
    cpu.set_pc(cpu.get_r(Reg::LR) & !1);
    true
}

fn host_connected(cpu: &mut Processor) -> bool {
    mailbox::status(cpu)
        .map(|status| status & mailbox::STATUS_CONNECTED != 0)
        .unwrap_or(false)
}

fn readable(cpu: &mut Processor, ptr: u32, len: usize) -> bool {
    ptr != 0 && (0..len).all(|i| cpu.read8(ptr.wrapping_add(i as u32)).is_ok())
}

fn read_block(cpu: &mut Processor, ptr: u32) -> Option<[u8; 16]> {
    if ptr == 0 { return None; }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = cpu.read8(ptr.wrapping_add(i as u32)).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_hci_opcodes_match_bluetooth_le() {
        assert_eq!(OPCODE_LE_ENCRYPT, 0x2017);
        assert_eq!(OPCODE_LE_START_ENCRYPTION, 0x2019);
        assert_eq!(OPCODE_LE_LTK_REQ_REPLY, 0x201A);
        assert_eq!(OPCODE_LE_LTK_REQ_NEG_REPLY, 0x201B);
    }

    #[test]
    fn arm32_security_event_sizes_are_stable() {
        assert_eq!(CMD_STATUS_BYTES, 6);
        assert_eq!(ENC_CHANGE_BYTES, 8);
    }
}
