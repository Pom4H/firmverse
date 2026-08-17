use std::collections::HashSet;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_OSAL_GET_SYSTEM_CLOCK: u32 = 0x0001_4948;
const ROM_OSAL_INIT_SYSTEM: u32 = 0x0001_4AEC;
const ROM_OSAL_MEM_ALLOC: u32 = 0x0001_4B3C;
const ROM_OSAL_MEMSET: u32 = 0x0001_4D14;
const ROM_OSAL_SET_EVENT: u32 = 0x0001_520C;
const ROM_OSAL_START_RELOAD_TIMER: u32 = 0x0001_5258;
const ROM_OSAL_START_SYSTEM: u32 = 0x0001_5284;
const ROM_OSAL_START_TIMER_EX: u32 = 0x0001_528A;
const ROM_OSAL_STOP_TIMER_EX: u32 = 0x0001_52B2;

// PHY6252 SDK jump table: item 1 is osalInitTasks, item 2 tasksArr,
// item 3 &tasksCnt, item 4 &tasksEvents. The table itself is linked at SRAM0.
const JUMP_TABLE_OSAL_INIT_TASKS: u32 = 0x1FFF_0004;
const JUMP_TABLE_TASKS_ARR: u32 = 0x1FFF_0008;
const JUMP_TABLE_TASKS_CNT_PTR: u32 = 0x1FFF_000C;
const JUMP_TABLE_TASKS_EVENTS_PTR: u32 = 0x1FFF_0010;

// Existing executable BX LR ROM shim used as the cooperative idle trampoline.
// HostOsal checks for ready tasks before zmu executes it.
const HOST_OSAL_IDLE_ROM: u32 = 0x0000_A9C8;

// DiscoveryBus host-control registers populated by the explicit osal_mem_set_heap shim.
const EMU_HEAP_BASE: u32 = 0x5000_FF30;
const EMU_HEAP_SIZE: u32 = 0x5000_FF34;
const SUCCESS: u32 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HostTimer {
    task_id: u8,
    event: u16,
    deadline_ms: u32,
    reload_ms: u32,
}

#[derive(Debug, Default)]
pub struct HostOsal {
    heap_next: Option<u32>,
    heap_end: u32,
    seen: HashSet<u32>,
    tasks_arr: Option<u32>,
    tasks_events: Option<u32>,
    tasks_cnt: u8,
    running_task: Option<u8>,
    scheduler_started: bool,
    timers: Vec<HostTimer>,
}

