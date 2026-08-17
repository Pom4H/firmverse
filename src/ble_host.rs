use std::path::PathBuf;
use std::process::ExitCode;

pub struct BleHostOpts {
    pub hex: PathBuf,
    pub strict: bool,
    pub max_insns: u64,
    pub name: String,
    pub service: String,
    pub rx_uuid: String,
    pub tx_uuid: String,
}

pub fn run(opts: BleHostOpts) -> Result<ExitCode, String> {
    #[cfg(target_os = "linux")]
    {
        linux::run(opts)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = opts;
        Err("--ble currently uses Linux BlueZ".into())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::BleHostOpts;
    use std::fs;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::path::PathBuf;
    use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use std::time::Duration;

    const BLUEZ_HELPER: &str = include_str!("../host/ble/bluez.py");

    #[derive(Clone, Copy)]
    enum Source {
        EmuOut,
        EmuErr,
        BleOut,
        BleErr,
        User,
    }

    pub fn run(opts: BleHostOpts) -> Result<ExitCode, String> {
        if !opts.hex.is_file() {
            return Err(format!("firmware not found: {}", opts.hex.display()));
        }

        let helper = helper_path()?;
        let self_exe = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;

        let mut emu_cmd = Command::new(self_exe);
        emu_cmd.arg("--raw");
        if opts.strict {
            emu_cmd.arg("--strict");
        }
        emu_cmd
            .arg("--max-insns")
            .arg(opts.max_insns.to_string())
            .arg(&opts.hex)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut emu = emu_cmd.spawn().map_err(|e| format!("start emulator child: {e}"))?;

        let mut ble = Command::new("python3")
            .arg("-u")
            .arg(&helper)
            .arg("--name")
            .arg(&opts.name)
            .arg("--service")
            .arg(&opts.service)
            .arg("--rx")
            .arg(&opts.rx_uuid)
            .arg("--tx")
            .arg(&opts.tx_uuid)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                let _ = emu.kill();
                format!("start BlueZ helper: {e}")
            })?;

        let mut emu_in = emu.stdin.take().ok_or("emulator stdin unavailable")?;
        let mut ble_in = ble.stdin.take().ok_or("BLE helper stdin unavailable")?;
        let (tx, rx) = mpsc::channel();

        spawn_lines(emu.stdout.take().ok_or("emulator stdout unavailable")?, Source::EmuOut, tx.clone());
        spawn_lines(emu.stderr.take().ok_or("emulator stderr unavailable")?, Source::EmuErr, tx.clone());
        spawn_lines(ble.stdout.take().ok_or("BLE stdout unavailable")?, Source::BleOut, tx.clone());
        spawn_lines(ble.stderr.take().ok_or("BLE stderr unavailable")?, Source::BleErr, tx.clone());
        spawn_stdin(tx);

        eprintln!(
            "BLE host=BlueZ name={} service={} firmware={}",
            opts.name,
            opts.service,
            opts.hex.display()
        );

        let code = bridge_loop(&mut emu, &mut ble, &mut emu_in, &mut ble_in, rx)?;
        stop_child(&mut ble, Some(&mut ble_in));
        stop_child(&mut emu, None);
        Ok(code)
    }

    fn helper_path() -> Result<PathBuf, String> {
        let dir = std::env::temp_dir().join(format!("phy6252-ble-{}", env!("CARGO_PKG_VERSION")));
        fs::create_dir_all(&dir).map_err(|e| format!("create BLE helper dir: {e}"))?;
        let path = dir.join("bluez.py");
        if fs::read_to_string(&path).ok().as_deref() != Some(BLUEZ_HELPER) {
            fs::write(&path, BLUEZ_HELPER).map_err(|e| format!("write BLE helper: {e}"))?;
        }
        Ok(path)
    }

    fn bridge_loop(
        emu: &mut Child,
        ble: &mut Child,
        emu_in: &mut ChildStdin,
        ble_in: &mut ChildStdin,
        rx: Receiver<(Source, String)>,
    ) -> Result<ExitCode, String> {
        let mut connected = false;
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok((Source::EmuOut, line)) => {
                    if let Some(frame) = line.strip_prefix("FRAME ") {
                        put(ble_in, &format!("TX {}", frame.trim()))?;
                    }
                    if !line.starts_with("ADV ") {
                        println!("{line}");
                    }
                }
                Ok((Source::EmuErr, line)) => eprintln!("{line}"),
                Ok((Source::BleErr, line)) => eprintln!("BLE {line}"),
                Ok((Source::BleOut, line)) => {
                    if let Some(frame) = line.strip_prefix("RX ") {
                        if !connected {
                            put(emu_in, "CONNECT")?;
                            connected = true;
                        }
                        put(emu_in, &format!("WRITE {}", frame.trim()))?;
                    } else {
                        match line.as_str() {
                            "CONNECTED" => {
                                if !connected {
                                    put(emu_in, "CONNECT")?;
                                    connected = true;
                                }
                            }
                            "SUBSCRIBED" => put(emu_in, "CCCD 1")?,
                            "UNSUBSCRIBED" => put(emu_in, "CCCD 0")?,
                            "DISCONNECTED" => {
                                put(emu_in, "CCCD 0")?;
                                put(emu_in, "DISCONNECT")?;
                                connected = false;
                            }
                            _ => {}
                        }
                    }
                    println!("BLE {line}");
                }
                Ok((Source::User, line)) => {
                    let quit = matches!(line.trim().to_ascii_lowercase().as_str(), "q" | "quit" | "exit");
                    put(emu_in, &line)?;
                    if quit {
                        let _ = put(ble_in, "QUIT");
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            if let Some(status) = emu.try_wait().map_err(|e| format!("emulator status: {e}"))? {
                return Ok(if status.success() { ExitCode::SUCCESS } else { ExitCode::from(1) });
            }
            if let Some(status) = ble.try_wait().map_err(|e| format!("BLE status: {e}"))? {
                return Err(format!("BlueZ helper exited with {status}"));
            }
        }
        Ok(ExitCode::SUCCESS)
    }

    fn spawn_lines<R: Read + Send + 'static>(reader: R, source: Source, tx: Sender<(Source, String)>) {
        thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                if tx.send((source, line)).is_err() {
                    break;
                }
            }
        });
    }

    fn spawn_stdin(tx: Sender<(Source, String)>) {
        thread::spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                if tx.send((Source::User, line)).is_err() {
                    break;
                }
            }
        });
    }

    fn put(stdin: &mut ChildStdin, line: &str) -> Result<(), String> {
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("bridge write {line:?}: {e}"))
    }

    fn stop_child(child: &mut Child, stdin: Option<&mut ChildStdin>) {
        if let Some(stdin) = stdin {
            let _ = put(stdin, "QUIT");
        }
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}
