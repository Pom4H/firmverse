use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, size, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

pub struct TuiOpts { pub hex: PathBuf, pub strict: bool, pub max_insns: u64 }

#[derive(Clone, Copy)] enum Stream { Stdout, Stderr }
enum UiEvent { Line(Stream, String) }

#[derive(Clone, Copy)]
struct Pin { label: &'static str, bit: u32, adc: Option<usize> }

const PINS: &[Pin] = &[
    Pin { label: "P0", bit: 0, adc: None }, Pin { label: "P2", bit: 2, adc: None },
    Pin { label: "P3", bit: 3, adc: None }, Pin { label: "P7", bit: 4, adc: None },
    Pin { label: "P11", bit: 7, adc: None }, Pin { label: "P14", bit: 8, adc: None },
    Pin { label: "P15", bit: 9, adc: Some(1) }, Pin { label: "P16", bit: 10, adc: None },
    Pin { label: "P17", bit: 11, adc: None }, Pin { label: "P18", bit: 12, adc: None },
    Pin { label: "P20", bit: 13, adc: Some(0) }, Pin { label: "P23", bit: 14, adc: Some(3) },
    Pin { label: "P24", bit: 15, adc: Some(2) }, Pin { label: "P31", bit: 19, adc: None },
    Pin { label: "P32", bit: 20, adc: None }, Pin { label: "P33", bit: 21, adc: None },
    Pin { label: "P34", bit: 22, adc: None },
];

const BOARD_ROWS: &[(&str, &str)] = &[
    ("P24", "P15"), ("P23", "P11"), ("P20", "P31"), ("P3", "P7"), ("P2", "P32"),
    ("3V3", "P33"), ("GND", "P14"), ("NC", "P16"), ("P34", "P17"), ("P0", "GND"),
    ("P18", "3V3"), ("RX0", "NC"), ("TX0", "NC"), ("GND", "GND"), ("3V3", "5V"),
];

struct State {
    started: Instant, image: String, strict: bool, status: String, adv: String,
    gpio_dr: u32, gpio_ddr: u32, ext_in: u32, pwm: [u32; 6], adc: [u16; 4],
    connected: bool, notify: bool, input: String, history: Vec<String>, history_pos: Option<usize>,
    logs: VecDeque<String>,
}

impl State {
    fn new(opts: &TuiOpts) -> Self {
        Self {
            started: Instant::now(),
            image: opts.hex.file_name().map(|v| v.to_string_lossy().into_owned()).unwrap_or_else(|| opts.hex.display().to_string()),
            strict: opts.strict, status: "STARTING".into(), adv: "-".into(),
            gpio_dr: 0, gpio_ddr: 0, ext_in: 0, pwm: [0; 6], adc: [3300, 1650, 2500, 3300],
            connected: false, notify: false, input: String::new(), history: Vec::new(), history_pos: None,
            logs: VecDeque::with_capacity(256),
        }
    }

    fn log(&mut self, line: impl Into<String>) {
        if self.logs.len() == 256 { self.logs.pop_front(); }
        self.logs.push_back(format!("[{:.3}] {}", self.started.elapsed().as_secs_f64(), line.into()));
    }

    fn raw(&mut self, stream: Stream, line: &str) {
        if matches!(stream, Stream::Stderr) { self.log(format!("! {line}")); return; }
        if line == "READY" { self.status = "RUNNING".into(); self.log("READY"); return; }
        if let Some(v) = line.strip_prefix("ADV ") { self.adv = v.into(); return; }
        if let Some(v) = line.strip_prefix("GPIO ") {
            let mut p = v.split_whitespace();
            if let (Some(a), Some(b)) = (p.next(), p.next()) {
                if let (Ok(a), Ok(b)) = (u32::from_str_radix(a, 16), u32::from_str_radix(b, 16)) { self.gpio_dr = a; self.gpio_ddr = b; return; }
            }
        }
        if let Some(v) = line.strip_prefix("PWM ") {
            let a: Vec<_> = v.split_whitespace().filter_map(|x| u32::from_str_radix(x, 16).ok()).collect();
            if a.len() == 6 { self.pwm.copy_from_slice(&a); return; }
        }
        if let Some(v) = line.strip_prefix("UART ") { self.log(format!("UART {v}")); return; }
        if let Some(v) = line.strip_prefix("FRAME ") { self.log(format!("ATT <- {v}")); return; }
        if let Some(v) = line.strip_prefix("STOP ") { self.status = format!("STOPPED: {v}"); self.log(format!("STOP {v}")); return; }
        self.log(line);
    }

