//! PHY62x2 UART bootloader: write an Intel HEX to a PB-03F-Kit.

#[path = "../programmer.rs"]
mod programmer;

use clap::Parser;
use programmer::{
    build_flash_image, chunk_is_erased, pad_cpbin_chunk, parse_intel_hex, FlashImage,
};
use serialport::{ClearBuffer, SerialPort};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const START_BAUD: u32 = 9600;
const RUN_BAUD: u32 = 115_200;
const SECTOR: u32 = 0x1000;
const CPBIN_BLK: u32 = 0x2000;

#[derive(Parser)]
#[command(
    name = "phy6252-flash",
    version,
    about = "Flash an Intel HEX to PHY6252 / PB-03F-Kit over USB-UART"
)]
struct Cli {
    /// Intel HEX image
    hex: Option<PathBuf>,
    /// Serial port (auto-detects CH340 / wchusbserial)
    #[arg(short, long)]
    port: Option<String>,
    /// Chip-erase before write (also wipes NVRAM / bonds)
    #[arg(long)]
    erase: bool,
    /// Do not send `reset` after a successful write
    #[arg(long)]
    no_reset: bool,
    /// Application start address (default: lowest SRAM segment)
    #[arg(long, value_parser = clap_u32)]
    start: Option<u32>,
    /// List serial ports and exit
    #[arg(long)]
    list_ports: bool,
}

fn clap_u32(s: &str) -> Result<u32, String> {
    parse_u32(s).ok_or_else(|| format!("not an integer: {s}"))
}

fn parse_u32(s: &str) -> Option<u32> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        t.parse().ok()
    }
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    if cli.list_ports {
        return list_ports();
    }
    let hex_path = cli.hex.ok_or("pass a .hex image, or use --list-ports")?;
    let port_name = match cli.port {
        Some(p) => p,
        None => auto_port()?,
    };
    let image = load_image(&hex_path, cli.start)?;

    println!("port    {port_name}");
    println!("image   {}", hex_path.display());
    println!("start   {:#010x}", image.start);
    for part in &image.parts {
        if part.load_addr == 0 {
            println!(
                "header  flash {:#010x}  {} bytes",
                part.flash_off,
                part.data.len()
            );
        } else {
            println!(
                "segment {:#010x} <- flash {:#010x}  {} bytes",
                part.load_addr,
                part.flash_off,
                part.data.len()
            );
        }
    }
    println!();
    println!("Hold KEY1 (RST/PROG) on the kit.");
    println!("Release KEY1 when this tool prints 'bootloader'.");
    println!();

    let mut phy = PhyPort::open(&port_name)?;
    phy.connect(RUN_BAUD)?;
    if cli.erase {
        phy.erase_all()?;
    }
    phy.init_spifs()?;
    phy.expand_flash_window()?;
    phy.write_image(&image)?;
    if !cli.no_reset {
        phy.reset()?;
        println!("reset sent — the board should boot the new image");
        phy.listen_app(Duration::from_secs(20))?;
    }
    println!("flash ok");
    Ok(())
}

fn load_image(path: &Path, start: Option<u32>) -> Result<FlashImage, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let hex = parse_intel_hex(&text)?;
    build_flash_image(&hex.segments, start.or(hex.entry))
}

fn list_ports() -> Result<(), String> {
    let ports = serialport::available_ports().map_err(|e| e.to_string())?;
    if ports.is_empty() {
        println!("no serial ports");
        return Ok(());
    }
    for p in ports {
        println!("{}", p.port_name);
    }
    Ok(())
}

fn auto_port() -> Result<String, String> {
    if let Ok(port) = std::env::var("PHY6252_PORT").or_else(|_| std::env::var("PORT")) {
        if !port.is_empty() {
            return Ok(port);
        }
    }
    let ports = serialport::available_ports().map_err(|e| e.to_string())?;
    let names: Vec<String> = ports.into_iter().map(|p| p.port_name).collect();
    pick_port(&names).ok_or_else(|| {
        "no USB-UART adapter found (CH340 / wchusbserial / ttyUSB). Pass --port".into()
    })
}

fn pick_port(names: &[String]) -> Option<String> {
    const PREFER: [&str; 5] = ["wchusbserial", "usbserial", "ttyUSB", "ttyACM", "COM"];
    for needle in PREFER {
        if let Some(name) = names
            .iter()
            .find(|n| n.contains(needle) && !n.to_ascii_lowercase().contains("bluetooth"))
        {
            return Some(name.clone());
        }
    }
    None
}

struct PhyPort {
    port: Box<dyn SerialPort>,
    flash_id: u32,
    patch_flash: u32,
    cpbin: u32,
    erased_from: u32,
    erased_to: u32,
}

