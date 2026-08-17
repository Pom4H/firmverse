mod bus;
mod hex;
mod mailbox;

use crate::bus::{
    GpioBank, Phy6252Bus, ADC_CH_COUNT, GPIO_PIN_MASK, PWM_CHANNELS, SRAM_BASE, SRAM_SIZE, XIP_BASE,
    XIP_SIZE,
};
use crate::hex::HexImage;
use std::cell::RefCell;
use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use zmu_cortex_m::core::fault::FaultTrapMode;
use zmu_cortex_m::core::register::BaseReg;
use zmu_cortex_m::core::reset::Reset;
use zmu_cortex_m::executor::Executor;
use zmu_cortex_m::Processor;

enum ChipCmd {
    In(u32),
    Write(Vec<u8>),
    Connect,
    Disconnect,
    Cccd(bool),
    Tick(u32),
    Adc([u16; 4]),
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut hex_path = None;
    let mut live = false;
    let mut max_insns: u64 = 2_000_000;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                return Ok(ExitCode::SUCCESS);
            }
            "--gpio" | "--live" => {
                live = true;
                if max_insns == 2_000_000 {
                    max_insns = 50_000_000;
                }
            }
            "--max-insns" => {
                i += 1;
                max_insns = args
                    .get(i)
                    .ok_or("missing --max-insns value")?
                    .parse()
                    .map_err(|_| "invalid --max-insns")?;
            }
            path if !path.starts_with('-') => hex_path = Some(PathBuf::from(path)),
            other => return Err(format!("unknown argument {other}")),
        }
        i += 1;
    }

    let hex_path = match hex_path {
        Some(path) => path,
        None => default_hex()?,
    };

    let image = HexImage::load(&hex_path).map_err(|e| format!("{}: {e}", hex_path.display()))?;
    let mut sram = vec![0u8; SRAM_SIZE];
    let mut xip = vec![0u8; XIP_SIZE];
    image.fill(SRAM_BASE, &mut sram);
    image.fill(XIP_BASE, &mut xip);

    let device = Phy6252Bus::new(sram, xip);
    let gpio = Rc::clone(&device.gpio);
    let gpio_changed = Rc::clone(&device.gpio_changed);
    let uart_rx = Rc::clone(&device.uart_rx);
    let pwm = Rc::clone(&device.pwm);
    let pwm_changed = Rc::clone(&device.pwm_changed);
    let adc_mv = Rc::clone(&device.adc_mv);
    let vectors = device.vector_table();
    let sp = u32::from_le_bytes([vectors[0], vectors[1], vectors[2], vectors[3]]);
    let reset = u32::from_le_bytes([vectors[4], vectors[5], vectors[6], vectors[7]]);
    if live {
        eprintln!("hex {}", hex_path.display());
        eprintln!("SP={sp:#010x} Reset={reset:#010x}");
    } else {
        println!("hex {}", hex_path.display());
        println!("SP={sp:#010x} Reset={reset:#010x}");
    }

    let ext_in = Arc::new(AtomicU32::new(0));
    let cmd_rx = if live {
        Some(spawn_cmd_reader())
    } else {
        None
    };

    let mut processor = Processor::new();
    processor.fault_trap_mode(FaultTrapMode::hardfault());
    processor.device(Some(Box::new(device)));
    processor.flash_memory(vectors.len(), &vectors);
    processor
        .reset()
        .map_err(|fault| format!("reset failed: {fault}"))?;
    mailbox::plant_magic(&mut processor).map_err(|fault| format!("mailbox plant {fault}"))?;

    let mut last_tx_seq = 0u32;
    let mut uart_line = String::new();
    let mut clock_ms = 0u32;

    if live {
        println!("READY");
        println!("ADV name=PB03FKIT service=6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001");
        emit_gpio(&gpio);
        emit_pwm(&pwm);
    }

    let burst: u32 = if live { 8_000 } else { 1 };
    let mut insn: u64 = 0;
    while insn < max_insns {
        apply_ext(&gpio, &ext_in);
        if let Some(trap) = processor.take_pending_fault_trap() {
            return report_stop(&mut processor, insn, live, &gpio, &format!("fault {trap:?}"));
        }
        if !processor.running {
            return report_stop(&mut processor, insn, live, &gpio, "halt");
        }
        if processor.sleeping {
            processor.sleeping = false;
        }

        if live {
            if let Some(rx) = cmd_rx.as_ref() {
                drain_cmds(rx, &ext_in, &mut processor, &adc_mv, &mut clock_ms);
            }
        }

        for _ in 0..burst {
            processor.step();
            insn += 1;
            if insn >= max_insns {
                break;
            }
        }

        flush_uart(&uart_rx, &mut uart_line);
        if take_flag(&gpio_changed) {
            emit_gpio(&gpio);
        }
        if take_flag(&pwm_changed) && (!live || clock_ms % 50 == 0) {
            emit_pwm(&pwm);
        }
        emit_frames(&mut processor, &mut last_tx_seq);

        if live {
            clock_ms = clock_ms.wrapping_add(1);
            let _ = mailbox::set_tick(&mut processor, clock_ms);
            let _ = io::stdout().flush();
            thread::sleep(Duration::from_millis(1));
        }
    }

    report_stop(&mut processor, insn, live, &gpio, "max instructions")
}

