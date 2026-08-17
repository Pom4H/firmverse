use crate::bus::{
    GpioBank, Phy6252Bus, ADC_CH_COUNT, GPIO_PIN_MASK, PWM_CHANNELS, SRAM_BASE, SRAM_SIZE, XIP_BASE,
    XIP_SIZE,
};
use crate::cmd::{gpio_silk, ChipCmd, HELP};
use crate::discovery::DiscoveryBus;
use crate::hex::HexImage;
use crate::mailbox;
use std::cell::RefCell;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
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

pub struct RunOpts {
    pub hex: PathBuf,
    pub live: bool,
    pub raw: bool,
    pub strict_mmio: bool,
    pub max_insns: u64,
}

pub fn default_hex() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("PHY6252_HEX") {
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
    Err("pass a .hex or set PHY6252_HEX".into())
}

pub fn run(opts: RunOpts) -> Result<ExitCode, String> {
    let hex_path = opts.hex;
    let live = opts.live;
    let raw = opts.raw;
    let strict_mmio = opts.strict_mmio;
    let max_insns = opts.max_insns;

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
    let device = DiscoveryBus::new(device, strict_mmio);
    let sp = u32::from_le_bytes([vectors[0], vectors[1], vectors[2], vectors[3]]);
    let reset = u32::from_le_bytes([vectors[4], vectors[5], vectors[6], vectors[7]]);
    eprintln!("hex {}", hex_path.display());
    eprintln!("SP={sp:#010x} Reset={reset:#010x}");
    if strict_mmio {
        eprintln!("MMIO discovery: strict");
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
        if raw {
            println!("READY");
            println!("ADV name=PB03FKIT service=6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001");
            emit_gpio(&gpio, true);
            emit_pwm(&pwm);
        } else {
            eprintln!("{}", HELP.trim_end());
            eprintln!("ready  {}", hex_label(&hex_path));
        }
    }

    let burst: u32 = if live { 8_000 } else { 1 };
    let mut insn: u64 = 0;
    while insn < max_insns {
        apply_ext(&gpio, &ext_in);
        if let Some(trap) = processor.take_pending_fault_trap() {
            return report_stop(&mut processor, insn, live, raw, &gpio, &format!("fault {trap:?}"));
        }
        if !processor.running {
            return report_stop(&mut processor, insn, live, raw, &gpio, "halt");
        }
        if processor.sleeping {
            processor.sleeping = false;
        }

        if live {
            if let Some(rx) = cmd_rx.as_ref() {
                if drain_cmds(rx, &ext_in, &mut processor, &adc_mv, &mut clock_ms, raw) {
                    return report_stop(&mut processor, insn, live, raw, &gpio, "quit");
                }
            }
        }

        for _ in 0..burst {
            processor.step();
            insn += 1;
            if insn >= max_insns {
                break;
            }
        }

        flush_uart(&uart_rx, &mut uart_line, raw);
        if take_flag(&gpio_changed) {
            emit_gpio(&gpio, raw);
        }
        if take_flag(&pwm_changed) && (!live || clock_ms % 50 == 0) {
            if raw {
                emit_pwm(&pwm);
            }
        }
        emit_frames(&mut processor, &mut last_tx_seq, raw);

        if live {
            clock_ms = clock_ms.wrapping_add(1);
            let _ = mailbox::set_tick(&mut processor, clock_ms);
            let _ = io::stdout().flush();
            thread::sleep(Duration::from_millis(1));
        }
    }

    report_stop(&mut processor, insn, live, raw, &gpio, "max instructions")
}

fn hex_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn drain_cmds(
    rx: &Receiver<ChipCmd>,
    ext_in: &Arc<AtomicU32>,
    processor: &mut Processor,
    adc_mv: &Rc<RefCell<[u16; ADC_CH_COUNT]>>,
    clock_ms: &mut u32,
    raw: bool,
) -> bool {
    while let Ok(cmd) = rx.try_recv() {
        if matches!(cmd, ChipCmd::Quit) {
            return true;
        }
        if matches!(cmd, ChipCmd::Help) {
            if !raw {
                eprintln!("{}", HELP.trim_end());
            }
            continue;
        }
        let result = match cmd {
            ChipCmd::In(value) => {
                ext_in.store(value & GPIO_PIN_MASK, Ordering::Relaxed);
                Ok(())
            }
            ChipCmd::Pin { bit, high } => {
                let mask = 1u32 << bit;
                let cur = ext_in.load(Ordering::Relaxed);
                let next = if high { cur | mask } else { cur & !mask };
                ext_in.store(next & GPIO_PIN_MASK, Ordering::Relaxed);
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
            ChipCmd::Help | ChipCmd::Quit => Ok(()),
        };
        if let Err(fault) = result {
            eprintln!("err {fault}");
        }
    }
    false
}

fn spawn_cmd_reader() -> Receiver<ChipCmd> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            match crate::cmd::parse_line(line.trim()) {
                Ok(Some(cmd)) => {
                    if tx.send(cmd).is_err() {
                        break;
                    }
                }
                Ok(None) => {}
                Err(err) => eprintln!("{err}"),
            }
        }
    });
    rx
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

fn emit_frames(processor: &mut Processor, last_tx_seq: &mut u32, raw: bool) {
    match mailbox::take_tx(processor, last_tx_seq) {
        Ok(Some(frame)) => {
            if raw {
                print!("FRAME ");
                for byte in &frame {
                    print!("{byte:02X}");
                }
                println!();
            } else {
                print!("att ");
                for byte in &frame {
                    print!("{byte:02x}");
                }
                println!();
            }
        }
        Ok(None) => {}
        Err(fault) => eprintln!("err mailbox {fault}"),
    }
}

fn emit_gpio(gpio: &Rc<RefCell<GpioBank>>, raw: bool) {
    let bank = gpio.borrow();
    if raw {
        println!("GPIO {:08x} {:08x}", bank.dr, bank.ddr);
    } else {
        println!("gpio {}", gpio_silk(bank.dr, bank.ddr));
    }
}

fn emit_pwm(pwm: &Rc<RefCell<[u32; PWM_CHANNELS]>>) {
    let duty = pwm.borrow();
    print!("PWM");
    for value in duty.iter() {
        print!(" {value:04x}");
    }
    println!();
}

fn flush_uart(uart_rx: &Rc<RefCell<Vec<u8>>>, line: &mut String, raw: bool) {
    let mut buf = uart_rx.borrow_mut();
    if buf.is_empty() {
        return;
    }
    let bytes = buf.split_off(0);
    drop(buf);
    for byte in bytes {
        if byte == b'\n' {
            if raw {
                println!("UART {line}");
            } else {
                println!("uart {line}");
            }
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
    raw: bool,
    gpio: &Rc<RefCell<GpioBank>>,
    reason: &str,
) -> Result<ExitCode, String> {
    emit_gpio(gpio, raw);
    if live {
        if raw {
            println!("STOP {reason}");
        } else {
            eprintln!("stop {reason}");
        }
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
