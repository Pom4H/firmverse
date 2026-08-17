mod aes;
mod bus;
mod cmd;
mod discovery;
mod emu;
mod hex;
mod mailbox;

use clap::Parser;
use emu::{default_hex, run, RunOpts};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "phy6252",
    version,
    about = "PHY6252 / PB-03F-Kit emulator",
    after_help = "Live REPL is the default. Type help, connect, write hi, p34 on, quit."
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