    fn command(&mut self, line: &str) {
        let s = line.trim().to_ascii_lowercase();
        match s.as_str() {
            "connect" => self.connected = true,
            "disconnect" => { self.connected = false; self.notify = false; }
            "notify on" | "cccd on" | "cccd 1" => self.notify = true,
            "notify off" | "cccd off" | "cccd 0" => self.notify = false,
            _ => {}
        }
        if let Some(v) = s.strip_prefix("adc ") {
            let a: Vec<_> = v.split_whitespace().filter_map(parse_mv).collect();
            if a.len() == 4 { self.adc.copy_from_slice(&a); }
        }
        if let Some(v) = s.strip_prefix("in ") {
            if let Ok(mask) = u32::from_str_radix(v.trim().trim_start_matches("0x"), 16) { self.ext_in = mask; }
        }
        let mut p = s.split_whitespace();
        if let (Some(pin), Some(level)) = (p.next(), p.next()) {
            if let Some(bit) = pin_bit(pin) {
                let mask = 1u32 << bit;
                if matches!(level, "on" | "1" | "high" | "true") { self.ext_in |= mask; }
                if matches!(level, "off" | "0" | "low" | "false") { self.ext_in &= !mask; }
            }
        }
    }

    fn pin(&self, pin: Pin) -> (bool, bool) {
        let output = (self.gpio_ddr >> pin.bit) & 1 != 0;
        let value = if output { (self.gpio_dr >> pin.bit) & 1 != 0 } else { (self.ext_in >> pin.bit) & 1 != 0 };
        (output, value)
    }
}

struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> Result<Self, String> { enable_raw_mode().map_err(|e| e.to_string())?; execute!(io::stdout(), EnterAlternateScreen, Hide).map_err(|e| e.to_string())?; Ok(Self) }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) { let _ = disable_raw_mode(); let _ = execute!(io::stdout(), Show, LeaveAlternateScreen); }
}

pub fn run(opts: TuiOpts) -> Result<ExitCode, String> {
    let mut child = spawn_emulator(&opts)?;
    let mut input = child.stdin.take().ok_or("child stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("child stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("child stderr unavailable")?;
    let (tx, rx) = mpsc::channel();
    spawn_reader(stdout, Stream::Stdout, tx.clone()); spawn_reader(stderr, Stream::Stderr, tx);
    let _terminal = TerminalGuard::enter()?;
    let mut state = State::new(&opts); state.log(format!("image {}", opts.hex.display()));
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    loop {
        drain(&rx, &mut state);
        if event::poll(Duration::from_millis(10)).map_err(|e| e.to_string())? {
            if let Event::Key(k) = event::read().map_err(|e| e.to_string())? {
                if k.kind == KeyEventKind::Press && key(k.code, k.modifiers, &mut state, &mut input)? { let _ = writeln!(input, "quit"); break; }
            }
        }
        if last_draw.elapsed() >= Duration::from_millis(50) { draw(&state)?; last_draw = Instant::now(); }
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? { drain(&rx, &mut state); state.status = format!("EXITED: {s}"); draw(&state)?; return Ok(ExitCode::from(s.code().unwrap_or(1) as u8)); }
    }
    for _ in 0..20 { if let Some(s) = child.try_wait().map_err(|e| e.to_string())? { return Ok(ExitCode::from(s.code().unwrap_or(0) as u8)); } thread::sleep(Duration::from_millis(10)); }
    let _ = child.kill(); Ok(ExitCode::SUCCESS)
}

fn spawn_emulator(opts: &TuiOpts) -> Result<Child, String> {
    let mut c = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    c.arg("--raw").arg("--max-insns").arg(opts.max_insns.to_string()); if opts.strict { c.arg("--strict"); }
    c.arg(&opts.hex).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()); c.spawn().map_err(|e| format!("spawn emulator: {e}"))
}

