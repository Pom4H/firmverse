//! PHY6252 package-pad metadata used by frontends and command parsing.
//!
//! Pad-to-AP_GPIO and ADC channel mapping is a SoC/package fact. Board profiles
//! may attach meanings such as LED colours or Restore to these pads, but they
//! must not redefine this table.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pin {
    pub label: &'static str,
    pub gpio_bit: u32,
    pub adc_channel: Option<usize>,
}

pub const PINS: &[Pin] = &[
    Pin {
        label: "P0",
        gpio_bit: 0,
        adc_channel: None,
    },
    Pin {
        label: "P2",
        gpio_bit: 2,
        adc_channel: None,
    },
    Pin {
        label: "P3",
        gpio_bit: 3,
        adc_channel: None,
    },
    Pin {
        label: "P7",
        gpio_bit: 4,
        adc_channel: None,
    },
    Pin {
        label: "P11",
        gpio_bit: 7,
        adc_channel: None,
    },
    Pin {
        label: "P14",
        gpio_bit: 8,
        adc_channel: None,
    },
    Pin {
        label: "P15",
        gpio_bit: 9,
        adc_channel: Some(1),
    },
    Pin {
        label: "P16",
        gpio_bit: 10,
        adc_channel: None,
    },
    Pin {
        label: "P17",
        gpio_bit: 11,
        adc_channel: None,
    },
    Pin {
        label: "P18",
        gpio_bit: 12,
        adc_channel: None,
    },
    Pin {
        label: "P20",
        gpio_bit: 13,
        adc_channel: Some(0),
    },
    Pin {
        label: "P23",
        gpio_bit: 14,
        adc_channel: Some(3),
    },
    Pin {
        label: "P24",
        gpio_bit: 15,
        adc_channel: Some(2),
    },
    Pin {
        label: "P31",
        gpio_bit: 19,
        adc_channel: None,
    },
    Pin {
        label: "P32",
        gpio_bit: 20,
        adc_channel: None,
    },
    Pin {
        label: "P33",
        gpio_bit: 21,
        adc_channel: None,
    },
    Pin {
        label: "P34",
        gpio_bit: 22,
        adc_channel: None,
    },
];

pub fn by_label(label: &str) -> Option<Pin> {
    PINS.iter()
        .copied()
        .find(|pin| pin.label.eq_ignore_ascii_case(label))
}

pub fn gpio_bit(label: &str) -> Option<u32> {
    by_label(label).map(|pin| pin.gpio_bit)
}

pub fn adc_pin(channel: usize) -> Option<Pin> {
    PINS.iter()
        .copied()
        .find(|pin| pin.adc_channel == Some(channel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_mapping_matches_phy6252_contract() {
        assert_eq!(gpio_bit("P15"), Some(9));
        assert_eq!(gpio_bit("p34"), Some(22));
        assert_eq!(adc_pin(0).map(|pin| pin.label), Some("P20"));
        assert_eq!(adc_pin(1).map(|pin| pin.label), Some("P15"));
        assert_eq!(adc_pin(2).map(|pin| pin.label), Some("P24"));
        assert_eq!(adc_pin(3).map(|pin| pin.label), Some("P23"));
    }
}
