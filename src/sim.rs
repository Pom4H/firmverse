//! Shared 1 ms world clock for one or more chips with their own firmware.
//!
//! World owns the environment between nodes. Each node keeps its own board
//! profile so presentation/wiring never leaks into the RF model.

use crate::board::{profile as board_profile, require_phy6252, BoardKind};
use crate::chip::{format_mac, mac_from_id, Apply, Chip};
use crate::cmd::{parse_line, ChipCmd, HELP};
use crate::emu::{self, emit_delta_for_board, emit_gpio_for_board, spawn_line_reader};
use crate::tui::TuiOpts;
use crate::world::World;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const BURST: u32 = 8_000;

const RESERVED_IDS: &[&str] = &[
    "help",
    "quit",
    "exit",
    "scan",
    "gone",
    "lost",
    "write",
    "rx",
    "connect",
    "disconnect",
    "tick",
    "adc",
    "in",
    "notify",
    "cccd",
    "q",
    "h",
];

pub struct SimOpts {
    pub nodes: Vec<NodeSpec>,
    pub world: String,
    pub looping: bool,
    pub live: bool,
    pub ticks: u32,
    pub raw: bool,
    pub strict: bool,
    pub max_insns: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeSpec {
    pub id: String,
    pub hex: PathBuf,
    pub board: BoardKind,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

pub fn run(opts: SimOpts) -> Result<ExitCode, String> {
    if opts.nodes.is_empty() {
        return Err("sim needs at least one --node or a firmware path".into());
    }
    let mut world = World::open(&opts.world, opts.looping)?;
    let mut chips = Vec::with_capacity(opts.nodes.len());
    let mut boards = Vec::with_capacity(opts.nodes.len());
    for spec in &opts.nodes {
        require_phy6252(spec.board)?;
        let mac = mac_from_id(&spec.id);
        let x = spec.x.unwrap_or(0.0);
        let y = spec.y.unwrap_or(0.0);
        chips.push(Chip::load(
            spec.id.clone(),
            &spec.hex,
            opts.strict,
            mac,
            x,
            y,
        )?);
        boards.push(spec.board);
    }

    let tagged = chips.len() > 1;
    let live = opts.live;
    let raw = opts.raw;
    let max_insns = opts.max_insns;
    let cmd_rx = if live {
        Some(spawn_line_reader())
    } else {
        None
    };

    if raw {
        println!("READY");
        println!(
            "WORLD {} loop={} nodes={}",
            world.name(),
            u8::from(opts.looping),
            chips.len()
        );
        for (chip, board) in chips.iter().zip(boards.iter().copied()) {
            println!(
                "NODE {} board={} mac={} x={} y={} hex={}",
                chip.id,
                board_profile(board).id,
                format_mac(&chip.mac),
                chip.x,
                chip.y,
                chip.hex_label
            );
            emit_gpio_for_board(&chip.gpio_bank(), true, tag_of(&chip.id, tagged), board);
        }
    } else if live {
        eprintln!("{}", HELP.trim_end());
        eprintln!(
            "sim world={} loop={} — prefix a command with a node id when there are several chips",
            world.name(),
            opts.looping
        );
        for (chip, board) in chips.iter().zip(boards.iter().copied()) {
            eprintln!(
                "node {}  {}  board={}  mac={}",
                chip.id,
                chip.hex_label,
                board_profile(board).id,
                format_mac(&chip.mac)
            );
        }
    }

    let mut now_ms = 0u32;
    loop {
        if live {
            if let Some(rx) = cmd_rx.as_ref() {
                if drain_lines(rx, &mut chips, raw, tagged)? {
                    return report_stop(&mut chips, &boards, live, raw, tagged, "quit");
                }
            }
        }

        let snapshot: Vec<(usize, [u8; 6], f64, f64)> = chips
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.mac, c.x, c.y))
            .collect();
        for event in world.radio(now_ms, &snapshot) {
            let _ = chips[event.listener].apply(event.cmd)?;
        }

        for (chip, board) in chips.iter_mut().zip(boards.iter().copied()) {
            let delta = chip.tick(BURST, max_insns, true);
            emit_delta_for_board(&delta, raw, live, tag_of(&chip.id, tagged), board);
        }
        if let Some((id, reason)) = chips.iter().find_map(|c| {
            c.stopped()
                .map(|reason| (c.id.as_str(), reason.to_string()))
        }) {
            let why = if tagged {
                format!("{id}: {reason}")
            } else {
                reason
            };
            return report_stop(&mut chips, &boards, live, raw, tagged, &why);
        }

        now_ms = now_ms.wrapping_add(1);
        if !live && now_ms >= opts.ticks {
            return report_stop(&mut chips, &boards, live, raw, tagged, "ticks");
        }
        if live {
            let _ = io::stdout().flush();
            thread::sleep(Duration::from_millis(1));
        }
    }
}

