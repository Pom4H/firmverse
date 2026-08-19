#!/usr/bin/env python3
from pathlib import Path


def must_replace(text: str, old: str, new: str, label: str) -> str:
    if old not in text:
        raise SystemExit(f"missing refactor anchor: {label}")
    return text.replace(old, new, 1)


def edit_tui() -> None:
    path = Path("src/tui.rs")
    s = path.read_text()

    s = must_replace(
        s,
        "use crossterm::cursor::{Hide, MoveTo, Show};\n",
        "use crate::board::{profile as board_profile, BoardKind, BoardProfile};\n"
        "use crate::soc::phy6252::pins::{self, Pin};\n"
        "use crossterm::cursor::{Hide, MoveTo, Show};\n",
        "tui imports",
    )
    s = must_replace(
        s,
        "pub struct TuiOpts {\n    pub hex: PathBuf,\n    pub strict: bool,\n    pub max_insns: u64,\n    /// If set, spawn these args instead of `phy6252 --raw <hex>`.\n    pub argv: Vec<String>,\n}\n",
        "pub struct TuiOpts {\n    pub hex: PathBuf,\n    pub board: BoardKind,\n    pub strict: bool,\n    pub max_insns: u64,\n    /// If set, spawn these args instead of the single-node raw frontend.\n    pub argv: Vec<String>,\n}\n",
        "TuiOpts",
    )

    start = s.index("#[derive(Clone, Copy)]\nstruct Pin {")
    end = s.index("struct State {", start)
    s = s[:start] + s[end:]

    s = must_replace(
        s,
        "struct State {\n    started: Instant,\n    image: String,\n    strict: bool,\n",
        "struct State {\n    started: Instant,\n    image: String,\n    board: BoardKind,\n    strict: bool,\n",
        "State board",
    )
    s = must_replace(
        s,
        "            strict: opts.strict,\n            status: \"STARTING\".into(),\n",
        "            board: opts.board,\n            strict: opts.strict,\n            status: \"STARTING\".into(),\n",
        "State init board",
    )
    s = must_replace(
        s,
        "            if let Some(bit) = pin_bit(pin) {\n",
        "            if let Some(bit) = pins::gpio_bit(pin) {\n",
        "command pin lookup",
    )
    s = must_replace(
        s,
        "    fn pin(&self, pin: Pin) -> (bool, bool) {\n        let output = (self.gpio_ddr >> pin.bit) & 1 != 0;\n        let value = if output {\n            (self.gpio_dr >> pin.bit) & 1 != 0\n        } else {\n            (self.ext_in >> pin.bit) & 1 != 0\n        };\n        (output, value)\n    }\n",
        "    fn pin(&self, pin: Pin) -> (bool, bool) {\n        let output = (self.gpio_ddr >> pin.gpio_bit) & 1 != 0;\n        let value = if output {\n            (self.gpio_dr >> pin.gpio_bit) & 1 != 0\n        } else {\n            (self.ext_in >> pin.gpio_bit) & 1 != 0\n        };\n        (output, value)\n    }\n",
        "State pin",
    )
    s = must_replace(
        s,
        "        c.arg(\"--raw\")\n            .arg(\"--max-insns\")\n            .arg(opts.max_insns.to_string());\n",
        "        c.arg(\"--raw\")\n            .arg(\"--board\")\n            .arg(board_profile(opts.board).id)\n            .arg(\"--max-insns\")\n            .arg(opts.max_insns.to_string());\n",
        "spawn board",
    )

    s = must_replace(
        s,
        "    let mut lines = Vec::new();\n    lines.push(format!(\n        \"PHY6252  {}  {}  {}\",\n        state.status,\n        if state.strict { \"STRICT\" } else { \"NORMAL\" },\n        state.image\n    ));\n",
        "    let board = board_profile(state.board);\n    let soc = crate::soc::profile(board.soc);\n    let mut lines = Vec::new();\n    lines.push(format!(\n        \"{} / {}  {}  {}  {}\",\n        soc.name,\n        board.name,\n        state.status,\n        if state.strict { \"STRICT\" } else { \"NORMAL\" },\n        state.image\n    ));\n",
        "draw title",
    )
    s = must_replace(
        s,
        "    lines.push(format!(\n        \"ADC P20={:.3}V P15/RST={:.3}V P24={:.3}V P23={:.3}V\",\n        volts(state.adc[0]),\n        volts(state.adc[1]),\n        volts(state.adc[2]),\n        volts(state.adc[3])\n    ));\n    if w >= 68 && h >= 26 {\n        lines.push(String::new());\n        lines.push(\"PB-03F-Kit bottom view (DIP-30, pin 1 P13 top-left)\".into());\n        for (left, right) in BOARD_ROWS {\n            lines.push(format!(\n                \"{:<31} | {:>31}\",\n                pin_text(state, left),\n                pin_text(state, right)\n            ));\n        }\n    } else {\n        lines.push(\"Pinout hidden: enlarge terminal to at least 68x26\".into());\n    }\n    lines.push(format!(\n        \"LED R/P7={} G/P11={} B/P18={} Y/P0={} W/P34={}\",\n        bit(state.gpio_dr, 4),\n        bit(state.gpio_dr, 7),\n        bit(state.gpio_dr, 12),\n        bit(state.gpio_dr, 0),\n        bit(state.gpio_dr, 22)\n    ));\n",
        "    lines.push(format!(\"ADC {}\", adc_summary(state)));\n    if !board.connector_rows.is_empty() {\n        if w >= 68 && h >= 26 {\n            lines.push(String::new());\n            if let Some(title) = board.pinout_title {\n                lines.push(title.into());\n            }\n            for row in board.connector_rows {\n                lines.push(format!(\n                    \"{:<31} | {:>31}\",\n                    pin_text(state, row.left),\n                    pin_text(state, row.right)\n                ));\n            }\n        } else {\n            lines.push(\"Pinout hidden: enlarge terminal to at least 68x26\".into());\n        }\n    }\n    if !board.indicators.is_empty() {\n        lines.push(indicator_summary(state, board));\n    }\n",
        "draw board metadata",
    )

    pin_start = s.index("fn pin_text(state: &State, label: &str) -> String {")
    pin_end = s.index("fn push_line(", pin_start)
    new_pin_helpers = '''fn adc_summary(state: &State) -> String {
    (0..state.adc.len())
        .filter_map(|channel| {
            pins::adc_pin(channel).map(|pin| {
                format!("{}={:.3}V", pin.label, volts(state.adc[channel]))
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn indicator_summary(state: &State, board: &BoardProfile) -> String {
    let values = board
        .indicators
        .iter()
        .map(|signal| {
            let level = bit(state.gpio_dr, signal.gpio_bit);
            format!("{}/{}={level}", signal.name, signal.pin)
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("LED {values}")
}

fn pin_text(state: &State, label: &str) -> String {
    let board = board_profile(state.board);
    if let Some(pin) = pins::by_label(label) {
        let (out, value) = state.pin(pin);
        let mut text = format!(
            "{:>3} e{:02} {}={}",
            pin.label,
            pin.gpio_bit,
            if out { "OUT" } else { "IN " },
            u8::from(value)
        );
        if let Some(channel) = pin.adc_channel {
            text.push_str(&format!(" {:.3}V", volts(state.adc[channel])));
        }
        if let Some(signal) = board.indicator_for_pin(pin.label) {
            text.push_str(&format!(" {}", signal.name));
        }
        if let Some(note) = board.pin_note(pin.label) {
            text.push_str(&format!(" {note}"));
        }
        return text;
    }

    match board.pin_note(label) {
        Some(note) => format!("{label} {note}"),
        None => label.into(),
    }
}

'''
    s = s[:pin_start] + new_pin_helpers + s[pin_end:]

    bit_start = s.index("fn pin_bit(s: &str) -> Option<u32> {")
    bit_end = s.index("#[cfg(test)]", bit_start)
    s = s[:bit_start] + s[bit_end:]

    s = must_replace(
        s,
        "            hex: PathBuf::from(\"demo.hex\"),\n            strict: false,\n",
        "            hex: PathBuf::from(\"demo.hex\"),\n            board: BoardKind::Pb03fKit,\n            strict: false,\n",
        "test opts board",
    )
    s = must_replace(
        s,
        "        let p = *PINS.iter().find(|p| p.label == \"P34\").unwrap();\n",
        "        let p = pins::by_label(\"P34\").unwrap();\n",
        "test pin source",
    )
    s = must_replace(
        s,
        "    fn physical_board_rows_match_pb03f_bottom_view() {\n        assert_eq!(BOARD_ROWS.first(), Some(&(\"P13\", \"P24\")));\n        assert_eq!(BOARD_ROWS[3], (\"P7\", \"P3\"));\n        assert_eq!(BOARD_ROWS[8], (\"P17\", \"P34\"));\n        assert_eq!(BOARD_ROWS.last(), Some(&(\"5V\", \"3V3\")));\n        assert_eq!(BOARD_ROWS.len(), 15);\n        let s = State::new(&opts());\n        assert!(pin_text(&s, \"P13\").contains(\"P13\"));\n        assert!(pin_text(&s, \"P15\").contains(\"RST\"));\n        assert!(pin_text(&s, \"P34\").contains(\"W\"));\n    }\n",
        "    fn physical_board_rows_come_from_profile() {\n        let board = board_profile(BoardKind::Pb03fKit);\n        assert_eq!(board.connector_rows.first().map(|row| (row.left, row.right)), Some((\"P13\", \"P24\")));\n        assert_eq!((board.connector_rows[3].left, board.connector_rows[3].right), (\"P7\", \"P3\"));\n        assert_eq!(board.connector_rows.len(), 15);\n        let s = State::new(&opts());\n        assert!(pin_text(&s, \"P13\").contains(\"silk\"));\n        assert!(pin_text(&s, \"P15\").contains(\"Restore\"));\n        assert!(pin_text(&s, \"P34\").contains(\"white\"));\n    }\n",
        "board row test",
    )

    path.write_text(s)


