use crate::mailbox;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

// Thumb entrypoints from the pinned public PHY6252 ROM symbol map.
const ROM_HCI_COMMAND_COMPLETE_EVENT: u32 = 0x0000_1174;
const ROM_HCI_LE_READ_LOCAL_SUPPORTED_FEATURES: u32 = 0x0000_1C98;
const ROM_HCI_LE_READ_MAX_DATA_LENGTH: u32 = 0x0000_1CB8;
const ROM_HCI_LE_READ_RESOLVING_LIST_SIZE: u32 = 0x0000_1DC4;
const ROM_HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH: u32 = 0x0000_1DE4;
const ROM_HCI_LE_READ_SUPPORTED_STATES: u32 = 0x0000_1E14;
const ROM_HCI_LE_READ_WHITE_LIST_SIZE: u32 = 0x0000_1E3C;
const ROM_HCI_LE_REMOVE_RESOLVING_LIST: u32 = 0x0000_1EE8;
const ROM_HCI_LE_REMOVE_WHITE_LIST: u32 = 0x0000_1F0C;
const ROM_HCI_LE_SET_ADDRESS_RESOLUTION_ENABLE: u32 = 0x0000_1F30;
const ROM_HCI_LE_SET_DATA_LENGTH: u32 = 0x0000_1FB0;
const ROM_HCI_LE_SET_EVENT_MASK: u32 = 0x0000_1FF4;
const ROM_HCI_LE_SET_RPA_TIMEOUT: u32 = 0x0000_21E8;
const ROM_HCI_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH: u32 = 0x0000_2338;

const SHADOW: u32 = mailbox::BASE + 0x780;
const HCI_SUCCESS: u8 = 0x00;
const HCI_ERROR_UNKNOWN_CONN_HANDLE: u8 = 0x02;
const HCI_ERROR_INVALID_PARAMS: u8 = 0x12;

const OPCODE_LE_READ_LOCAL_SUPPORTED_FEATURES: u16 = 0x2003;
const OPCODE_LE_READ_WHITE_LIST_SIZE: u16 = 0x200F;
const OPCODE_LE_REMOVE_WHITE_LIST: u16 = 0x2012;
const OPCODE_LE_READ_SUPPORTED_STATES: u16 = 0x201C;
const OPCODE_LE_SET_DATA_LENGTH: u16 = 0x2022;
const OPCODE_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH: u16 = 0x2023;
const OPCODE_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH: u16 = 0x2024;
const OPCODE_LE_REMOVE_RESOLVING_LIST: u16 = 0x2028;
const OPCODE_LE_READ_RESOLVING_LIST_SIZE: u16 = 0x202A;
const OPCODE_LE_SET_ADDRESS_RESOLUTION_ENABLE: u16 = 0x202D;
const OPCODE_LE_SET_RPA_TIMEOUT: u16 = 0x202E;
const OPCODE_LE_READ_MAX_DATA_LENGTH: u16 = 0x202F;
const OPCODE_LE_SET_EVENT_MASK: u16 = 0x2001;

// Host-controller limits intentionally match the already exposed 251-byte ACL path.
const MAX_DATA_OCTETS: u16 = 251;
const MAX_DATA_TIME_US: u16 = 2120;
const WHITE_LIST_SIZE: u8 = 8;
const RESOLVING_LIST_SIZE: u8 = 8;

pub fn handle(cpu: &mut Processor) -> bool {
    match cpu.get_pc() {
        ROM_HCI_LE_READ_LOCAL_SUPPORTED_FEATURES => read_features(cpu),
        ROM_HCI_LE_READ_MAX_DATA_LENGTH => read_max_data_length(cpu),
        ROM_HCI_LE_READ_RESOLVING_LIST_SIZE => complete(cpu, OPCODE_LE_READ_RESOLVING_LIST_SIZE, &[HCI_SUCCESS, RESOLVING_LIST_SIZE]),
        ROM_HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH => read_suggested_data_length(cpu),
        ROM_HCI_LE_READ_SUPPORTED_STATES => read_supported_states(cpu),
        ROM_HCI_LE_READ_WHITE_LIST_SIZE => complete(cpu, OPCODE_LE_READ_WHITE_LIST_SIZE, &[HCI_SUCCESS, WHITE_LIST_SIZE]),
        ROM_HCI_LE_REMOVE_RESOLVING_LIST => remove_list_entry(cpu, OPCODE_LE_REMOVE_RESOLVING_LIST),
        ROM_HCI_LE_REMOVE_WHITE_LIST => remove_list_entry(cpu, OPCODE_LE_REMOVE_WHITE_LIST),
        ROM_HCI_LE_SET_ADDRESS_RESOLUTION_ENABLE => set_bool(cpu, OPCODE_LE_SET_ADDRESS_RESOLUTION_ENABLE),
        ROM_HCI_LE_SET_DATA_LENGTH => set_data_length(cpu),
        ROM_HCI_LE_SET_EVENT_MASK => set_event_mask(cpu),
        ROM_HCI_LE_SET_RPA_TIMEOUT => set_rpa_timeout(cpu),
        ROM_HCI_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH => write_suggested_data_length(cpu),
        _ => false,
    }
}

fn read_features(cpu: &mut Processor) -> bool {
    // LE Encryption, conn-parameter request, extended reject, peripheral feature
    // exchange, ping, data-length extension and LL privacy.
    let params = [HCI_SUCCESS, 0x7F, 0, 0, 0, 0, 0, 0, 0];
    eprintln!("BLE HCI LE_ReadLocalSupportedFeatures host-controller baseline");
    complete(cpu, OPCODE_LE_READ_LOCAL_SUPPORTED_FEATURES, &params)
}

