//! Chip commands: wire protocol and a small REPL language.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChipCmd {
    In(u32),
    Pin { bit: u32, high: bool },
    Write(Vec<u8>),
    Scan { addr: [u8; 6], rssi: i8 },
    Gone { addr: [u8; 6] },
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

  scan AA:BB:CC:DD:EE:FF -42   advertise / update RSSI
  gone AA:BB:CC:DD:EE:FF       device left / timed out
  connect            BLE link up
  disconnect         BLE link down
  notify on|off      CCCD / indications
  write Hello        GATT write (text or hex)
  adc 3.3 1.65 2.5 3.3   P20 P15 P24 P23 (V or mV)
  p15 on             PHY6252 pad P15 (PB-03F Restore)
  p34 on             PHY6252 pad P34 (PB-03F white LED)
  in 00000200        raw AP_GPIO ext mask
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

pub const SCAN_PKT_MAGIC: u8 = 0xB1;
pub const SCAN_PKT_SEEN: u8 = 0;
pub const SCAN_PKT_GONE: u8 = 1;

pub fn scan_packet(addr: &[u8; 6], rssi: i8, gone: bool) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(9);
    pkt.push(SCAN_PKT_MAGIC);
    pkt.push(if gone { SCAN_PKT_GONE } else { SCAN_PKT_SEEN });
    pkt.extend_from_slice(addr);
    pkt.push(rssi as u8);
    pkt
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
    if let Some(rest) = line.strip_prefix("SCAN ") {
        return parse_scan_args(rest, false);
    }
    if let Some(rest) = line.strip_prefix("GONE ") {
        return parse_scan_args(rest, true);
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
        "adc" => parse_adc(line[verb.len()..].trim())
            .ok_or_else(|| "adc P20 P15 P24 P23 — volts (3.3) or millivolts (3300)".into()),
        "tick" => {
            let ms = parts
                .next()
                .and_then(|s| s.parse().ok())
                .ok_or("tick <ms>")?;
            Ok(ChipCmd::Tick(ms))
        }
        "scan" => parse_scan_args(&line[verb.len()..], false)
            .ok_or_else(|| "scan <aa:bb:cc:dd:ee:ff> <rssi>".into()),
        "gone" | "lost" => parse_scan_args(&line[verb.len()..], true)
            .ok_or_else(|| "gone <aa:bb:cc:dd:ee:ff>".into()),
        "in" => {
            let value = parts
                .next()
                .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .ok_or("in <hex mask>")?;
            Ok(ChipCmd::In(value))
        }
        pin if pin.starts_with('p') || pin.starts_with('P') => {
            let bit = silk_bit(pin).ok_or_else(|| format!("unknown PHY6252 pad {pin}"))?;
            let high = parse_on_off(parts.next().unwrap_or("on"))?;
            Ok(ChipCmd::Pin { bit, high })
        }
        other => Err(format!("unknown command {other:?} — help")),
    }
}

fn parse_scan_args(rest: &str, gone: bool) -> Option<ChipCmd> {
    let mut parts = rest.split_whitespace();
    let addr = parse_mac(parts.next()?)?;
    if gone {
        return Some(ChipCmd::Gone { addr });
    }
    let rssi = parse_rssi(parts.next()?)?;
    Some(ChipCmd::Scan { addr, rssi })
}

fn parse_mac(text: &str) -> Option<[u8; 6]> {
    let compact: String = text.chars().filter(|c| *c != ':' && *c != '-').collect();
    if compact.len() != 12 {
        return None;
    }
    let bytes = parse_hex_bytes(&compact)?;
    bytes.try_into().ok()
}

fn parse_rssi(text: &str) -> Option<i8> {
    if let Ok(value) = text.parse::<i16>() {
        if (-128..=127).contains(&value) {
            return Some(value as i8);
        }
    }
    if text.len() == 2 {
        let bytes = parse_hex_bytes(text)?;
        if bytes.len() == 1 {
            return Some(bytes[0] as i8);
        }
    }
    None
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
        if !(0.0..=65535.0).contains(&mv) {
            return None;
        }
        return Some(mv as u16);
    }
    text.parse().ok()
}

fn parse_write_payload(text: &str) -> Option<Vec<u8>> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() >= 2 && compact.len().is_multiple_of(2) && compact.bytes().all(is_hex_byte) {
        return parse_hex_bytes(&compact);
    }
    if text.is_empty() {
        return None;
    }
    Some(text.as_bytes().to_vec())
}

pub fn parse_hex_bytes(text: &str) -> Option<Vec<u8>> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.len().is_multiple_of(2) || compact.is_empty() {
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

/// PHY6252 package pad to AP_GPIO bit mapping.
/// This belongs to the SoC/package contract, not to a particular development board.
fn silk_bit(label: &str) -> Option<u32> {
    crate::soc::phy6252::pins::gpio_bit(label)
}

pub fn gpio_silk(dr: u32, ddr: u32) -> String {
    crate::board::gpio_summary(crate::board::BoardKind::Pb03fKit, dr, ddr)
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
        match parse_line("p15 on").unwrap() {
            Some(ChipCmd::Pin { bit, high }) => {
                assert_eq!(bit, 9);
                assert!(high);
            }
            _ => panic!("restore"),
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
        assert!(matches!(
            parse_line("connect").unwrap(),
            Some(ChipCmd::Connect)
        ));
        match parse_line("scan aa:bb:cc:dd:ee:01 -40").unwrap() {
            Some(ChipCmd::Scan { addr, rssi }) => {
                assert_eq!(addr, [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x01]);
                assert_eq!(rssi, -40);
            }
            _ => panic!("scan"),
        }
        match parse_line("GONE aabbccddee02").unwrap() {
            Some(ChipCmd::Gone { addr }) => assert_eq!(addr, [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x02]),
            _ => panic!("gone"),
        }
    }

    #[test]
    fn scan_packet_layout() {
        let pkt = super::scan_packet(&[1, 2, 3, 4, 5, 6], -42, false);
        assert_eq!(pkt, vec![0xB1, 0, 1, 2, 3, 4, 5, 6, (-42i8) as u8]);
        let gone = super::scan_packet(&[1, 2, 3, 4, 5, 6], 0, true);
        assert_eq!(gone[1], 1);
    }
}