def edit_sim() -> None:
    path = Path("src/sim.rs")
    s = path.read_text()
    guard = '''    if spec.board != BoardKind::Pb03fKit {
        return Err(format!(
            "TUI pinout currently renders {}; board {} is available in raw mode",
            board_profile(BoardKind::Pb03fKit).name,
            board_profile(spec.board).id
        ));
    }
'''
    s = must_replace(s, guard, "", "sim TUI board guard")
    s = must_replace(
        s,
        "    Ok(TuiOpts {\n        hex: spec.hex.clone(),\n        strict: opts.strict,\n",
        "    Ok(TuiOpts {\n        hex: spec.hex.clone(),\n        board: spec.board,\n        strict: opts.strict,\n",
        "sim TuiOpts board",
    )
    path.write_text(s)


def edit_cmd() -> None:
    path = Path("src/soc/phy6252/cmd.rs")
    s = path.read_text()
    start = s.index("fn silk_bit(label: &str) -> Option<u32> {")
    end = s.index("\n}\n\npub fn gpio_silk", start) + 2
    s = s[:start] + "fn silk_bit(label: &str) -> Option<u32> {\n    crate::soc::phy6252::pins::gpio_bit(label)\n}" + s[end:]
    path.write_text(s)


def edit_ci() -> None:
    path = Path(".github/workflows/ci.yml")
    s = path.read_text().replace("actions/cache@v4", "actions/cache@v5")
    s = s.replace(
        "run: cargo clippy --all-targets --all-features -- -D warnings",
        "run: cargo clippy --locked --all-targets --all-features -- -D warnings",
    )
    s = s.replace("run: cargo test\n", "run: cargo test --locked\n")
    s = s.replace("run: cargo build --quiet\n", "run: cargo build --quiet --locked\n")
    marker = '''      - name: Firmverse binary
        run: cargo build --quiet --locked
'''
    smoke = marker + '''      - name: Firmverse registry smoke
        run: |
          ./target/debug/firmverse socs | grep -q '^phy6252'
          ./target/debug/firmverse socs | grep -q '^ch592f'
          ./target/debug/firmverse boards | grep -q '^pb03f-kit'
          ./target/debug/firmverse worlds | grep -q '^mesh'
'''
    s = must_replace(s, marker, smoke, "registry smoke")
    path.write_text(s)


edit_tui()
edit_sim()
edit_cmd()
edit_ci()