fn spawn_reader<R: io::Read + Send + 'static>(r: R, stream: Stream, tx: Sender<UiEvent>) { thread::spawn(move || for line in BufReader::new(r).lines() { let Ok(line) = line else { break }; if tx.send(UiEvent::Line(stream, line)).is_err() { break } }); }
fn drain(rx: &Receiver<UiEvent>, state: &mut State) { while let Ok(UiEvent::Line(stream, line)) = rx.try_recv() { state.raw(stream, &line); } }

fn key(code: KeyCode, mods: KeyModifiers, state: &mut State, child: &mut impl Write) -> Result<bool, String> {
    if code == KeyCode::Esc || (code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL)) { return Ok(true); }
    match code {
        KeyCode::Enter => { let line = state.input.trim().to_string(); state.input.clear(); state.history_pos = None; if !line.is_empty() { state.log(format!("> {line}")); state.command(&line); writeln!(child, "{line}").map_err(|e| e.to_string())?; child.flush().map_err(|e| e.to_string())?; if state.history.last() != Some(&line) { state.history.push(line.clone()); } if matches!(line.as_str(), "q" | "quit" | "exit") { return Ok(true); } } }
        KeyCode::Backspace => { state.input.pop(); }
        KeyCode::Char(c) if !mods.contains(KeyModifiers::CONTROL) => state.input.push(c),
        KeyCode::Up => { if !state.history.is_empty() { let p = state.history_pos.map_or(state.history.len() - 1, |p| p.saturating_sub(1)); state.history_pos = Some(p); state.input = state.history[p].clone(); } }
        KeyCode::Down => { if let Some(p) = state.history_pos { if p + 1 < state.history.len() { state.history_pos = Some(p + 1); state.input = state.history[p + 1].clone(); } else { state.history_pos = None; state.input.clear(); } } }
        _ => {}
    }
    Ok(false)
}

fn draw(state: &State) -> Result<(), String> {
    let (w, h) = size().unwrap_or((100, 36)); let w = usize::from(w).max(1); let h = usize::from(h).max(1); let mut lines = Vec::new();
    lines.push(format!("PHY6252  {}  {}  {}", state.status, if state.strict { "STRICT" } else { "NORMAL" }, state.image));
    lines.push(format!("BLE {}  notify={}  {}", if state.connected { "CONNECTED" } else { "OFFLINE" }, on_off(state.notify), state.adv));
    lines.push(format!("ADC P20={:.3}V P15={:.3}V P24={:.3}V P23={:.3}V", volts(state.adc[0]), volts(state.adc[1]), volts(state.adc[2]), volts(state.adc[3])));
    if w >= 68 && h >= 26 {
        lines.push(String::new()); lines.push("PB-03F-Kit bottom view - pin / gpio / direction / level".into());
        for (left, right) in BOARD_ROWS { lines.push(format!("{:<31} | {:>31}", pin_text(state, left), pin_text(state, right))); }
    } else { lines.push("Pinout hidden: enlarge terminal to at least 68x26".into()); }
    lines.push(format!("LED R/P7={} G/P11={} B/P18={} W/P0={}", bit(state.gpio_dr, 4), bit(state.gpio_dr, 7), bit(state.gpio_dr, 12), bit(state.gpio_dr, 0)));
    lines.push(format!("PWM {:04x} {:04x} {:04x} {:04x} {:04x} {:04x}", state.pwm[0], state.pwm[1], state.pwm[2], state.pwm[3], state.pwm[4], state.pwm[5]));
    lines.push(String::new()); lines.push("LOG".into());
    let body = h.saturating_sub(2); let n = body.saturating_sub(lines.len()).max(1); for v in state.logs.iter().skip(state.logs.len().saturating_sub(n)) { lines.push(format!("  {v}")); }
    lines.truncate(body); while lines.len() < body { lines.push(String::new()); }
    lines.push(format!("> {}", state.input)); lines.push("Enter send | Up/Down history | Esc quit | connect | adc 3.3 1.65 2.5 3.3 | p34 on".into());
    let mut text = String::new(); for line in lines.into_iter().take(h) { push_line(&mut text, w, &line); }
    let mut out = io::stdout(); execute!(out, MoveTo(0, 0), Clear(ClearType::All)).map_err(|e| e.to_string())?; out.write_all(text.as_bytes()).map_err(|e| e.to_string())?; out.flush().map_err(|e| e.to_string())
}

