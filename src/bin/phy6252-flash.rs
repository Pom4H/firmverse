//! PHY62x2 UART bootloader CLI for PB-03F-Kit.

use clap::Parser;
use firmverse::flash::{
    FlashOptions, Flasher, HarnessTarget, Pb03fBoot, Pb03fKit, SerialTransport,
};
use firmverse::programmer::{build_flash_image, parse_intel_hex, FlashImage};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "phy6252-flash",
    version,
    about = "Flash an Intel HEX to PHY6252 / PB-03F-Kit over the shared Firmverse flasher core"
)]
struct Cli {
    /// Intel HEX image
    hex: Option<PathBuf>,
    /// Serial port (auto-detects CH340 / wchusbserial)
    #[arg(short, long)]
    port: Option<String>,
    /// Run the exact same flasher against the deterministic in-memory PHY62xx ROM harness.
    #[arg(long, conflicts_with_all = ["port", "control_lines", "list_ports"])]
    harness: bool,
    /// Virtual NOR size used by --harness. PHY6252/PB-03F uses 256 KiB.
    #[arg(long, default_value_t = 256 * 1024, requires = "harness")]
    harness_flash_size: usize,
    /// Chip-erase before write (also wipes NVRAM / bonds)
    #[arg(long)]
    erase: bool,
    /// Do not send `reset` after a successful write
    #[arg(long)]
    no_reset: bool,
    /// Application start address (default: lowest SRAM segment)
    #[arg(long, value_parser = clap_u32)]
    start: Option<u32>,
    /// Use RTS/DTR boot control for adapters that physically route those lines.
    /// PB-03F-Kit normally uses a manual power-cycle/KEY1 sequence.
    #[arg(long)]
    control_lines: bool,
    /// List serial ports and exit
    #[arg(long)]
    list_ports: bool,
}

fn clap_u32(s: &str) -> Result<u32, String> {
    parse_u32(s).ok_or_else(|| format!("not an integer: {s}"))
}

fn parse_u32(s: &str) -> Option<u32> {
    let text = s.trim();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        text.parse().ok()
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    if cli.list_ports {
        return list_ports();
    }

    let hex_path = cli.hex.ok_or("pass a .hex image, or use --list-ports")?;
    let image = load_image(&hex_path, cli.start)?;
    let options = FlashOptions {
        baud: 115_200,
        chip_erase: cli.erase,
        reset_after: !cli.no_reset,
    };

    if cli.harness {
        print_plan("firmverse-harness", &hex_path, &image);
        return flash_harness(&image, cli.harness_flash_size, options);
    }

    let port_name = cli.port.unwrap_or(auto_port()?);
    print_plan(&port_name, &hex_path, &image);

    let boot = if cli.control_lines {
        Pb03fBoot::ControlLines
    } else {
        println!("Hold KEY1 / power off the PB-03F-Kit now.");
        println!("Start/release power while the tool is sending UXTDWU at 9600 baud.");
        Pb03fBoot::ManualPowerCycle
    };

    let link = SerialTransport::open(&port_name)?;
    let mut target = Pb03fKit::new(link, boot);
    let report = Flasher::new(options).flash(&mut target, &image)?;

    print_report(&report.revision, report.flash_size, report.flash_id, report.bytes_written);
    if !cli.no_reset {
        println!("reset sent — application boot requested");
    }
    println!("flash ok");
    Ok(())
}

fn flash_harness(image: &FlashImage, flash_size: usize, options: FlashOptions) -> Result<(), String> {
    let mut target = HarnessTarget::new(flash_size)?;
    let report = Flasher::new(options).flash(&mut target, image)?;

    for part in &image.parts {
        let start = part.flash_off as usize;
        let end = start
            .checked_add(part.data.len())
            .ok_or("flash image range overflow")?;
        if end > target.flash().len() {
            return Err(format!(
                "flash image part {:#x}..{:#x} exceeds harness NOR",
                start, end
            ));
        }
        if target.flash()[start..end] != part.data {
            return Err(format!(
                "harness verification failed at flash offset {:#x}",
                start
            ));
        }
    }

    if options.reset_after && target.reset_count() != 1 {
        return Err(format!(
            "expected one ROM reset after programming, got {}",
            target.reset_count()
        ));
    }

    print_report(&report.revision, report.flash_size, report.flash_id, report.bytes_written);
    println!("verified {} image part(s) byte-for-byte in virtual NOR", image.parts.len());
    if options.reset_after {
        println!("reset observed by harness");
    }
    println!("harness flash ok");
    Ok(())
}

fn print_report(revision: &str, flash_size: u32, flash_id: u32, bytes_written: usize) {
    println!(
        "ROM {} flash={} KiB id={:#010x}",
        revision,
        flash_size >> 10,
        flash_id
    );
    println!("programmed {} bytes", bytes_written);
}

fn load_image(path: &Path, start: Option<u32>) -> Result<FlashImage, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let hex = parse_intel_hex(&text)?;
    build_flash_image(&hex.segments, start.or(hex.entry))
}

fn print_plan(port: &str, path: &Path, image: &FlashImage) {
    println!("port    {port}");
    println!("image   {}", path.display());
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
}

fn list_ports() -> Result<(), String> {
    let ports = serialport::available_ports().map_err(|error| error.to_string())?;
    if ports.is_empty() {
        println!("no serial ports");
        return Ok(());
    }
    for port in ports {
        println!("{}", port.port_name);
    }
    Ok(())
}

fn auto_port() -> Result<String, String> {
    if let Ok(port) = std::env::var("PHY6252_PORT").or_else(|_| std::env::var("PORT")) {
        if !port.is_empty() {
            return Ok(port);
        }
    }
    let ports = serialport::available_ports().map_err(|error| error.to_string())?;
    let names: Vec<String> = ports.into_iter().map(|port| port.port_name).collect();
    pick_port(&names).ok_or_else(|| {
        "no USB-UART adapter found (CH340 / wchusbserial / ttyUSB). Pass --port".into()
    })
}

fn pick_port(names: &[String]) -> Option<String> {
    const PREFER: [&str; 5] = ["wchusbserial", "usbserial", "ttyUSB", "ttyACM", "COM"];
    for needle in PREFER {
        if let Some(name) = names
            .iter()
            .find(|name| name.contains(needle) && !name.to_ascii_lowercase().contains("bluetooth"))
        {
            return Some(name.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{flash_harness, pick_port};
    use firmverse::flash::FlashOptions;
    use firmverse::programmer::{build_flash_image, Segment};

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

    #[test]
    fn cli_harness_programs_a_real_flash_image() {
        let image = build_flash_image(
            &[Segment {
                load_addr: 0x1FFF_0000,
                data: vec![0x00, 0x80, 0xFF, 0x1F, 0x01, 0x01, 0xFF, 0x1F],
            }],
            Some(0x1FFF_0101),
        )
        .unwrap();
        flash_harness(&image, 256 * 1024, FlashOptions::default()).unwrap();
    }
}
