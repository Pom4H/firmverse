//! Cortex-M system-reset interception for the PHY6252 SoC model.
//!
//! zmu handles the architectural SCB, but Firmverse needs the PHY6252-level
//! consequence of AIRCR.SYSRESETREQ: leave the running application and expose
//! the chip ROM boot path. The wrapper claims only SCB.AIRCR and delegates the
//! rest of the address space unchanged.

use crate::discovery::DiscoveryBus;
use std::cell::Cell;
use std::rc::Rc;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::fault::Fault;

pub const SCB_AIRCR: u32 = 0xE000_ED0C;
const AIRCR_VECTKEY_MASK: u32 = 0xFFFF_0000;
const AIRCR_VECTKEY: u32 = 0x05FA_0000;
const AIRCR_SYSRESETREQ: u32 = 1 << 2;

pub struct ResetAwareBus {
    inner: DiscoveryBus,
    reset_requested: Rc<Cell<bool>>,
}

impl ResetAwareBus {
    pub fn new(inner: DiscoveryBus, reset_requested: Rc<Cell<bool>>) -> Self {
        Self {
            inner,
            reset_requested,
        }
    }
}

impl Bus for ResetAwareBus {
    fn read32(&mut self, addr: u32) -> Result<u32, Fault> {
        if addr == SCB_AIRCR {
            return Ok(0);
        }
        self.inner.read32(addr)
    }

    fn read16(&self, addr: u32) -> Result<u16, Fault> {
        self.inner.read16(addr)
    }

    fn read8(&self, addr: u32) -> Result<u8, Fault> {
        self.inner.read8(addr)
    }

    fn write32(&mut self, addr: u32, value: u32) -> Result<(), Fault> {
        if addr == SCB_AIRCR {
            let key_ok = value & AIRCR_VECTKEY_MASK == AIRCR_VECTKEY;
            if key_ok && value & AIRCR_SYSRESETREQ != 0 {
                self.reset_requested.set(true);
            }
            return Ok(());
        }
        self.inner.write32(addr, value)
    }

    fn write16(&mut self, addr: u32, value: u16) -> Result<(), Fault> {
        self.inner.write16(addr, value)
    }

    fn write8(&mut self, addr: u32, value: u8) -> Result<(), Fault> {
        self.inner.write8(addr, value)
    }

    fn in_range(&self, addr: u32) -> bool {
        addr == SCB_AIRCR || self.inner.in_range(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{Phy6252Bus, SRAM_SIZE, XIP_SIZE};
    use zmu_cortex_m::Processor;

    fn bus(flag: Rc<Cell<bool>>) -> ResetAwareBus {
        ResetAwareBus::new(
            DiscoveryBus::new(
                Phy6252Bus::new(vec![0; SRAM_SIZE], vec![0xff; XIP_SIZE]),
                false,
            ),
            flag,
        )
    }

    #[test]
    fn sysresetreq_with_vectkey_is_latched() {
        let flag = Rc::new(Cell::new(false));
        let mut bus = bus(Rc::clone(&flag));
        bus.write32(SCB_AIRCR, AIRCR_VECTKEY | AIRCR_SYSRESETREQ)
            .unwrap();
        assert!(flag.get());
    }

    #[test]
    fn sysresetreq_without_vectkey_is_ignored() {
        let flag = Rc::new(Cell::new(false));
        let mut bus = bus(Rc::clone(&flag));
        bus.write32(SCB_AIRCR, AIRCR_SYSRESETREQ).unwrap();
        assert!(!flag.get());
    }

    #[test]
    fn processor_routes_aircr_write_to_soc_wrapper() {
        let flag = Rc::new(Cell::new(false));
        let mut processor = Processor::new();
        processor.device(Some(Box::new(bus(Rc::clone(&flag)))));
        processor
            .write32(SCB_AIRCR, AIRCR_VECTKEY | AIRCR_SYSRESETREQ)
            .unwrap();
        assert!(flag.get());
    }
}
