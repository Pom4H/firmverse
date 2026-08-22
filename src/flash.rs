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
const BOOT_INFO_OFFSET: usize = 0x2000;
const OK: &[u8] = b"#OK>>:";

pub trait Transport {
    fn baud(&self) -> Result<u32, String>;
    fn set_baud(&mut self, baud: u32) -> Result<(), String>;
    fn set_timeout(&mut self, timeout: Duration) -> Result<(), String>;
    fn delay(&mut self, duration: Duration) -> Result<(), String>;
    fn clear(&mut self) -> Result<(), String>;
    fn set_break(&mut self, enabled: bool) -> Result<(), String>;
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), String>;
    fn read(&mut self, len: usize) -> Result<Vec<u8>, String>;
}

pub trait TargetAdapter {
    type Link: Transport;

    fn enter_bootloader(&mut self) -> Result<BootloaderState, String>;
    fn link(&mut self) -> &mut Self::Link;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootloaderState {
    /// The target is waiting for the ROM `UXTDWU` synchronization word at 9600 baud.
    AwaitSync9600,
    /// Synchronization already completed and the ROM command monitor is at 115200 baud.
    CommandMonitor115200,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationHandoff {
    pub app_baud: u32,
    pub break_duration: Duration,
    pub wake_delay: Duration,
    pub token: Vec<u8>,
    pub token_repetitions: usize,
    pub attempts: usize,
}

impl ApplicationHandoff {
    pub fn new(token: Vec<u8>) -> Result<Self, String> {
        if token.is_empty() {
            return Err("application handoff token cannot be empty".into());
        }
        Ok(Self {
            app_baud: DEFAULT_BAUD,
            break_duration: Duration::from_millis(60),
            wake_delay: Duration::from_millis(150),
            token,
            token_repetitions: 32,
            attempts: 4,
        })
    }

    fn validate(&self) -> Result<(), String> {
        if self.token.is_empty() {
            return Err("application handoff token cannot be empty".into());
        }
        if self.app_baud == 0 || self.token_repetitions == 0 || self.attempts == 0 {
            return Err(
                "application handoff baud, repetitions and attempts must be non-zero".into(),
            );
        }
        Ok(())
    }
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
        let state = target.enter_bootloader()?;
        let mut rom = Phy62xxRom::new(target.link(), self.options.baud);
        let revision = match state {
            BootloaderState::AwaitSync9600 => rom.connect()?,
            BootloaderState::CommandMonitor115200 => rom.attach()?,
        };
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
                return self.attach();
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

    pub fn attach(&mut self) -> Result<String, String> {
        self.io.set_baud(DEFAULT_BAUD)?;
        self.io.set_timeout(Duration::from_millis(200))?;
        let revision = self.read_revision()?;
        self.flash_unlock()?;
        self.set_baud(self.run_baud)?;
        Ok(revision)
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
        self.io.write_all(format!("rdreg{addr:08x} ").as_bytes())?;
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
        self.io.delay(Duration::from_millis(50))?;
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
            self.io.delay(Duration::from_millis(100))?;
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

/// Ask a running application to invalidate its boot header and reset into the
/// PHY62xx ROM. The same routine also recovers when the board is already in
/// either the 9600-baud synchronization state or the 115200-baud command monitor.
pub fn enter_via_application<T: Transport>(
    io: &mut T,
    handoff: &ApplicationHandoff,
) -> Result<BootloaderState, String> {
    handoff.validate()?;

    // A previous interrupted run may already have synchronized the ROM. Probe
    // before sending an application token that the command monitor cannot use.
    io.set_baud(DEFAULT_BAUD)?;
    io.set_timeout(Duration::from_millis(200))?;
    io.clear()?;
    io.write_all(b"rdrev+ ")?;
    let revision = io.read(26)?;
    if revision.len() == 26 && revision.starts_with(b"0x") && &revision[20..] == OK {
        return Ok(BootloaderState::CommandMonitor115200);
    }

    let mut last = Vec::new();
    for _ in 0..handoff.attempts {
        io.set_baud(handoff.app_baud)?;
        io.set_timeout(Duration::from_millis(50))?;
        io.clear()?;
        io.set_break(true)?;
        io.delay(handoff.break_duration)?;
        io.set_break(false)?;
        io.delay(handoff.wake_delay)?;
        for _ in 0..handoff.token_repetitions {
            io.write_all(&handoff.token)?;
        }

        io.set_baud(START_BAUD)?;
        io.set_timeout(Duration::from_millis(40))?;
        io.clear()?;
        for _ in 0..12 {
            io.write_all(b"UXTDWU")?;
            let read = io.read(6)?;
            if read.as_slice() == b"cmd>>:" {
                io.set_baud(DEFAULT_BAUD)?;
                io.set_timeout(Duration::from_millis(200))?;
                return Ok(BootloaderState::CommandMonitor115200);
            }
            if read.as_slice() == b"fct>>:" {
                return Err("chip is in FCT mode".into());
            }
            last = read;
            io.delay(Duration::from_millis(5))?;
        }
    }

    Err(format!(
        "application handoff did not reach the PHY62xx ROM: {}",
        String::from_utf8_lossy(&last)
    ))
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

    fn delay(&mut self, duration: Duration) -> Result<(), String> {
        std::thread::sleep(duration);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), String> {
        self.port
            .clear(serialport::ClearBuffer::All)
            .map_err(|error| error.to_string())
    }

    fn set_break(&mut self, enabled: bool) -> Result<(), String> {
        if enabled {
            self.port.set_break().map_err(|error| error.to_string())
        } else {
            self.port.clear_break().map_err(|error| error.to_string())
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), String> {
        use std::io::Write;
        self.port
            .write_all(bytes)
            .map_err(|error| error.to_string())?;
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pb03fBoot {
    /// The operator/power harness owns the physical power-cycle. The flasher
    /// only starts sending the ROM synchronization word.
    ManualPowerCycle,
    /// Compatibility with adapters that route RTS/DTR to reset/test control.
    ControlLines,
    /// Cooperate with a running application that can invalidate boot-info and
    /// reset into the ROM after receiving an authenticated UART token.
    ApplicationHandoff(ApplicationHandoff),
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

    fn enter_bootloader(&mut self) -> Result<BootloaderState, String> {
        match &self.boot {
            Pb03fBoot::ManualPowerCycle => {
                self.link.set_baud(START_BAUD)?;
                self.link.set_timeout(Duration::from_millis(40))?;
                Ok(BootloaderState::AwaitSync9600)
            }
            Pb03fBoot::ControlLines => {
                self.link.set_baud(START_BAUD)?;
                self.link.set_timeout(Duration::from_millis(40))?;
                self.link.set_rts(true)?;
                self.link.set_dtr(true)?;
                std::thread::sleep(Duration::from_millis(100));
                self.link.clear()?;
                std::thread::sleep(Duration::from_millis(100));
                self.link.set_dtr(false)?;
                self.link.set_rts(false)?;
                Ok(BootloaderState::AwaitSync9600)
            }
            Pb03fBoot::ApplicationHandoff(handoff) => {
                // Do not let USB-UART modem-control defaults accidentally hold
                // boards whose RTS/DTR pins are connected to test/reset lines.
                self.link.set_dtr(false)?;
                self.link.set_rts(false)?;
                enter_via_application(&mut self.link, handoff)
            }
        }
    }

    fn link(&mut self) -> &mut Self::Link {
        &mut self.link
    }
}

#[derive(Debug)]
enum HarnessState {
    Application,
    AwaitSync {
        matched: usize,
    },
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
    application_handoff: Option<ApplicationHandoff>,
    application_matched: usize,
    break_asserted: bool,
    break_seen: bool,
    handoff_count: u32,
}

impl HarnessTransport {
    pub fn new(flash_size: usize) -> Result<Self, String> {
        if !flash_size.is_power_of_two() || !(0x1_0000..=0x10_00000).contains(&flash_size) {
            return Err(
                "harness flash size must be a power of two between 64 KiB and 16 MiB".into(),
            );
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
            application_handoff: None,
            application_matched: 0,
            break_asserted: false,
            break_seen: false,
            handoff_count: 0,
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

    pub fn handoff_count(&self) -> u32 {
        self.handoff_count
    }

    pub fn boot_info_valid(&self) -> bool {
        let count = u32::from_le_bytes(
            self.flash[BOOT_INFO_OFFSET..BOOT_INFO_OFFSET + 4]
                .try_into()
                .expect("four boot-info bytes"),
        );
        count != 0 && count != u32::MAX
    }

    pub fn install_image(&mut self, image: &FlashImage) -> Result<(), String> {
        self.flash.fill(0xff);
        for part in &image.parts {
            let start = part.flash_off as usize;
            let end = start
                .checked_add(part.data.len())
                .ok_or("harness image range overflow")?;
            let destination = self
                .flash
                .get_mut(start..end)
                .ok_or("harness image exceeds virtual NOR")?;
            destination.copy_from_slice(&part.data);
        }
        self.state = HarnessState::Application;
        Ok(())
    }

    fn configure_application_handoff(&mut self, handoff: ApplicationHandoff) {
        self.application_handoff = Some(handoff);
    }

    fn reset_from_boot_info(&mut self) {
        if self.boot_info_valid() {
            self.state = HarnessState::Application;
        } else {
            self.state = HarnessState::AwaitSync { matched: 0 };
            self.rom_baud = START_BAUD;
        }
        self.rx.clear();
    }

    fn accept_application(&mut self, bytes: &[u8]) -> Result<(), String> {
        let Some(handoff) = &self.application_handoff else {
            return Ok(());
        };
        if self.baud != handoff.app_baud || !self.break_seen {
            return Ok(());
        }
        let token = handoff.token.clone();
        for byte in bytes {
            if *byte == token[self.application_matched] {
                self.application_matched += 1;
            } else {
                self.application_matched = usize::from(*byte == token[0]);
            }
            if self.application_matched == token.len() {
                for value in &mut self.flash[BOOT_INFO_OFFSET..BOOT_INFO_OFFSET + 4] {
                    *value &= 0;
                }
                self.application_matched = 0;
                self.break_seen = false;
                self.handoff_count = self.handoff_count.wrapping_add(1);
                self.reset_from_boot_info();
                break;
            }
        }
        Ok(())
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
            let baud = rest
                .trim()
                .parse::<u32>()
                .map_err(|error| error.to_string())?;
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
            if offset
                .checked_add(len)
                .is_none_or(|end| end > self.flash.len())
            {
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
            self.reset_count = self.reset_count.wrapping_add(1);
            self.reset_from_boot_info();
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

    fn delay(&mut self, _duration: Duration) -> Result<(), String> {
        Ok(())
    }

    fn clear(&mut self) -> Result<(), String> {
        self.rx.clear();
        Ok(())
    }

    fn set_break(&mut self, enabled: bool) -> Result<(), String> {
        if self.break_asserted && !enabled {
            self.break_seen = true;
        }
        self.break_asserted = enabled;
        Ok(())
    }

    fn write_all(&mut self, bytes: &[u8]) -> Result<(), String> {
        if matches!(self.state, HarnessState::Application) {
            return self.accept_application(bytes);
        }
        if self.baud != self.rom_baud {
            return Ok(());
        }
        match &mut self.state {
            HarnessState::Application => unreachable!(),
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
    boot: HarnessBoot,
}

enum HarnessBoot {
    Direct,
    ApplicationHandoff(ApplicationHandoff),
}

impl HarnessTarget {
    pub fn new(flash_size: usize) -> Result<Self, String> {
        Ok(Self {
            link: HarnessTransport::new(flash_size)?,
            boot: HarnessBoot::Direct,
        })
    }

    pub fn new_application(flash_size: usize, handoff: ApplicationHandoff) -> Result<Self, String> {
        handoff.validate()?;
        let mut link = HarnessTransport::new(flash_size)?;
        link.configure_application_handoff(handoff.clone());
        Ok(Self {
            link,
            boot: HarnessBoot::ApplicationHandoff(handoff),
        })
    }

    pub fn install_image(&mut self, image: &FlashImage) -> Result<(), String> {
        self.link.install_image(image)
    }

    pub fn flash(&self) -> &[u8] {
        self.link.flash()
    }

    pub fn reset_count(&self) -> u32 {
        self.link.reset_count()
    }

    pub fn handoff_count(&self) -> u32 {
        self.link.handoff_count()
    }

    pub fn boot_info_valid(&self) -> bool {
        self.link.boot_info_valid()
    }
}

impl TargetAdapter for HarnessTarget {
    type Link = HarnessTransport;

    fn enter_bootloader(&mut self) -> Result<BootloaderState, String> {
        match &self.boot {
            HarnessBoot::Direct => {
                self.link.enter_bootloader();
                Ok(BootloaderState::AwaitSync9600)
            }
            HarnessBoot::ApplicationHandoff(handoff) => {
                enter_via_application(&mut self.link, handoff)
            }
        }
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
    fn application_handoff_invalidates_boot_info_and_reflashes_without_manual_reset() {
        let image = synthetic_firmware();
        let token = vec![
            0x00, 0xd5, b'D', b'P', b'L', b'S', b'-', b'R', b'O', b'M', 0xa5,
        ];
        let handoff = ApplicationHandoff::new(token).unwrap();
        let mut target = HarnessTarget::new_application(256 * 1024, handoff).unwrap();
        target.install_image(&image).unwrap();
        assert!(target.boot_info_valid());

        let report = Flasher::new(FlashOptions::default())
            .flash(&mut target, &image)
            .unwrap();

        assert_eq!(report.flash_size, 256 * 1024);
        assert_eq!(target.handoff_count(), 1);
        assert_eq!(target.reset_count(), 1);
        assert!(target.boot_info_valid());
        for part in &image.parts {
            let start = part.flash_off as usize;
            assert_eq!(&target.flash()[start..start + part.data.len()], &part.data);
        }
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

    #[test]
    fn application_handoff_recovers_an_already_open_command_monitor() {
        let handoff = ApplicationHandoff::new(vec![0xa5, 0x5a]).unwrap();
        let mut link = HarnessTransport::new(256 * 1024).unwrap();
        link.enter_bootloader();
        link.set_baud(START_BAUD).unwrap();
        link.write_all(b"UXTDWU").unwrap();
        assert_eq!(link.read(6).unwrap(), b"cmd>>:");

        assert_eq!(
            enter_via_application(&mut link, &handoff).unwrap(),
            BootloaderState::CommandMonitor115200
        );
    }
}
