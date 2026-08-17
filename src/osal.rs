use std::collections::HashSet;
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_CLOCK: u32 = 0x0001_4948;
const ROM_INIT: u32 = 0x0001_4AEC;
const ROM_ALLOC: u32 = 0x0001_4B3C;
const ROM_MEMSET: u32 = 0x0001_4D14;
const ROM_SET_EVENT: u32 = 0x0001_520C;
const ROM_RELOAD_TIMER: u32 = 0x0001_5258;
const ROM_START: u32 = 0x0001_5284;
const ROM_START_TIMER: u32 = 0x0001_528A;
const ROM_STOP_TIMER: u32 = 0x0001_52B2;

const JT_INIT: u32 = 0x1FFF_0004;
const JT_TASKS: u32 = 0x1FFF_0008;
const JT_COUNT_PTR: u32 = 0x1FFF_000C;
const JT_EVENTS_PTR: u32 = 0x1FFF_0010;
const IDLE_BX_LR_ROM: u32 = 0x0000_A9C8;
const EMU_HEAP_BASE: u32 = 0x5000_FF30;
const EMU_HEAP_SIZE: u32 = 0x5000_FF34;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Timer {
    task: u8,
    event: u16,
    deadline: u32,
    reload: u32,
}

#[derive(Debug, Default)]
pub struct HostOsal {
    heap_next: Option<u32>,
    heap_end: u32,
    seen: HashSet<u32>,
    tasks: Option<u32>,
    events: Option<u32>,
    count: u8,
    running: Option<u8>,
    started: bool,
    timers: Vec<Timer>,
}

