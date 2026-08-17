use std::collections::HashSet;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_OSAL_INIT_SYSTEM: u32 = 0x0001_4AEC;
const ROM_OSAL_MEM_ALLOC: u32 = 0x0001_4B3C;
const ROM_OSAL_MEMSET: u32 = 0x0001_4D14;

// PHY6252 SDK jump_table.c: item 1 is osalInitTasks, item 2 tasksArr,
// item 3 &tasksCnt, item 4 &tasksEvents. The table itself is linked at SRAM0.
const JUMP_TABLE_OSAL_INIT_TASKS: u32 = 0x1FFF_0004;

// DiscoveryBus host-control registers populated by the explicit osal_mem_set_heap shim.
const EMU_HEAP_BASE: u32 = 0x5000_FF30;
const EMU_HEAP_SIZE: u32 = 0x5000_FF34;

#[derive(Debug, Default)]
pub struct HostOsal {
    heap_next: Option<u32>,
    heap_end: u32,
    seen: HashSet<u32>,
}

impl HostOsal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle only ROM ABI that is explicitly backed by guest-visible SDK state.
    /// Returns true when the current PC was consumed and no vendor ROM fetch should occur.
    pub fn handle(&mut self, processor: &mut Processor) -> bool {
        let pc = processor.get_pc();
        match pc {
            ROM_OSAL_INIT_SYSTEM => self.init_system(processor),
            ROM_OSAL_MEM_ALLOC => self.mem_alloc(processor),
            ROM_OSAL_MEMSET => self.memset(processor),
            _ => false,
        }
    }

    fn log_once(&mut self, pc: u32, message: impl FnOnce()) {
        if self.seen.insert(pc) {
            message();
        }
    }

    fn init_system(&mut self, processor: &mut Processor) -> bool {
        let task_init = match processor.read32(JUMP_TABLE_OSAL_INIT_TASKS) {
            Ok(value) if value & 1 == 1 => value,
            Ok(value) => {
                self.log_once(ROM_OSAL_INIT_SYSTEM, || {
                    eprintln!(
                        "OSAL strict osal_init_system: jump_table[1]={value:#010x} is not a Thumb callback"
                    );
                });
                return false;
            }
            Err(fault) => {
                eprintln!("OSAL strict osal_init_system: cannot read jump_table[1]: {fault}");
                return false;
            }
        };

        self.log_once(ROM_OSAL_INIT_SYSTEM, || {
            eprintln!(
                "OSAL host osal_init_system entry={ROM_OSAL_INIT_SYSTEM:#010x} task_init={task_init:#010x} behavior=jump-table task init + host heap"
            );
        });

        // SRAM/BSS starts zeroed by the firmware image/reset path. The host allocator below
        // replaces the vendor heap metadata that ROM osal_mem_init would otherwise establish.
        // Preserve LR: osalInitTasks returns directly to the caller of osal_init_system.
        processor.set_pc(task_init & !1);
        true
    }

    fn ensure_heap(&mut self, processor: &mut Processor) -> bool {
        if self.heap_next.is_some() {
            return true;
        }
        let base = match processor.read32(EMU_HEAP_BASE) {
            Ok(value) => value,
            Err(fault) => {
                eprintln!("OSAL strict heap: cannot read captured base: {fault}");
                return false;
            }
        };
        let size = match processor.read32(EMU_HEAP_SIZE) {
            Ok(value) => value,
            Err(fault) => {
                eprintln!("OSAL strict heap: cannot read captured size: {fault}");
                return false;
            }
        };
        if base == 0 || size == 0 {
            eprintln!("OSAL strict heap: osal_mem_set_heap was not observed before allocation");
            return false;
        }
        let next = align4(base);
        let Some(end) = base.checked_add(size) else {
            eprintln!("OSAL strict heap: captured heap range overflows");
            return false;
        };
        self.heap_next = Some(next);
        self.heap_end = end;
        eprintln!("OSAL host heap init base={base:#010x} size={size:#x} end={end:#010x}");
        true
    }

    fn mem_alloc(&mut self, processor: &mut Processor) -> bool {
        if !self.ensure_heap(processor) {
            return false;
        }
        let size = processor.get_r(Reg::R0);
        let start = self.heap_next.unwrap();
        let Some(end) = start.checked_add(align4(size)) else {
            processor.set_r(Reg::R0, 0);
            return_from_rom(processor);
            return true;
        };
        let ptr = if size == 0 || end > self.heap_end {
            0
        } else {
            self.heap_next = Some(end);
            start
        };
        if self.seen.insert(ROM_OSAL_MEM_ALLOC) {
            eprintln!(
                "OSAL host osal_mem_alloc entry={ROM_OSAL_MEM_ALLOC:#010x} allocator=aligned-bump heap_end={:#010x}",
                self.heap_end
            );
        }
        eprintln!("OSAL alloc size={size:#x} -> {ptr:#010x}");
        processor.set_r(Reg::R0, ptr);
        return_from_rom(processor);
        true
    }

    fn memset(&mut self, processor: &mut Processor) -> bool {
        let dest = processor.get_r(Reg::R0);
        let value = processor.get_r(Reg::R1) as u8;
        let len = processor.get_r(Reg::R2);
        for offset in 0..len {
            if let Err(fault) = processor.write8(dest.wrapping_add(offset), value) {
                eprintln!(
                    "OSAL strict osal_memset dest={dest:#010x} len={len:#x} offset={offset:#x}: {fault}"
                );
                return false;
            }
        }
        if self.seen.insert(ROM_OSAL_MEMSET) {
            eprintln!(
                "OSAL host osal_memset entry={ROM_OSAL_MEMSET:#010x} behavior=guest-byte-fill"
            );
        }
        processor.set_r(Reg::R0, dest);
        return_from_rom(processor);
        true
    }
}

fn return_from_rom(processor: &mut Processor) {
    let lr = processor.get_r(Reg::LR);
    processor.set_pc(lr & !1);
}

fn align4(value: u32) -> u32 {
    value.saturating_add(3) & !3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align4_preserves_word_alignment() {
        assert_eq!(align4(0), 0);
        assert_eq!(align4(1), 4);
        assert_eq!(align4(4), 4);
        assert_eq!(align4(5), 8);
    }

    #[test]
    fn bump_allocator_bounds_are_deterministic() {
        let mut host = HostOsal {
            heap_next: Some(0x1004),
            heap_end: 0x1010,
            seen: HashSet::new(),
        };
        let first = host.heap_next.unwrap();
        let next = first + align4(5);
        assert_eq!(first, 0x1004);
        assert_eq!(next, 0x100c);
        host.heap_next = Some(next);
        assert_eq!(host.heap_next.unwrap() + align4(4), host.heap_end);
    }
}
