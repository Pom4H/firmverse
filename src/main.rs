use clap::{Parser, Subcommand};
use firmverse::ble_host::{self, BleHostOpts};
use firmverse::board::{profile as board_profile, require_phy6252, BoardKind, PROFILES};
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
    about = "Virtual embedded systems lab for real firmware, SoCs, boards and multi-node worlds",
    after_help = "Live REPL is the default for PHY6252. Generic Cortex-M probe boards require --once. `firmverse sim` runs PHY6252 nodes in a shared World.",
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
    /// List built-in Worlds
    Worlds,
    /// List board profiles and the SoC each one requires
    Boards,
    /// List SoC models and CPU backends
    Socs,
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
