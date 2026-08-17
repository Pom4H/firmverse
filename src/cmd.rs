//! Chip commands: wire protocol and a small REPL language.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChipCmd {
    In(u32),
    Pin { bit: u32, high: bool },
    Write(Vec<u8>),
    Connect,
    Disconnect,
    Cccd(bool),
    Tick(u32),
    Adc([u16; 4]),
    Help,
    Quit,
}

pub const HELP: &str = "\
phy6252  —  type a command, then enter

  connect            BLE link up
  disconnect         BLE link down
  notify on|off      CCCD / indications
  write Hello        GATT write (text or hex)
  adc 3.3 1.65 2.5 3.3   P20 P15 P24 P23 (V or mV)
  p34 on             button / reset pad
  in 00400000        raw AP_GPIO ext mask
  tick 80            advance mailbox clock
  help  quit
";

pub fn parse_line(line: &str) -> Result<Option<ChipCmd>, String> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    let lower = line.to_ascii_lowercase();
    match lower.as_str() {
        "help" | "?" | "h" => return Ok(Some(ChipCmd::Help)),
        "quit" | "exit" | "q" => return Ok(Some(ChipCmd::Quit)),
        "connect" => return Ok(Some(ChipCmd::Connect)),
        "disconnect" => return Ok(Some(ChipCmd::Disconnect)),
        _ => {}
    }
    if let Some(cmd) = parse_protocol(line) {
        return Ok(Some(cmd));
    }
    parse_friendly(line).map(Some)
}

fn parse_protocol(line: &str) -> Option<ChipCmd> {
    if let Some(rest) = line.strip_prefix("IN ") {
        let value = u32::from_str_radix(rest.trim(), 16).ok()?;
        return Some(ChipCmd::In(value));
    }
    if let Some(rest) = line.strip_prefix("WRITE ") {
        return Some(ChipCmd::Write(parse_write_payload(rest.trim())?));
    }
    if line == "CONNECT" {
        return Some(ChipCmd::Connect);
    }
    if line == "DISCONNECT" {
        return Some(ChipCmd::Disconnect);
    }
    if let Some(rest) = line.strip_prefix("CCCD ") {
        let n: u32 = rest.trim().parse().ok()?;
        return Some(ChipCmd::Cccd(n != 0));
    }
    if let Some(rest) = line.strip_prefix("TICK ") {
        let value = rest.trim().parse::<u32>().ok()?;
        return Some(ChipCmd::Tick(value));
    }
    if let Some(rest) = line.strip_prefix("ADC ") {
        return parse_adc(rest);
    }
    None
}

fn parse_friendly(line: &str) -> Result<ChipCmd, String> {
    let mut parts = line.split_whitespace();
    let verb = parts.next().unwrap_or("").to_ascii_lowercase();
    match verb.as_str() {
        "notify" | "cccd" => {
            let on = parse_on_off(parts.next().unwrap_or("on"))?;
            Ok(ChipCmd::Cccd(on))
        }
        "write" | "rx" => {
            let rest = line[verb.len()..].trim();
            parse_write_payload(rest)
                .map(ChipCmd::Write)
                .ok_or_else(|| "write needs text or even-length hex".into())
        }
        "adc" => parse_adc(line[verb.len()..].trim()).ok_or_else(|| {
            "adc P20 P15 P24 P23 — volts (3.3) or millivolts (3300)".into()
        }),
        "tick" => {
            let ms = parts
                .next()
                .and_then(|s| s.parse().ok())
                .ok_or("tick <ms>")?;
            Ok(ChipCmd::Tick(ms))
        }
        "in" => {
            let value = parts
                .next()
                .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .ok_or("in <hex mask>")?;
            Ok(ChipCmd::In(value))
        }
        pin if pin.starts_with('p') || pin.starts_with('P') => {
            let bit = silk_bit(pin).ok_or_else(|| format!("unknown pad {pin}"))?;
            let high = parse_on_off(parts.next().unwrap_or("on"))?;
            Ok(ChipCmd::Pin { bit, high })
        }
        other => Err(format!("unknown command {other:?} — help")),
    }
}

