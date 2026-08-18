#![allow(dead_code)]

mod aes;
mod arm_abi;
mod ble_host;
mod ble_rom;
mod bm_rom;
mod board;
mod bus;
mod cbtimer_rom;
mod chip;
mod cmd;
mod discovery;
mod dma_engine;
mod emu;
mod flash_state;
mod hci_caps;
mod hci_extra;
mod hci_rom;
mod hci_security;
mod hci_task;
mod hex;
mod ll_crypto;
mod ll_rom;
mod mailbox;
mod osal;
mod osal_power;
mod osal_queue;
mod silicon_regs;
mod sim;
mod tui;
mod world;

use ble_host::BleHostOpts;
use board::{profile, require_phy6252, BoardKind, PROFILES};
use clap::{Parser, Subcommand};
use emu::{default_hex, run, RunOpts};
use std::path::PathBuf;
use std::process::ExitCode;
use tui::TuiOpts;

const DEFAULT_BLE_SERVICE: &str = "6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001";
const DEFAULT_BLE_RX: &str = "6B1D0002-7C8E-4A91-9F2B-E3A14C5B0001";
const DEFAULT_BLE_TX: &str = "6B1D0003-7C8E-4A91-9F2B-E3A14C5B0001";

#[derive(Parser)]
#[command(
    name = "phy6252",
    version,
    about = "PHY6252 emulator with pluggable board profiles",
    after_help = "Live REPL is the default. `phy6252 sim` runs a shared RF world with one or more chips (mesh). Use --board headless to remove PB-03F-Kit wiring assumptions. `phy6252 boards` lists known board profiles.",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Cli {
    /// Intel HEX image
    hex: Option<PathBuf>,
    /// Board wiring/profile layered above the PHY6252 SoC
    #[arg(long, value_enum, default_value_t = BoardKind::Pb03fKit)]
    board: BoardKind,
    /// Run until halt or --max-insns, no REPL
    #[arg(long)]
    once: bool,
    /// Machine line protocol (GPIO / UART / FRAME)
    #[arg(long)]
    raw: bool,
    /// Realtime terminal dashboard with live pinout, ADC/PWM/BLE state and logs
    #[arg(long, conflicts_with_all = ["once", "raw", "ble"])]
    tui: bool,
    /// Expose the generic ATT mailbox through the host Bluetooth adapter (Linux BlueZ or macOS)
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
    /// Fault on unmodeled PHY6252 MMIO or vendor ROM accesses
    #[arg(long = "strict", visible_alias = "strict-mmio")]
    strict_mmio: bool,
    #[arg(long)]
    max_insns: Option<u64>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Shared RF world with one or more chips (each with its own firmware)
    Sim(SimCli),
    /// List built-in RF worlds
    Worlds,
    /// List board profiles and the SoC each one requires
    Boards,
}

#[derive(Parser)]
struct SimCli {
    /// Node: id[@x,y]=firmware.hex (repeat). Default pose is 3 m apart on X.
    #[arg(long = "node", value_name = "SPEC")]
    nodes: Vec<String>,
    /// Built-in world: crowd, still, mesh
    #[arg(long)]
    world: Option<String>,
    /// Wrap the world timeline (implied unless --once)
    #[arg(long = "loop")]
    looping: bool,
    /// Finite world ticks, no sleep (for scripts / CI)
    #[arg(long)]
    once: bool,
    /// Tick count used with --once
    #[arg(long, default_value_t = 2000)]
    ticks: u32,
    /// Machine line protocol (GPIO / UART / FRAME), tagged `[id]` when several chips run
    #[arg(long)]
    raw: bool,
    /// Realtime dashboard (one chip only)
    #[arg(long, conflicts_with_all = ["once", "raw"])]
    tui: bool,
    /// Fault on unmodeled PHY6252 MMIO or vendor ROM accesses
    #[arg(long = "strict", visible_alias = "strict-mmio")]
    strict: bool,
    #[arg(long)]
    max_insns: Option<u64>,
    /// HEX used as a node when --node is omitted
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
        Some(Command::Sim(sim_cli)) => return run_sim(sim_cli),
        None => {}
    }

    let board = require_phy6252(cli.board)?;
    let hex = match cli.hex {
        Some(path) => path,
        None => default_hex()?,
    };

    if cli.tui {
        if cli.board != BoardKind::Pb03fKit {
            return Err(format!(
                "TUI pinout currently models {}; use --raw for board {} until the TUI consumes BoardProfile",
                profile(BoardKind::Pb03fKit).name,
                board.id
            ));
        }
        return tui::run(TuiOpts {
            hex,
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

fn print_boards() {
    for board in PROFILES {
        println!(
            "{:<14} soc={:<8} {}{}",
            board.id,
            board.soc.id(),
            board.name,
            if board.soc == board::SocKind::Phy6252 {
                ""
            } else {
                " [SoC not implemented]"
            }
        );
        println!("  {}", board.description);
    }
}

fn run_sim(cli: SimCli) -> Result<ExitCode, String> {
    let nodes = sim::collect_nodes(&cli.nodes, cli.hex)?;
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
