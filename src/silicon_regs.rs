//! Exact PHY6252 MMIO registers whose state is deliberately modeled as R/W storage.
//!
//! Keep this file address-by-address. A register only belongs here after its identity and
//! reset value are established from the pinned SDK or real firmware. Behavioral peripherals
//! stay in their dedicated models instead of being hidden behind broad MMIO windows.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageReg {
    pub addr: u32,
    pub name: &'static str,
    pub reset: u32,
}

pub const AP_CACHE_CTRL0: u32 = 0x4000_C000;
pub const AP_CACHE_CTRL1: u32 = 0x4000_C004;

const STORAGE_REGS: &[StorageReg] = &[
    StorageReg {
        addr: AP_CACHE_CTRL0,
        name: "CACHE.CTRL0",
        reset: 0,
    },
    StorageReg {
        addr: AP_CACHE_CTRL1,
        name: "CACHE.CTRL1",
        reset: 0,
    },
];

pub fn storage_reg(addr: u32) -> Option<StorageReg> {
    let aligned = addr & !3;
    STORAGE_REGS.iter().copied().find(|reg| reg.addr == aligned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_control_registers_are_exact_not_a_window() {
        assert_eq!(storage_reg(AP_CACHE_CTRL0).unwrap().reset, 0);
        assert_eq!(storage_reg(AP_CACHE_CTRL1).unwrap().reset, 0);
        assert!(storage_reg(0x4000_C008).is_none());
        assert!(storage_reg(0x4000_C800).is_none());
    }
}
