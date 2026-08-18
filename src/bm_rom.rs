use std::cell::RefCell;
use std::collections::HashMap;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_HCI_BM_ALLOC: u32 = 0x0000_28E8;
const ROM_OSAL_BM_ADJUST_HEADER: u32 = 0x0001_4954;
const ROM_OSAL_BM_ADJUST_TAIL: u32 = 0x0001_497C;
const ROM_OSAL_BM_ALLOC: u32 = 0x0001_49A8;
const ROM_OSAL_BM_FREE: u32 = 0x0001_49D8;
const ROM_OSAL_MEM_ALLOC: u32 = 0x0001_4B3C;
const ROM_OSAL_MEM_FREE: u32 = 0x0001_4C00;

thread_local! {
    static BASE_BY_VIEW: RefCell<HashMap<u32, u32>> = RefCell::new(HashMap::new());
}

/// Handle PHY6252 ROM buffer-manager ABI.
///
/// Allocation/free reuse the single HostOsal heap by redirecting to the
/// already-modelled osal_mem_alloc/free entries. Header adjustment follows
/// the SDK contract: positive size adds headroom (pointer moves backward),
/// negative size removes header bytes (pointer moves forward).
pub fn handle(cpu: &mut Processor) -> bool {
    match cpu.get_pc() {
        ROM_HCI_BM_ALLOC | ROM_OSAL_BM_ALLOC => {
            cpu.set_pc(ROM_OSAL_MEM_ALLOC);
            false
        }
        ROM_OSAL_BM_FREE => free(cpu),
        ROM_OSAL_BM_ADJUST_HEADER => adjust_header(cpu),
        ROM_OSAL_BM_ADJUST_TAIL => adjust_tail(cpu),
        _ => false,
    }
}

fn adjust_header(cpu: &mut Processor) -> bool {
    let ptr = cpu.get_r(Reg::R0);
    let size = cpu.get_r(Reg::R1) as u16 as i16 as i32;
    let base = BASE_BY_VIEW.with(|views| views.borrow().get(&ptr).copied().unwrap_or(ptr));
    let adjusted = if size >= 0 {
        ptr.wrapping_sub(size as u32)
    } else {
        ptr.wrapping_add((-size) as u32)
    };
    BASE_BY_VIEW.with(|views| {
        let mut views = views.borrow_mut();
        views.insert(ptr, base);
        views.insert(adjusted, base);
    });
    cpu.set_r(Reg::R0, adjusted);
    ret(cpu);
    true
}

fn adjust_tail(cpu: &mut Processor) -> bool {
    // Tail adjustment changes the usable extent but never the payload base.
    ret(cpu);
    true
}

fn free(cpu: &mut Processor) -> bool {
    let ptr = cpu.get_r(Reg::R0);
    let base = BASE_BY_VIEW.with(|views| {
        let mut views = views.borrow_mut();
        let base = views.get(&ptr).copied().unwrap_or(ptr);
        views.retain(|_, candidate| *candidate != base);
        base
    });
    cpu.set_r(Reg::R0, base);
    cpu.set_pc(ROM_OSAL_MEM_FREE);
    false
}

fn ret(cpu: &mut Processor) {
    cpu.set_pc(cpu.get_r(Reg::LR) & !1);
}

#[cfg(test)]
mod tests {
    #[test]
    fn sdk_header_adjustment_direction() {
        let ptr = 0x2000_1000u32;
        assert_eq!(ptr.wrapping_sub(4), 0x2000_0ffc);
        assert_eq!(ptr.wrapping_add(4), 0x2000_1004);
    }
}