fn read_max_data_length(cpu: &mut Processor) -> bool {
    let mut p = [0u8; 9];
    p[0] = HCI_SUCCESS;
    put16(&mut p[1..3], MAX_DATA_OCTETS);
    put16(&mut p[3..5], MAX_DATA_TIME_US);
    put16(&mut p[5..7], MAX_DATA_OCTETS);
    put16(&mut p[7..9], MAX_DATA_TIME_US);
    complete(cpu, OPCODE_LE_READ_MAX_DATA_LENGTH, &p)
}

fn read_suggested_data_length(cpu: &mut Processor) -> bool {
    let mut p = [0u8; 5];
    p[0] = HCI_SUCCESS;
    put16(&mut p[1..3], MAX_DATA_OCTETS);
    put16(&mut p[3..5], MAX_DATA_TIME_US);
    complete(cpu, OPCODE_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH, &p)
}

fn read_supported_states(cpu: &mut Processor) -> bool {
    // Peripheral/advertising plus connection combinations used by the generic
    // host bridge. Returning only supported states is safer than claiming the
    // full vendor radio scheduler is emulated.
    let params = [HCI_SUCCESS, 0x1F, 0x00, 0x00, 0x00, 0, 0, 0, 0];
    complete(cpu, OPCODE_LE_READ_SUPPORTED_STATES, &params)
}

fn remove_list_entry(cpu: &mut Processor, opcode: u16) -> bool {
    let addr_type = cpu.get_r(Reg::R0) as u8;
    let addr = cpu.get_r(Reg::R1);
    let status = if addr_type <= 1 && readable(cpu, addr, 6) { HCI_SUCCESS } else { HCI_ERROR_INVALID_PARAMS };
    complete(cpu, opcode, &[status])
}

fn set_bool(cpu: &mut Processor, opcode: u16) -> bool {
    let value = cpu.get_r(Reg::R0) as u8;
    complete(cpu, opcode, &[if value <= 1 { HCI_SUCCESS } else { HCI_ERROR_INVALID_PARAMS }])
}

fn set_event_mask(cpu: &mut Processor) -> bool {
    let mask = cpu.get_r(Reg::R0);
    let status = if readable(cpu, mask, 8) { HCI_SUCCESS } else { HCI_ERROR_INVALID_PARAMS };
    complete(cpu, OPCODE_LE_SET_EVENT_MASK, &[status])
}

fn set_rpa_timeout(cpu: &mut Processor) -> bool {
    let seconds = cpu.get_r(Reg::R0) as u16;
    let status = if (1..=0xA1B8).contains(&seconds) { HCI_SUCCESS } else { HCI_ERROR_INVALID_PARAMS };
    complete(cpu, OPCODE_LE_SET_RPA_TIMEOUT, &[status])
}

fn write_suggested_data_length(cpu: &mut Processor) -> bool {
    let octets = cpu.get_r(Reg::R0) as u16;
    let time = cpu.get_r(Reg::R1) as u16;
    let status = if valid_data_length(octets, time) { HCI_SUCCESS } else { HCI_ERROR_INVALID_PARAMS };
    complete(cpu, OPCODE_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH, &[status])
}

fn set_data_length(cpu: &mut Processor) -> bool {
    let handle = cpu.get_r(Reg::R0) as u16;
    let octets = cpu.get_r(Reg::R1) as u16;
    let time = cpu.get_r(Reg::R2) as u16;
    let connected = mailbox::status(cpu)
        .map(|s| s & mailbox::STATUS_CONNECTED != 0)
        .unwrap_or(false);
    let status = if handle != 0 || !connected {
        HCI_ERROR_UNKNOWN_CONN_HANDLE
    } else if !valid_data_length(octets, time) {
        HCI_ERROR_INVALID_PARAMS
    } else {
        HCI_SUCCESS
    };
    let params = [status, handle as u8, (handle >> 8) as u8];
    complete(cpu, OPCODE_LE_SET_DATA_LENGTH, &params)
}

fn valid_data_length(octets: u16, time: u16) -> bool {
    (27..=MAX_DATA_OCTETS).contains(&octets) && (328..=MAX_DATA_TIME_US).contains(&time)
}

fn complete(cpu: &mut Processor, opcode: u16, params: &[u8]) -> bool {
    if params.len() > 32 { return false; }
    for (i, byte) in params.iter().copied().enumerate() {
        if cpu.write8(SHADOW + i as u32, byte).is_err() { return false; }
    }
    // Reuse the already modeled ROM HCI_CommandCompleteEvent ABI. Its
    // continuation will return directly to this command's original caller.
    cpu.set_r(Reg::R0, u32::from(opcode));
    cpu.set_r(Reg::R1, params.len() as u32);
    cpu.set_r(Reg::R2, SHADOW);
    cpu.set_pc(ROM_HCI_COMMAND_COMPLETE_EVENT);
    true
}

fn readable(cpu: &mut Processor, ptr: u32, len: usize) -> bool {
    ptr != 0 && (0..len).all(|i| cpu.read8(ptr.wrapping_add(i as u32)).is_ok())
}

fn put16(dst: &mut [u8], value: u16) {
    dst.copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_data_length_matches_acl_bridge() {
        assert_eq!(MAX_DATA_OCTETS, 251);
        assert!(valid_data_length(27, 328));
        assert!(valid_data_length(251, 2120));
        assert!(!valid_data_length(26, 328));
    }

    #[test]
    fn privacy_and_dle_opcodes_match_hci_le() {
        assert_eq!(OPCODE_LE_SET_DATA_LENGTH, 0x2022);
        assert_eq!(OPCODE_LE_SET_ADDRESS_RESOLUTION_ENABLE, 0x202D);
        assert_eq!(OPCODE_LE_SET_RPA_TIMEOUT, 0x202E);
    }
}
