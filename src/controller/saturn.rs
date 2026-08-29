//! Saturn-PLC package for Firmverse.
//!
//! The program artifact is the real `.fbdbin` consumed by upstream fbd-runtime
//! v11. Rust owns validation, terminal metadata and the safe frontend API; C owns
//! FBD execution semantics.

#[cfg(firmverse_saturn_native)]
use std::ffi::CStr;
#[cfg(firmverse_saturn_native)]
use std::os::raw::{c_char, c_int};

const END_MARK: u8 = 0x94;
const ELEMENT_MASK: u8 = 0x3f;
const ELEM_INP_MDBS: u8 = 33;
const ELEM_OUT_MDBS: u8 = 34;
const ELEM_WP: u8 = 22;
const ELEM_SP: u8 = 23;
const INPUT_COUNTS: [usize; 41] = [
    1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0, 4, 3, 3, 5, 1, 1, 0, 2, 2, 2, 3, 2, 2, 2, 2,
    2, 0, 1, 2, 0, 1, 5, 1, 5,
];
const PARAM_COUNTS: [usize; 41] = [
    1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 0, 0, 0, 0, 0, 1, 5, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 3, 2, 0, 4, 2, 1, 66, 0,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalDirection {
    Input,
    Output,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalKind {
    Discrete,
    Analog,
    Temperature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalProfile {
    pub name: &'static str,
    pub runtime_index: i32,
    pub direction: TerminalDirection,
    pub kind: TerminalKind,
}

pub const INPUT_TERMINALS: &[TerminalProfile] = &[
    TerminalProfile {
        name: "DI1",
        runtime_index: 1,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DI2",
        runtime_index: 2,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DI3",
        runtime_index: 3,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DI4",
        runtime_index: 4,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DI5",
        runtime_index: 5,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DI6",
        runtime_index: 6,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DI7",
        runtime_index: 7,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DI8",
        runtime_index: 8,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DI9",
        runtime_index: 9,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DI10",
        runtime_index: 10,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "AI1",
        runtime_index: 11,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Analog,
    },
    TerminalProfile {
        name: "AI2",
        runtime_index: 12,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Analog,
    },
    TerminalProfile {
        name: "T1",
        runtime_index: 13,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Temperature,
    },
    TerminalProfile {
        name: "T2",
        runtime_index: 14,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Temperature,
    },
    TerminalProfile {
        name: "T3",
        runtime_index: 15,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Temperature,
    },
    TerminalProfile {
        name: "T4",
        runtime_index: 16,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Temperature,
    },
    TerminalProfile {
        name: "T5",
        runtime_index: 17,
        direction: TerminalDirection::Input,
        kind: TerminalKind::Temperature,
    },
];

pub const OUTPUT_TERMINALS: &[TerminalProfile] = &[
    TerminalProfile {
        name: "DO1",
        runtime_index: 1,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DO2",
        runtime_index: 2,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DO3",
        runtime_index: 3,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DO4",
        runtime_index: 4,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DO5",
        runtime_index: 5,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DO6",
        runtime_index: 6,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DO7",
        runtime_index: 7,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DO8",
        runtime_index: 8,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DO9",
        runtime_index: 9,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DO10",
        runtime_index: 10,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "DO11",
        runtime_index: 11,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Discrete,
    },
    TerminalProfile {
        name: "AO1",
        runtime_index: 12,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Analog,
    },
    TerminalProfile {
        name: "AO2",
        runtime_index: 13,
        direction: TerminalDirection::Output,
        kind: TerminalKind::Analog,
    },
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FbdbinInfo {
    pub element_count: usize,
    pub watchpoint_count: usize,
    pub setpoint_count: usize,
    pub required_rtl: i32,
    pub screen_count: i32,
    pub hint_count: i32,
    pub schema_size: usize,
    pub crc_checked: bool,
    pub uses_modbus: bool,
}

fn read_i32(bytes: &[u8], offset: usize) -> Result<i32, String> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "truncated int32 in fbdbin".to_string())?;
    Ok(i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

pub fn fbd_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = if crc & 1 != 0 { 0xffff_ffff } else { 0 };
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    crc
}

pub fn inspect_fbdbin(bytes: &[u8]) -> Result<FbdbinInfo, String> {
    if bytes.is_empty() {
        return Err("empty fbdbin".into());
    }

    let mut cursor = 0usize;
    let mut input_total = 0usize;
    let mut parameter_total = 0usize;
    let mut element_count = 0usize;
    let mut watchpoint_count = 0usize;
    let mut setpoint_count = 0usize;
    let mut uses_modbus = false;

    loop {
        let raw = *bytes
            .get(cursor)
            .ok_or_else(|| "fbdbin has no END_MARK".to_string())?;
        cursor += 1;
        if raw & 0x80 != 0 {
            if raw != END_MARK {
                return Err(format!(
                    "invalid FBD END_MARK 0x{raw:02x}, expected 0x{END_MARK:02x}"
                ));
            }
            break;
        }
        let kind = raw & ELEMENT_MASK;
        let index = usize::from(kind);
        if index >= INPUT_COUNTS.len() {
            return Err(format!("unsupported FBD element type {kind}"));
        }
        input_total = input_total
            .checked_add(INPUT_COUNTS[index])
            .ok_or_else(|| "FBD input count overflow".to_string())?;
        parameter_total = parameter_total
            .checked_add(PARAM_COUNTS[index])
            .ok_or_else(|| "FBD parameter count overflow".to_string())?;
        element_count += 1;
        watchpoint_count += usize::from(kind == ELEM_WP);
        setpoint_count += usize::from(kind == ELEM_SP);
        uses_modbus |= kind == ELEM_INP_MDBS || kind == ELEM_OUT_MDBS;
    }

    let inputs_bytes = input_total
        .checked_mul(2)
        .ok_or_else(|| "FBD input byte count overflow".to_string())?;
    let parameters_bytes = parameter_total
        .checked_mul(4)
        .ok_or_else(|| "FBD parameter byte count overflow".to_string())?;
    cursor = cursor
        .checked_add(inputs_bytes)
        .and_then(|value| value.checked_add(parameters_bytes))
        .ok_or_else(|| "FBD layout overflow".to_string())?;
    if cursor >= bytes.len() {
        return Err("fbdbin is truncated before global options".into());
    }

    let option_count = usize::from(bytes[cursor]);
    cursor += 1;
    let options_bytes = option_count
        .checked_mul(4)
        .ok_or_else(|| "FBD option byte count overflow".to_string())?;
    if cursor + options_bytes > bytes.len() {
        return Err("fbdbin is truncated in global options".into());
    }
    let option = |index: usize| -> Result<i32, String> {
        if index >= option_count {
            return Ok(0);
        }
        read_i32(bytes, cursor + index * 4)
    };

    let required_rtl = option(0)?;
    let screen_count = option(4)?;
    let declared_size = option(5)?;
    let hint_count = option(6)?;
    if required_rtl < 0 || screen_count < 0 || hint_count < 0 || declared_size < 0 {
        return Err("negative FBD global option".into());
    }

    let schema_size = usize::try_from(declared_size).map_err(|_| "invalid FBD schema size")?;
    let crc_checked = schema_size != 0;
    if crc_checked {
        if schema_size != bytes.len() {
            return Err(format!(
                "FBD schema size mismatch: header={schema_size}, file={}",
                bytes.len()
            ));
        }
        if fbd_crc32(bytes) != 0 {
            return Err("FBD CRC32 mismatch".into());
        }
    }

    Ok(FbdbinInfo {
        element_count,
        watchpoint_count,
        setpoint_count,
        required_rtl,
        screen_count,
        hint_count,
        schema_size,
        crc_checked,
        uses_modbus,
    })
}

pub fn input_terminal(name: &str) -> Option<&'static TerminalProfile> {
    INPUT_TERMINALS
        .iter()
        .find(|terminal| terminal.name.eq_ignore_ascii_case(name))
}

pub fn output_terminal(name: &str) -> Option<&'static TerminalProfile> {
    OUTPUT_TERMINALS
        .iter()
        .find(|terminal| terminal.name.eq_ignore_ascii_case(name))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HardwareProperty {
    Ethernet = 0,
    Ntp = 1,
    TimezoneMinutes = 2,
    BatteryCentivolts = 3,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectDescription {
    pub name: String,
    pub version: String,
    pub build_time: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Setpoint {
    pub index: usize,
    pub caption: String,
    pub value: i32,
    pub low: i32,
    pub high: i32,
    pub default: i32,
    pub divider: i32,
    pub step: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Watchpoint {
    pub index: usize,
    pub caption: String,
    pub value: i32,
    pub divider: i32,
}

pub const fn native_runtime_available() -> bool {
    cfg!(firmverse_saturn_native)
}

fn decode_cp1251(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match *byte {
            0x00..=0x7f => char::from(*byte),
            0xa8 => 'Ё',
            0xb0 => '°',
            0xb8 => 'ё',
            0xb9 => '№',
            0xc0..=0xff => char::from_u32(0x0410 + u32::from(*byte - 0xc0)).unwrap_or('�'),
            _ => '�',
        })
        .collect()
}

#[cfg(firmverse_saturn_native)]
fn c_text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    // SAFETY: upstream FBD HMI returns stable NUL-terminated pointers owned by
    // the loaded schema for the lifetime guarded by SaturnPlc.
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    decode_cp1251(bytes)
}

#[cfg(firmverse_saturn_native)]
unsafe extern "C" {
    fn fv_fbd_load(data: *const u8, length: c_int, reset_nvram: c_int) -> c_int;
    fn fv_fbd_unload();
    fn fv_fbd_memory_size() -> c_int;
    fn fv_fbd_step(period: c_int);
    fn fv_fbd_set_input(pin: c_int, value: c_int);
    fn fv_fbd_get_input(pin: c_int) -> c_int;
    fn fv_fbd_get_output(pin: c_int) -> c_int;
    fn fv_fbd_set_hardware(index: c_int, value: c_int);
    fn fv_fbd_sp_count() -> c_int;
    fn fv_fbd_sp_value(index: c_int) -> c_int;
    fn fv_fbd_sp_low(index: c_int) -> c_int;
    fn fv_fbd_sp_high(index: c_int) -> c_int;
    fn fv_fbd_sp_default(index: c_int) -> c_int;
    fn fv_fbd_sp_divider(index: c_int) -> c_int;
    fn fv_fbd_sp_step(index: c_int) -> c_int;
    fn fv_fbd_sp_caption(index: c_int) -> *const c_char;
    fn fv_fbd_sp_set(index: c_int, value: c_int);
    fn fv_fbd_wp_count() -> c_int;
    fn fv_fbd_wp_value(index: c_int) -> c_int;
    fn fv_fbd_wp_divider(index: c_int) -> c_int;
    fn fv_fbd_wp_caption(index: c_int) -> *const c_char;
    fn fv_fbd_project_field(field: c_int) -> *const c_char;
    fn fv_fbd_io_hint(kind: c_int, index: c_int) -> *const c_char;
}

#[cfg(firmverse_saturn_native)]
static RUNTIME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(firmverse_saturn_native)]
pub struct SaturnPlc {
    info: FbdbinInfo,
    memory_bytes: usize,
    _runtime: std::sync::MutexGuard<'static, ()>,
}

#[cfg(firmverse_saturn_native)]
impl SaturnPlc {
    pub fn load(bytes: &[u8], reset_nvram: bool) -> Result<Self, String> {
        let info = inspect_fbdbin(bytes)?;
        let length = c_int::try_from(bytes.len()).map_err(|_| "fbdbin is too large".to_string())?;
        let runtime = RUNTIME_LOCK
            .lock()
            .map_err(|_| "Saturn FBD runtime lock is poisoned".to_string())?;
        // SAFETY: inspection validates the file boundary; the C bridge copies
        // the bytes before fbd-runtime stores its schema pointer.
        let result = unsafe { fv_fbd_load(bytes.as_ptr(), length, c_int::from(reset_nvram)) };
        if result <= 0 {
            return Err(format!("fbd-runtime rejected program with code {result}"));
        }
        // SAFETY: runtime is initialized by the successful load above.
        let memory_bytes = usize::try_from(unsafe { fv_fbd_memory_size() })
            .map_err(|_| "invalid fbd-runtime memory size".to_string())?;
        Ok(Self {
            info,
            memory_bytes,
            _runtime: runtime,
        })
    }

    pub fn info(&self) -> &FbdbinInfo {
        &self.info
    }

    pub const fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    pub fn project(&self) -> ProjectDescription {
        // SAFETY: a SaturnPlc instance owns the runtime lock and loaded schema.
        unsafe {
            ProjectDescription {
                name: c_text(fv_fbd_project_field(0)),
                version: c_text(fv_fbd_project_field(1)),
                build_time: c_text(fv_fbd_project_field(2)),
            }
        }
    }

    pub fn set_input(&mut self, terminal: &str, value: i32) -> Result<(), String> {
        let terminal = input_terminal(terminal)
            .ok_or_else(|| format!("unknown Saturn-PLC input terminal {terminal:?}"))?;
        // SAFETY: terminal indexes are bounded by the bridge profile.
        unsafe { fv_fbd_set_input(terminal.runtime_index, value) };
        Ok(())
    }

    pub fn input(&self, terminal: &str) -> Result<i32, String> {
        let terminal = input_terminal(terminal)
            .ok_or_else(|| format!("unknown Saturn-PLC input terminal {terminal:?}"))?;
        // SAFETY: terminal indexes are bounded by the bridge profile.
        Ok(unsafe { fv_fbd_get_input(terminal.runtime_index) })
    }

    pub fn output(&self, terminal: &str) -> Result<i32, String> {
        let terminal = output_terminal(terminal)
            .ok_or_else(|| format!("unknown Saturn-PLC output terminal {terminal:?}"))?;
        // SAFETY: terminal indexes are bounded by the bridge profile.
        Ok(unsafe { fv_fbd_get_output(terminal.runtime_index) })
    }

    pub fn outputs(&self) -> Vec<(&'static str, i32)> {
        OUTPUT_TERMINALS
            .iter()
            .map(|terminal| {
                // SAFETY: terminal indexes are bounded by the bridge profile.
                (terminal.name, unsafe {
                    fv_fbd_get_output(terminal.runtime_index)
                })
            })
            .collect()
    }

    pub fn set_hardware(&mut self, property: HardwareProperty, value: i32) {
        // SAFETY: HardwareProperty values follow FBD_GETHRDWTYPE v11.
        unsafe { fv_fbd_set_hardware(property as c_int, value) };
    }

    pub fn step(&mut self, period_ms: u32) -> Result<(), String> {
        let period =
            c_int::try_from(period_ms).map_err(|_| "FBD period exceeds int32".to_string())?;
        // SAFETY: runtime is initialized and exclusively locked by self.
        unsafe { fv_fbd_step(period) };
        Ok(())
    }

    pub fn setpoints(&self) -> Vec<Setpoint> {
        // SAFETY: runtime is initialized and exclusively locked by self.
        let count = unsafe { fv_fbd_sp_count() }.max(0) as usize;
        (0..count)
            .filter_map(|index| self.setpoint(index))
            .collect()
    }

    pub fn setpoint(&self, index: usize) -> Option<Setpoint> {
        let raw = c_int::try_from(index).ok()?;
        // SAFETY: count check prevents an invalid HMI index.
        if raw >= unsafe { fv_fbd_sp_count() } {
            return None;
        }
        // SAFETY: runtime is initialized and raw is in range.
        Some(unsafe {
            Setpoint {
                index,
                caption: c_text(fv_fbd_sp_caption(raw)),
                value: fv_fbd_sp_value(raw),
                low: fv_fbd_sp_low(raw),
                high: fv_fbd_sp_high(raw),
                default: fv_fbd_sp_default(raw),
                divider: fv_fbd_sp_divider(raw),
                step: fv_fbd_sp_step(raw),
            }
        })
    }

    pub fn set_setpoint(&mut self, index: usize, value: i32) -> Result<(), String> {
        let point = self
            .setpoint(index)
            .ok_or_else(|| format!("unknown Saturn-PLC setpoint {index}"))?;
        if value < point.low || value > point.high {
            return Err(format!(
                "setpoint {} value {} is outside {}..{}",
                point.caption, value, point.low, point.high
            ));
        }
        let raw = c_int::try_from(index).map_err(|_| "setpoint index exceeds int32".to_string())?;
        // SAFETY: range was checked through fbdHMIgetSP above.
        unsafe { fv_fbd_sp_set(raw, value) };
        Ok(())
    }

    pub fn watchpoints(&self) -> Vec<Watchpoint> {
        // SAFETY: runtime is initialized and exclusively locked by self.
        let count = unsafe { fv_fbd_wp_count() }.max(0) as usize;
        (0..count)
            .filter_map(|index| self.watchpoint(index))
            .collect()
    }

    pub fn watchpoint(&self, index: usize) -> Option<Watchpoint> {
        let raw = c_int::try_from(index).ok()?;
        // SAFETY: count check prevents an invalid HMI index.
        if raw >= unsafe { fv_fbd_wp_count() } {
            return None;
        }
        // SAFETY: runtime is initialized and raw is in range.
        Some(unsafe {
            Watchpoint {
                index,
                caption: c_text(fv_fbd_wp_caption(raw)),
                value: fv_fbd_wp_value(raw),
                divider: fv_fbd_wp_divider(raw),
            }
        })
    }

    pub fn io_hint(&self, direction: TerminalDirection, terminal: &str) -> Result<String, String> {
        let profile = match direction {
            TerminalDirection::Input => input_terminal(terminal),
            TerminalDirection::Output => output_terminal(terminal),
        }
        .ok_or_else(|| format!("unknown Saturn-PLC terminal {terminal:?}"))?;
        let kind = match direction {
            TerminalDirection::Input => 0,
            TerminalDirection::Output => 1,
        };
        // SAFETY: runtime is initialized and terminal index is bounded.
        Ok(unsafe { c_text(fv_fbd_io_hint(kind, profile.runtime_index)) })
    }
}

#[cfg(firmverse_saturn_native)]
impl Drop for SaturnPlc {
    fn drop(&mut self) {
        // SAFETY: self owns the process-global runtime lock.
        unsafe { fv_fbd_unload() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_i32(out: &mut Vec<u8>, value: i32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn test_program() -> Vec<u8> {
        // INP_PIN(1) -> AND(CONST 1) -> OUT_PIN(1) + WP, plus one SP.
        let mut bytes = vec![15, 1, 3, 0, 22, 23, END_MARK];
        for input in [0u16, 1, 2, 2] {
            push_u16(&mut bytes, input);
        }
        for parameter in [1, 1, 1, 0, 0, 100, 50, 0, 1] {
            push_i32(&mut bytes, parameter);
        }

        bytes.push(10);
        let options_offset = bytes.len();
        for option in [8, 0, 0, 0, 0, 0, 2, 0, 0, 0] {
            push_i32(&mut bytes, option);
        }

        bytes.extend_from_slice(b"RUN\0SP\0Saturn test\0v1\x002026-08-29\0");
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        bytes.extend_from_slice(&[0, 1]);
        bytes.extend_from_slice(b"Input DI1\0");
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend_from_slice(b"Output DO1\0");

        let final_size = bytes.len() + 4;
        let size_offset = options_offset + 5 * 4;
        bytes[size_offset..size_offset + 4].copy_from_slice(&(final_size as i32).to_le_bytes());
        let crc = fbd_crc32(&bytes);
        bytes.extend_from_slice(&crc.to_le_bytes());
        bytes
    }

    #[test]
    fn inspects_real_fbdbin_layout_and_crc() {
        let program = test_program();
        let info = inspect_fbdbin(&program).expect("inspect test program");
        assert_eq!(info.element_count, 6);
        assert_eq!(info.watchpoint_count, 1);
        assert_eq!(info.setpoint_count, 1);
        assert_eq!(info.required_rtl, 8);
        assert_eq!(info.hint_count, 2);
        assert_eq!(info.schema_size, program.len());
        assert!(info.crc_checked);
        assert_eq!(fbd_crc32(&program), 0);
    }

    #[test]
    fn rejects_size_or_crc_drift() {
        let mut program = test_program();
        let marker = program
            .windows(b"Saturn test".len())
            .position(|window| window == b"Saturn test")
            .expect("project caption");
        program[marker] ^= 1;
        assert!(inspect_fbdbin(&program).unwrap_err().contains("CRC32"));
    }

    #[test]
    fn base_terminal_map_matches_saturn_plc_contract() {
        assert_eq!(input_terminal("DI1").map(|p| p.runtime_index), Some(1));
        assert_eq!(input_terminal("AI1").map(|p| p.runtime_index), Some(11));
        assert_eq!(input_terminal("T5").map(|p| p.runtime_index), Some(17));
        assert_eq!(output_terminal("DO11").map(|p| p.runtime_index), Some(11));
        assert_eq!(output_terminal("AO2").map(|p| p.runtime_index), Some(13));
    }

    #[cfg(firmverse_saturn_native)]
    #[test]
    fn exact_upstream_runtime_executes_io_hmi_and_metadata() {
        let program = test_program();
        let mut plc = SaturnPlc::load(&program, true).expect("load FBD");
        assert!(plc.memory_bytes() > 0);
        assert_eq!(plc.project().name, "Saturn test");
        assert_eq!(plc.setpoint(0).map(|point| point.value), Some(50));
        assert_eq!(
            plc.io_hint(TerminalDirection::Input, "DI1").unwrap(),
            "Input DI1"
        );

        plc.set_input("DI1", 1).unwrap();
        plc.step(0).unwrap();
        assert_eq!(plc.output("DO1").unwrap(), 1);
        assert_eq!(plc.watchpoint(0).map(|point| point.value), Some(1));

        plc.set_input("DI1", 0).unwrap();
        plc.step(10).unwrap();
        assert_eq!(plc.output("DO1").unwrap(), 0);
        assert_eq!(plc.watchpoint(0).map(|point| point.value), Some(0));

        plc.set_setpoint(0, 75).unwrap();
        assert_eq!(plc.setpoint(0).map(|point| point.value), Some(75));
        assert!(plc.set_setpoint(0, 101).is_err());
    }
}
