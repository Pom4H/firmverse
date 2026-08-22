#![allow(dead_code)]
// Rust 1.98 started preferring `as_chunks` for constant-size slices. Keep the
// portable iterator form here instead of raising Firmverse's Rust API floor
// just to satisfy a toolchain-style lint.
#![allow(clippy::chunks_exact_to_as_chunks)]

// PHY6252 is physically grouped below src/soc/phy6252 while its internal
// modules still use the historical crate-level names. Keeping that migration
// shim in the library makes every frontend (CLI and browser) execute the exact
// same SoC implementation.
#[path = "soc/phy6252/aes.rs"]
mod aes;
#[path = "soc/phy6252/arm_abi.rs"]
mod arm_abi;
#[path = "soc/phy6252/ble_rom.rs"]
mod ble_rom;
#[path = "soc/phy6252/bm_rom.rs"]
mod bm_rom;
#[path = "soc/phy6252/bus.rs"]
mod bus;
#[path = "soc/phy6252/cbtimer_rom.rs"]
mod cbtimer_rom;
#[path = "soc/phy6252/chip.rs"]
mod chip;
#[path = "soc/phy6252/cmd.rs"]
mod cmd;
#[path = "soc/phy6252/discovery.rs"]
mod discovery;
#[path = "soc/phy6252/dma_engine.rs"]
mod dma_engine;
#[path = "soc/phy6252/flash_state.rs"]
mod flash_state;
#[path = "soc/phy6252/hci_caps.rs"]
mod hci_caps;
#[path = "soc/phy6252/hci_extra.rs"]
mod hci_extra;
#[path = "soc/phy6252/hci_rom.rs"]
mod hci_rom;
#[path = "soc/phy6252/hci_security.rs"]
mod hci_security;
#[path = "soc/phy6252/hci_task.rs"]
mod hci_task;
#[path = "soc/phy6252/ll_crypto.rs"]
mod ll_crypto;
#[path = "soc/phy6252/ll_rom.rs"]
mod ll_rom;
#[path = "soc/phy6252/mailbox.rs"]
mod mailbox;
#[path = "soc/phy6252/osal.rs"]
mod osal;
#[path = "soc/phy6252/osal_power.rs"]
mod osal_power;
#[path = "soc/phy6252/osal_queue.rs"]
mod osal_queue;
#[path = "soc/phy6252/silicon_regs.rs"]
mod silicon_regs;

pub mod board;
pub mod hex;
pub mod soc;
pub mod web_runtime;
pub mod world;

#[cfg(not(target_arch = "wasm32"))]
pub mod ble_host;
#[cfg(not(target_arch = "wasm32"))]
pub mod emu;
#[cfg(not(target_arch = "wasm32"))]
pub mod sim;
#[cfg(not(target_arch = "wasm32"))]
pub mod tui;
