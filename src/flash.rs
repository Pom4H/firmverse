//! Transport-agnostic PHY62xx ROM flasher.
//!
//! The host-side protocol is intentionally independent from a concrete serial
//! adapter. The same `Flasher` is used against a PB-03F-Kit USB-UART link and
//! against the deterministic in-memory harness used by tests and agents.

use crate::programmer::{chunk_is_erased, pad_cpbin_chunk, FlashImage};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

pub const START_BAUD: u32 = 9_600;
pub const DEFAULT_BAUD: u32 = 115_200;
const SECTOR: u32 = 0x1000;
const CPBIN_BLK: u32 = 0x2000;
const OK: &[u8] = b"#OK>>:";

pub trait Transport {
    fn baud(&self) -> Result<u32, String>;
    fn set_baud(&mut self, baud: u32) -> Result<(), String>;
    fn set_timeout(&mut self, timeout: Duration) -> Result<(), String>;
    fn clear(&mut self) -> Result<(), String>;
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), String>;
    fn read(&mut self, len: usize) -> Result<Vec<u8>, String>;
}

pub trait TargetAdapter {
    type Link: Transport;

    fn enter_bootloader(&mut self) -> Result<(), String>;
    fn link(&mut self) -> &mut Self::Link;
}

#[derive(Clone, Copy, Debug)]
pub struct FlashOptions {
    pub baud: u32,
    pub chip_erase: bool,
    pub reset_after: bool,
}

