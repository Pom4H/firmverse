use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, size, Clear, ClearType, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

pub struct TuiOpts {
    pub hex: PathBuf,
    pub strict: bool,
    pub max_insns: u64,
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

enum UiEvent {
    Line(Stream, String),
}

#[derive(Clone, Copy)]
struct Pin {
    label: &'static str,
    bit: u32,
    adc_slot: Option<usize>,
}

const PINS: &[Pin] = &[
    Pin { label: "P0", bit: 0, adc_slot: None },
    Pin { label: "P2", bit: 2, adc_slot: None },
    Pin { label: "P3", bit: 3, adc_slot: None },
    Pin { label: "P7", bit: 4, adc_slot: None },
    Pin { label: "P11", bit: 7, adc_slot: None },
    Pin { label: "P14", bit: 8, adc_slot: None },
    Pin { label: "P15", bit: 9, adc_slot: Some(1) },
    Pin { label: "P16", bit: 10, adc_slot: None },
    Pin { label: "P17", bit: 11, adc_slot: None },
    Pin { label: "P18", bit: 12, adc_slot: None },
    Pin { label: "P20", bit: 13, adc_slot: Some(0) },
    Pin { label: "P23", bit: 14, adc_slot: Some(3) },
    Pin { label: "P24", bit: 15, adc_slot: Some(2) },
    Pin { label: "P31", bit: 19, adc_slot: None },
    Pin { label: "P32", bit: 20, adc_slot: None },
    Pin { label: "P33", bit: 21, adc_slot: None },
    Pin { label: "P34", bit: 22, adc_slot: None },
];

// PB-03F-Kit V1.0.0, figure 7: bottom view, top-to-bottom silkscreen order.
const BOARD_ROWS: &[(&str, &str)] = &[
    ("P24", "P15"),
    ("P23", "P11"),
    ("P20", "P31"),
    ("P3", "P7"),
    ("P2", "P32"),
    ("3V3", "P33"),
    ("GND", "P14"),
    ("NC", "P16"),
    ("P34", "P17"),
    ("P0", "GND"),
    ("P18", "3V3"),
    ("RX0", "NC"),
    ("TX0", "NC"),
    ("GND", "GND"),
    ("3V3", "5V"),
];

struct State {
    started: Instant,
    image: String,
    strict: bool,
    status: String,
    adv: String,
    gpio_dr: u32,
    gpio_ddr: u32,
    ext_in: u32,
    pwm: [u32; 6],
    adc_mv: [u16; 4], // P20, P15, P24, P23
    connected: bool,
    notify: bool,
    input: String,
    history: Vec<String>,
    history_pos: Option<usize>,
    logs: VecDeque<String>,
}

impl State {
    fn new(opts: &TuiOpts) -> Self {
        Self {
            started: Instant::now(),
            image: opts
                .hex
                .file_name()
                .map(|v| v.to_string_lossy().into_owned())
                .unwrap_or_else(|| opts.hex.display().to_string()),
            strict: opts.strict,
            status: "starting".into(),
            adv: "—".into(),
            gpio_dr: 0,
            gpio_ddr: 0,
            ext_in: 0,
            pwm: [0; 6],
            adc_mv: [3300, 1650, 2500, 3300],
            connected: false,
            notify: false,
            input: String::new(),
            history: Vec::new(),
            history_pos: None,
            logs: VecDeque::with_capacity(256),
        }
    }

    fn log(&mut self, line: impl Into<String>) {
        if self.logs.len() >= 256 {
            self.logs.pop_front();
        }
        let elapsed = self.started.elapsed().as_secs_f64();
        self.logs.push_back(format!("[{elapsed:8.3}] {}", line.into()));
    }

    fn apply_raw_line(&mut self, stream: Stream, line: &str) {
        if matches!(stream, Stream::Stderr) {
            self.log(format!("! {line}"));
            return;
        }

        if line == "READY" {
            self.status = "running".into();
            self.log("READY");
            return;
        }
        if let Some(rest) = line.strip_prefix("ADV ") {
            self.adv = rest.to_string();
            self.log(format!("ADV {rest}"));
            return;
        }
        if let Some(rest) = line.strip_prefix("GPIO ") {
            let mut p = rest.split_whitespace();
            if let (Some(dr), Some(ddr)) = (p.next(), p.next()) {
                if let (Ok(dr), Ok(ddr)) =
                    (u32::from_str_radix(dr, 16), u32::from_str_radix(ddr, 16))
                {
                    self.gpio_dr = dr;
                    self.gpio_ddr = ddr;
                    return;
                }
            }
        }
        if let Some(rest) = line.strip_prefix("PWM ") {
            let values: Vec<_> = rest
                .split_whitespace()
                .filter_map(|v| u32::from_str_radix(v, 16).ok())
                .collect();
            if values.len() == 6 {
                self.pwm.copy_from_slice(&values);
                return;
            }
        }
        if let Some(rest) = line.strip_prefix("UART ") {
            self.log(format!("UART {rest}"));
            return;
        }
        if let Some(rest) = line.strip_prefix("FRAME ") {
            self.log(format!("ATT ← {rest}"));
            return;
        }
        if let Some(rest) = line.strip_prefix("STOP ") {
            self.status = format!("stopped: {rest}");
            self.log(format!("STOP {rest}"));
            return;
        }
        self.log(line.to_string());
    }

