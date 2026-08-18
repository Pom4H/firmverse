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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        bridge::run(opts)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let BleHostOpts { hex, name, .. } = opts;
        Err(format!(
            "--ble hosts the ATT mailbox on Linux BlueZ or macOS (not {name} / {})",
            hex.display()
        ))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod bridge {
    use super::BleHostOpts;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use std::time::Duration;

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

        let self_exe = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;
        let backend = backend_name();

        let mut emu_cmd = Command::new(self_exe);
        emu_cmd.arg("--raw");
        if opts.strict {
            emu_cmd.arg("--strict");
        }
        emu_cmd
            .arg("--max-insns")
            .arg(opts.max_insns.to_string())
            .arg(&opts.hex)
            .env("PHY6252_GUEST_RX_UUID", &opts.rx_uuid)
            .env("PHY6252_GUEST_TX_UUID", &opts.tx_uuid)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut emu = emu_cmd
            .spawn()
            .map_err(|e| format!("start emulator child: {e}"))?;

        let mut ble = spawn_helper(&opts).inspect_err(|_| {
            let _ = emu.kill();
        })?;

        let mut emu_in = emu.stdin.take().ok_or("emulator stdin unavailable")?;
        let mut ble_in = ble.stdin.take().ok_or("BLE helper stdin unavailable")?;
        let (tx, rx) = mpsc::channel();

        spawn_lines(
            emu.stdout.take().ok_or("emulator stdout unavailable")?,
            Source::EmuOut,
            tx.clone(),
        );
        spawn_lines(
            emu.stderr.take().ok_or("emulator stderr unavailable")?,
            Source::EmuErr,
            tx.clone(),
        );
        spawn_lines(
            ble.stdout.take().ok_or("BLE stdout unavailable")?,
            Source::BleOut,
            tx.clone(),
        );
        spawn_lines(
            ble.stderr.take().ok_or("BLE stderr unavailable")?,
            Source::BleErr,
            tx.clone(),
        );
        spawn_stdin(tx);

        eprintln!(
            "BLE host={backend} name={} service={} firmware={}",
            opts.name,
            opts.service,
            opts.hex.display()
        );

        let code = bridge_loop(&mut emu, &mut ble, &mut emu_in, &mut ble_in, rx)?;
        stop_child(&mut ble, Some(&mut ble_in));
        stop_child(&mut emu, None);
        Ok(code)
    }

    fn spawn_helper(opts: &BleHostOpts) -> Result<Child, String> {
        let mut cmd = helper_command()?;
        cmd.arg("--name")
            .arg(&opts.name)
            .arg("--service")
            .arg(&opts.service)
            .arg("--rx")
            .arg(&opts.rx_uuid)
            .arg("--tx")
            .arg(&opts.tx_uuid)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.spawn()
            .map_err(|e| format!("start {backend} helper: {e}", backend = backend_name()))
    }

    fn helper_command() -> Result<Command, String> {
        #[cfg(target_os = "linux")]
        {
            linux_helper()
        }
        #[cfg(target_os = "macos")]
        {
            darwin_helper()
        }
    }

    fn backend_name() -> &'static str {
        #[cfg(target_os = "linux")]
        {
            "BlueZ"
        }
        #[cfg(target_os = "macos")]
        {
            "macOS"
        }
    }

    #[cfg(target_os = "linux")]
    fn linux_helper() -> Result<Command, String> {
        const BLUEZ_HELPER: &str = include_str!("../host/ble/bluez.py");
        let dir = std::env::temp_dir().join(format!("phy6252-ble-{}", env!("CARGO_PKG_VERSION")));
        std::fs::create_dir_all(&dir).map_err(|e| format!("create BLE helper dir: {e}"))?;
        let path = dir.join("bluez.py");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(BLUEZ_HELPER) {
            std::fs::write(&path, BLUEZ_HELPER).map_err(|e| format!("write BLE helper: {e}"))?;
        }
        let mut cmd = Command::new("python3");
        cmd.arg("-u").arg(path);
        Ok(cmd)
    }

    #[cfg(target_os = "macos")]
    fn darwin_helper() -> Result<Command, String> {
        Ok(Command::new(darwin::ensure_helper()?))
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
                    let quit = matches!(
                        line.trim().to_ascii_lowercase().as_str(),
                        "q" | "quit" | "exit"
                    );
                    put(emu_in, &line)?;
                    if quit {
                        let _ = put(ble_in, "QUIT");
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            if let Some(status) = emu
                .try_wait()
                .map_err(|e| format!("emulator status: {e}"))?
            {
                return Ok(if status.success() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                });
            }
            if let Some(status) = ble.try_wait().map_err(|e| format!("BLE status: {e}"))? {
                return Err(format!(
                    "{backend} helper exited with {status}",
                    backend = backend_name()
                ));
            }
        }
        Ok(ExitCode::SUCCESS)
    }

    fn spawn_lines<R: Read + Send + 'static>(
        reader: R,
        source: Source,
        tx: Sender<(Source, String)>,
    ) {
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

    #[cfg(target_os = "macos")]
    mod darwin {
        use std::fs;
        use std::path::PathBuf;
        use std::process::Command;

        const SOURCE: &str = include_str!("../host/ble/darwin.swift");
        const PLIST: &str = include_str!("../host/ble/darwin.plist");

        pub fn ensure_helper() -> Result<PathBuf, String> {
            let root =
                std::env::temp_dir().join(format!("phy6252-ble-{}", env!("CARGO_PKG_VERSION")));
            let contents = root.join("Phy6252Ble.app/Contents");
            let macos_dir = contents.join("MacOS");
            let bin = macos_dir.join("phy6252-ble");
            let src = root.join("darwin.swift");
            let plist = contents.join("Info.plist");
            fs::create_dir_all(&macos_dir).map_err(|e| format!("create BLE helper dir: {e}"))?;
            if fs::read_to_string(&src).ok().as_deref() != Some(SOURCE) {
                fs::write(&src, SOURCE).map_err(|e| format!("write BLE helper: {e}"))?;
                let _ = fs::remove_file(&bin);
            }
            if fs::read_to_string(&plist).ok().as_deref() != Some(PLIST) {
                fs::write(&plist, PLIST).map_err(|e| format!("write BLE Info.plist: {e}"))?;
            }
            if bin.is_file() {
                return Ok(bin);
            }
            let output = Command::new("xcrun")
                .args([
                    "swiftc",
                    "-O",
                    "-framework",
                    "Foundation",
                    "-framework",
                    "CoreBluetooth",
                    "-o",
                ])
                .arg(&bin)
                .arg(&src)
                .output()
                .map_err(|e| format!("xcrun swiftc (install Xcode Command Line Tools): {e}"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                return Err(format!("compile macOS BLE helper failed: {stderr}{stdout}"));
            }
            Ok(bin)
        }
    }
}