impl PhyPort {
    fn open(name: &str) -> Result<Self, String> {
        let port = serialport::new(name, START_BAUD)
            .timeout(Duration::from_millis(40))
            .flow_control(serialport::FlowControl::None)
            .dtr_on_open(true)
            .open()
            .map_err(|e| format!("open {name}: {e}"))?;
        Ok(Self {
            port,
            flash_id: 0,
            patch_flash: 0,
            cpbin: 0,
            erased_from: 0x40_0000,
            erased_to: 0x40_0000,
        })
    }

    fn connect(&mut self, baud: u32) -> Result<(), String> {
        self.port
            .write_request_to_send(true)
            .map_err(|e| e.to_string())?;
        self.port
            .write_data_terminal_ready(true)
            .map_err(|e| e.to_string())?;
        thread::sleep(Duration::from_millis(100));
        let _ = self.port.clear(ClearBuffer::All);
        thread::sleep(Duration::from_millis(100));
        println!("waiting for bootloader — release KEY1 now");
        self.port
            .write_request_to_send(false)
            .map_err(|e| e.to_string())?;
        self.port
            .write_data_terminal_ready(false)
            .map_err(|e| e.to_string())?;
        self.port
            .set_timeout(Duration::from_millis(40))
            .map_err(|e| e.to_string())?;

        let mut last = Vec::new();
        let mut found = false;
        for _ in 0..4000 {
            self.write_bytes(b"UXTDWU")?;
            let read = self.read_bytes(6)?;
            if read.as_slice() == b"cmd>>:" {
                println!("bootloader  cmd>>:");
                found = true;
                break;
            }
            if read.as_slice() == b"fct>>:" {
                return Err(
                    "chip is in FCT mode — power-cycle and retry, or pass --erase after a clean reset"
                        .into(),
                );
            }
            last = read;
        }
        if !found {
            return Err(format!(
                "no bootloader response ({})",
                String::from_utf8_lossy(&last)
            ));
        }
        self.port
            .set_baud_rate(RUN_BAUD)
            .map_err(|e| e.to_string())?;
        self.port
            .set_timeout(Duration::from_millis(200))
            .map_err(|e| e.to_string())?;
        self.read_revision()?;
        self.flash_unlock()?;
        println!("chip connected");
        self.set_baud(baud)
    }

    fn read_revision(&mut self) -> Result<(), String> {
        self.write_bytes(b"rdrev+ ")?;
        self.port
            .set_timeout(Duration::from_millis(100))
            .map_err(|e| e.to_string())?;
        let read = self.read_bytes(26)?;
        if read.len() == 26 && read.starts_with(b"0x") && read[20..26] == *b"#OK>>:" {
            let id_txt = std::str::from_utf8(&read[2..10]).map_err(|e| e.to_string())?;
            self.flash_id = u32::from_str_radix(id_txt, 16).map_err(|e| e.to_string())?;
            let size = 1u32 << ((self.flash_id >> 16) & 0xff);
            self.patch_flash = size << 1;
            println!(
                "revision {}  flash {:#08x}  {} KiB",
                std::str::from_utf8(&read[2..19]).unwrap_or("?"),
                self.flash_id & 0xFF_FFFF,
                size >> 10
            );
            return Ok(());
        }
        if read.len() >= 16 && read.starts_with(b"0x") {
            return Err("unexpected TG7100/short revision response".into());
        }
        Err(format!(
            "revision failed ({})",
            String::from_utf8_lossy(&read)
        ))
    }

    fn flash_unlock(&mut self) -> Result<(), String> {
        let man = self.flash_id & 0xFF;
        self.wr_flash_cmd(6, 0, 0, 0)?;
        if man == 0x85 {
            self.wr_flash_cmd(0x50, 0, 0, 0)?;
            self.wr_flash_cmd(1, 0, 2, 0)
        } else {
            self.wr_flash_cmd(1, 0, 1, 0)
        }
    }

    fn wr_flash_cmd(&mut self, cmd: u32, data: u32, wrlen: u32, rdlen: u32) -> Result<(), String> {
        let mut regcmd = cmd << 24;
        if wrlen > 0 {
            regcmd |= 0x8000 | ((wrlen - 1) << 12);
            self.write_reg(0x4000_C8A8, data)?;
        }
        if rdlen > 0 {
            regcmd |= 0x80_0000 | ((rdlen - 1) << 20);
        }
        self.write_reg(0x4000_C890, regcmd | 1)
    }

