use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug)]
pub struct HexImage {
    pub bytes: Vec<(u32, u8)>,
}

impl HexImage {
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        let mut bytes = Vec::new();
        let mut ext = 0u32;
        for raw in text.lines() {
            let line = raw.trim();
            if !line.starts_with(':') {
                continue;
            }
            let data = decode_line(line)?;
            let count = data[0] as usize;
            let addr = u16::from_be_bytes([data[1], data[2]]) as u32;
            let kind = data[3];
            match kind {
                0 => {
                    let base = ext.wrapping_add(addr);
                    for (i, value) in data[4..4 + count].iter().enumerate() {
                        bytes.push((base + i as u32, *value));
                    }
                }
                1 => break,
                2 => ext = u32::from(u16::from_be_bytes([data[4], data[5]])) << 4,
                4 => ext = u32::from(u16::from_be_bytes([data[4], data[5]])) << 16,
                5 | 3 => {}
                other => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unsupported HEX record {other}"),
                    ));
                }
            }
        }
        Ok(Self { bytes })
    }

    pub fn fill(&self, base: u32, dest: &mut [u8]) {
        for (addr, value) in &self.bytes {
            if *addr >= base {
                let offset = (*addr - base) as usize;
                if offset < dest.len() {
                    dest[offset] = *value;
                }
            }
        }
    }
}

fn decode_line(line: &str) -> io::Result<Vec<u8>> {
    if line.len() < 11 || (line.len() - 1) % 2 != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated HEX line"));
    }
    let mut out = Vec::with_capacity((line.len() - 1) / 2);
    let chars: Vec<u8> = line[1..].bytes().collect();
    for chunk in chars.chunks_exact(2) {
        let hi = hex_digit(chunk[0])?;
        let lo = hex_digit(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_digit(c: u8) -> io::Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "non-hex digit")),
    }
}

#[cfg(test)]
mod tests {
    use super::decode_line;

    #[test]
    fn decodes_data_record() {
        let data = decode_line(":020000040000FA").expect("hex");
        assert_eq!(data[0], 2);
        assert_eq!(data[3], 4);
    }
}