fn drain_cmds(
    rx: &Receiver<ChipCmd>,
    ext_in: &Arc<AtomicU32>,
    processor: &mut Processor,
    adc_mv: &Rc<RefCell<[u16; ADC_CH_COUNT]>>,
    clock_ms: &mut u32,
) {
    while let Ok(cmd) = rx.try_recv() {
        let result = match cmd {
            ChipCmd::In(value) => {
                ext_in.store(value & GPIO_PIN_MASK, Ordering::Relaxed);
                Ok(())
            }
            ChipCmd::Write(bytes) => mailbox::write_rx(processor, &bytes),
            ChipCmd::Connect => mailbox::connect(processor, true),
            ChipCmd::Disconnect => mailbox::connect(processor, false),
            ChipCmd::Cccd(on) => mailbox::cccd(processor, on),
            ChipCmd::Tick(ms) => {
                *clock_ms = clock_ms.wrapping_add(ms);
                mailbox::set_tick(processor, *clock_ms)
            }
            ChipCmd::Adc(pads) => {
                let mut adc = adc_mv.borrow_mut();
                adc[7] = pads[0];
                adc[6] = pads[1];
                adc[4] = pads[2];
                adc[3] = pads[3];
                Ok(())
            }
        };
        if let Err(fault) = result {
            println!("ERR {fault}");
        }
    }
}

fn spawn_cmd_reader() -> Receiver<ChipCmd> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if let Some(cmd) = parse_cmd(line.trim()) {
                if tx.send(cmd).is_err() {
                    break;
                }
            }
        }
    });
    rx
}

fn parse_cmd(line: &str) -> Option<ChipCmd> {
    if let Some(rest) = line.strip_prefix("IN ") {
        let value = u32::from_str_radix(rest.trim(), 16).ok()?;
        return Some(ChipCmd::In(value));
    }
    if let Some(rest) = line.strip_prefix("WRITE ") {
        return Some(ChipCmd::Write(parse_hex_bytes(rest.trim())?));
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
        let mut parts = rest.split_whitespace();
        let p20 = parts.next()?.parse::<u16>().ok()?;
        let p15 = parts.next()?.parse::<u16>().ok()?;
        let p24 = parts.next()?.parse::<u16>().ok()?;
        let p23 = parts.next()?.parse::<u16>().ok()?;
        return Some(ChipCmd::Adc([p20, p15, p24, p23]));
    }
    None
}