    fn set_baud(&mut self, baud: u32) -> Result<(), String> {
        if self.port.baud_rate().map_err(|e| e.to_string())? == baud {
            return Ok(());
        }
        self.port
            .set_timeout(Duration::from_millis(700))
            .map_err(|e| e.to_string())?;
        let cmd = format!("uarts{baud}");
        self.write_bytes(cmd.as_bytes())?;
        let ack = self.read_bytes(3)?;
        self.port.set_baud_rate(baud).map_err(|e| e.to_string())?;
        if ack.as_slice() != b"#OK" && self.read_reg(0x1FFF_0000).is_err() {
            return Err(format!("failed to switch UART to {baud}"));
        }
        println!("uart {baud}");
        self.port
            .set_timeout(Duration::from_millis(200))
            .map_err(|e| e.to_string())?;
        thread::sleep(Duration::from_millis(50));
        let _ = self.port.clear(ClearBuffer::All);
        Ok(())
    }

    fn init_spifs(&mut self) -> Result<(), String> {
        self.cmd("spifs 0 1 3 0 ")?;
        self.cmd("sfmod 2 2 ")?;
        self.cmd("cpnum ffffffff ")
    }

    fn expand_flash_window(&mut self) -> Result<(), String> {
        let size = self.patch_flash << 2;
        self.write_reg(0x1FFF_0898, size)?;
        println!("flash window {} KiB", size >> 10);
        Ok(())
    }

    fn erase_all(&mut self) -> Result<(), String> {
        println!("chip erase...");
        self.wr_flash_cmd(6, 0, 0, 0)?;
        self.wr_flash_cmd(0x60, 0, 0, 0)?;
        for _ in 0..77 {
            if self.flash_status()? & 1 == 0 {
                println!("chip erase ok");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err("chip erase timeout".into())
    }

    fn flash_status(&mut self) -> Result<u32, String> {
        self.wr_flash_cmd(5, 0, 0, 2)?;
        Ok(self.read_reg(0x4000_C8A0)? & 0xFFFF)
    }

    fn reset(&mut self) -> Result<(), String> {
        self.write_bytes(b"reset ")?;
        Ok(())
    }

    fn listen_at(&mut self, got: &mut Vec<u8>, for_how_long: Duration) -> Result<(), String> {
        let mut buf = [0u8; 256];
        let start = Instant::now();
        while start.elapsed() < for_how_long {
            match self.port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    got.extend_from_slice(&buf[..n]);
                    print!("{}", String::from_utf8_lossy(&buf[..n]));
                    let _ = io::stdout().flush();
                }
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::TimedOut => {}
                Err(err) => return Err(err.to_string()),
            }
        }
        Ok(())
    }

    fn listen_app(&mut self, for_how_long: Duration) -> Result<(), String> {
        self.port
            .write_data_terminal_ready(false)
            .map_err(|e| e.to_string())?;
        self.port
            .write_request_to_send(false)
            .map_err(|e| e.to_string())?;
        self.port
            .set_timeout(Duration::from_millis(200))
            .map_err(|e| e.to_string())?;
        let _ = self.port.clear(ClearBuffer::All);
        let boot = Duration::from_secs(4);
        let rest = for_how_long.saturating_sub(boot);
        let mut got: Vec<u8> = Vec::new();
        println!("uart listen {boot:?} @ 230400 then {rest:?} @ 115200");
        self.port
            .set_baud_rate(230_400)
            .map_err(|e| e.to_string())?;
        self.listen_at(&mut got, boot)?;
        println!();
        self.port
            .set_baud_rate(115_200)
            .map_err(|e| e.to_string())?;
        self.listen_at(&mut got, rest)?;
        println!();
        if got.is_empty() {
            println!("uart: 0 bytes");
        } else {
            println!(
                "uart: {} bytes {:02x?}",
                got.len(),
                &got[..got.len().min(32)]
            );
        }
        Ok(())
    }

    fn write_image(&mut self, image: &FlashImage) -> Result<(), String> {
        for part in &image.parts {
            self.write_block(part.flash_off, &part.data)?;
        }
        Ok(())
    }

    fn write_block(&mut self, mut offset: u32, data: &[u8]) -> Result<(), String> {
        let mut erase_size = data.len() as u32;
        let tail = erase_size % CPBIN_BLK;
        if tail > 0x1000 {
            erase_size += CPBIN_BLK - tail;
        }
        self.erase_range(offset, erase_size)?;
        let mut rest = data;
        while !rest.is_empty() {
            let n = (rest.len() as u32).min(CPBIN_BLK) as usize;
            self.send_chunk(offset, &rest[..n])?;
            offset += n as u32;
            rest = &rest[n..];
            self.cpbin += 1;
        }
        Ok(())
    }

