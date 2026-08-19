//! Board profiles live above the emulated SoC.
//!
//! A board may name GPIO-backed indicators and inputs, describe connector
//! layout, and attach human meanings to SoC pads. It must not own SoC MMIO,
//! ROM ABI, timers, flash, CPU behavior, or pad-to-GPIO mapping.

use crate::soc::SocKind;
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum BoardKind {
    #[default]
    #[value(name = "pb03f-kit")]
    Pb03fKit,
    #[value(name = "headless")]
    Headless,
    #[value(name = "weact-ch592f")]
    WeactCh592f,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpioSignal {
    pub name: &'static str,
    pub pin: &'static str,
    pub gpio_bit: u32,
    pub active_high: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectorRow {
    pub left: &'static str,
    pub right: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PinNote {
    pub pin: &'static str,
    pub note: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct BoardProfile {
    pub kind: BoardKind,
    pub id: &'static str,
    pub name: &'static str,
    pub soc: SocKind,
    pub description: &'static str,
    pub indicators: &'static [GpioSignal],
    pub pinout_title: Option<&'static str>,
    pub connector_rows: &'static [ConnectorRow],
    pub pin_notes: &'static [PinNote],
}

impl BoardProfile {
    pub fn pin_note(&self, pin: &str) -> Option<&'static str> {
        self.pin_notes
            .iter()
            .find(|note| note.pin.eq_ignore_ascii_case(pin))
            .map(|note| note.note)
    }

    pub fn indicator_for_pin(&self, pin: &str) -> Option<&'static GpioSignal> {
        self.indicators
            .iter()
            .find(|signal| signal.pin.eq_ignore_ascii_case(pin))
    }
}

const PB03F_INDICATORS: &[GpioSignal] = &[
    GpioSignal {
        name: "red",
        pin: "P7",
        gpio_bit: 4,
        active_high: true,
    },
    GpioSignal {
        name: "green",
        pin: "P11",
        gpio_bit: 7,
        active_high: true,
    },
    GpioSignal {
        name: "blue",
        pin: "P18",
        gpio_bit: 12,
        active_high: true,
    },
    GpioSignal {
        name: "yellow",
        pin: "P0",
        gpio_bit: 0,
        active_high: true,
    },
    GpioSignal {
        name: "white",
        pin: "P34",
        gpio_bit: 22,
        active_high: true,
    },
];

const PB03F_CONNECTOR_ROWS: &[ConnectorRow] = &[
    ConnectorRow {
        left: "P13",
        right: "P24",
    },
    ConnectorRow {
        left: "P11",
        right: "P23",
    },
    ConnectorRow {
        left: "P31",
        right: "P20",
    },
    ConnectorRow {
        left: "P7",
        right: "P3",
    },
    ConnectorRow {
        left: "P32",
        right: "P2",
    },
    ConnectorRow {
        left: "P33",
        right: "3V3",
    },
    ConnectorRow {
        left: "P14",
        right: "GND",
    },
    ConnectorRow {
        left: "P16",
        right: "NC",
    },
    ConnectorRow {
        left: "P17",
        right: "P34",
    },
    ConnectorRow {
        left: "GND",
        right: "P0",
    },
    ConnectorRow {
        left: "3V3",
        right: "P18",
    },
    ConnectorRow {
        left: "NC",
        right: "RX0",
    },
    ConnectorRow {
        left: "NC",
        right: "TX0",
    },
    ConnectorRow {
        left: "GND",
        right: "GND",
    },
    ConnectorRow {
        left: "5V",
        right: "3V3",
    },
];

const PB03F_PIN_NOTES: &[PinNote] = &[
    PinNote {
        pin: "P15",
        note: "Restore",
    },
    PinNote {
        pin: "P13",
        note: "silk only; no gpio_pin_e mapping",
    },
    PinNote {
        pin: "3V3",
        note: "power",
    },
    PinNote {
        pin: "5V",
        note: "power",
    },
    PinNote {
        pin: "GND",
        note: "ground",
    },
    PinNote {
        pin: "NC",
        note: "not connected",
    },
    PinNote {
        pin: "TX0",
        note: "UART0 TX",
    },
    PinNote {
        pin: "RX0",
        note: "UART0 RX",
    },
];

pub const PB03F_KIT: BoardProfile = BoardProfile {
    kind: BoardKind::Pb03fKit,
    id: "pb03f-kit",
    name: "AI-Thinker PB-03F-Kit",
    soc: SocKind::Phy6252,
    description: "PHY6252 development kit with five GPIO LEDs and Restore input",
    indicators: PB03F_INDICATORS,
    pinout_title: Some("PB-03F-Kit bottom view (DIP-30, pin 1 P13 top-left)"),
    connector_rows: PB03F_CONNECTOR_ROWS,
    pin_notes: PB03F_PIN_NOTES,
};

pub const HEADLESS_PHY6252: BoardProfile = BoardProfile {
    kind: BoardKind::Headless,
    id: "headless",
    name: "Headless PHY6252",
    soc: SocKind::Phy6252,
    description: "bare PHY6252 SoC with no board-level wiring assumptions",
    indicators: &[],
    pinout_title: None,
    connector_rows: &[],
    pin_notes: &[],
};

pub const WEACT_CH592F: BoardProfile = BoardProfile {
    kind: BoardKind::WeactCh592f,
    id: "weact-ch592f",
    name: "WeAct Studio CH592F Core Board",
    soc: SocKind::Ch592f,
    description: "CH592F board profile reserved for the future RISC-V SoC backend",
    indicators: &[],
    pinout_title: None,
    connector_rows: &[],
    pin_notes: &[],
};

pub const PROFILES: &[BoardProfile] = &[PB03F_KIT, HEADLESS_PHY6252, WEACT_CH592F];

pub const fn profile(kind: BoardKind) -> &'static BoardProfile {
    match kind {
        BoardKind::Pb03fKit => &PB03F_KIT,
        BoardKind::Headless => &HEADLESS_PHY6252,
        BoardKind::WeactCh592f => &WEACT_CH592F,
    }
}

/// Guard the current execution path, which is still the PHY6252 implementation.
pub fn require_phy6252(kind: BoardKind) -> Result<&'static BoardProfile, String> {
    let board = profile(kind);
    if board.soc != SocKind::Phy6252 {
        let soc = crate::soc::profile(board.soc);
        return Err(format!(
            "board {} requires SoC {} ({}) but this runtime currently executes PHY6252 only",
            board.id,
            soc.id,
            soc.cpu.label()
        ));
    }
    crate::soc::require_implemented(board.soc)?;
    Ok(board)
}

