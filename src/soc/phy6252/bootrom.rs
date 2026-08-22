//! Minimal PHY6252 ROM UART programmer state machine.
//!
//! This models the externally observable part of the ROM path used by the
//! vendor/pvvx flashing tools: after a system reset the ROM listens on UART0
//! at 9600 baud for `UXTDWU`, replies `cmd>>:`, then uses 115200 baud for the
//! command monitor. It intentionally does not pretend to be a dumped ROM
//! image or a complete programmer implementation.

use std::collections::VecDeque;

pub const ENTRY_BAUD: u32 = 9_600;
pub const COMMAND_BAUD: u32 = 115_200;
const ENTRY_MAGIC: &[u8] = b"UXTDWU";
const RESET_COMMAND: &[u8] = b"reset";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Application,
    AwaitSync { matched: usize },
    Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    RunApplication,
}

pub struct BootRom {
    state: State,
    tx: VecDeque<(u32, Vec<u8>)>,
    command_buf: Vec<u8>,
}

impl Default for BootRom {
    fn default() -> Self {
        Self {
            state: State::Application,
            tx: VecDeque::new(),
            command_buf: Vec::new(),
        }
    }
}

impl BootRom {
    pub fn enter_after_system_reset(&mut self) {
        self.state = State::AwaitSync { matched: 0 };
        self.tx.clear();
        self.command_buf.clear();
    }

    pub fn active(&self) -> bool {
        self.state != State::Application
    }

    pub fn feed_uart0(&mut self, baud: u32, bytes: &[u8]) -> Action {
        if let State::AwaitSync { matched } = &mut self.state {
            if baud != ENTRY_BAUD {
                return Action::None;
            }
            let mut entered = false;
            for &byte in bytes {
                if byte == ENTRY_MAGIC[*matched] {
                    *matched += 1;
                } else {
                    *matched = usize::from(byte == ENTRY_MAGIC[0]);
                }
                if *matched == ENTRY_MAGIC.len() {
                    entered = true;
                    break;
                }
            }
            if entered {
                self.state = State::Command;
                self.tx.push_back((ENTRY_BAUD, b"cmd>>:".to_vec()));
            }
            return Action::None;
        }

        if self.state != State::Command || baud != COMMAND_BAUD {
            return Action::None;
        }

        self.command_buf.extend_from_slice(bytes);
        if self
            .command_buf
            .windows(RESET_COMMAND.len())
            .any(|window| window == RESET_COMMAND)
        {
            self.state = State::Application;
            self.command_buf.clear();
            return Action::RunApplication;
        }

        // The monitor command parser is deliberately tiny for now. Keep enough
        // tail bytes for commands split across host writes without letting an
        // unsupported command stream grow forever.
        if self.command_buf.len() > 64 {
            let keep = RESET_COMMAND.len().saturating_sub(1);
            let drain = self.command_buf.len().saturating_sub(keep);
            self.command_buf.drain(..drain);
        }
        Action::None
    }

    pub fn take_tx(&mut self) -> Option<(u32, Vec<u8>)> {
        self.tx.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_requires_9600_and_accepts_split_magic() {
        let mut rom = BootRom::default();
        rom.enter_after_system_reset();
        assert_eq!(rom.feed_uart0(COMMAND_BAUD, b"UXTDWU"), Action::None);
        assert!(rom.take_tx().is_none());
        assert_eq!(rom.feed_uart0(ENTRY_BAUD, b"UXT"), Action::None);
        assert_eq!(rom.feed_uart0(ENTRY_BAUD, b"DWU"), Action::None);
        assert_eq!(rom.take_tx(), Some((ENTRY_BAUD, b"cmd>>:".to_vec())));
        assert!(rom.active());
    }

    #[test]
    fn repeated_stream_can_resynchronize_after_noise() {
        let mut rom = BootRom::default();
        rom.enter_after_system_reset();
        rom.feed_uart0(ENTRY_BAUD, b"xxUUXTDWUUXTDWU");
        assert_eq!(rom.take_tx(), Some((ENTRY_BAUD, b"cmd>>:".to_vec())));
    }

    #[test]
    fn command_mode_reset_returns_to_application() {
        let mut rom = BootRom::default();
        rom.enter_after_system_reset();
        rom.feed_uart0(ENTRY_BAUD, b"UXTDWU");
        let _ = rom.take_tx();
        assert_eq!(rom.feed_uart0(COMMAND_BAUD, b"res"), Action::None);
        assert_eq!(rom.feed_uart0(COMMAND_BAUD, b"et "), Action::RunApplication);
        assert!(!rom.active());
    }
}