fn pin_text(state: &State, label: &str) -> String {
    if let Some(p) = PINS.iter().copied().find(|p| p.label == label) { let (out, value) = state.pin(p); let mut s = format!("{:>3} e{:02} {}={}", p.label, p.bit, if out { "OUT" } else { "IN " }, u8::from(value)); if let Some(i) = p.adc { s.push_str(&format!(" {:.3}V", volts(state.adc[i]))); } return s; }
    match label { "3V3" => "3V3 PWR".into(), "5V" => "5V PWR".into(), "GND" => "GND".into(), "NC" => "NC".into(), "TX0" => "TX0 UART0 TX".into(), "RX0" => "RX0 UART0 RX".into(), _ => label.into() }
}

fn push_line(out: &mut String, width: usize, text: &str) { for (i, c) in text.chars().enumerate() { if i >= width.saturating_sub(1) { break; } out.push(c); } out.push('\n'); }
fn volts(mv: u16) -> f64 { f64::from(mv) / 1000.0 }
fn parse_mv(s: &str) -> Option<u16> { if s.contains('.') { let v = (s.parse::<f64>().ok()? * 1000.0).round(); (0.0..=65535.0).contains(&v).then_some(v as u16) } else { s.parse().ok() } }
fn bit(v: u32, b: u32) -> u8 { ((v >> b) & 1) as u8 }
fn on_off(v: bool) -> &'static str { if v { "ON" } else { "OFF" } }
fn pin_bit(s: &str) -> Option<u32> { match s.to_ascii_uppercase().as_str() { "P0" => Some(0), "P2" => Some(2), "P3" => Some(3), "P7" => Some(4), "P11" => Some(7), "P14" => Some(8), "P15" => Some(9), "P16" => Some(10), "P17" => Some(11), "P18" => Some(12), "P20" => Some(13), "P23" => Some(14), "P24" => Some(15), "P31" => Some(19), "P32" => Some(20), "P33" => Some(21), "P34" => Some(22), _ => None } }

#[cfg(test)] mod tests {
    use super::*;
    fn opts() -> TuiOpts { TuiOpts { hex: PathBuf::from("demo.hex"), strict: false, max_insns: 1 } }
    #[test] fn pin_state_tracks_direction_and_external_level() { let mut s = State::new(&opts()); let p = *PINS.iter().find(|p| p.label == "P34").unwrap(); s.ext_in = 1 << 22; assert_eq!(s.pin(p), (false, true)); s.gpio_ddr = 1 << 22; s.gpio_dr = 0; assert_eq!(s.pin(p), (true, false)); }
    #[test] fn local_command_state_tracks_adc_and_link() { let mut s = State::new(&opts()); s.command("connect"); s.command("notify on"); s.command("adc 3.3 1.65 2.5 3.3"); assert!(s.connected && s.notify); assert_eq!(s.adc, [3300, 1650, 2500, 3300]); }
    #[test] fn physical_board_rows_match_pb03f_bottom_view() { assert_eq!(BOARD_ROWS.first(), Some(&("P24", "P15"))); assert_eq!(BOARD_ROWS.last(), Some(&("3V3", "5V"))); assert_eq!(BOARD_ROWS.len(), 15); }
    #[test] fn clipping_respects_terminal_width() { let mut s = String::new(); push_line(&mut s, 8, "123456789"); assert_eq!(s.trim_end().len(), 7); }
}