pub fn print_worlds() {
    for (name, desc) in World::list() {
        println!("{name:8} {desc}");
    }
}

pub fn default_world(node_count: usize) -> &'static str {
    if node_count >= 2 {
        "mesh"
    } else {
        "crowd"
    }
}

pub fn collect_nodes(node_flags: &[String], hex: Option<PathBuf>) -> Result<Vec<NodeSpec>, String> {
    let mut specs = Vec::new();
    for flag in node_flags {
        specs.push(parse_node_spec(flag)?);
    }
    if let Some(path) = hex {
        specs.push(NodeSpec {
            id: String::new(),
            hex: path,
            board: BoardKind::Pb03fKit,
            x: None,
            y: None,
        });
    }
    if specs.is_empty() {
        let hex = emu::find_firmware_hex("rssi-rank.hex").or_else(|_| emu::default_hex())?;
        specs.push(NodeSpec {
            id: "n0".into(),
            hex,
            board: BoardKind::Pb03fKit,
            x: Some(0.0),
            y: Some(0.0),
        });
    }
    finalize_nodes(specs)
}

pub fn tui_opts(opts: &SimOpts) -> Result<TuiOpts, String> {
    if opts.nodes.len() != 1 {
        return Err("TUI can watch one chip — drop extra --node flags".into());
    }
    let spec = &opts.nodes[0];
    let node = match (spec.x, spec.y) {
        (Some(x), Some(y)) => format!("{}@{x},{y}={}", spec.id, spec.hex.display()),
        _ => format!("{}={}", spec.id, spec.hex.display()),
    };
    let mut args = vec![
        "sim".into(),
        "--raw".into(),
        "--max-insns".into(),
        opts.max_insns.to_string(),
        "--world".into(),
        opts.world.clone(),
        "--board".into(),
        board_profile(spec.board).id.into(),
        "--node".into(),
        node,
    ];
    if opts.strict {
        args.push("--strict".into());
    }
    Ok(TuiOpts {
        hex: spec.hex.clone(),
        board: spec.board,
        strict: opts.strict,
        max_insns: opts.max_insns,
        argv: args,
    })
}

pub fn parse_node_spec(spec: &str) -> Result<NodeSpec, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("empty --node".into());
    }
    if let Some((left, path)) = spec.split_once('=') {
        if path.is_empty() {
            return Err(format!("--node {spec:?} needs a firmware path after ="));
        }
        let (id, x, y) = parse_id_pose(left)?;
        Ok(NodeSpec {
            id,
            hex: PathBuf::from(path),
            board: BoardKind::Pb03fKit,
            x,
            y,
        })
    } else {
        Ok(NodeSpec {
            id: String::new(),
            hex: PathBuf::from(spec),
            board: BoardKind::Pb03fKit,
            x: None,
            y: None,
        })
    }
}

fn parse_id_pose(left: &str) -> Result<(String, Option<f64>, Option<f64>), String> {
    if let Some((id, pose)) = left.split_once('@') {
        if !valid_id(id) {
            return Err(format!("bad node id {id:?}"));
        }
        let mut xy = pose.split(',');
        let x_s = xy.next().ok_or("pose is id@x,y")?;
        let y_s = xy.next().ok_or("pose is id@x,y")?;
        if xy.next().is_some() {
            return Err("pose is id@x,y".into());
        }
        let x = x_s
            .parse::<f64>()
            .map_err(|_| format!("bad x in {left:?}"))?;
        let y = y_s
            .parse::<f64>()
            .map_err(|_| format!("bad y in {left:?}"))?;
        Ok((id.to_string(), Some(x), Some(y)))
    } else {
        if !valid_id(left) {
            return Err(format!("bad node id {left:?}"));
        }
        Ok((left.to_string(), None, None))
    }
}

