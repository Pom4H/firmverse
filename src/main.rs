use clap::{Parser, Subcommand};
use firmverse::ble_host::{self, BleHostOpts};
use firmverse::board::{profile as board_profile, require_phy6252, BoardKind, PROFILES};
#[cfg(firmverse_saturn_native)]
use firmverse::controller::saturn::SaturnPlc;
use firmverse::controller::{self, ControllerKind};
use firmverse::cortex_m::{self, ProbeOpts};
use firmverse::emu::{default_hex, run, RunOpts};
use firmverse::soc::SocKind;
use firmverse::{sim, soc, tui};
use std::path::PathBuf;
use std::process::ExitCode;
use tui::TuiOpts;

const DEFAULT_BLE_SERVICE: &str = "6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001";
const DEFAULT_BLE_RX: &str = "6B1D0002-7C8E-4A91-9F2B-E3A14C5B0001";
const DEFAULT_BLE_TX: &str = "6B1D0003-7C8E-4A91-9F2B-E3A14C5B0001";

#[derive(Parser)]
#[command(
    name = "firmverse",
    version,
    about = "Virtual embedded systems lab for real firmware and managed controllers",
    after_help = "MCU targets execute CPU -> SoC -> Board -> World. Managed targets such as Saturn-PLC execute their exact program runtime through `firmverse plc`.",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Cli {
    /// Firmware image (Intel HEX)
    hex: Option<PathBuf>,
    /// Physical board profile layered above the selected SoC
    #[arg(long, value_enum, default_value_t = BoardKind::Pb03fKit)]
    board: BoardKind,
    /// Run until completion, fault, or --max-insns
    #[arg(long)]
    once: bool,
    /// Machine line protocol
    #[arg(long)]
    raw: bool,
    /// Realtime terminal dashboard with live pinout, ADC/PWM/BLE state and logs
    #[arg(long, conflicts_with_all = ["once", "raw", "ble"])]
    tui: bool,
    /// Expose the generic ATT mailbox through the host Bluetooth adapter
    #[arg(long, conflicts_with_all = ["once", "raw", "tui"])]
    ble: bool,
    /// BLE local name used by --ble
    #[arg(long, default_value = "PB03FKIT", requires = "ble")]
    ble_name: String,
    /// Generic GATT service UUID used by --ble
    #[arg(long, default_value = DEFAULT_BLE_SERVICE, requires = "ble")]
    ble_service: String,
    /// Generic GATT write characteristic UUID used by --ble
    #[arg(long, default_value = DEFAULT_BLE_RX, requires = "ble")]
    ble_rx: String,
    /// Generic GATT notify characteristic UUID used by --ble
    #[arg(long, default_value = DEFAULT_BLE_TX, requires = "ble")]
    ble_tx: String,
    /// Fault on unmodeled SoC MMIO or vendor ROM accesses
    #[arg(long = "strict", visible_alias = "strict-mmio")]
    strict_mmio: bool,
    #[arg(long)]
    max_insns: Option<u64>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Shared World with one or more PHY6252 firmware nodes
    Sim(SimCli),
    /// Execute a managed PLC artifact in its native runtime
    Plc(PlcCli),
    /// List built-in Worlds
    Worlds,
    /// List board profiles and the SoC each one requires
    Boards,
    /// List SoC models and CPU backends
    Socs,
    /// List managed controller targets
    Controllers,
}

#[derive(Parser)]
struct PlcCli {
    /// Controller program artifact, e.g. Saturn `.fbdbin`
    program: PathBuf,
    /// Managed controller target
    #[arg(long, value_enum, default_value_t = ControllerKind::SaturnPlc)]
    controller: ControllerKind,
    /// Input assignment, e.g. AI1=450 or DI1=1 (repeat)
    #[arg(long = "input", value_name = "TERMINAL=VALUE")]
    inputs: Vec<String>,
    /// Setpoint assignment by HMI index, e.g. 0=450 (repeat)
    #[arg(long = "setpoint", value_name = "INDEX=VALUE")]
    setpoints: Vec<String>,
    /// Runtime cycle period in milliseconds
    #[arg(long, default_value_t = 10)]
    period_ms: u32,
    /// Number of timed cycles after the initial zero-time evaluation
    #[arg(long, default_value_t = 1)]
    steps: u32,
    /// Keep emulated NVRAM when reloading inside the same process
    #[arg(long)]
    preserve_nvram: bool,
    /// Machine-readable line protocol
    #[arg(long)]
    raw: bool,
}