impl HostOsal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle ROM ABI and the cooperative scheduler return/idle gates. Returns
    /// true when the current PC was consumed and no vendor ROM fetch should occur.
    pub fn handle(&mut self, processor: &mut Processor) -> bool {
        let now_ms = simulated_time_ms(processor);
        self.expire_timers(processor, now_ms);

        let pc = processor.get_pc();
        if pc == HOST_OSAL_IDLE_ROM && self.scheduler_started && self.running_task.is_none() {
            return self.dispatch_next(processor);
        }

        match pc {
            ROM_OSAL_GET_SYSTEM_CLOCK => self.get_system_clock(processor, now_ms),
            ROM_OSAL_INIT_SYSTEM => self.init_system(processor),
            ROM_OSAL_MEM_ALLOC => self.mem_alloc(processor),
            ROM_OSAL_MEMSET => self.memset(processor),
            ROM_OSAL_SET_EVENT => self.set_event_abi(processor),
            ROM_OSAL_START_RELOAD_TIMER => self.start_timer_abi(processor, now_ms, true),
            ROM_OSAL_START_SYSTEM => self.start_system(processor),
            ROM_OSAL_START_TIMER_EX => self.start_timer_abi(processor, now_ms, false),
            ROM_OSAL_STOP_TIMER_EX => self.stop_timer_abi(processor),
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

        // Preserve LR: osalInitTasks returns directly to the caller of osal_init_system.
        processor.set_pc(task_init & !1);
        true
    }

    fn start_system(&mut self, processor: &mut Processor) -> bool {
        // Task handlers return their unprocessed event bits in R0. Pointing LR
        // back at osal_start_system makes the real ROM entry double as a host
        // scheduler return gate without reserving an invented executable address.
        if self.running_task.is_some() {
            return self.finish_task_and_dispatch(processor);
        }
        if !self.scheduler_started {
            if !self.resolve_scheduler_tables(processor) {
                return false;
            }
            self.scheduler_started = true;
            self.log_once(ROM_OSAL_START_SYSTEM, || {
                eprintln!(
                    "OSAL host osal_start_system entry={ROM_OSAL_START_SYSTEM:#010x} behavior=cooperative guest task dispatcher"
                );
            });
        }
        self.dispatch_next(processor)
    }

    fn resolve_scheduler_tables(&mut self, processor: &mut Processor) -> bool {
        if self.tasks_arr.is_some() && self.tasks_events.is_some() && self.tasks_cnt != 0 {
            return true;
        }

        let tasks_arr = match processor.read32(JUMP_TABLE_TASKS_ARR) {
            Ok(value) if value != 0 => value,
            Ok(_) => {
                eprintln!("OSAL strict scheduler: tasksArr is null");
                return false;
            }
            Err(fault) => {
                eprintln!("OSAL strict scheduler: cannot read tasksArr: {fault}");
                return false;
            }
        };
        let tasks_cnt_ptr = match processor.read32(JUMP_TABLE_TASKS_CNT_PTR) {
            Ok(value) if value != 0 => value,
            Ok(_) => {
                eprintln!("OSAL strict scheduler: &tasksCnt is null");
                return false;
            }
            Err(fault) => {
                eprintln!("OSAL strict scheduler: cannot read &tasksCnt: {fault}");
                return false;
            }
        };
        let tasks_events_ptr_ptr = match processor.read32(JUMP_TABLE_TASKS_EVENTS_PTR) {
            Ok(value) if value != 0 => value,
            Ok(_) => {
                eprintln!("OSAL strict scheduler: &tasksEvents is null");
                return false;
            }
            Err(fault) => {
                eprintln!("OSAL strict scheduler: cannot read &tasksEvents: {fault}");
                return false;
            }
        };
        let tasks_cnt = match processor.read8(tasks_cnt_ptr) {
            Ok(value) if value != 0 && value <= 64 => value,
            Ok(value) => {
                eprintln!("OSAL strict scheduler: invalid tasksCnt={value}");
                return false;
            }
            Err(fault) => {
                eprintln!("OSAL strict scheduler: cannot read tasksCnt: {fault}");
                return false;
            }
        };
        let tasks_events = match processor.read32(tasks_events_ptr_ptr) {
            Ok(value) if value != 0 => value,
            Ok(_) => {
                eprintln!("OSAL strict scheduler: tasksEvents is null after task init");
                return false;
            }
            Err(fault) => {
                eprintln!("OSAL strict scheduler: cannot read tasksEvents: {fault}");
                return false;
            }
        };

        self.tasks_arr = Some(tasks_arr);
        self.tasks_events = Some(tasks_events);
        self.tasks_cnt = tasks_cnt;
        eprintln!(
            "OSAL scheduler tasks={} handlers={tasks_arr:#010x} events={tasks_events:#010x}",
            tasks_cnt
        );
        true
    }

    fn finish_task_and_dispatch(&mut self, processor: &mut Processor) -> bool {
        if let Some(task_id) = self.running_task.take() {
            let remaining = processor.get_r(Reg::R0) as u16;
            if remaining != 0 && !self.or_task_event(processor, task_id, remaining) {
                return false;
            }
        }
        self.dispatch_next(processor)
    }

    fn dispatch_next(&mut self, processor: &mut Processor) -> bool {
        if !self.scheduler_started {
            return false;
        }
        let Some(tasks_arr) = self.tasks_arr else {
            return false;
        };
        let Some(tasks_events) = self.tasks_events else {
            return false;
        };

        for task_id in 0..self.tasks_cnt {
            let event_addr = tasks_events.wrapping_add(u32::from(task_id) * 2);
            let events = match processor.read16(event_addr) {
                Ok(value) => value,
                Err(fault) => {
                    eprintln!("OSAL strict scheduler: read task {task_id} events: {fault}");
                    return false;
                }
            };
            if events == 0 {
                continue;
            }

            let handler_addr = tasks_arr.wrapping_add(u32::from(task_id) * 4);
            let handler = match processor.read32(handler_addr) {
                Ok(value) if value & 1 == 1 => value,
                Ok(value) => {
                    eprintln!(
                        "OSAL strict scheduler: task {task_id} handler={value:#010x} is not Thumb"
                    );
                    return false;
                }
                Err(fault) => {
                    eprintln!("OSAL strict scheduler: read task {task_id} handler: {fault}");
                    return false;
                }
            };

            if let Err(fault) = processor.write16(event_addr, 0) {
                eprintln!("OSAL strict scheduler: clear task {task_id} events: {fault}");
                return false;
            }
            self.running_task = Some(task_id);
            processor.set_r(Reg::R0, u32::from(task_id));
            processor.set_r(Reg::R1, u32::from(events));
            processor.set_r(Reg::LR, ROM_OSAL_START_SYSTEM | 1);
            processor.set_pc(handler & !1);
            return true;
        }

        // Cooperative idle: reuse an existing BX LR ROM shim instead of executing
        // a fake scheduler loop. HostOsal checks task events before each iteration.
        processor.set_r(Reg::LR, HOST_OSAL_IDLE_ROM | 1);
        processor.set_pc(HOST_OSAL_IDLE_ROM);
        true
    }

    fn event_addr(&self, task_id: u8) -> Option<u32> {
        if task_id >= self.tasks_cnt {
            return None;
        }
        self.tasks_events
            .map(|base| base.wrapping_add(u32::from(task_id) * 2))
    }

    fn or_task_event(&self, processor: &mut Processor, task_id: u8, event: u16) -> bool {
        let Some(addr) = self.event_addr(task_id) else {
            eprintln!("OSAL strict event: invalid task id {task_id}");
            return false;
        };
        let current = match processor.read16(addr) {
            Ok(value) => value,
            Err(fault) => {
                eprintln!("OSAL strict event: read task {task_id}: {fault}");
                return false;
            }
        };
        if let Err(fault) = processor.write16(addr, current | event) {
            eprintln!("OSAL strict event: write task {task_id}: {fault}");
            return false;
        }
        true
    }

    fn set_event_abi(&mut self, processor: &mut Processor) -> bool {
        if !self.resolve_scheduler_tables(processor) {
            return false;
        }
        let task_id = processor.get_r(Reg::R0) as u8;
        let event = processor.get_r(Reg::R1) as u16;
        if !self.or_task_event(processor, task_id, event) {
            return false;
        }
        self.log_once(ROM_OSAL_SET_EVENT, || {
            eprintln!(
                "OSAL host osal_set_event entry={ROM_OSAL_SET_EVENT:#010x} behavior=OR guest task event bits"
            );
        });
        processor.set_r(Reg::R0, SUCCESS);
        return_from_rom(processor);
        true
    }

    fn get_system_clock(&mut self, processor: &mut Processor, now_ms: u32) -> bool {
        self.log_once(ROM_OSAL_GET_SYSTEM_CLOCK, || {
            eprintln!(
                "OSAL host osal_GetSystemClock entry={ROM_OSAL_GET_SYSTEM_CLOCK:#010x} unit=ms"
            );
        });
        processor.set_r(Reg::R0, now_ms);
        return_from_rom(processor);
        true
    }

    fn start_timer_abi(&mut self, processor: &mut Processor, now_ms: u32, reload: bool) -> bool {
        if !self.resolve_scheduler_tables(processor) {
            return false;
        }
        let task_id = processor.get_r(Reg::R0) as u8;
        let event = processor.get_r(Reg::R1) as u16;
        let timeout_ms = processor.get_r(Reg::R2);
        if task_id >= self.tasks_cnt || event == 0 {
            eprintln!("OSAL strict timer: invalid task={task_id} event={event:#06x}");
            return false;
        }

        self.timers
            .retain(|timer| !(timer.task_id == task_id && timer.event == event));
        if timeout_ms == 0 {
            if !self.or_task_event(processor, task_id, event) {
                return false;
            }
        } else {
            self.timers.push(HostTimer {
                task_id,
                event,
                deadline_ms: now_ms.wrapping_add(timeout_ms),
                reload_ms: if reload { timeout_ms } else { 0 },
            });
        }

        let entry = if reload {
            ROM_OSAL_START_RELOAD_TIMER
        } else {
            ROM_OSAL_START_TIMER_EX
        };
        self.log_once(entry, || {
            eprintln!(
                "OSAL host timer entry={entry:#010x} behavior={} ms deadline -> task event",
                if reload { "reload" } else { "one-shot" }
            );
        });
        processor.set_r(Reg::R0, SUCCESS);
        return_from_rom(processor);
        true
    }

    fn stop_timer_abi(&mut self, processor: &mut Processor) -> bool {
        let task_id = processor.get_r(Reg::R0) as u8;
        let event = processor.get_r(Reg::R1) as u16;
        self.timers
            .retain(|timer| !(timer.task_id == task_id && timer.event == event));
        self.log_once(ROM_OSAL_STOP_TIMER_EX, || {
            eprintln!(
                "OSAL host osal_stop_timerEx entry={ROM_OSAL_STOP_TIMER_EX:#010x} behavior=remove matching host timer"
            );
        });
        processor.set_r(Reg::R0, SUCCESS);
        return_from_rom(processor);
        true
    }

    fn expire_timers(&mut self, processor: &mut Processor, now_ms: u32) {
        if self.timers.is_empty() || self.tasks_events.is_none() {
            return;
        }

        let mut expired = Vec::new();
        for (index, timer) in self.timers.iter().enumerate() {
            if deadline_reached(now_ms, timer.deadline_ms) {
                expired.push(index);
            }
        }
        for index in expired.into_iter().rev() {
            let mut timer = self.timers.remove(index);
            let _ = self.or_task_event(processor, timer.task_id, timer.event);
            if timer.reload_ms != 0 {
                do {
                    timer.deadline_ms = timer.deadline_ms.wrapping_add(timer.reload_ms);
                } while deadline_reached(now_ms, timer.deadline_ms);
                self.timers.push(timer);
            }
        }
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

fn simulated_time_ms(processor: &Processor) -> u32 {
    // zmu cycle_count keeps OSAL time deterministic in both --once and live mode.
    // 16 MHz is the conservative bootstrap clock until dynamic HCLK timing is modeled.
    (processor.cycle_count / 16_000) as u32
}

fn deadline_reached(now: u32, deadline: u32) -> bool {
    now.wrapping_sub(deadline) < 0x8000_0000
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
            ..HostOsal::default()
        };
        let first = host.heap_next.unwrap();
        let next = first + align4(5);
        assert_eq!(first, 0x1004);
        assert_eq!(next, 0x100c);
        host.heap_next = Some(next);
        assert_eq!(host.heap_next.unwrap() + align4(4), host.heap_end);
    }

    #[test]
    fn deadline_comparison_survives_wraparound() {
        assert!(!deadline_reached(9, 10));
        assert!(deadline_reached(10, 10));
        assert!(deadline_reached(11, 10));
        assert!(deadline_reached(1, 0xffff_fffe));
    }

    #[test]
    fn timer_identity_is_task_and_event() {
        let timers = [
            HostTimer {
                task_id: 1,
                event: 0x20,
                deadline_ms: 10,
                reload_ms: 0,
            },
            HostTimer {
                task_id: 2,
                event: 0x20,
                deadline_ms: 20,
                reload_ms: 0,
            },
        ];
        assert_ne!(timers[0].task_id, timers[1].task_id);
        assert_eq!(timers[0].event, timers[1].event);
    }
}