    fn apply_command_state(&mut self, line: &str) {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        match lower.as_str() {
            "connect" => self.connected = true,
            "disconnect" => {
                self.connected = false;
                self.notify = false;
            }
            "notify on" | "cccd on" | "cccd 1" => self.notify = true,
            "notify off" | "cccd off" | "cccd 0" => self.notify = false,
            _ => {}
        }

        if let Some(rest) = lower.strip_prefix("adc ") {
            let values: Vec<_> = rest.split_whitespace().filter_map(parse_mv).collect();
            if values.len() == 4 {
                self.adc_mv.copy_from_slice(&values);
            }
        }

        if let Some(rest) = lower.strip_prefix("in ") {
            if let Ok(mask) = u32::from_str_radix(rest.trim().trim_start_matches("0x"), 16) {
                self.ext_in = mask;
            }
        }

        let mut words = lower.split_whitespace();
        if let (Some(pin), Some(level)) = (words.next(), words.next()) {
            if let Some(bit) = pin_bit(pin) {
                let high = matches!(level, "on" | "1" | "high" | "true");
                let low = matches!(level, "off" | "0" | "low" | "false");
                if high || low {
                    let mask = 1u32 << bit;
                    if high {
                        self.ext_in |= mask;
                    } else {
                        self.ext_in &= !mask;
                    }
                }
            }
        }
    }

    fn pin_value(&self, pin: Pin) -> (bool, bool) {
        let output = ((self.gpio_ddr >> pin.bit) & 1) != 0;
        let value = if output {
            ((self.gpio_dr >> pin.bit) & 1) != 0
        } else {
            ((self.ext_in >> pin.bit) & 1) != 0
        };
        (output, value)
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self, String> {
        enable_raw_mode().map_err(|e| e.to_string())?;
        execute!(io::stdout(), EnterAlternateScreen, Hide).map_err(|e| e.to_string())?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
    }
}

pub fn run(opts: TuiOpts) -> Result<ExitCode, String> {
    let mut child = spawn_emulator(&opts)?;
    let mut child_stdin = child.stdin.take().ok_or("child stdin unavailable")?;
    let child_stdout = child.stdout.take().ok_or("child stdout unavailable")?;
    let child_stderr = child.stderr.take().ok_or("child stderr unavailable")?;

    let (tx, rx) = mpsc::channel();
    spawn_reader(child_stdout, Stream::Stdout, tx.clone());
    spawn_reader(child_stderr, Stream::Stderr, tx);

    let _terminal = TerminalGuard::enter()?;
    let mut state = State::new(&opts);
    state.log(format!("image {}", opts.hex.display()));
    state.log("type help for emulator commands; Esc or Ctrl-C quits");

    let mut last_draw = Instant::now() - Duration::from_secs(1);
    loop {
        drain_events(&rx, &mut state);

        if event::poll(Duration::from_millis(10)).map_err(|e| e.to_string())? {
            if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
                if key.kind == KeyEventKind::Press {
                    if handle_key(key.code, key.modifiers, &mut state, &mut child_stdin)? {
                        let _ = writeln!(child_stdin, "quit");
                        let _ = child_stdin.flush();
                        break;
                    }
                }
            }
        }

        if last_draw.elapsed() >= Duration::from_millis(50) {
            draw(&state)?;
            last_draw = Instant::now();
        }

        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            drain_events(&rx, &mut state);
            state.status = format!("exited: {status}");
            draw(&state)?;
            return Ok(ExitCode::from(status.code().unwrap_or(1) as u8));
        }
    }

    for _ in 0..20 {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(ExitCode::from(status.code().unwrap_or(0) as u8));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    Ok(ExitCode::SUCCESS)
}

fn spawn_emulator(opts: &TuiOpts) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(exe);
    cmd.arg("--raw")
        .arg("--max-insns")
        .arg(opts.max_insns.to_string());
    if opts.strict {
        cmd.arg("--strict");
    }
    cmd.arg(&opts.hex)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.spawn().map_err(|e| format!("spawn emulator: {e}"))
}

fn spawn_reader<R: io::Read + Send + 'static>(reader: R, stream: Stream, tx: Sender<UiEvent>) {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else { break };
            if tx.send(UiEvent::Line(stream, line)).is_err() {
                break;
            }
        }
    });
}

