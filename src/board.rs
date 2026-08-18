//! Board profiles live above the emulated SoC.
//!
//! A board may name GPIO-backed indicators and inputs, but it must not own
//! PHY6252 MMIO, ROM ABI, timers, flash, or CPU behavior. Those stay in the
//! SoC layer. This separation also lets the project grow a CH592F SoC without
//! pretending it is another PHY6252 board.

use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocKind {
    Phy6252,
    Ch592f,
}

impl SocKind {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Phy6252 => "phy6252",
            Self::Ch592f => "ch592f",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum BoardKind {
    #[value(name = "pb03f-kit")]
    Pb03fKit,
    #[value(name = "headless")]
    Headless,
    #[value(name = "weact-ch592f")]
    WeactCh592f,
}

impl Default for BoardKind {
    fn default() -> Self {
        Self::Pb03fKit
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpioSignal {
    pub name: &'static str,
    pub gpio_bit: u32,
    pub active_high: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct BoardProfile {
    pub kind: BoardKind,
    pub id: &'static str,
    pub name: &'static str,
    pub soc: SocKind,
    pub description: &'static str,
    pub indicators: &'static [GpioSignal],
}

const PB03F_INDICATORS: &[GpioSignal] = &[
    GpioSignal {
        name: "red",
        gpio_bit: 4,
        active_high: true,
    },
    GpioSignal {
        name: "green",
        gpio_bit: 7,
        active_high: true,
    },
    GpioSignal {
        name: "blue",
        gpio_bit: 12,
        active_high: true,
    },
    GpioSignal {
        name: "yellow",
        gpio_bit: 0,
        active_high: true,
    },
    GpioSignal {
        name: "white",
        gpio_bit: 22,
        active_high: true,
    },
];

pub const PB03F_KIT: BoardProfile = BoardProfile {
    kind: BoardKind::Pb03fKit,
    id: "pb03f-kit",
    name: "AI-Thinker PB-03F-Kit",
    soc: SocKind::Phy6252,
    description: "PHY6252 development kit with five GPIO LEDs and Restore input",
    indicators: PB03F_INDICATORS,
};

pub const HEADLESS_PHY6252: BoardProfile = BoardProfile {
    kind: BoardKind::Headless,
    id: "headless",
    name: "Headless PHY6252",
    soc: SocKind::Phy6252,
    description: "bare PHY6252 SoC with no board-level wiring assumptions",
    indicators: &[],
};

pub const WEACT_CH592F: BoardProfile = BoardProfile {
    kind: BoardKind::WeactCh592f,
    id: "weact-ch592f",
    name: "WeAct Studio CH592F Core Board",
    soc: SocKind::Ch592f,
    description: "future CH592F board profile; CH592F SoC emulation is not implemented yet",
    indicators: &[],
};

pub const PROFILES: &[BoardProfile] = &[PB03F_KIT, HEADLESS_PHY6252, WEACT_CH592F];

pub const fn profile(kind: BoardKind) -> &'static BoardProfile {
    match kind {
        BoardKind::Pb03fKit => &PB03F_KIT,
        BoardKind::Headless => &HEADLESS_PHY6252,
        BoardKind::WeactCh592f => &WEACT_CH592F,
    }
}

pub fn require_phy6252(kind: BoardKind) -> Result<&'static BoardProfile, String> {
    let board = profile(kind);
    if board.soc != SocKind::Phy6252 {
        return Err(format!(
            "board {} requires SoC {}; this binary currently emulates PHY6252 only",
            board.id,
            board.soc.id()
        ));
    }
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
        assert!(profile(BoardKind::Headless).indicators.is_empty());
        assert_eq!(gpio_summary(BoardKind::Headless, 1, 1), "dr=00000001 ddr=00000001");
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
    }
}
