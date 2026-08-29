//! Browser-facing Firmverse runtime.
//!
//! The browser is only another frontend. Firmware still executes in the same
//! PHY6252/zmu core as the native CLI; the web layer supplies in-memory HEX,
//! editor commands and a compact JSON ABI suitable for a Web Worker.

use crate::board::{self, require_phy6252, BoardKind};
use crate::chip::{format_mac, mac_from_id, Chip};
use crate::cmd::ChipCmd;
use crate::controller;
use crate::controller::saturn::{INPUT_TERMINALS, OUTPUT_TERMINALS};
use crate::soc;
use crate::soc::phy6252::pins;
use crate::world::World;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

const DEFAULT_BURST: u32 = 2_000;
const DEFAULT_MAX_INSNS: u64 = 50_000_000;
const LOG_LINES: usize = 160;

struct BrowserNode {
    board: BoardKind,
    chip: Chip,
    pwm: Vec<u32>,
    uart: VecDeque<String>,
    frames: VecDeque<String>,
}

pub struct BrowserLab {
    world: World,
    looping: bool,
    strict: bool,
    max_insns: u64,
    now_ms: u32,
    nodes: Vec<BrowserNode>,
    links: HashMap<(usize, [u8; 6]), i8>,
}

impl BrowserLab {
    pub fn new(world: &str, looping: bool, strict: bool, max_insns: u64) -> Result<Self, String> {
        Ok(Self {
            world: World::open(world, looping)?,
            looping,
            strict,
            max_insns: max_insns.max(1),
            now_ms: 0,
            nodes: Vec::new(),
            links: HashMap::new(),
        })
    }

    pub fn add_node(
        &mut self,
        id: &str,
        board_id: &str,
        label: &str,
        firmware_hex: &str,
        x: f64,
        y: f64,
    ) -> Result<(), String> {
        if id.is_empty() {
            return Err("node id must not be empty".into());
        }
        if self.nodes.iter().any(|node| node.chip.id == id) {
            return Err(format!("node {id:?} already exists"));
        }
        let board = board_kind(board_id)?;
        require_phy6252(board)?;
        let chip = Chip::load_text(
            id.to_string(),
            label.to_string(),
            firmware_hex,
            self.strict,
            mac_from_id(id),
            x,
            y,
        )?;
        self.nodes.push(BrowserNode {
            board,
            chip,
            pwm: Vec::new(),
            uart: VecDeque::new(),
            frames: VecDeque::new(),
        });
        Ok(())
    }

    pub fn remove_node(&mut self, id: &str) -> Result<(), String> {
        let index = self.node_index(id)?;
        self.nodes.remove(index);
        self.links.clear();
        Ok(())
    }

    pub fn move_node(&mut self, id: &str, x: f64, y: f64) -> Result<(), String> {
        let index = self.node_index(id)?;
        self.nodes[index].chip.x = x;
        self.nodes[index].chip.y = y;
        Ok(())
    }

    pub fn set_world(&mut self, name: &str, looping: bool) -> Result<(), String> {
        self.world = World::open(name, looping)?;
        self.looping = looping;
        self.links.clear();
        Ok(())
    }

    pub fn pin(&mut self, id: &str, pin: &str, high: bool) -> Result<(), String> {
        let bit = pins::gpio_bit(pin).ok_or_else(|| format!("unknown PHY6252 pin {pin:?}"))?;
        let index = self.node_index(id)?;
        self.nodes[index].chip.apply(ChipCmd::Pin { bit, high })?;
        Ok(())
    }

    pub fn inputs(&mut self, id: &str, mask: u32) -> Result<(), String> {
        let index = self.node_index(id)?;
        self.nodes[index].chip.apply(ChipCmd::In(mask))?;
        Ok(())
    }

    pub fn adc(&mut self, id: &str, values: [u16; 4]) -> Result<(), String> {
        let index = self.node_index(id)?;
        self.nodes[index].chip.apply(ChipCmd::Adc(values))?;
        Ok(())
    }

    pub fn tick(&mut self, ticks: u32, burst: u32) -> Result<Value, String> {
        let ticks = ticks.clamp(1, 1_000);
        let burst = burst.clamp(1, 50_000);
        for _ in 0..ticks {
            let radios: Vec<(usize, [u8; 6], f64, f64)> = self
                .nodes
                .iter()
                .enumerate()
                .map(|(index, node)| (index, node.chip.mac, node.chip.x, node.chip.y))
                .collect();

            for event in self.world.radio(self.now_ms, &radios) {
                match &event.cmd {
                    ChipCmd::Scan { addr, rssi } => {
                        self.links.insert((event.listener, *addr), *rssi);
                    }
                    ChipCmd::Gone { addr } => {
                        self.links.remove(&(event.listener, *addr));
                    }
                    _ => {}
                }
                self.nodes[event.listener].chip.apply(event.cmd)?;
            }

            for node in &mut self.nodes {
                let delta = node.chip.tick(burst, self.max_insns, true);
                for line in delta.uart_lines {
                    push_bounded(&mut node.uart, line);
                }
                if let Some(pwm) = delta.pwm {
                    node.pwm = pwm.to_vec();
                }
                for frame in delta.frames {
                    push_bounded(&mut node.frames, hex(&frame));
                }
            }
            self.now_ms = self.now_ms.wrapping_add(1);
        }
        Ok(self.snapshot())
    }