fn drain_events(rx: &Receiver<UiEvent>, state: &mut State) {
    while let Ok(event) = rx.try_recv() {
        match event {
            UiEvent::Line(stream, line) => state.apply_raw_line(stream, &line),
        }
    }
}

fn handle_key(
    code: KeyCode,
    modifiers: KeyModifiers,
    state: &mut State,
    child_stdin: &mut impl Write,
) -> Result<bool, String> {
    if code == KeyCode::Esc
        || (code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL))
    {
        return Ok(true);
    }

    match code {
        KeyCode::Enter => {
            let line = state.input.trim().to_string();
            state.input.clear();
            state.history_pos = None;
            if !line.is_empty() {
                state.log(format!("> {line}"));
                state.apply_command_state(&line);
                writeln!(child_stdin, "{line}").map_err(|e| e.to_string())?;
                child_stdin.flush().map_err(|e| e.to_string())?;
                if state.history.last().map(String::as_str) != Some(line.as_str()) {
                    state.history.push(line.clone());
                }
                if matches!(line.as_str(), "q" | "quit" | "exit") {
                    return Ok(true);
                }
            }
        }
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => state.input.push(c),
        KeyCode::Up => history_up(state),
        KeyCode::Down => history_down(state),
        _ => {}
    }
    Ok(false)
}

fn history_up(state: &mut State) {
    if state.history.is_empty() {
        return;
    }
    let next = match state.history_pos {
        None => state.history.len() - 1,
        Some(0) => 0,
        Some(pos) => pos - 1,
    };
    state.history_pos = Some(next);
    state.input = state.history[next].clone();
}

fn history_down(state: &mut State) {
    let Some(pos) = state.history_pos else { return };
    if pos + 1 >= state.history.len() {
        state.history_pos = None;
        state.input.clear();
    } else {
        state.history_pos = Some(pos + 1);
        state.input = state.history[pos + 1].clone();
    }
}

fn draw(state: &State) -> Result<(), String> {
    let (width, height) = size().unwrap_or((120, 42));
    let mut out = String::new();
    let w = usize::from(width).max(86);

    push_line(
        &mut out,
        w,
        &format!(
            "PHY6252 / PB-03F LIVE   image={}   mode={}   {}",
            state.image,
            if state.strict { "STRICT" } else { "NORMAL" },
            state.status
        ),
    );
    push_line(
        &mut out,
        w,
        &format!(
            "BLE  link={}  notify={}  {}",
            on_off(state.connected),
            on_off(state.notify),
            state.adv
        ),
    );
    push_line(&mut out, w, "");
    push_line(
        &mut out,
        w,
        "PB-03F-KIT PINOUT — physical bottom view (Ai-Thinker V1.0.0 figure 7)",
    );
    push_line(&mut out, w, "       LEFT HEADER                         BOARD                         RIGHT HEADER");
    push_line(&mut out, w, "  ┌──────────────────────────────┐   ┌──────────────────┐   ┌──────────────────────────────┐");

    for (row, (left, right)) in BOARD_ROWS.iter().enumerate() {
        let left_text = board_pin_text(state, left);
        let right_text = board_pin_text(state, right);
        let middle = match row {
            0 => "│   PB-03F-Kit    │",
            5 => "│    PHY6252      │",
            10 => "│   NodeMCU       │",
            14 => "│      USB        │",
            _ => "│                  │",
        };
        push_line(
            &mut out,
            w,
            &format!("  │ {left_text:<28} │───{middle}───│ {right_text:<28} │"),
        );
    }
    push_line(&mut out, w, "  └──────────────────────────────┘   └──────────────────┘   └──────────────────────────────┘");
    push_line(&mut out, w, "  GPIO shows gpio_pin_e mapping; OUT=value from DR, IN=value from host-driven external level.");

    let leds = [
        ("R/P7", 4u32),
        ("G/P11", 7),
        ("B/P18", 12),
        ("W/P0", 0),
    ];
    let led_text = leds
        .iter()
        .map(|(name, bit)| format!("{name}={}", bit_value(state.gpio_dr, *bit)))
        .collect::<Vec<_>>()
        .join("  ");
    push_line(&mut out, w, &format!("LED  {led_text}"));
    push_line(
        &mut out,
        w,
        &format!(
            "PWM  c0={:04x} c1={:04x} c2={:04x} c3={:04x} c4={:04x} c5={:04x}",
            state.pwm[0], state.pwm[1], state.pwm[2], state.pwm[3], state.pwm[4], state.pwm[5]
        ),
    );
    push_line(&mut out, w, "");
    push_line(&mut out, w, "LOGS  (host elapsed seconds; UART / ATT / ROM / MMIO / power / secure diagnostics)");

    let fixed_rows = 27usize;
    let log_rows = usize::from(height).saturating_sub(fixed_rows).max(4);
    let start = state.logs.len().saturating_sub(log_rows);
    for line in state.logs.iter().skip(start) {
        push_line(&mut out, w, &format!("  {line}"));
    }
    while out.lines().count() < usize::from(height).saturating_sub(2) {
        out.push('\n');
    }
    push_line(&mut out, w, &format!("> {}", state.input));
    push_line(
        &mut out,
        w,
        "Enter=send  ↑/↓=history  Esc/Ctrl-C=quit   try: connect | adc 3.3 1.65 2.5 3.3 | p34 on",
    );

    let mut stdout = io::stdout();
    execute!(stdout, MoveTo(0, 0), Clear(ClearType::All)).map_err(|e| e.to_string())?;
    stdout.write_all(out.as_bytes()).map_err(|e| e.to_string())?;
    stdout.flush().map_err(|e| e.to_string())
}

