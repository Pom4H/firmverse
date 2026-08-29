//! Saturn-PLC `.fbdbin` compiler.
//!
//! This is the binary boundary of the Saturn package. Higher-level languages
//! should compile to `ControlIr`; only this module knows the exact FBD v11
//! byte layout.

use super::saturn::{fbd_crc32, inspect_fbdbin};
use serde_json::Value;
use std::collections::HashMap;

pub const CONTROL_IR_SCHEMA: &str = "firmverse/saturn-control-ir@1";
const END_MARK: u8 = 0x94;
const INVERT_FLAG: u8 = 0x40;
const ELEMENT_MASK: u8 = 0x3f;
const GLOBAL_OPTIONS_COUNT: usize = 10;
const OPT_REQ_VERSION: usize = 0;
const OPT_SCREEN_COUNT: usize = 4;
const OPT_SCHEMA_SIZE: usize = 5;
const OPT_HINTS_COUNT: usize = 6;

pub const ELEMENT_NAMES: [&str; 41] = [
    "OUT_PIN", "CONST", "NOT", "AND", "OR", "XOR", "RSTRG", "DTRG", "ADD", "SUB", "MUL", "DIV",
    "TON", "CMP", "OUT_VAR", "INP_PIN", "INP_VAR", "PID", "SUM", "COUNTER", "MUX", "ABS", "WP",
    "SP", "TP", "MIN", "MAX", "LIM", "EQ", "BAND", "BOR", "BXOR", "GEN", "INP_MDBS", "OUT_MDBS",
    "MOD", "MFUN", "EVENT", "LUT", "NLUT", "SUMM",
];

const INPUT_COUNTS: [usize; 41] = [
    1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0, 4, 3, 3, 5, 1, 1, 0, 2, 2, 2, 3, 2, 2, 2, 2,
    2, 0, 1, 2, 0, 1, 5, 1, 5,
];

const PARAM_COUNTS: [usize; 41] = [
    1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 0, 0, 0, 0, 0, 1, 5, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 3, 2, 0, 4, 2, 1, 66, 0,
];

pub const ELEM_OUT_PIN: u8 = 0;
pub const ELEM_CONST: u8 = 1;
pub const ELEM_AND: u8 = 3;
pub const ELEM_INP_PIN: u8 = 15;
pub const ELEM_WP: u8 = 22;
pub const ELEM_SP: u8 = 23;
pub const ELEM_INP_MDBS: u8 = 33;
pub const ELEM_OUT_MDBS: u8 = 34;
pub const ELEM_MOD: u8 = 35;
pub const ELEM_SUMM: u8 = 40;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ElementSpec {
    pub id: String,
    pub kind: u8,
    pub invert: bool,
    pub inputs: Vec<String>,
    pub params: Vec<i32>,
    pub caption: Option<String>,
    pub comment: Option<String>,
}