impl Default for FlashOptions {
    fn default() -> Self {
        Self {
            baud: DEFAULT_BAUD,
            chip_erase: false,
            reset_after: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashReport {
    pub revision: String,
    pub flash_id: u32,
    pub flash_size: u32,
    pub bytes_written: usize,
}

pub struct Flasher {
    options: FlashOptions,
}

impl Flasher {
    pub fn new(options: FlashOptions) -> Self {
        Self { options }
    }

    pub fn flash<T: TargetAdapter>(
        &self,
        target: &mut T,
        image: &FlashImage,
    ) -> Result<FlashReport, String> {
        target.enter_bootloader()?;
        let mut rom = Phy62xxRom::new(target.link(), self.options.baud);
        let revision = rom.connect()?;
        if self.options.chip_erase {
            rom.erase_all()?;
        }
        rom.init_spifs()?;
        rom.expand_flash_window()?;
        let bytes_written = rom.write_image(image)?;
        if self.options.reset_after {
            rom.reset()?;
        }
        Ok(FlashReport {
            revision,
            flash_id: rom.flash_id,
            flash_size: rom.flash_size(),
            bytes_written,
        })
    }
}

pub struct Phy62xxRom<'a, T: Transport> {
    io: &'a mut T,
    run_baud: u32,
    flash_id: u32,
    patch_flash: u32,
    cpbin: u32,
    erased_from: u32,
    erased_to: u32,
}

impl<'a, T: Transport> Phy62xxRom<'a, T> {
    pub fn new(io: &'a mut T, run_baud: u32) -> Self {
        Self {
            io,
            run_baud,
            flash_id: 0,
            patch_flash: 0,
            cpbin: 0,
            erased_from: 0x40_0000,
            erased_to: 0x40_0000,
        }
    }

    pub fn flash_size(&self) -> u32 {
        if self.flash_id == 0 {
            0
        } else {
            1u32 << ((self.flash_id >> 16) & 0xff)
        }
    }

    pub fn connect(&mut self) -> Result<String, String> {
        self.io.set_baud(START_BAUD)?;
        self.io.set_timeout(Duration::from_millis(40))?;
        self.io.clear()?;

        let mut last = Vec::new();
        for _ in 0..4_000 {
            self.io.write_all(b"UXTDWU")?;
            let read = self.io.read(6)?;
            if read.as_slice() == b"cmd>>:" {
                self.io.set_baud(DEFAULT_BAUD)?;
                self.io.set_timeout(Duration::from_millis(200))?;
                let revision = self.read_revision()?;
                self.flash_unlock()?;
                self.set_baud(self.run_baud)?;
                return Ok(revision);
            }
            if read.as_slice() == b"fct>>:" {
                return Err("chip is in FCT mode".into());
            }
            last = read;
        }
        Err(format!(
            "PHY62xx ROM did not answer UXTDWU: {}",
            String::from_utf8_lossy(&last)
        ))
    }

    pub fn read_revision(&mut self) -> Result<String, String> {
        self.io.write_all(b"rdrev+ ")?;
        let read = self.io.read(26)?;
        if read.len() != 26 || !read.starts_with(b"0x") || &read[20..] != OK {
            return Err(format!(
                "unexpected PHY62xx revision response: {}",
                String::from_utf8_lossy(&read)
            ));
        }
        let id = std::str::from_utf8(&read[2..10]).map_err(|error| error.to_string())?;
        self.flash_id = u32::from_str_radix(id, 16).map_err(|error| error.to_string())?;
        let size = self.flash_size();
        if size == 0 {
            return Err("PHY62xx ROM reported invalid flash size".into());
        }
        self.patch_flash = size << 1;
        Ok(String::from_utf8_lossy(&read[2..20]).trim().to_string())
    }

    pub fn read_reg(&mut self, addr: u32) -> Result<u32, String> {
        self.io
            .write_all(format!("rdreg{addr:08x} ").as_bytes())?;
        let read = self.io.read(17)?;
        if read.len() != 17 || !read.starts_with(b"=0x") || &read[11..] != OK {
            return Err(format!(
                "rdreg {addr:#010x} failed: {}",
                String::from_utf8_lossy(&read)
            ));
        }
        let value = std::str::from_utf8(&read[3..11]).map_err(|error| error.to_string())?;
        u32::from_str_radix(value, 16).map_err(|error| error.to_string())
    }

    pub fn write_reg(&mut self, addr: u32, value: u32) -> Result<(), String> {
        self.command(&format!("wrreg{addr:08x} {value:08x} "))
    }

    pub fn set_baud(&mut self, baud: u32) -> Result<(), String> {
        if self.io.baud()? == baud {
            return Ok(());
        }
        self.io.set_timeout(Duration::from_millis(700))?;
        self.io.write_all(format!("uarts{baud}").as_bytes())?;
        let ack = self.io.read(3)?;
        self.io.set_baud(baud)?;
        self.io.set_timeout(Duration::from_millis(200))?;
        if ack.as_slice() != b"#OK" {
            self.read_reg(0x1FFF_0000)?;
        }
        self.io.clear()?;
        Ok(())
    }

    pub fn init_spifs(&mut self) -> Result<(), String> {
        self.command("spifs 0 1 3 0 ")?;
        self.command("sfmod 2 2 ")?;
        self.command("cpnum ffffffff ")
    }

    pub fn expand_flash_window(&mut self) -> Result<(), String> {
        self.write_reg(0x1FFF_0898, self.patch_flash << 2)
    }

    pub fn erase_all(&mut self) -> Result<(), String> {
        self.flash_command(6, 0, 0, 0)?;
        self.flash_command(0x60, 0, 0, 0)?;
        for _ in 0..77 {
            if self.flash_status()? & 1 == 0 {
                self.erased_from = 0;
                self.erased_to = self.flash_size();
                return Ok(());
            }
        }
        Err("PHY62xx chip erase timed out".into())
    }

    pub fn reset(&mut self) -> Result<(), String> {
        self.io.write_all(b"reset ")
    }

    pub fn write_image(&mut self, image: &FlashImage) -> Result<usize, String> {
        let mut written = 0usize;
        for part in &image.parts {
            written += self.write_block(part.flash_off, &part.data)?;
        }
        Ok(written)
    }

    fn flash_unlock(&mut self) -> Result<(), String> {
        let manufacturer = self.flash_id & 0xff;
        self.flash_command(6, 0, 0, 0)?;
        if manufacturer == 0x85 {
            self.flash_command(0x50, 0, 0, 0)?;
            self.flash_command(1, 0, 2, 0)
        } else {
            self.flash_command(1, 0, 1, 0)
        }
    }

    fn flash_command(
        &mut self,
        command: u32,
        data: u32,
        write_len: u32,
        read_len: u32,
    ) -> Result<(), String> {
        let mut reg = command << 24;
        if write_len > 0 {
            reg |= 0x8000 | ((write_len - 1) << 12);
            self.write_reg(0x4000_C8A8, data)?;
        }
        if read_len > 0 {
            reg |= 0x80_0000 | ((read_len - 1) << 20);
        }
        self.write_reg(0x4000_C890, reg | 1)
    }

    fn flash_status(&mut self) -> Result<u32, String> {
        self.flash_command(5, 0, 0, 2)?;
        Ok(self.read_reg(0x4000_C8A0)? & 0xffff)
    }

    fn write_block(&mut self, mut offset: u32, data: &[u8]) -> Result<usize, String> {
        let mut erase_size = data.len() as u32;
        let tail = erase_size % CPBIN_BLK;
        if tail > 0x1000 {
            erase_size += CPBIN_BLK - tail;
        }
        self.erase_range(offset, erase_size)?;

        let mut rest = data;
        let mut written = 0usize;
        while !rest.is_empty() {
            let len = (rest.len() as u32).min(CPBIN_BLK) as usize;
            written += self.send_chunk(offset, &rest[..len])?;
            offset += len as u32;
            rest = &rest[len..];
            self.cpbin = self.cpbin.wrapping_add(1);
        }
        Ok(written)
    }

    fn erase_range(&mut self, mut offset: u32, size: u32) -> Result<(), String> {
        let mut count = (size + SECTOR - 1 + (offset & (SECTOR - 1))) / SECTOR;
        offset &= !(SECTOR - 1);
        while count > 0 {
            if offset >= self.erased_from && offset < self.erased_to {
                offset += SECTOR;
                count -= 1;
                continue;
            }
            if offset & 0xffff == 0 && count > 15 {
                self.io.set_timeout(Duration::from_millis(2_000))?;
                self.command(&format!("er64k {:X}", offset | self.patch_flash))?;
                self.io.set_timeout(Duration::from_millis(200))?;
                self.erased_from = offset;
                self.erased_to = offset + 0x1_0000;
                offset += 0x1_0000;
                count -= 16;
            } else {
                self.io.set_timeout(Duration::from_millis(500))?;
                self.command(&format!("era4k {:X}", offset | self.patch_flash))?;
                self.io.set_timeout(Duration::from_millis(200))?;
                self.erased_from = offset;
                self.erased_to = offset + SECTOR;
                offset += SECTOR;
                count -= 1;
            }
        }
        Ok(())
    }

    fn send_chunk(&mut self, offset: u32, data: &[u8]) -> Result<usize, String> {
        let payload = pad_cpbin_chunk(data);
        if chunk_is_erased(&payload) {
            return Ok(0);
        }
        self.io.set_timeout(Duration::from_millis(1_000))?;
        self.io.write_all(
            format!(
                "cpbin c{} {:X} {:X} {:X}",
                self.cpbin,
                offset | self.patch_flash,
                payload.len(),
                offset
            )
            .as_bytes(),
        )?;
        if self.io.read(12)?.as_slice() != b"by hex mode:" {
            return Err(format!("cpbin rejected at flash offset {offset:#x}"));
        }
        self.io.write_all(&payload)?;
        let checksum = self.io.read(23)?;
        if checksum.len() != 23 || &checksum[..15] != b"checksum is: 0x" {
            return Err(format!(
                "bad cpbin checksum challenge: {}",
                String::from_utf8_lossy(&checksum)
            ));
        }
        self.io.write_all(&checksum[15..23])?;
        if self.io.read(6)?.as_slice() != OK {
            return Err(format!("cpbin checksum rejected at {offset:#x}"));
        }
        self.io.set_timeout(Duration::from_millis(200))?;
        Ok(payload.len())
    }

    fn command(&mut self, command: &str) -> Result<(), String> {
        self.io.write_all(command.as_bytes())?;
        let ack = self.io.read(6)?;
        if ack.as_slice() == OK {
            Ok(())
        } else {
            Err(format!(
                "ROM command {command:?} failed: {}",
                String::from_utf8_lossy(&ack)
            ))
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SerialTransport {
    pub fn open(name: &str) -> Result<Self, String> {
        let port = serialport::new(name, START_BAUD)
            .timeout(Duration::from_millis(40))
            .flow_control(serialport::FlowControl::None)
            .dtr_on_open(true)
            .open()
            .map_err(|error| format!("open {name}: {error}"))?;
        Ok(Self { port })
    }

    pub fn set_rts(&mut self, value: bool) -> Result<(), String> {
        self.port
            .write_request_to_send(value)
            .map_err(|error| error.to_string())
    }

    pub fn set_dtr(&mut self, value: bool) -> Result<(), String> {
        self.port
            .write_data_terminal_ready(value)
            .map_err(|error| error.to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Transport for SerialTransport {
    fn baud(&self) -> Result<u32, String> {
        self.port.baud_rate().map_err(|error| error.to_string())
    }

    fn set_baud(&mut self, baud: u32) -> Result<(), String> {
        self.port
            .set_baud_rate(baud)
            .map_err(|error| error.to_string())
    }

    fn set_timeout(&mut self, timeout: Duration) -> Result<(), String> {
        self.port
            .set_timeout(timeout)
            .map_err(|error| error.to_string())
    }

    fn clear(&mut self) -> Result<(), String> {
        self.port
            .clear(serialport::ClearBuffer::All)
            .map_err(|error| error.to_string())
    }

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), String> {
        use std::io::Write;
        self.port.write_all(bytes).map_err(|error| error.to_string())?;
        self.port.flush().map_err(|error| error.to_string())
    }

    fn read(&mut self, len: usize) -> Result<Vec<u8>, String> {
        use std::io::{self, Read};
        let mut bytes = vec![0u8; len];
        let mut got = 0;
        while got < len {
            match self.port.read(&mut bytes[got..]) {
                Ok(0) => break,
                Ok(count) => got += count,
                Err(error) if error.kind() == io::ErrorKind::TimedOut => break,
                Err(error) => return Err(error.to_string()),
            }
        }
        bytes.truncate(got);
        Ok(bytes)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pb03fBoot {
    /// The operator/power harness owns the physical power-cycle. The flasher
    /// only starts sending the ROM synchronization word.
    ManualPowerCycle,
    /// Compatibility with adapters that route RTS/DTR to reset/test control.
    ControlLines,
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Pb03fKit {
    link: SerialTransport,
    boot: Pb03fBoot,
}

#[cfg(not(target_arch = "wasm32"))]
impl Pb03fKit {
    pub fn new(link: SerialTransport, boot: Pb03fBoot) -> Self {
        Self { link, boot }
    }

    pub fn link_mut(&mut self) -> &mut SerialTransport {
        &mut self.link
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl TargetAdapter for Pb03fKit {
    type Link = SerialTransport;

    fn enter_bootloader(&mut self) -> Result<(), String> {
        self.link.set_baud(START_BAUD)?;
        self.link.set_timeout(Duration::from_millis(40))?;
        if self.boot == Pb03fBoot::ControlLines {
            self.link.set_rts(true)?;
            self.link.set_dtr(true)?;
            std::thread::sleep(Duration::from_millis(100));
            self.link.clear()?;
            std::thread::sleep(Duration::from_millis(100));
            self.link.set_dtr(false)?;
            self.link.set_rts(false)?;
        }
        Ok(())
    }

    fn link(&mut self) -> &mut Self::Link {
        &mut self.link
    }
}

#[derive(Debug)]
enum HarnessState {
    Application,
    AwaitSync { matched: usize },
    Command,
    CpbinData {
        offset: usize,
        len: usize,
        data: Vec<u8>,
    },
    CpbinChecksum {
        offset: usize,
        data: Vec<u8>,
        expected: [u8; 8],
    },
}

pub struct HarnessTransport {
    baud: u32,
    rom_baud: u32,
    timeout: Duration,
    state: HarnessState,
    rx: VecDeque<u8>,
    flash: Vec<u8>,
    regs: HashMap<u32, u32>,
    flash_id: u32,
    reset_count: u32,
}

impl HarnessTransport {
    pub fn new(flash_size: usize) -> Result<Self, String> {
        if !flash_size.is_power_of_two() || !(0x1_0000..=0x10_00000).contains(&flash_size) {
            return Err("harness flash size must be a power of two between 64 KiB and 16 MiB".into());
        }
        let density = flash_size.trailing_zeros();
        let flash_id = (density << 16) | 0x0085;
        Ok(Self {
            baud: START_BAUD,
            rom_baud: START_BAUD,
            timeout: Duration::from_millis(40),
            state: HarnessState::Application,
            rx: VecDeque::new(),
            flash: vec![0xff; flash_size],
            regs: HashMap::new(),
            flash_id,
            reset_count: 0,
        })
    }

    pub fn enter_bootloader(&mut self) {
        self.state = HarnessState::AwaitSync { matched: 0 };
        self.rom_baud = START_BAUD;
        self.rx.clear();
    }

    pub fn flash(&self) -> &[u8] {
        &self.flash
    }

    pub fn reset_count(&self) -> u32 {
        self.reset_count
    }

    fn queue(&mut self, bytes: &[u8]) {
        self.rx.extend(bytes.iter().copied());
    }

    fn command(&mut self, bytes: &[u8]) -> Result<(), String> {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        if text.starts_with("rdrev+") {
            self.queue(format!("0x{:08X}PHY6252ROM{}", self.flash_id, "#OK>>:").as_bytes());
            return Ok(());
        }
        if let Some(rest) = text.strip_prefix("rdreg") {
            let addr = parse_hex_word(rest.trim())?;
            let value = *self.regs.get(&addr).unwrap_or(&0);
            self.queue(format!("=0x{value:08X}#OK>>:").as_bytes());
            return Ok(());
        }
        if let Some(rest) = text.strip_prefix("wrreg") {
            let mut words = rest.split_whitespace();
            let addr = parse_hex_word(words.next().ok_or("wrreg address")?)?;
            let value = parse_hex_word(words.next().ok_or("wrreg value")?)?;
            self.regs.insert(addr, value);
            if addr == 0x4000_C890 && value >> 24 == 0x60 {
                self.flash.fill(0xff);
            }
            self.queue(OK);
            return Ok(());
        }
        if let Some(rest) = text.strip_prefix("uarts") {
            let baud = rest.trim().parse::<u32>().map_err(|error| error.to_string())?;
            self.queue(b"#OK");
            self.rom_baud = baud;
            return Ok(());
        }
        if text.starts_with("spifs ") || text.starts_with("sfmod ") || text.starts_with("cpnum ") {
            self.queue(OK);
            return Ok(());
        }
        if let Some(rest) = text.strip_prefix("era4k ") {
            self.erase_encoded(rest, 0x1000)?;
            self.queue(OK);
            return Ok(());
        }
        if let Some(rest) = text.strip_prefix("er64k ") {
            self.erase_encoded(rest, 0x1_0000)?;
            self.queue(OK);
            return Ok(());
        }
        if let Some(rest) = text.strip_prefix("cpbin ") {
            let mut words = rest.split_whitespace();
            let _block = words.next().ok_or("cpbin block")?;
            let _encoded = parse_hex_word(words.next().ok_or("cpbin encoded address")?)?;
            let len = parse_hex_word(words.next().ok_or("cpbin length")?)? as usize;
            let offset = parse_hex_word(words.next().ok_or("cpbin offset")?)? as usize;
            if offset.checked_add(len).is_none_or(|end| end > self.flash.len()) {
                return Err("cpbin range exceeds harness flash".into());
            }
            self.queue(b"by hex mode:");
            self.state = HarnessState::CpbinData {
                offset,
                len,
                data: Vec::with_capacity(len),
            };
            return Ok(());
        }
        if text.starts_with("reset") {
            self.state = HarnessState::Application;
            self.reset_count = self.reset_count.wrapping_add(1);
            return Ok(());
        }
        Err(format!("harness ROM does not implement command {text:?}"))
    }

    fn erase_encoded(&mut self, text: &str, size: usize) -> Result<(), String> {
        let encoded = parse_hex_word(text.trim())? as usize;
        let offset = encoded & (self.flash.len() - 1);
        let start = offset & !(size - 1);
        let end = start.saturating_add(size).min(self.flash.len());
        self.flash[start..end].fill(0xff);
        Ok(())
    }

    fn accept_cpbin_data(&mut self, bytes: &[u8]) -> Result<(), String> {
        let HarnessState::CpbinData {
            offset,
            len,
            mut data,
        } = std::mem::replace(&mut self.state, HarnessState::Command)
        else {
            unreachable!();
        };
        data.extend_from_slice(bytes);
        if data.len() > len {
            return Err("cpbin payload is longer than declared".into());
        }
        if data.len() < len {
            self.state = HarnessState::CpbinData { offset, len, data };
            return Ok(());
        }
        let checksum = data
            .iter()
            .fold(0u32, |sum, byte| sum.wrapping_add(u32::from(*byte)));
        let text = format!("{checksum:08X}");
        let expected: [u8; 8] = text.as_bytes().try_into().expect("eight hex digits");
        self.queue(format!("checksum is: 0x{text}").as_bytes());
        self.state = HarnessState::CpbinChecksum {
            offset,
            data,
            expected,
        };
        Ok(())
    }

    fn accept_cpbin_checksum(&mut self, bytes: &[u8]) -> Result<(), String> {
        let HarnessState::CpbinChecksum {
            offset,
            data,
            expected,
        } = std::mem::replace(&mut self.state, HarnessState::Command)
        else {
            unreachable!();
        };
        if bytes != expected {
            return Err("cpbin checksum echo mismatch".into());
        }
        for (dst, src) in self.flash[offset..offset + data.len()].iter_mut().zip(data) {
            *dst &= src;
        }
        self.queue(OK);
        Ok(())
    }
}

impl Transport for HarnessTransport {
    fn baud(&self) -> Result<u32, String> {
        Ok(self.baud)
    }

    fn set_baud(&mut self, baud: u32) -> Result<(), String> {
        self.baud = baud;
        Ok(())
    }

    fn set_timeout(&mut self, timeout: Duration) -> Result<(), String> {
        self.timeout = timeout;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), String> {
        self.rx.clear();
        Ok(())
    }

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.baud != self.rom_baud {
            return Ok(());
        }
        match &mut self.state {
            HarnessState::Application => Ok(()),
            HarnessState::AwaitSync { matched } => {
                const MAGIC: &[u8] = b"UXTDWU";
                let mut entered = false;
                for byte in bytes {
                    if *byte == MAGIC[*matched] {
                        *matched += 1;
                    } else {
                        *matched = usize::from(*byte == MAGIC[0]);
                    }
                    if *matched == MAGIC.len() {
                        entered = true;
                        break;
                    }
                }
                if entered {
                    self.queue(b"cmd>>:");
                    self.state = HarnessState::Command;
                    self.rom_baud = DEFAULT_BAUD;
                }
                Ok(())
            }
            HarnessState::Command => self.command(bytes),
            HarnessState::CpbinData { .. } => self.accept_cpbin_data(bytes),
            HarnessState::CpbinChecksum { .. } => self.accept_cpbin_checksum(bytes),
        }
    }

    fn read(&mut self, len: usize) -> Result<Vec<u8>, String> {
        let count = len.min(self.rx.len());
        Ok(self.rx.drain(..count).collect())
    }
}

pub struct HarnessTarget {
    link: HarnessTransport,
}

impl HarnessTarget {
    pub fn new(flash_size: usize) -> Result<Self, String> {
        Ok(Self {
            link: HarnessTransport::new(flash_size)?,
        })
    }

    pub fn flash(&self) -> &[u8] {
        self.link.flash()
    }

    pub fn reset_count(&self) -> u32 {
        self.link.reset_count()
    }
}

impl TargetAdapter for HarnessTarget {
    type Link = HarnessTransport;

    fn enter_bootloader(&mut self) -> Result<(), String> {
        self.link.enter_bootloader();
        Ok(())
    }

    fn link(&mut self) -> &mut Self::Link {
        &mut self.link
    }
}

fn parse_hex_word(text: &str) -> Result<u32, String> {
    u32::from_str_radix(text.trim_start_matches("0x"), 16).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::programmer::{build_flash_image, Segment};

    fn synthetic_firmware() -> FlashImage {
        let mut bytes = vec![0xff; 0x108];
        bytes[0..4].copy_from_slice(&0x1FFF_8000u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x1FFF_0101u32.to_le_bytes());
        // MOVS r0,#1 ; NOP ; B .
        bytes[0x100..0x106].copy_from_slice(&[0x01, 0x20, 0x00, 0xBF, 0xFE, 0xE7]);
        build_flash_image(
            &[Segment {
                load_addr: 0x1FFF_0000,
                data: bytes,
            }],
            Some(0x1FFF_0101),
        )
        .unwrap()
    }

    #[test]
    fn same_flasher_programs_synthetic_firmware_through_harness() {
        let image = synthetic_firmware();
        let mut target = HarnessTarget::new(256 * 1024).unwrap();
        let report = Flasher::new(FlashOptions::default())
            .flash(&mut target, &image)
            .unwrap();

        assert_eq!(report.flash_size, 256 * 1024);
        assert!(report.bytes_written >= 0x108);
        assert_eq!(target.reset_count(), 1);
        for part in &image.parts {
            let start = part.flash_off as usize;
            assert_eq!(&target.flash()[start..start + part.data.len()], &part.data);
        }
        assert_eq!(
            &target.flash()[0x2000 + 8..0x2000 + 12],
            &0x1FFF_0101u32.to_le_bytes()
        );
    }

    #[test]
    fn harness_enforces_boot_baud_and_rom_baud_switch() {
        let mut link = HarnessTransport::new(256 * 1024).unwrap();
        link.enter_bootloader();
        link.set_baud(DEFAULT_BAUD).unwrap();
        link.write_all(b"UXTDWU").unwrap();
        assert!(link.read(6).unwrap().is_empty());
        link.set_baud(START_BAUD).unwrap();
        link.write_all(b"UXTDWU").unwrap();
        assert_eq!(link.read(6).unwrap(), b"cmd>>:");
        link.set_baud(DEFAULT_BAUD).unwrap();
        link.write_all(b"rdrev+ ").unwrap();
        assert_eq!(link.read(26).unwrap().len(), 26);
    }
}