fn parse_hex_bytes(text: &str) -> Option<Vec<u8>> {
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

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn apply_ext(gpio: &Rc<RefCell<GpioBank>>, ext_in: &Arc<AtomicU32>) {
    let value = ext_in.load(Ordering::Relaxed) & GPIO_PIN_MASK;
    let mut bank = gpio.borrow_mut();
    if bank.ext != value {
        bank.ext = value;
    }
}

fn take_flag(flag: &Rc<RefCell<bool>>) -> bool {
    let mut changed = flag.borrow_mut();
    let value = *changed;
    *changed = false;
    value
}

fn emit_frames(processor: &mut Processor, last_tx_seq: &mut u32) {
    match mailbox::take_tx(processor, last_tx_seq) {
        Ok(Some(frame)) => {
            print!("FRAME ");
            for byte in &frame {
                print!("{byte:02X}");
            }
            println!();
        }
        Ok(None) => {}
        Err(fault) => println!("ERR mailbox {fault}"),
    }
}

fn emit_gpio(gpio: &Rc<RefCell<GpioBank>>) {
    let bank = gpio.borrow();
    println!("GPIO {:08x} {:08x}", bank.dr, bank.ddr);
}

fn emit_pwm(pwm: &Rc<RefCell<[u32; PWM_CHANNELS]>>) {
    let duty = pwm.borrow();
    print!("PWM");
    for value in duty.iter() {
        print!(" {value:04x}");
    }
    println!();
}

fn flush_uart(uart_rx: &Rc<RefCell<Vec<u8>>>, line: &mut String) {
    let mut buf = uart_rx.borrow_mut();
    if buf.is_empty() {
        return;
    }
    let bytes = buf.split_off(0);
    drop(buf);
    for byte in bytes {
        if byte == b'\n' {
            println!("UART {line}");
            line.clear();
        } else if byte >= 32 && byte < 127 {
            line.push(char::from(byte));
        }
    }
}

fn report_stop(
    processor: &mut Processor,
    insns: u64,
    live: bool,
    gpio: &Rc<RefCell<GpioBank>>,
    reason: &str,
) -> Result<ExitCode, String> {
    emit_gpio(gpio);
    if live {
        println!("STOP {reason}");
    } else {
        println!("stop: {reason}");
    }
    eprintln!(
        "insns={insns} pc={:#010x} lr={:#010x} msp={:#010x}",
        processor.get_pc(),
        processor.lr,
        processor.msp
    );
    Ok(ExitCode::from(if reason.starts_with("fault") { 2 } else { 0 }))
}

fn default_hex() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("PHY6252_HEX") {
        return Ok(PathBuf::from(path));
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("firmware/kit-demo.hex");
    if dir.is_file() {
        return Ok(dir);
    }
    let mut built = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    built.push("firmware/build/kit-demo.hex");
    if built.is_file() {
        return Ok(built);
    }
    Err("pass firmware.hex or set PHY6252_HEX".into())
}

fn print_usage() {
    eprintln!(
        "usage: phy6252-emu [--live] [--max-insns N] [firmware.hex]\n\
         PHY6252 Cortex-M0 emulator. --live streams GPIO/UART/FRAME and reads stdin.\n\
         stdin: IN <hex> | WRITE <hex> | CONNECT | DISCONNECT | CCCD <n> | TICK <ms> | ADC <p20> <p15> <p24> <p23>\n\
         BLE air (macOS): bash scripts/air.sh — laptop radio is the RF PHY, ATT goes to the hex mailbox."
    );
}

#[cfg(test)]
mod tests {
    use super::parse_cmd;

    #[test]
    fn parses_adc_and_write() {
        match parse_cmd("ADC 12000 5000 3300 1650") {
            Some(super::ChipCmd::Adc(v)) => assert_eq!(v, [12000, 5000, 3300, 1650]),
            _ => panic!("adc"),
        }
        match parse_cmd("WRITE 48656c6c6f") {
            Some(super::ChipCmd::Write(b)) => assert_eq!(b, b"Hello"),
            _ => panic!("write"),
        }
    }
}