    fn erase_range(&mut self, mut offset: u32, size: u32) -> Result<(), String> {
        let mut count = (size + SECTOR - 1 + (offset & (SECTOR - 1))) / SECTOR;
        offset &= !0xFFF;
        while count > 0 {
            if offset >= self.erased_from && offset < self.erased_to {
                offset += SECTOR;
                count -= 1;
                continue;
            }
            if offset & 0xFFFF == 0 && count > 15 {
                print!("erase 64K {offset:#010x} ... ");
                let _ = io::stdout().flush();
                self.port
                    .set_timeout(Duration::from_millis(2000))
                    .map_err(|e| e.to_string())?;
                self.cmd(&format!("er64k {:X}", offset | self.patch_flash))?;
                self.port
                    .set_timeout(Duration::from_millis(200))
                    .map_err(|e| e.to_string())?;
                println!("ok");
                self.erased_from = offset;
                self.erased_to = offset + 0x10000;
                offset += 0x10000;
                count -= 16;
            } else {
                print!("erase 4K {offset:#010x} ... ");
                let _ = io::stdout().flush();
                self.port
                    .set_timeout(Duration::from_millis(500))
                    .map_err(|e| e.to_string())?;
                self.cmd(&format!("era4k {:X}", offset | self.patch_flash))?;
                self.port
                    .set_timeout(Duration::from_millis(200))
                    .map_err(|e| e.to_string())?;
                println!("ok");
                self.erased_from = offset;
                self.erased_to = offset + SECTOR;
                offset += SECTOR;
                count -= 1;
            }
        }
        Ok(())
    }

    fn send_chunk(&mut self, offset: u32, data: &[u8]) -> Result<(), String> {
        let payload = pad_cpbin_chunk(data);
        if chunk_is_erased(&payload) {
            return Ok(());
        }
        print!("write  {:#010x}  {} bytes ... ", offset, payload.len());
        let _ = io::stdout().flush();
        self.port
            .set_timeout(Duration::from_millis(1000))
            .map_err(|e| e.to_string())?;
        let cmd = format!(
            "cpbin c{} {:X} {:X} {:X}",
            self.cpbin,
            offset | self.patch_flash,
            payload.len(),
            offset
        );
        self.write_bytes(cmd.as_bytes())?;
        let prompt = self.read_bytes(12)?;
        if prompt.as_slice() != b"by hex mode:" {
            return Err(format!(
                "cpbin rejected: {}",
                String::from_utf8_lossy(&prompt)
            ));
        }
        self.write_bytes(&payload)?;
        let ck = self.read_bytes(23)?;
        if ck.len() < 23 || &ck[0..15] != b"checksum is: 0x" {
            return Err(format!("checksum prompt: {}", String::from_utf8_lossy(&ck)));
        }
        self.write_bytes(&ck[15..])?;
        let ack = self.read_bytes(6)?;
        if ack.as_slice() != b"#OK>>:" {
            return Err(format!("crc: {}", String::from_utf8_lossy(&ack)));
        }
        println!("ok");
        self.port
            .set_timeout(Duration::from_millis(200))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn cmd(&mut self, pkt: &str) -> Result<(), String> {
        self.write_bytes(pkt.as_bytes())?;
        let ack = self.read_bytes(6)?;
        if ack.as_slice() == b"#OK>>:" {
            Ok(())
        } else {
            Err(format!(
                "command {pkt:?} failed: {}",
                String::from_utf8_lossy(&ack)
            ))
        }
    }

    fn write_reg(&mut self, addr: u32, data: u32) -> Result<(), String> {
        self.cmd(&format!("wrreg{addr:08x} {data:08x} "))
    }

    fn read_reg(&mut self, addr: u32) -> Result<u32, String> {
        self.write_bytes(format!("rdreg{addr:08x} ").as_bytes())?;
        let read = self.read_bytes(17)?;
        if read.len() == 17 && read.starts_with(b"=0x") && read[11..17] == *b"#OK>>:" {
            let txt = std::str::from_utf8(&read[3..11]).map_err(|e| e.to_string())?;
            u32::from_str_radix(txt, 16).map_err(|e| e.to_string())
        } else {
            Err(format!(
                "rdreg {:#010x}: {}",
                addr,
                String::from_utf8_lossy(&read)
            ))
        }
    }

    fn write_bytes(&mut self, data: &[u8]) -> Result<(), String> {
        self.port.write_all(data).map_err(|e| e.to_string())?;
        self.port.flush().map_err(|e| e.to_string())
    }

    fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let mut out = vec![0u8; n];
        let mut got = 0;
        while got < n {
            match self.port.read(&mut out[got..]) {
                Ok(0) => break,
                Ok(k) => got += k,
                Err(err) if err.kind() == io::ErrorKind::TimedOut => break,
                Err(err) => return Err(err.to_string()),
            }
        }
        out.truncate(got);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::pick_port;

    #[test]
    fn prefers_wchusbserial() {
        let names = [
            "/dev/cu.Bluetooth-Incoming-Port".to_string(),
            "/dev/cu.wchusbserial1410".to_string(),
        ];
        assert_eq!(
            pick_port(&names).as_deref(),
            Some("/dev/cu.wchusbserial1410")
        );
    }
}