fn valid_id(id: &str) -> bool {
    if id.is_empty() || RESERVED_IDS.contains(&id) {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn finalize_nodes(mut specs: Vec<NodeSpec>) -> Result<Vec<NodeSpec>, String> {
    for (i, spec) in specs.iter_mut().enumerate() {
        if spec.id.is_empty() {
            spec.id = format!("n{i}");
        }
        if spec.x.is_none() {
            spec.x = Some(i as f64 * 3.0);
        }
        if spec.y.is_none() {
            spec.y = Some(0.0);
        }
        if !spec.hex.is_file() {
            return Err(format!("firmware not found: {}", spec.hex.display()));
        }
    }
    let mut seen = Vec::new();
    for spec in &specs {
        if seen.iter().any(|id| id == &spec.id) {
            return Err(format!("duplicate node id {}", spec.id));
        }
        seen.push(spec.id.clone());
    }
    Ok(specs)
}

fn tag_of(id: &str, tagged: bool) -> &str {
    if tagged {
        id
    } else {
        ""
    }
}

fn drain_lines(
    rx: &std::sync::mpsc::Receiver<String>,
    chips: &mut [Chip],
    raw: bool,
    tagged: bool,
) -> Result<bool, String> {
    while let Ok(line) = rx.try_recv() {
        let ids: Vec<&str> = chips.iter().map(|c| c.id.as_str()).collect();
        let (target, rest) = split_target(line.trim(), &ids);
        match parse_line(rest) {
            Ok(None) => {}
            Ok(Some(cmd)) => {
                let outcome = if let Some(i) = target {
                    chips[i].apply(cmd)?
                } else {
                    apply_all(chips, cmd, raw, tagged)?
                };
                match outcome {
                    Apply::Quit => return Ok(true),
                    Apply::Help => {
                        if target.is_some() && !raw {
                            eprintln!("{}", HELP.trim_end());
                        }
                    }
                    Apply::Continue => {}
                }
            }
            Err(err) => eprintln!("{err}"),
        }
    }
    Ok(false)
}

/// Broadcast a command to every chip (unprefixed stdin).
fn apply_all(chips: &mut [Chip], cmd: ChipCmd, raw: bool, tagged: bool) -> Result<Apply, String> {
    match cmd {
        ChipCmd::Quit => Ok(Apply::Quit),
        ChipCmd::Help => {
            if !raw {
                eprintln!("{}", HELP.trim_end());
                if tagged {
                    eprint!("nodes:");
                    for chip in chips.iter() {
                        eprint!(" {}", chip.id);
                    }
                    eprintln!();
                }
            }
            Ok(Apply::Help)
        }
        other => {
            for chip in chips.iter_mut() {
                let _ = chip.apply(other.clone())?;
            }
            Ok(Apply::Continue)
        }
    }
}

fn report_stop(
    chips: &mut [Chip],
    boards: &[BoardKind],
    live: bool,
    raw: bool,
    tagged: bool,
    reason: &str,
) -> Result<ExitCode, String> {
    for (chip, board) in chips.iter_mut().zip(boards.iter().copied()) {
        emit_gpio_for_board(&chip.gpio_bank(), raw, tag_of(&chip.id, tagged), board);
        let (pc, lr, msp) = chip.pc_lr_msp();
        eprintln!(
            "node {} board={} insns={} pc={pc:#010x} lr={lr:#010x} msp={msp:#010x}",
            chip.id,
            board_profile(board).id,
            chip.insn
        );
    }
    if live {
        if raw {
            println!("STOP {reason}");
        } else {
            eprintln!("stop {reason}");
        }
    } else {
        println!("stop: {reason}");
    }
    Ok(ExitCode::from(if reason.contains("fault") { 2 } else { 0 }))
}

/// Parse `a scan …` / `[b] gone …` targeting. Unprefixed lines go to every chip.
pub fn split_target<'a>(line: &'a str, ids: &[&str]) -> (Option<usize>, &'a str) {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let id = rest[..end].trim();
            if let Some(i) = ids.iter().position(|n| *n == id) {
                return (Some(i), rest[end + 1..].trim());
            }
        }
    }
    if let Some((first, rest)) = line.split_once(char::is_whitespace) {
        if let Some(i) = ids.iter().position(|n| *n == first) {
            return (Some(i), rest.trim());
        }
    }
    (None, line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_spec_id_pose_and_path() {
        let spec = parse_node_spec("lamp@1.5,-2=/tmp/a.hex").unwrap();
        assert_eq!(spec.id, "lamp");
        assert_eq!(spec.x, Some(1.5));
        assert_eq!(spec.y, Some(-2.0));
        assert_eq!(spec.hex, PathBuf::from("/tmp/a.hex"));
        assert_eq!(spec.board, BoardKind::Pb03fKit);
        let path_only = parse_node_spec("fw.hex").unwrap();
        assert!(path_only.id.is_empty());
        assert_eq!(path_only.hex, PathBuf::from("fw.hex"));
        assert!(parse_node_spec("scan=fw.hex").is_err());
    }

    #[test]
    fn target_prefix() {
        let ids = ["a", "b"];
        assert_eq!(split_target("[b] gone aa", &ids), (Some(1), "gone aa"));
        assert_eq!(split_target("a scan ff", &ids), (Some(0), "scan ff"));
        assert_eq!(split_target("scan ff", &ids), (None, "scan ff"));
    }

    #[test]
    fn two_nodes_default_to_mesh() {
        assert_eq!(default_world(2), "mesh");
        assert_eq!(default_world(1), "crowd");
    }
}