impl ElementSpec {
    pub fn new(id: impl Into<String>, kind: u8) -> Self {
        Self {
            id: id.into(),
            kind,
            invert: false,
            inputs: Vec::new(),
            params: Vec::new(),
            caption: None,
            comment: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HintKind {
    Input = 0,
    Output = 1,
    Event = 2,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IoHint {
    pub kind: HintKind,
    pub index: u8,
    pub text: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SchemaMeta {
    pub project_name: String,
    pub project_version: String,
    pub build_time: String,
    pub hints: Vec<IoHint>,
    /// Exact encoded `tScreen` records. A higher HMI DSL/compiler owns their
    /// structure; this compiler only places exact runtime records in the file.
    pub screens: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlIr {
    pub schema: String,
    pub elements: Vec<ElementSpec>,
    pub meta: SchemaMeta,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListingRow {
    pub index: usize,
    pub id: String,
    pub kind: &'static str,
    pub inputs: Vec<String>,
    pub params: Vec<i32>,
    pub comment: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledSchema {
    pub fbdbin: Vec<u8>,
    pub element_count: usize,
    pub screen_count: usize,
    pub required_rtl: i32,
    pub listing: Vec<ListingRow>,
}

pub fn element_code(name: &str) -> Option<u8> {
    ELEMENT_NAMES
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(name))
        .and_then(|index| u8::try_from(index).ok())
}

pub fn element_name(kind: u8) -> Option<&'static str> {
    ELEMENT_NAMES.get(usize::from(kind)).copied()
}

pub fn required_rtl_version(element_types: &[u8], screen_count: usize) -> i32 {
    let mut version = 7;
    for kind in element_types {
        if *kind == ELEM_SUMM {
            version = version.max(11);
        } else if *kind >= ELEM_MOD {
            version = version.max(10);
        } else if *kind == ELEM_INP_MDBS || *kind == ELEM_OUT_MDBS {
            version = version.max(9);
        }
    }
    if screen_count > 0 {
        version = version.max(8);
    }
    version
}

pub fn encode_cp1251(text: &str) -> Vec<u8> {
    text.chars()
        .map(|character| match character {
            '\u{2013}' | '\u{2014}' | '\u{2212}' => b'-',
            character if character.is_ascii() => character as u8,
            '\u{0410}'..='\u{044f}' => {
                let code = u32::from(character) - 0x0410 + 0xc0;
                u8::try_from(code).expect("CP1251 Cyrillic range fits u8")
            }
            '\u{0401}' => 0xa8,
            '\u{0451}' => 0xb8,
            '\u{2116}' => 0xb9,
            '\u{00b0}' => 0xb0,
            _ => b'?',
        })
        .collect()
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_i32(out: &mut [u8], offset: usize, value: i32) -> Result<(), String> {
    let target = out
        .get_mut(offset..offset + 4)
        .ok_or_else(|| "internal compiler offset overflow".to_string())?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

pub fn compile_control_ir(ir: &ControlIr) -> Result<CompiledSchema, String> {
    if ir.schema != CONTROL_IR_SCHEMA {
        return Err(format!(
            "unsupported Saturn ControlIR schema {:?}; expected {CONTROL_IR_SCHEMA:?}",
            ir.schema
        ));
    }
    build_schema(&ir.elements, &ir.meta)
}

pub fn build_schema(elements: &[ElementSpec], meta: &SchemaMeta) -> Result<CompiledSchema, String> {
    if elements.is_empty() {
        return Err("Saturn FBD schema is empty".into());
    }
    if elements.len() > usize::from(u16::MAX - 1) {
        return Err("Saturn FBD schema has too many elements".into());
    }
    if meta.screens.len() > i32::MAX as usize || meta.hints.len() > i32::MAX as usize {
        return Err("Saturn FBD metadata count exceeds int32".into());
    }

    let mut index_of = HashMap::<&str, u16>::new();
    for (index, element) in elements.iter().enumerate() {
        if element.id.is_empty() {
            return Err(format!("element {index} has an empty id"));
        }
        let index = u16::try_from(index).map_err(|_| "element index exceeds uint16")?;
        if index_of.insert(element.id.as_str(), index).is_some() {
            return Err(format!("duplicate Saturn FBD element id {:?}", element.id));
        }
    }

    for element in elements {
        let kind_index = usize::from(element.kind);
        let name = element_name(element.kind)
            .ok_or_else(|| format!("unsupported Saturn FBD element type {}", element.kind))?;
        let expected_inputs = INPUT_COUNTS[kind_index];
        let expected_params = PARAM_COUNTS[kind_index];
        if element.inputs.len() != expected_inputs {
            return Err(format!(
                "{} ({name}) requires {expected_inputs} inputs, got {}",
                element.id,
                element.inputs.len()
            ));
        }
        if element.params.len() != expected_params {
            return Err(format!(
                "{} ({name}) requires {expected_params} parameters, got {}",
                element.id,
                element.params.len()
            ));
        }
        for source in &element.inputs {
            if !index_of.contains_key(source.as_str()) {
                return Err(format!(
                    "{} input references unknown element {:?}",
                    element.id, source
                ));
            }
        }
        if matches!(element.kind, ELEM_WP | ELEM_SP) && element.caption.is_none() {
            return Err(format!("{} ({name}) requires a caption", element.id));
        }
    }

    let required_rtl = required_rtl_version(
        &elements
            .iter()
            .map(|element| element.kind)
            .collect::<Vec<_>>(),
        meta.screens.len(),
    );
    let mut bytes = Vec::<u8>::new();

    for element in elements {
        bytes.push((element.kind & ELEMENT_MASK) | (u8::from(element.invert) * INVERT_FLAG));
    }
    bytes.push(END_MARK);

    for element in elements {
        for source in &element.inputs {
            let index = *index_of
                .get(source.as_str())
                .ok_or_else(|| format!("unknown element reference {source:?}"))?;
            push_u16(&mut bytes, index);
        }
    }

    for element in elements {
        for parameter in &element.params {
            push_i32(&mut bytes, *parameter);
        }
    }

    bytes.push(GLOBAL_OPTIONS_COUNT as u8);
    let options_offset = bytes.len();
    let mut options = [0i32; GLOBAL_OPTIONS_COUNT];
    options[OPT_REQ_VERSION] = required_rtl;
    options[OPT_SCREEN_COUNT] =
        i32::try_from(meta.screens.len()).map_err(|_| "screen count exceeds int32")?;
    options[OPT_HINTS_COUNT] =
        i32::try_from(meta.hints.len()).map_err(|_| "hint count exceeds int32")?;
    for option in options {
        push_i32(&mut bytes, option);
    }

    for element in elements {
        if matches!(element.kind, ELEM_WP | ELEM_SP) {
            bytes.extend_from_slice(&encode_cp1251(
                element.caption.as_deref().unwrap_or_default(),
            ));
            bytes.push(0);
        }
    }
    for text in [
        meta.project_name.as_str(),
        meta.project_version.as_str(),
        meta.build_time.as_str(),
    ] {
        bytes.extend_from_slice(&encode_cp1251(text));
        bytes.push(0);
    }

    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }

    for screen in &meta.screens {
        bytes.extend_from_slice(screen);
    }

    for hint in &meta.hints {
        bytes.push(hint.kind as u8);
        bytes.push(hint.index);
        bytes.extend_from_slice(&encode_cp1251(&hint.text));
        bytes.push(0);
    }

    let total_size = bytes
        .len()
        .checked_add(4)
        .ok_or_else(|| "Saturn FBD schema size overflow".to_string())?;
    let total_size_i32 =
        i32::try_from(total_size).map_err(|_| "Saturn FBD schema exceeds int32")?;
    write_i32(
        &mut bytes,
        options_offset + OPT_SCHEMA_SIZE * 4,
        total_size_i32,
    )?;

    let crc = fbd_crc32(&bytes);
    bytes.extend_from_slice(&crc.to_le_bytes());

    let inspected = inspect_fbdbin(&bytes)?;
    if inspected.element_count != elements.len()
        || inspected.required_rtl != required_rtl
        || usize::try_from(inspected.screen_count).ok() != Some(meta.screens.len())
        || usize::try_from(inspected.hint_count).ok() != Some(meta.hints.len())
    {
        return Err("compiled Saturn FBD schema failed structural round-trip".into());
    }

    let listing = elements
        .iter()
        .enumerate()
        .map(|(index, element)| ListingRow {
            index,
            id: element.id.clone(),
            kind: element_name(element.kind).unwrap_or("UNKNOWN"),
            inputs: element.inputs.clone(),
            params: element.params.clone(),
            comment: element
                .comment
                .clone()
                .or_else(|| element.caption.clone())
                .unwrap_or_default(),
        })
        .collect();

    Ok(CompiledSchema {
        fbdbin: bytes,
        element_count: elements.len(),
        screen_count: meta.screens.len(),
        required_rtl,
        listing,
    })
}

pub fn parse_control_ir_json(source: &str) -> Result<ControlIr, String> {
    let value: Value =
        serde_json::from_str(source).map_err(|error| format!("invalid ControlIR JSON: {error}"))?;
    let root = value
        .as_object()
        .ok_or_else(|| "ControlIR root must be an object".to_string())?;
    let schema = required_string(root.get("schema"), "schema")?.to_string();
    if schema != CONTROL_IR_SCHEMA {
        return Err(format!(
            "unsupported Saturn ControlIR schema {schema:?}; expected {CONTROL_IR_SCHEMA:?}"
        ));
    }

    let project = root
        .get("project")
        .and_then(Value::as_object)
        .ok_or_else(|| "ControlIR project must be an object".to_string())?;
    let project_name = required_string(project.get("name"), "project.name")?.to_string();
    let project_version = required_string(project.get("version"), "project.version")?.to_string();
    let build_time = required_string(project.get("buildTime"), "project.buildTime")?.to_string();

    let element_values = root
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| "ControlIR elements must be an array".to_string())?;
    let mut elements = Vec::with_capacity(element_values.len());
    for (index, value) in element_values.iter().enumerate() {
        let object = value
            .as_object()
            .ok_or_else(|| format!("elements[{index}] must be an object"))?;
        let id = required_string(object.get("id"), &format!("elements[{index}].id"))?.to_string();
        let kind_value = object
            .get("type")
            .ok_or_else(|| format!("elements[{index}].type is required"))?;
        let kind = if let Some(name) = kind_value.as_str() {
            element_code(name).ok_or_else(|| format!("unknown FBD element type {name:?}"))?
        } else if let Some(code) = kind_value.as_u64() {
            let code =
                u8::try_from(code).map_err(|_| format!("invalid FBD element type {code}"))?;
            element_name(code).ok_or_else(|| format!("unknown FBD element type {code}"))?;
            code
        } else {
            return Err(format!("elements[{index}].type must be a name or integer"));
        };
        let inputs = string_array(object.get("inputs"), &format!("elements[{index}].inputs"))?;
        let params = i32_array(object.get("params"), &format!("elements[{index}].params"))?;
        let caption =
            optional_string(object.get("caption"), &format!("elements[{index}].caption"))?;
        let comment =
            optional_string(object.get("comment"), &format!("elements[{index}].comment"))?;
        let invert = object
            .get("invert")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        elements.push(ElementSpec {
            id,
            kind,
            invert,
            inputs,
            params,
            caption,
            comment,
        });
    }

    let mut hints = Vec::new();
    if let Some(values) = root.get("hints") {
        let values = values
            .as_array()
            .ok_or_else(|| "ControlIR hints must be an array".to_string())?;
        for (index, value) in values.iter().enumerate() {
            let object = value
                .as_object()
                .ok_or_else(|| format!("hints[{index}] must be an object"))?;
            let kind_text = required_string(object.get("type"), &format!("hints[{index}].type"))?;
            let kind = match kind_text.to_ascii_lowercase().as_str() {
                "input" => HintKind::Input,
                "output" => HintKind::Output,
                "event" => HintKind::Event,
                _ => return Err(format!("unknown hint type {kind_text:?}")),
            };
            let raw_index = object
                .get("index")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("hints[{index}].index must be an unsigned integer"))?;
            let hint_index = u8::try_from(raw_index)
                .map_err(|_| format!("hints[{index}].index exceeds uint8"))?;
            let text =
                required_string(object.get("text"), &format!("hints[{index}].text"))?.to_string();
            hints.push(IoHint {
                kind,
                index: hint_index,
                text,
            });
        }
    }

    let mut screens = Vec::new();
    if let Some(values) = root.get("screens") {
        let values = values
            .as_array()
            .ok_or_else(|| "ControlIR screens must be an array".to_string())?;
        for (index, value) in values.iter().enumerate() {
            let byte_values = value
                .as_array()
                .ok_or_else(|| format!("screens[{index}] must be a byte array"))?;
            let mut bytes = Vec::with_capacity(byte_values.len());
            for (byte_index, value) in byte_values.iter().enumerate() {
                let raw = value.as_u64().ok_or_else(|| {
                    format!("screens[{index}][{byte_index}] must be an unsigned integer")
                })?;
                bytes.push(
                    u8::try_from(raw)
                        .map_err(|_| format!("screens[{index}][{byte_index}] exceeds uint8"))?,
                );
            }
            screens.push(bytes);
        }
    }

    Ok(ControlIr {
        schema,
        elements,
        meta: SchemaMeta {
            project_name,
            project_version,
            build_time,
            hints,
            screens,
        },
    })
}

fn required_string<'a>(value: Option<&'a Value>, path: &str) -> Result<&'a str, String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{path} must be a non-empty string"))
}

fn optional_string(value: Option<&Value>, path: &str) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("{path} must be a string or null")),
    }
}