    pub fn snapshot(&self) -> Value {
        let nodes = self
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| self.node_snapshot(index, node))
            .collect::<Vec<_>>();
        json!({
            "world": {
                "name": self.world.name(),
                "looping": self.looping,
                "nowMs": self.now_ms,
            },
            "nodes": nodes,
        })
    }

    fn node_snapshot(&self, index: usize, node: &BrowserNode) -> Value {
        let bank = node.chip.gpio_bank();
        let bank = bank.borrow();
        let profile = board::profile(node.board);
        let indicators = profile
            .indicators
            .iter()
            .map(|signal| {
                let output = ((bank.ddr >> signal.gpio_bit) & 1) != 0;
                let level = ((bank.dr >> signal.gpio_bit) & 1) != 0;
                let active = output && if signal.active_high { level } else { !level };
                json!({
                    "name": signal.name,
                    "pin": signal.pin,
                    "active": active,
                    "output": output,
                    "level": level,
                })
            })
            .collect::<Vec<_>>();

        let mut heard = self
            .links
            .iter()
            .filter(|((listener, _), _)| *listener == index)
            .map(|((_, mac), rssi)| {
                let node_id = self
                    .nodes
                    .iter()
                    .find(|candidate| candidate.chip.mac == *mac)
                    .map(|candidate| candidate.chip.id.as_str());
                json!({
                    "mac": format_mac(mac),
                    "nodeId": node_id,
                    "rssi": rssi,
                })
            })
            .collect::<Vec<_>>();
        heard.sort_by_key(|entry| {
            entry
                .get("mac")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        });

        json!({
            "id": node.chip.id,
            "board": profile.id,
            "soc": profile.soc.id(),
            "firmware": node.chip.hex_label,
            "mac": format_mac(&node.chip.mac),
            "x": node.chip.x,
            "y": node.chip.y,
            "insns": node.chip.insn,
            "stopped": node.chip.stopped(),
            "power": {
                "sleeping": node.chip.sleeping(),
                "sleepEntries": node.chip.sleep_entries(),
                "wakeCount": node.chip.wake_count(),
                "lastWakePin": node.chip.last_wake_pin(),
            },
            "gpio": {
                "dr": bank.dr,
                "ddr": bank.ddr,
            },
            "indicators": indicators,
            "pwm": node.pwm,
            "uart": node.uart,
            "frames": node.frames,
            "heard": heard,
        })
    }

    fn node_index(&self, id: &str) -> Result<usize, String> {
        self.nodes
            .iter()
            .position(|node| node.chip.id == id)
            .ok_or_else(|| format!("unknown node {id:?}"))
    }
}