pub fn gpio_summary(kind: BoardKind, dr: u32, ddr: u32) -> String {
    let board = profile(kind);
    if board.indicators.is_empty() {
        return format!("dr={dr:08x} ddr={ddr:08x}");
    }

    let mut active = Vec::new();
    for signal in board.indicators {
        let output = ((ddr >> signal.gpio_bit) & 1) != 0;
        let level = ((dr >> signal.gpio_bit) & 1) != 0;
        let on = if signal.active_high { level } else { !level };
        if output && on {
            active.push(signal.name);
        }
    }

    if active.is_empty() {
        "—".into()
    } else {
        active.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_has_no_pb03f_wiring() {
        let board = profile(BoardKind::Headless);
        assert!(board.indicators.is_empty());
        assert!(board.connector_rows.is_empty());
        assert_eq!(
            gpio_summary(BoardKind::Headless, 1, 1),
            "dr=00000001 ddr=00000001"
        );
    }

    #[test]
    fn pb03f_profile_owns_board_wiring() {
        let board = profile(BoardKind::Pb03fKit);
        assert_eq!(board.connector_rows.len(), 15);
        assert_eq!(
            board.connector_rows[3],
            ConnectorRow {
                left: "P7",
                right: "P3"
            }
        );
        assert_eq!(board.pin_note("P15"), Some("Restore"));
        assert_eq!(
            board.indicator_for_pin("P34").map(|signal| signal.name),
            Some("white")
        );
    }

    #[test]
    fn pb03f_summary_uses_board_signals() {
        let red = 1u32 << 4;
        let white = 1u32 << 22;
        assert_eq!(
            gpio_summary(BoardKind::Pb03fKit, red | white, red | white),
            "red white"
        );
    }

    #[test]
    fn ch592_board_is_not_accepted_by_phy6252_runtime() {
        let err = require_phy6252(BoardKind::WeactCh592f).unwrap_err();
        assert!(err.contains("requires SoC ch592f"));
        assert!(err.contains("qingke-v4c"));
    }
}