fn string_array(value: Option<&Value>, path: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| format!("{path} must be an array"))?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("{path}[{index}] must be a string"))
        })
        .collect()
}

fn i32_array(value: Option<&Value>, path: &str) -> Result<Vec<i32>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| format!("{path} must be an array"))?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let raw = value
                .as_i64()
                .ok_or_else(|| format!("{path}[{index}] must be an integer"))?;
            i32::try_from(raw).map_err(|_| format!("{path}[{index}] exceeds int32"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact_runtime_program() -> ControlIr {
        let mut input = ElementSpec::new("di1", ELEM_INP_PIN);
        input.params = vec![1];
        let mut one = ElementSpec::new("one", ELEM_CONST);
        one.params = vec![1];
        let mut gate = ElementSpec::new("gate", ELEM_AND);
        gate.inputs = vec!["di1".into(), "one".into()];
        let mut output = ElementSpec::new("do1", ELEM_OUT_PIN);
        output.inputs = vec!["gate".into()];
        output.params = vec![1];
        let mut watch = ElementSpec::new("watch", ELEM_WP);
        watch.inputs = vec!["gate".into()];
        watch.params = vec![0];
        watch.caption = Some("Состояние".into());
        let mut setpoint = ElementSpec::new("setpoint", ELEM_SP);
        setpoint.params = vec![0, 100, 50, 0, 1];
        setpoint.caption = Some("Уставка".into());

        ControlIr {
            schema: CONTROL_IR_SCHEMA.into(),
            elements: vec![input, one, gate, output, watch, setpoint],
            meta: SchemaMeta {
                project_name: "Saturn compiler test".into(),
                project_version: "v1".into(),
                build_time: "2026-08-29".into(),
                hints: vec![
                    IoHint {
                        kind: HintKind::Input,
                        index: 1,
                        text: "Вход DI1".into(),
                    },
                    IoHint {
                        kind: HintKind::Output,
                        index: 1,
                        text: "Выход DO1".into(),
                    },
                ],
                screens: Vec::new(),
            },
        }
    }

    #[test]
    fn compiler_emits_runtime_valid_fbdbin() {
        let compiled = compile_control_ir(&exact_runtime_program()).expect("compile ControlIR");
        let info = inspect_fbdbin(&compiled.fbdbin).expect("inspect compiled schema");
        assert_eq!(info.element_count, 6);
        assert_eq!(info.watchpoint_count, 1);
        assert_eq!(info.setpoint_count, 1);
        assert_eq!(info.required_rtl, 7);
        assert_eq!(info.hint_count, 2);
        assert_eq!(info.schema_size, compiled.fbdbin.len());
        assert_eq!(fbd_crc32(&compiled.fbdbin), 0);
    }

    #[test]
    fn compiler_arity_table_matches_runtime_inspector_for_every_element() {
        for kind in 0u8..41 {
            let mut element = ElementSpec::new("self", kind);
            element.inputs = vec!["self".into(); INPUT_COUNTS[usize::from(kind)]];
            element.params = vec![0; PARAM_COUNTS[usize::from(kind)]];
            if matches!(kind, ELEM_WP | ELEM_SP) {
                element.caption = Some("point".into());
            }
            let result = build_schema(
                &[element],
                &SchemaMeta {
                    project_name: "arity".into(),
                    project_version: "1".into(),
                    build_time: "now".into(),
                    hints: Vec::new(),
                    screens: Vec::new(),
                },
            );
            assert!(
                result.is_ok(),
                "kind {kind} ({:?}) failed: {result:?}",
                element_name(kind)
            );
        }
    }

    #[test]
    fn control_ir_json_is_a_versioned_boundary() {
        let json = r#"{
          "schema":"firmverse/saturn-control-ir@1",
          "project":{"name":"Pump","version":"1","buildTime":"2026-08-29"},
          "elements":[
            {"id":"di","type":"INP_PIN","params":[1]},
            {"id":"do","type":"OUT_PIN","inputs":["di"],"params":[1]}
          ],
          "hints":[{"type":"input","index":1,"text":"Пуск"}]
        }"#;
        let ir = parse_control_ir_json(json).expect("parse ControlIR");
        let compiled = compile_control_ir(&ir).expect("compile ControlIR");
        assert_eq!(compiled.element_count, 2);
        assert_eq!(fbd_crc32(&compiled.fbdbin), 0);
    }

    #[test]
    fn cp1251_matches_saturn_strings_and_normalizes_typographic_minus() {
        assert_eq!(
            encode_cp1251("Ёё № °"),
            vec![0xa8, 0xb8, 0x20, 0xb9, 0x20, 0xb0]
        );
        assert_eq!(encode_cp1251("2.0–3.5"), b"2.0-3.5".to_vec());
    }

    #[cfg(firmverse_saturn_native)]
    #[test]
    fn compiler_output_executes_in_the_exact_upstream_runtime() {
        use super::super::saturn::SaturnPlc;

        let compiled = compile_control_ir(&exact_runtime_program()).expect("compile ControlIR");
        let mut plc = SaturnPlc::load(&compiled.fbdbin, true).expect("load compiled schema");
        plc.set_input("DI1", 1).expect("set DI1");
        plc.step(0).expect("evaluate");
        assert_eq!(plc.output("DO1").expect("read DO1"), 1);
        assert_eq!(plc.watchpoints()[0].value, 1);
        assert_eq!(plc.setpoints()[0].value, 50);
    }
}
