use crate::bus::{GpioBank, PWM_CHANNELS};
use crate::chip::{Apply, Chip};
use crate::cmd::{gpio_silk, ChipCmd, HELP};
use std::cell::RefCell;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

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
    find_firmware_hex("kit-demo.hex")
}

pub fn find_firmware_hex(name: &str) -> Result<PathBuf, String> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("firmware");
    dir.push(name);
    if dir.is_file() {
        return Ok(dir);
    }
    let mut built = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    built.push("firmware/build");
    built.push(name);
    if built.is_file() {
        return Ok(built);
    }
    Err(format!("no {name} under firmware/ or firmware/build/"))
}

pub fn run(opts: RunOpts) -> Result<ExitCode, String> {
    let mut chip = Chip::load(
        "chip".into(),
        &opts.hex,
        opts.strict_mmio,
        crate::chip::mac_from_id("chip"),
        0.0,
        0.0,
    )?;
    let live = opts.live;
    let raw = opts.raw;
    let max_insns = opts.max_insns;
    let cmd_rx = if live { Some(spawn_cmd_reader()) } else { None };

    if live {
        if raw {
            println!("READY");
            println!("ADV name=PB03FKIT service=6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001");
            emit_gpio(&chip.gpio_bank(), true, "");
            emit_pwm(&[0; PWM_CHANNELS], "");
        } else {
            eprintln!("{}", HELP.trim_end());
            eprintln!("ready  {}", chip.hex_label);
        }
    }

    let burst: u32 = if live { 8_000 } else { 1 };
    loop {
        if live {
            if let Some(rx) = cmd_rx.as_ref() {
                if drain_cmds(rx, &mut chip, raw)? {
                    return report_stop(&mut chip, live, raw, "quit");
                }
            }
        }
        let delta = chip.tick(burst, max_insns, live);
        emit_delta(&delta, raw, live, "");
        if let Some(reason) = chip.stopped().map(str::to_string) {
            return report_stop(&mut chip, live, raw, &reason);
        }
        if live {
            let _ = io::stdout().flush();
            thread::sleep(Duration::from_millis(1));
        } else if chip.insn >= max_insns {
            return report_stop(&mut chip, live, raw, "max instructions");
        }
    }
}

fn drain_cmds(rx: &Receiver<ChipCmd>, chip: &mut Chip, raw: bool) -> Result<bool, String> {
    while let Ok(cmd) = rx.try_recv() {
        match chip.apply(cmd)? {
            Apply::Quit => return Ok(true),
            Apply::Help => {
                if !raw {
                    eprintln!("{}", HELP.trim_end());
                }
            }
            Apply::Continue => {}
        }
    }
    Ok(false)
}

pub(crate) fn spawn_cmd_reader() -> Receiver<ChipCmd> {
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

pub(crate) fn spawn_line_reader() -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

pub(crate) fn emit_delta(delta: &crate::chip::ChipDelta, raw: bool, live: bool, tag: &str) {
    for line in &delta.uart_lines {
        tag_out(tag);
        if raw {
            println!("UART {line}");
        } else {
            println!("uart {line}");
        }
    }
    if let Some((dr, ddr)) = delta.gpio {
        emit_gpio_pair(dr, ddr, raw, tag);
    }
    if let Some(duty) = delta.pwm {
        if raw || !live {
            emit_pwm(&duty, tag);
        }
    }
    for frame in &delta.frames {
        tag_out(tag);
        if raw {
            print!("FRAME ");
            for byte in frame {
                print!("{byte:02X}");
            }
            println!();
        } else {
            print!("att ");
            for byte in frame {
                print!("{byte:02x}");
            }
            println!();
        }
    }
}

pub(crate) fn emit_gpio(gpio: &Rc<RefCell<GpioBank>>, raw: bool, tag: &str) {
    let bank = gpio.borrow();
    emit_gpio_pair(bank.dr, bank.ddr, raw, tag);
}

fn emit_gpio_pair(dr: u32, ddr: u32, raw: bool, tag: &str) {
    tag_out(tag);
    if raw {
        println!("GPIO {dr:08x} {ddr:08x}");
    } else {
        println!("gpio {}", gpio_silk(dr, ddr));
    }
}

fn emit_pwm(duty: &[u32; PWM_CHANNELS], tag: &str) {
    tag_out(tag);
    print!("PWM");
    for value in duty {
        print!(" {value:04x}");
    }
    println!();
}

pub(crate) fn tag_out(tag: &str) {
    if !tag.is_empty() {
        print!("[{tag}] ");
    }
}

fn report_stop(chip: &mut Chip, live: bool, raw: bool, reason: &str) -> Result<ExitCode, String> {
    emit_gpio(&chip.gpio_bank(), raw, "");
    if live {
        if raw {
            println!("STOP {reason}");
        } else {
            eprintln!("stop {reason}");
        }
    } else {
        println!("stop: {reason}");
    }
    let (pc, lr, msp) = chip.pc_lr_msp();
    eprintln!(
        "insns={} pc={pc:#010x} lr={lr:#010x} msp={msp:#010x}",
        chip.insn
    );
    Ok(ExitCode::from(if reason.starts_with("fault") {
        2
    } else {
        0
    }))
}