#[derive(Parser)]
struct SimCli {
    /// Node: id[@x,y]=firmware.hex (repeat). Default pose is 3 m apart on X.
    #[arg(long = "node", value_name = "SPEC")]
    nodes: Vec<String>,
    /// Board profile used by every node in this run
    #[arg(long, value_enum, default_value_t = BoardKind::Pb03fKit)]
    board: BoardKind,
    /// Built-in World: crowd, still, mesh
    #[arg(long)]
    world: Option<String>,
    /// Wrap the World timeline (implied unless --once)
    #[arg(long = "loop")]
    looping: bool,
    /// Finite World ticks, no sleep (for scripts / CI)
    #[arg(long)]
    once: bool,
    /// Tick count used with --once
    #[arg(long, default_value_t = 2000)]
    ticks: u32,
    /// Machine line protocol, tagged `[id]` when several chips run
    #[arg(long)]
    raw: bool,
    /// Realtime dashboard (one chip only)
    #[arg(long, conflicts_with_all = ["once", "raw"])]
    tui: bool,
    /// Fault on unmodeled SoC MMIO or vendor ROM accesses
    #[arg(long = "strict", visible_alias = "strict-mmio")]
    strict: bool,
    #[arg(long)]
    max_insns: Option<u64>,
    /// Firmware used as a node when --node is omitted
    hex: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run_cli() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}

fn run_cli() -> Result<ExitCode, String> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Worlds) => {
            sim::print_worlds();
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Boards) => {
            print_boards();
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Socs) => {
            print_socs();
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Controllers) => {
            print_controllers();
            return Ok(ExitCode::SUCCESS);
        }
        Some(Command::Plc(plc_cli)) => return run_plc(plc_cli),
        Some(Command::Sim(sim_cli)) => return run_sim(sim_cli),
        None => {}
    }

    match board_profile(cli.board).soc {
        SocKind::Phy6252 => run_phy6252(cli),
        SocKind::GenericCortexM4 => run_generic_cortex_m4(cli),
        kind => {
            soc::require_implemented(kind)?;
            Err(format!(
                "no runtime composition exists for SoC {}",
                kind.id()
            ))
        }
    }
}

fn run_phy6252(cli: Cli) -> Result<ExitCode, String> {
    require_phy6252(cli.board)?;
    let hex = match cli.hex {
        Some(path) => path,
        None => default_hex()?,
    };

    if cli.tui {
        return tui::run(TuiOpts {
            hex,
            board: cli.board,
            strict: cli.strict_mmio,
            max_insns: cli.max_insns.unwrap_or(50_000_000),
            argv: Vec::new(),
        });
    }

    if cli.ble {
        return ble_host::run(BleHostOpts {
            hex,
            strict: cli.strict_mmio,
            max_insns: cli.max_insns.unwrap_or(50_000_000),
            name: cli.ble_name,
            service: cli.ble_service,
            rx_uuid: cli.ble_rx,
            tx_uuid: cli.ble_tx,
        });
    }

    let live = !cli.once;
    let max_insns = cli
        .max_insns
        .unwrap_or(if live { 50_000_000 } else { 2_000_000 });
    run(RunOpts {
        hex,
        board: cli.board,
        live,
        raw: cli.raw,
        strict_mmio: cli.strict_mmio,
        max_insns,
    })
}

fn run_generic_cortex_m4(cli: Cli) -> Result<ExitCode, String> {
    if !cli.once {
        return Err("generic Cortex-M probes require --once".into());
    }
    if cli.tui || cli.ble {
        return Err("generic Cortex-M probes do not expose PHY6252 TUI/BLE frontends".into());
    }
    let hex = cli
        .hex
        .ok_or_else(|| "generic Cortex-M probes require an Intel HEX image".to_string())?;
    cortex_m::run(ProbeOpts {
        hex,
        board: cli.board,
        strict: cli.strict_mmio,
        max_insns: cli.max_insns.unwrap_or(100_000_000),
    })
}

fn parse_assignment(spec: &str) -> Result<(&str, i32), String> {
    let (name, raw) = spec
        .split_once('=')
        .ok_or_else(|| format!("expected NAME=VALUE, got {spec:?}"))?;
    if name.trim().is_empty() {
        return Err("assignment name must not be empty".into());
    }
    let value = raw
        .trim()
        .parse::<i32>()
        .map_err(|error| format!("invalid value in {spec:?}: {error}"))?;
    Ok((name.trim(), value))
}