fn board_pin_text(state: &State, label: &str) -> String {
    if let Some(pin) = PINS.iter().copied().find(|pin| pin.label == label) {
        return pin_text(state, pin);
    }
    match label {
        "3V3" => "3V3   PWR 3.3V".into(),
        "5V" => "5V    PWR 5V".into(),
        "GND" => "GND   0V".into(),
        "NC" => "NC    —".into(),
        "TX0" => "TX0   UART0 TX".into(),
        "RX0" => "RX0   UART0 RX".into(),
        other => other.into(),
    }
}

fn push_line(out: &mut String, width: usize, text: &str) {
    for (used, ch) in text.chars().enumerate() {
        if used >= width.saturating_sub(1) {
            break;
        }
        out.push(ch);
    }
    out.push('\n');
}

fn pin_text(state: &State, pin: Pin) -> String {
    let (output, value) = state.pin_value(pin);
    let mut text = format!(
        "{:>3} e{:02} {:>3}={}",
        pin.label,
        pin.bit,
        if output { "OUT" } else { "IN" },
        if value { 1 } else { 0 }
    );
    if let Some(slot) = pin.adc_slot {
        text.push_str(&format!(" {:.3}V", f64::from(state.adc_mv[slot]) / 1000.0));
    }
    text
}

fn parse_mv(text: &str) -> Option<u16> {
    if text.contains('.') {
        let volts: f64 = text.parse().ok()?;
        let mv = (volts * 1000.0).round();
        if !(0.0..=65535.0).contains(&mv) {
            return None;
        }
        Some(mv as u16)
    } else {
        text.parse().ok()
    }
}

fn pin_bit(label: &str) -> Option<u32> {
    match label.to_ascii_uppercase().as_str() {
        "P0" => Some(0),
        "P2" => Some(2),
        "P3" => Some(3),
        "P7" => Some(4),
        "P11" => Some(7),
        "P14" => Some(8),
        "P15" => Some(9),
        "P16" => Some(10),
        "P17" => Some(11),
        "P18" => Some(12),
        "P20" => Some(13),
        "P23" => Some(14),
        "P24" => Some(15),
        "P31" => Some(19),
        "P32" => Some(20),
        "P33" => Some(21),
        "P34" => Some(22),
        _ => None,
    }
}

fn bit_value(value: u32, bit: u32) -> u8 {
    ((value >> bit) & 1) as u8
}

fn on_off(value: bool) -> &'static str {
    if value { "ON" } else { "OFF" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> TuiOpts {
        TuiOpts {
            hex: PathBuf::from("demo.hex"),
            strict: false,
            max_insns: 1,
        }
    }

    #[test]
    fn pin_state_tracks_direction_and_external_level() {
        let mut state = State::new(&opts());
        let p34 = *PINS.iter().find(|p| p.label == "P34").unwrap();
        state.ext_in = 1 << 22;
        assert_eq!(state.pin_value(p34), (false, true));
        state.gpio_ddr = 1 << 22;
        state.gpio_dr = 0;
        assert_eq!(state.pin_value(p34), (true, false));
    }

    #[test]
    fn local_command_state_tracks_adc_and_link() {
        let mut state = State::new(&opts());
        state.apply_command_state("connect");
        state.apply_command_state("notify on");
        state.apply_command_state("adc 3.3 1.65 2.5 3.2");
        assert!(state.connected);
        assert!(state.notify);
        assert_eq!(state.adc_mv, [3300, 1650, 2500, 3200]);
    }

    #[test]
    fn physical_board_rows_match_pb03f_bottom_view() {
        assert_eq!(BOARD_ROWS.first(), Some(&("P24", "P15")));
        assert_eq!(BOARD_ROWS.last(), Some(&("3V3", "5V")));
        assert_eq!(BOARD_ROWS.len(), 15);
    }
}