fn parse_on_off(word: &str) -> Result<bool, String> {
    match word.to_ascii_lowercase().as_str() {
        "1" | "on" | "high" | "true" => Ok(true),
        "0" | "off" | "low" | "false" => Ok(false),
        other => Err(format!("expected on/off, got {other}")),
    }
}

fn parse_adc(rest: &str) -> Option<ChipCmd> {
    let mut parts = rest.split_whitespace();
    let p20 = parse_mv(parts.next()?)?;
    let p15 = parse_mv(parts.next()?)?;
    let p24 = parse_mv(parts.next()?)?;
    let p23 = parse_mv(parts.next()?)?;
    Some(ChipCmd::Adc([p20, p15, p24, p23]))
}

fn parse_mv(text: &str) -> Option<u16> {
    if text.contains('.') {
        let volts: f64 = text.parse().ok()?;
        let mv = (volts * 1000.0).round();
        if mv < 0.0 || mv > 65535.0 {
            return None;
        }
        return Some(mv as u16);
    }
    text.parse().ok()
}

fn parse_write_payload(text: &str) -> Option<Vec<u8>> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() >= 2 && compact.len() % 2 == 0 && compact.bytes().all(is_hex_byte) {
        return parse_hex_bytes(&compact);
    }
    if text.is_empty() {
        return None;
    }
    Some(text.as_bytes().to_vec())
}

pub fn parse_hex_bytes(text: &str) -> Option<Vec<u8>> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() % 2 != 0 || compact.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    let bytes = compact.as_bytes();
    for chunk in bytes.chunks_exact(2) {
        let hi = hex_digit(chunk[0])?;
        let lo = hex_digit(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn is_hex_byte(c: u8) -> bool {
    hex_digit(c).is_some()
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn silk_bit(label: &str) -> Option<u32> {
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

pub fn gpio_silk(dr: u32, ddr: u32) -> String {
    const PADS: [(&str, u32); 11] = [
        ("R", 4),
        ("G", 7),
        ("B", 12),
        ("W", 0),
        ("P14", 8),
        ("P16", 10),
        ("P17", 11),
        ("P31", 19),
        ("P32", 20),
        ("P33", 21),
        ("P34", 22),
    ];
    let mut on = Vec::new();
    for (name, bit) in PADS {
        if ((ddr >> bit) & 1) == 1 && ((dr >> bit) & 1) == 1 {
            on.push(name);
        }
    }
    if on.is_empty() {
        "—".into()
    } else {
        on.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_line, ChipCmd};

    #[test]
    fn protocol_adc_write() {
        match parse_line("ADC 12000 5000 3300 1650").unwrap() {
            Some(ChipCmd::Adc(v)) => assert_eq!(v, [12000, 5000, 3300, 1650]),
            _ => panic!("adc"),
        }
        match parse_line("WRITE 48656c6c6f").unwrap() {
            Some(ChipCmd::Write(b)) => assert_eq!(b, b"Hello"),
            _ => panic!("write"),
        }
    }

    #[test]
    fn friendly() {
        match parse_line("adc 3.3 1.65 2.5 3.3").unwrap() {
            Some(ChipCmd::Adc(v)) => assert_eq!(v, [3300, 1650, 2500, 3300]),
            _ => panic!("volts"),
        }
        match parse_line("p34 on").unwrap() {
            Some(ChipCmd::Pin { bit, high }) => {
                assert_eq!(bit, 22);
                assert!(high);
            }
            _ => panic!("pin"),
        }
        match parse_line("write hi").unwrap() {
            Some(ChipCmd::Write(b)) => assert_eq!(b, b"hi"),
            _ => panic!("text write"),
        }
        assert!(matches!(parse_line("connect").unwrap(), Some(ChipCmd::Connect)));
    }
}
