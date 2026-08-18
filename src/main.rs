mod aes;
mod arm_abi;
mod ble_host;
mod ble_rom;
mod bm_rom;
mod bus;
mod cbtimer_rom;
mod cmd;
mod discovery;
mod emu;
mod hci_caps;
mod hci_extra;
mod hci_rom;
mod hci_security;
mod hex;
mod ll_crypto;
mod ll_rom;
mod mailbox;
mod osal;
mod osal_power;
mod silicon_regs;
mod tui;

use ble_host::BleHostOpts;
use clap::Parser;
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
    about = "PHY6252 / PB-03F-Kit emulator",
    after_help = "Live REPL is the default. Use --tui for a realtime pinout + logs dashboard or --ble to bridge the generic ATT mailbox to Linux BlueZ."
)]
struct Cli {
    /// Intel HEX image
    hex: Option<PathBuf>,
    /// Run until halt or --max-insns, no REPL
    #[arg(long)]
    once: bool,
    /// Machine line protocol (GPIO / UART / FRAME)
    #[arg(long)]
    raw: bool,
    /// Realtime terminal dashboard with live pinout, ADC/PWM/BLE state and logs
    #[arg(long, conflicts_with_all = ["once", "raw", "ble"])]
    tui: bool,
    /// Expose the generic ATT mailbox through the Linux host Bluetooth adapter (BlueZ)
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
    let hex = match cli.hex {
        Some(path) => path,
        None => default_hex()?,
    };

    if cli.tui {
        return tui::run(TuiOpts {
            hex,
            strict: cli.strict_mmio,
            max_insns: cli.max_insns.unwrap_or(50_000_000),
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
    let max_insns = cli.max_insns.unwrap_or(if live { 50_000_000 } else { 2_000_000 });
    run(RunOpts {
        hex,
        live,
        raw: cli.raw,
        strict_mmio: cli.strict_mmio,
        max_insns,
    })
}