impl HostOsal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&mut self, cpu: &mut Processor) -> bool {
        let now = simulated_ms(cpu);
        self.expire(cpu, now);
        let pc = cpu.get_pc();
        if pc == IDLE_BX_LR_ROM && self.started && self.running.is_none() {
            return self.dispatch(cpu);
        }
        match pc {
            ROM_CLOCK => self.clock(cpu, now),
            ROM_INIT => self.init(cpu),
            ROM_ALLOC => self.alloc(cpu),
            ROM_MEMSET => self.memset(cpu),
            ROM_SET_EVENT => self.set_event_call(cpu),
            ROM_RELOAD_TIMER => self.timer_call(cpu, now, true),
            ROM_START => self.start(cpu),
            ROM_START_TIMER => self.timer_call(cpu, now, false),
            ROM_STOP_TIMER => self.stop_timer(cpu),
            _ => false,
        }
    }

    fn once(&mut self, pc: u32, f: impl FnOnce()) {
        if self.seen.insert(pc) {
            f();
        }
    }

    fn init(&mut self, cpu: &mut Processor) -> bool {
        let entry = match cpu.read32(JT_INIT) {
            Ok(v) if v & 1 == 1 => v,
            Ok(v) => {
                eprintln!("OSAL strict init callback={v:#010x} is not Thumb");
                return false;
            }
            Err(e) => {
                eprintln!("OSAL strict init callback read: {e}");
                return false;
            }
        };
        self.once(ROM_INIT, || eprintln!("OSAL host init task_init={entry:#010x}"));
        cpu.set_pc(entry & !1);
        true
    }

    fn start(&mut self, cpu: &mut Processor) -> bool {
        if self.running.is_some() {
            return self.finish(cpu);
        }
        if !self.started {
            if !self.resolve(cpu) {
                return false;
            }
            self.started = true;
            self.once(ROM_START, || eprintln!("OSAL host cooperative scheduler started"));
        }
        self.dispatch(cpu)
    }

    fn resolve(&mut self, cpu: &mut Processor) -> bool {
        if self.tasks.is_some() && self.events.is_some() && self.count != 0 {
            return true;
        }
        let tasks = match cpu.read32(JT_TASKS) {
            Ok(v) if v != 0 => v,
            _ => return false,
        };
        let count_ptr = match cpu.read32(JT_COUNT_PTR) {
            Ok(v) if v != 0 => v,
            _ => return false,
        };
        let events_ptr_ptr = match cpu.read32(JT_EVENTS_PTR) {
            Ok(v) if v != 0 => v,
            _ => return false,
        };
        let count = match cpu.read8(count_ptr) {
            Ok(v) if v > 0 && v <= 64 => v,
            _ => return false,
        };
        let events = match cpu.read32(events_ptr_ptr) {
            Ok(v) if v != 0 => v,
            _ => return false,
        };
        self.tasks = Some(tasks);
        self.events = Some(events);
        self.count = count;
        eprintln!("OSAL scheduler tasks={count} handlers={tasks:#010x} events={events:#010x}");
        true
    }

    fn finish(&mut self, cpu: &mut Processor) -> bool {
        if let Some(task) = self.running.take() {
            let left = cpu.get_r(Reg::R0) as u16;
            if left != 0 && !self.post(cpu, task, left) {
                return false;
            }
        }
        self.dispatch(cpu)
    }

    fn dispatch(&mut self, cpu: &mut Processor) -> bool {
        let (Some(tasks), Some(events)) = (self.tasks, self.events) else {
            return false;
        };
        for task in 0..self.count {
            let event_addr = events.wrapping_add(u32::from(task) * 2);
            let bits = match cpu.read16(event_addr) {
                Ok(v) => v,
                Err(_) => return false,
            };
            if bits == 0 {
                continue;
            }
            let handler = match cpu.read32(tasks.wrapping_add(u32::from(task) * 4)) {
                Ok(v) if v & 1 == 1 => v,
                _ => return false,
            };
            if cpu.write16(event_addr, 0).is_err() {
                return false;
            }
            self.running = Some(task);
            cpu.set_r(Reg::R0, u32::from(task));
            cpu.set_r(Reg::R1, u32::from(bits));
            cpu.set_r(Reg::LR, ROM_START | 1);
            cpu.set_pc(handler & !1);
            return true;
        }
        cpu.set_r(Reg::LR, IDLE_BX_LR_ROM | 1);
        cpu.set_pc(IDLE_BX_LR_ROM);
        true
    }

    fn event_addr(&self, task: u8) -> Option<u32> {
        if task >= self.count {
            return None;
        }
        self.events.map(|p| p + u32::from(task) * 2)
    }

    fn post(&self, cpu: &mut Processor, task: u8, event: u16) -> bool {
        let Some(addr) = self.event_addr(task) else {
            return false;
        };
        let current = match cpu.read16(addr) {
            Ok(v) => v,
            Err(_) => return false,
        };
        cpu.write16(addr, current | event).is_ok()
    }

    fn set_event_call(&mut self, cpu: &mut Processor) -> bool {
        if !self.resolve(cpu) {
            return false;
        }
        let task = cpu.get_r(Reg::R0) as u8;
        let event = cpu.get_r(Reg::R1) as u16;
        if !self.post(cpu, task, event) {
            return false;
        }
        self.once(ROM_SET_EVENT, || eprintln!("OSAL host set_event -> guest event bitmap"));
        cpu.set_r(Reg::R0, 0);
        ret(cpu);
        true
    }

    fn clock(&mut self, cpu: &mut Processor, now: u32) -> bool {
        self.once(ROM_CLOCK, || eprintln!("OSAL host system clock unit=ms"));
        cpu.set_r(Reg::R0, now);
        ret(cpu);
        true
    }

    fn timer_call(&mut self, cpu: &mut Processor, now: u32, reload: bool) -> bool {
        if !self.resolve(cpu) {
            return false;
        }
        let task = cpu.get_r(Reg::R0) as u8;
        let event = cpu.get_r(Reg::R1) as u16;
        let ms = cpu.get_r(Reg::R2);
        if task >= self.count || event == 0 {
            return false;
        }
        self.timers.retain(|t| !(t.task == task && t.event == event));
        if ms == 0 {
            if !self.post(cpu, task, event) {
                return false;
            }
        } else {
            self.timers.push(Timer {
                task,
                event,
                deadline: now.wrapping_add(ms),
                reload: if reload { ms } else { 0 },
            });
        }
        let entry = if reload { ROM_RELOAD_TIMER } else { ROM_START_TIMER };
        self.once(entry, || {
            eprintln!("OSAL host {} timer", if reload { "reload" } else { "one-shot" })
        });
        cpu.set_r(Reg::R0, 0);
        ret(cpu);
        true
    }

    fn stop_timer(&mut self, cpu: &mut Processor) -> bool {
        let task = cpu.get_r(Reg::R0) as u8;
        let event = cpu.get_r(Reg::R1) as u16;
        self.timers.retain(|t| !(t.task == task && t.event == event));
        self.once(ROM_STOP_TIMER, || eprintln!("OSAL host stop_timer"));
        cpu.set_r(Reg::R0, 0);
        ret(cpu);
        true
    }

    fn expire(&mut self, cpu: &mut Processor, now: u32) {
        let mut due = Vec::new();
        for (i, t) in self.timers.iter().enumerate() {
            if reached(now, t.deadline) {
                due.push(i);
            }
        }
        for i in due.into_iter().rev() {
            let mut t = self.timers.remove(i);
            let _ = self.post(cpu, t.task, t.event);
            if t.reload != 0 {
                loop {
                    t.deadline = t.deadline.wrapping_add(t.reload);
                    if !reached(now, t.deadline) {
                        break;
                    }
                }
                self.timers.push(t);
            }
        }
    }

    fn heap(&mut self, cpu: &mut Processor) -> bool {
        if self.heap_next.is_some() {
            return true;
        }
        let base = match cpu.read32(EMU_HEAP_BASE) {
            Ok(v) if v != 0 => v,
            _ => return false,
        };
        let size = match cpu.read32(EMU_HEAP_SIZE) {
            Ok(v) if v != 0 => v,
            _ => return false,
        };
        let Some(end) = base.checked_add(size) else {
            return false;
        };
        self.heap_next = Some(align4(base));
        self.heap_end = end;
        eprintln!("OSAL host heap base={base:#010x} size={size:#x}");
        true
    }

    fn alloc(&mut self, cpu: &mut Processor) -> bool {
        if !self.heap(cpu) {
            return false;
        }
        let size = cpu.get_r(Reg::R0);
        let start = self.heap_next.unwrap();
        let end = start.checked_add(align4(size));
        let ptr = match end {
            Some(end) if size != 0 && end <= self.heap_end => {
                self.heap_next = Some(end);
                start
            }
            _ => 0,
        };
        self.once(ROM_ALLOC, || eprintln!("OSAL host bump allocator"));
        cpu.set_r(Reg::R0, ptr);
        ret(cpu);
        true
    }

    fn memset(&mut self, cpu: &mut Processor) -> bool {
        let dst = cpu.get_r(Reg::R0);
        let value = cpu.get_r(Reg::R1) as u8;
        let len = cpu.get_r(Reg::R2);
        for i in 0..len {
            if cpu.write8(dst.wrapping_add(i), value).is_err() {
                return false;
            }
        }
        self.once(ROM_MEMSET, || eprintln!("OSAL host memset"));
        cpu.set_r(Reg::R0, dst);
        ret(cpu);
        true
    }
}

fn ret(cpu: &mut Processor) {
    cpu.set_pc(cpu.get_r(Reg::LR) & !1);
}

fn align4(v: u32) -> u32 {
    v.saturating_add(3) & !3
}

fn simulated_ms(cpu: &Processor) -> u32 {
    (cpu.cycle_count / 16_000) as u32
}

fn reached(now: u32, deadline: u32) -> bool {
    now.wrapping_sub(deadline) < 0x8000_0000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment() {
        assert_eq!(align4(1), 4);
        assert_eq!(align4(5), 8);
    }

    #[test]
    fn wrap_deadline() {
        assert!(!reached(9, 10));
        assert!(reached(10, 10));
        assert!(reached(1, 0xffff_fffe));
    }

    #[test]
    fn timer_key_is_task_and_event() {
        let a = Timer { task: 1, event: 2, deadline: 3, reload: 0 };
        let b = Timer { task: 2, event: 2, deadline: 3, reload: 0 };
        assert_ne!(a.task, b.task);
        assert_eq!(a.event, b.event);
    }
}