pub fn registry() -> Value {
    let boards = board::PROFILES
        .iter()
        .map(|profile| {
            json!({
                "id": profile.id,
                "name": profile.name,
                "soc": profile.soc.id(),
                "description": profile.description,
                "implemented": soc::profile(profile.soc).implemented,
                "pinoutTitle": profile.pinout_title,
                "connectorRows": profile.connector_rows.iter().map(|row| json!({
                    "left": row.left,
                    "right": row.right,
                })).collect::<Vec<_>>(),
                "pinNotes": profile.pin_notes.iter().map(|note| json!({
                    "pin": note.pin,
                    "note": note.note,
                })).collect::<Vec<_>>(),
                "indicators": profile.indicators.iter().map(|signal| json!({
                    "name": signal.name,
                    "pin": signal.pin,
                    "gpioBit": signal.gpio_bit,
                    "activeHigh": signal.active_high,
                })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let socs = soc::PROFILES
        .iter()
        .map(|profile| {
            json!({
                "id": profile.id,
                "name": profile.name,
                "cpu": profile.cpu.label(),
                "implemented": profile.implemented,
                "description": profile.description,
            })
        })
        .collect::<Vec<_>>();
    let phy6252_pins = pins::PINS
        .iter()
        .map(|pin| {
            json!({
                "label": pin.label,
                "gpioBit": pin.gpio_bit,
                "adcChannel": pin.adc_channel,
            })
        })
        .collect::<Vec<_>>();
    let controllers = controller::PROFILES
        .iter()
        .map(|profile| {
            json!({
                "id": profile.id,
                "name": profile.name,
                "manufacturer": profile.manufacturer,
                "runtime": profile.runtime.id(),
                "artifact": profile.artifact,
                "nativeExecution": profile.native_execution,
                "browserExecution": profile.browser_execution,
                "description": profile.description,
            })
        })
        .collect::<Vec<_>>();
    let saturn_inputs = INPUT_TERMINALS
        .iter()
        .map(|terminal| {
            json!({
                "name": terminal.name,
                "runtimeIndex": terminal.runtime_index,
                "direction": "input",
                "kind": format!("{:?}", terminal.kind).to_lowercase(),
            })
        })
        .collect::<Vec<_>>();
    let saturn_outputs = OUTPUT_TERMINALS
        .iter()
        .map(|terminal| {
            json!({
                "name": terminal.name,
                "runtimeIndex": terminal.runtime_index,
                "direction": "output",
                "kind": format!("{:?}", terminal.kind).to_lowercase(),
            })
        })
        .collect::<Vec<_>>();
    let worlds = World::list()
        .iter()
        .map(|(id, description)| json!({ "id": id, "description": description }))
        .collect::<Vec<_>>();

    json!({
        "boards": boards,
        "socs": socs,
        "controllers": controllers,
        "pins": {
            "phy6252": phy6252_pins,
        },
        "terminals": {
            "saturn-plc": {
                "inputs": saturn_inputs,
                "outputs": saturn_outputs,
            },
        },
        "worlds": worlds,
    })
}

fn board_kind(id: &str) -> Result<BoardKind, String> {
    match id {
        "pb03f-kit" => Ok(BoardKind::Pb03fKit),
        "headless" => Ok(BoardKind::Headless),
        "weact-ch592f" => Ok(BoardKind::WeactCh592f),
        _ => Err(format!("unknown board {id:?}")),
    }
}

fn push_bounded(queue: &mut VecDeque<String>, value: String) {
    if queue.len() == LOG_LINES {
        queue.pop_front();
    }
    queue.push_back(value);
}

fn hex(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(ALPHABET[(byte >> 4) as usize]));
        out.push(char::from(ALPHABET[(byte & 0x0f) as usize]));
    }
    out
}

thread_local! {
    static LAB: RefCell<Option<BrowserLab>> = const { RefCell::new(None) };
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn dispatch(raw: &str) -> Result<Value, String> {
    let request: Value =
        serde_json::from_str(raw).map_err(|error| format!("bad request JSON: {error}"))?;
    let op = string(&request, "op")?;
    match op {
        "registry" => Ok(json!({ "ok": true, "registry": registry() })),
        "new" => {
            let world = request
                .get("world")
                .and_then(Value::as_str)
                .unwrap_or("mesh");
            let looping = request
                .get("looping")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let strict = request
                .get("strict")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let max_insns = request
                .get("maxInsns")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_MAX_INSNS);
            let lab = BrowserLab::new(world, looping, strict, max_insns)?;
            let snapshot = lab.snapshot();
            LAB.with(|slot| *slot.borrow_mut() = Some(lab));
            Ok(json!({ "ok": true, "snapshot": snapshot }))
        }
        "reset" => {
            LAB.with(|slot| *slot.borrow_mut() = None);
            Ok(json!({ "ok": true }))
        }
        _ => LAB.with(|slot| {
            let mut slot = slot.borrow_mut();
            let lab = slot
                .as_mut()
                .ok_or_else(|| "browser lab is not initialized; call op=new first".to_string())?;
            match op {
                "addNode" => {
                    let id = string(&request, "id")?;
                    let board = request
                        .get("board")
                        .and_then(Value::as_str)
                        .unwrap_or("pb03f-kit");
                    let label = request
                        .get("label")
                        .and_then(Value::as_str)
                        .unwrap_or("firmware.hex");
                    let firmware = string(&request, "firmware")?;
                    let x = number(&request, "x", 0.0)?;
                    let y = number(&request, "y", 0.0)?;
                    lab.add_node(id, board, label, firmware, x, y)?;
                    Ok(json!({ "ok": true, "snapshot": lab.snapshot() }))
                }
                "removeNode" => {
                    lab.remove_node(string(&request, "id")?)?;
                    Ok(json!({ "ok": true, "snapshot": lab.snapshot() }))
                }
                "moveNode" => {
                    lab.move_node(
                        string(&request, "id")?,
                        number(&request, "x", 0.0)?,
                        number(&request, "y", 0.0)?,
                    )?;
                    Ok(json!({ "ok": true, "snapshot": lab.snapshot() }))
                }
                "setWorld" => {
                    let world = string(&request, "world")?;
                    let looping = request
                        .get("looping")
                        .and_then(Value::as_bool)
                        .unwrap_or(true);
                    lab.set_world(world, looping)?;
                    Ok(json!({ "ok": true, "snapshot": lab.snapshot() }))
                }
                "pin" => {
                    lab.pin(
                        string(&request, "id")?,
                        string(&request, "pin")?,
                        request
                            .get("high")
                            .and_then(Value::as_bool)
                            .ok_or_else(|| "field high must be boolean".to_string())?,
                    )?;
                    Ok(json!({ "ok": true, "snapshot": lab.snapshot() }))
                }
                "inputs" => {
                    let mask = request
                        .get("mask")
                        .and_then(Value::as_u64)
                        .filter(|value| *value <= u64::from(u32::MAX))
                        .ok_or_else(|| "field mask must fit u32".to_string())?;
                    lab.inputs(string(&request, "id")?, mask as u32)?;
                    Ok(json!({ "ok": true, "snapshot": lab.snapshot() }))
                }
                "adc" => {
                    let values =
                        request
                            .get("values")
                            .and_then(Value::as_array)
                            .ok_or_else(|| {
                                "field values must be an array of four millivolt values".to_string()
                            })?;
                    if values.len() != 4 {
                        return Err("field values must contain exactly four entries".into());
                    }
                    let mut adc = [0u16; 4];
                    for (index, value) in values.iter().enumerate() {
                        let mv = value
                            .as_u64()
                            .filter(|mv| *mv <= u64::from(u16::MAX))
                            .ok_or_else(|| format!("values[{index}] must fit u16"))?;
                        adc[index] = mv as u16;
                    }
                    lab.adc(string(&request, "id")?, adc)?;
                    Ok(json!({ "ok": true, "snapshot": lab.snapshot() }))
                }
                "tick" => {
                    let ticks = request
                        .get("ticks")
                        .and_then(Value::as_u64)
                        .unwrap_or(1)
                        .min(u64::from(u32::MAX)) as u32;
                    let burst = request
                        .get("burst")
                        .and_then(Value::as_u64)
                        .unwrap_or(u64::from(DEFAULT_BURST))
                        .min(u64::from(u32::MAX)) as u32;
                    Ok(json!({ "ok": true, "snapshot": lab.tick(ticks, burst)? }))
                }
                "snapshot" => Ok(json!({ "ok": true, "snapshot": lab.snapshot() })),
                other => Err(format!("unknown browser op {other:?}")),
            }
        }),
    }
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("field {field} must be a string"))
}

fn number(value: &Value, field: &str, default: f64) -> Result<f64, String> {
    let Some(value) = value.get(field) else {
        return Ok(default);
    };
    value
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("field {field} must be a finite number"))
}

fn set_result(value: Value) -> i32 {
    match serde_json::to_vec(&value) {
        Ok(bytes) => {
            RESULT.with(|result| *result.borrow_mut() = bytes);
            0
        }
        Err(error) => {
            RESULT.with(|result| {
                *result.borrow_mut() =
                    format!("{{\"ok\":false,\"error\":{:?}}}", error.to_string()).into_bytes();
            });
            1
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn firmverse_input_reserve(len: usize) -> usize {
    INPUT.with(|input| {
        let mut input = input.borrow_mut();
        input.resize(len, 0);
        input.as_mut_ptr() as usize
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn firmverse_call(len: usize) -> i32 {
    let raw = INPUT.with(|input| {
        let input = input.borrow();
        if len > input.len() {
            return Err(format!(
                "request length {len} exceeds reserved input {}",
                input.len()
            ));
        }
        std::str::from_utf8(&input[..len])
            .map(str::to_owned)
            .map_err(|error| format!("request is not UTF-8: {error}"))
    });
    match raw.and_then(|raw| dispatch(&raw)) {
        Ok(value) => set_result(value),
        Err(error) => {
            set_result(json!({ "ok": false, "error": error }));
            1
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn firmverse_result_ptr() -> usize {
    RESULT.with(|result| result.borrow().as_ptr() as usize)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn firmverse_result_len() -> usize {
    RESULT.with(|result| result.borrow().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_generated_from_core_profiles() {
        let registry = registry();
        let boards = registry["boards"].as_array().expect("boards");
        assert!(boards.iter().any(|board| board["id"] == "pb03f-kit"));
        assert_eq!(registry["pins"]["phy6252"][0]["label"], "P0");
        assert!(registry["worlds"]
            .as_array()
            .expect("worlds")
            .iter()
            .any(|world| world["id"] == "mesh"));
    }

    #[test]
    fn registry_rpc_does_not_require_a_lab() {
        let response = dispatch(r#"{"op":"registry"}"#).expect("registry rpc");
        assert_eq!(response["ok"], true);
        assert_eq!(response["registry"]["socs"][0]["id"], "phy6252");
    }
}