#[cfg(firmverse_saturn_native)]
fn run_plc(cli: PlcCli) -> Result<ExitCode, String> {
    let profile = controller::profile(cli.controller);
    if !profile.native_execution {
        return Err(format!(
            "controller {} has no native runtime in this build",
            profile.id
        ));
    }
    let bytes = std::fs::read(&cli.program)
        .map_err(|error| format!("{}: {error}", cli.program.display()))?;
    let mut plc = SaturnPlc::load(&bytes, !cli.preserve_nvram)?;

    for assignment in &cli.inputs {
        let (terminal, value) = parse_assignment(assignment)?;
        plc.set_input(terminal, value)?;
    }
    for assignment in &cli.setpoints {
        let (index, value) = parse_assignment(assignment)?;
        let index = index
            .parse::<usize>()
            .map_err(|error| format!("invalid setpoint index {index:?}: {error}"))?;
        plc.set_setpoint(index, value)?;
    }

    plc.step(0)?;
    for _ in 0..cli.steps {
        plc.step(cli.period_ms)?;
    }

    let project = plc.project();
    if cli.raw {
        println!("READY");
        println!(
            "CONTROLLER {} runtime={} artifact={}",
            profile.id,
            profile.runtime.id(),
            profile.artifact
        );
        println!(
            "PROGRAM elements={} ram={} rtl={} screens={} modbus={}",
            plc.info().element_count,
            plc.memory_bytes(),
            plc.info().required_rtl,
            plc.info().screen_count,
            u8::from(plc.info().uses_modbus)
        );
        println!(
            "PROJECT {:?} {:?} {:?}",
            project.name, project.version, project.build_time
        );
        for point in plc.setpoints() {
            println!(
                "SP {} value={} low={} high={} divider={} step={} {:?}",
                point.index,
                point.value,
                point.low,
                point.high,
                point.divider,
                point.step,
                point.caption
            );
        }
        for point in plc.watchpoints() {
            println!(
                "WP {} value={} divider={} {:?}",
                point.index, point.value, point.divider, point.caption
            );
        }
        for (terminal, value) in plc.outputs() {
            println!("OUT {terminal} {value}");
        }
    } else {
        println!("{} · {}", profile.name, profile.runtime.id());
        println!("  program: {}", cli.program.display());
        println!(
            "  project: {} · {} · {}",
            project.name, project.version, project.build_time
        );
        println!(
            "  FBD: {} elements · {} B RAM · RTL {} · {} screens · {} SP · {} WP{}",
            plc.info().element_count,
            plc.memory_bytes(),
            plc.info().required_rtl,
            plc.info().screen_count,
            plc.info().setpoint_count,
            plc.info().watchpoint_count,
            if plc.info().uses_modbus {
                " · Modbus"
            } else {
                ""
            }
        );
        println!("  setpoints:");
        for point in plc.setpoints() {
            println!(
                "    [{}] {} = {} ({}..{}, step {})",
                point.index, point.caption, point.value, point.low, point.high, point.step
            );
        }
        println!("  watchpoints:");
        for point in plc.watchpoints() {
            println!("    [{}] {} = {}", point.index, point.caption, point.value);
        }
        println!("  outputs:");
        for (terminal, value) in plc.outputs() {
            println!("    {terminal:<4} {value}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(firmverse_saturn_native))]
fn run_plc(cli: PlcCli) -> Result<ExitCode, String> {
    let profile = controller::profile(cli.controller);
    Err(format!(
        "controller {} uses {} but native execution is unavailable in this build",
        profile.id,
        profile.runtime.id()
    ))
}

fn print_boards() {
    for board in PROFILES {
        let soc = soc::profile(board.soc);
        println!(
            "{:<20} soc={:<18} {}{}",
            board.id,
            soc.id,
            board.name,
            if soc.implemented {
                ""
            } else {
                " [SoC unavailable in this build]"
            }
        );
        println!("  {}", board.description);
    }
}

fn print_socs() {
    for soc in soc::PROFILES {
        println!(
            "{:<18} cpu={:<24} {}{}",
            soc.id,
            soc.cpu.label(),
            soc.name,
            if soc.implemented {
                ""
            } else {
                " [unavailable in this build]"
            }
        );
        println!("  {}", soc.description);
    }
    let zmu = soc::ZMU_CORTEX_M_PROFILES
        .iter()
        .map(|profile| profile.id())
        .collect::<Vec<_>>()
        .join(", ");
    println!("zmu Cortex-M profiles: {zmu}");
}

fn print_controllers() {
    for profile in controller::PROFILES {
        println!(
            "{:<18} runtime={:<18} artifact={:<8} {}{}",
            profile.id,
            profile.runtime.id(),
            profile.artifact,
            profile.name,
            if profile.native_execution {
                ""
            } else {
                " [native runtime unavailable in this build]"
            }
        );
        println!("  {}", profile.description);
    }
}

fn run_sim(cli: SimCli) -> Result<ExitCode, String> {
    require_phy6252(cli.board)?;
    let mut nodes = sim::collect_nodes(&cli.nodes, cli.hex)?;
    for node in &mut nodes {
        node.board = cli.board;
    }
    let world = cli
        .world
        .unwrap_or_else(|| sim::default_world(nodes.len()).to_string());
    let live = !cli.once;
    let looping = cli.looping || live;
    let max_insns = cli.max_insns.unwrap_or(if live {
        50_000_000
    } else {
        u64::from(cli.ticks) * 8_000 + 1_000_000
    });
    let opts = sim::SimOpts {
        nodes,
        world,
        looping,
        live,
        ticks: cli.ticks,
        raw: cli.raw,
        strict: cli.strict,
        max_insns,
    };
    if cli.tui {
        return tui::run(sim::tui_opts(&opts)?);
    }
    sim::run(opts)
}
