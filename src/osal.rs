use std::collections::{HashMap, HashSet, VecDeque};
use zmu_cortex_m::bus::Bus;
use zmu_cortex_m::core::register::{BaseReg, Reg};
use zmu_cortex_m::Processor;

const ROM_CLOCK: u32 = 0x0001_4948;
const ROM_CLEAR_EVENT: u32 = 0x0001_4A88;
const ROM_INIT: u32 = 0x0001_4AEC;
const ROM_ALLOC: u32 = 0x0001_4B3C;
const ROM_FREE: u32 = 0x0001_4C00;
const ROM_MEMCPY: u32 = 0x0001_4CE8;
const ROM_MEMDUP: u32 = 0x0001_4CF8;
const ROM_MEMSET: u32 = 0x0001_4D14;
const ROM_MSG_ALLOC: u32 = 0x0001_4D1C;
const ROM_MSG_DEALLOC: u32 = 0x0001_4D42;
const ROM_MSG_RECEIVE: u32 = 0x0001_4EF4;
const ROM_MSG_SEND: u32 = 0x0001_4F58;
const ROM_REVMEMCPY: u32 = 0x0001_5144;
const ROM_SELF: u32 = 0x0001_51F4;
const ROM_SET_EVENT: u32 = 0x0001_520C;
const ROM_RELOAD_TIMER: u32 = 0x0001_5258;
const ROM_START: u32 = 0x0001_5284;
const ROM_START_TIMER: u32 = 0x0001_528A;
const ROM_STOP_TIMER: u32 = 0x0001_52B2;
const ROM_STRLEN: u32 = 0x0001_52DC;

const JT_INIT: u32 = 0x1FFF_0004;
const JT_TASKS: u32 = 0x1FFF_0008;
const JT_COUNT_PTR: u32 = 0x1FFF_000C;
const JT_EVENTS_PTR: u32 = 0x1FFF_0010;
const IDLE_BX_LR_ROM: u32 = 0x0000_A9C8;
const EMU_HEAP_BASE: u32 = 0x5000_FF30;
const EMU_HEAP_SIZE: u32 = 0x5000_FF34;
const MSG_HDR: u32 = 8;
const MSG_LEN_OFF: u32 = 4;
const MSG_DEST_OFF: u32 = 6;
const SYS_EVENT_MSG: u16 = 0x8000;
const INVALID_TASK: u8 = 0xFF;

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
    free: Vec<(u32, u32)>,
    allocs: HashMap<u32, u32>,
    messages: VecDeque<u32>,
    seen: HashSet<u32>,
    tasks: Option<u32>,
    events: Option<u32>,
    count: u8,
    running: Option<u8>,
    started: bool,
    timers: Vec<Timer>,
}

impl HostOsal {
    pub fn new() -> Self { Self::default() }

    pub fn handle(&mut self, cpu: &mut Processor) -> bool {
        let now = simulated_ms(cpu);
        self.expire(cpu, now);
        let pc = cpu.get_pc();
        if pc == IDLE_BX_LR_ROM && self.started && self.running.is_none() {
            return self.dispatch(cpu);
        }
        match pc {
            ROM_CLOCK => self.clock(cpu, now),
            ROM_CLEAR_EVENT => self.clear_event_call(cpu),
            ROM_INIT => self.init(cpu),
            ROM_ALLOC => self.alloc_call(cpu),
            ROM_FREE => self.free_call(cpu),
            ROM_MEMCPY => self.memcpy_call(cpu),
            ROM_MEMDUP => self.memdup_call(cpu),
            ROM_MEMSET => self.memset(cpu),
            ROM_MSG_ALLOC => self.msg_alloc(cpu),
            ROM_MSG_DEALLOC => self.msg_dealloc(cpu),
            ROM_MSG_RECEIVE => self.msg_receive(cpu),
            ROM_MSG_SEND => self.msg_send(cpu),
            ROM_REVMEMCPY => self.revmemcpy_call(cpu),
            ROM_SELF => self.self_call(cpu),
            ROM_SET_EVENT => self.set_event_call(cpu),
            ROM_RELOAD_TIMER => self.timer_call(cpu, now, true),
            ROM_START => self.start(cpu),
            ROM_START_TIMER => self.timer_call(cpu, now, false),
            ROM_STOP_TIMER => self.stop_timer(cpu),
            ROM_STRLEN => self.strlen_call(cpu),
            _ => false,
        }
    }

    fn once(&mut self, pc: u32, f: impl FnOnce()) { if self.seen.insert(pc) { f(); } }

    fn init(&mut self, cpu: &mut Processor) -> bool {
        let entry = match cpu.read32(JT_INIT) {
            Ok(v) if v & 1 == 1 => v,
            Ok(v) => { eprintln!("OSAL strict init callback={v:#010x} is not Thumb"); return false; }
            Err(e) => { eprintln!("OSAL strict init callback read: {e}"); return false; }
        };
        self.once(ROM_INIT, || eprintln!("OSAL host init task_init={entry:#010x}"));
        cpu.set_pc(entry & !1);
        true
    }

    fn start(&mut self, cpu: &mut Processor) -> bool {
        if self.running.is_some() { return self.finish(cpu); }
        if !self.started {
            if !self.resolve(cpu) { return false; }
            self.started = true;
            self.once(ROM_START, || eprintln!("OSAL host cooperative scheduler started"));
        }
        self.dispatch(cpu)
    }

    fn resolve(&mut self, cpu: &mut Processor) -> bool {
        if self.tasks.is_some() && self.events.is_some() && self.count != 0 { return true; }
        let tasks = match cpu.read32(JT_TASKS) { Ok(v) if v != 0 => v, _ => return false };
        let count_ptr = match cpu.read32(JT_COUNT_PTR) { Ok(v) if v != 0 => v, _ => return false };
        let events_ptr_ptr = match cpu.read32(JT_EVENTS_PTR) { Ok(v) if v != 0 => v, _ => return false };
        let count = match cpu.read8(count_ptr) { Ok(v) if v > 0 && v <= 64 => v, _ => return false };
        let events = match cpu.read32(events_ptr_ptr) { Ok(v) if v != 0 => v, _ => return false };
        self.tasks = Some(tasks); self.events = Some(events); self.count = count;
        eprintln!("OSAL scheduler tasks={count} handlers={tasks:#010x} events={events:#010x}");
        true
    }

    fn finish(&mut self, cpu: &mut Processor) -> bool {
        if let Some(task) = self.running.take() {
            let left = cpu.get_r(Reg::R0) as u16;
            if left != 0 && !self.post(cpu, task, left) { return false; }
        }
        self.dispatch(cpu)
    }

    fn dispatch(&mut self, cpu: &mut Processor) -> bool {
        let (Some(tasks), Some(events)) = (self.tasks, self.events) else { return false; };
        for task in 0..self.count {
            let event_addr = events.wrapping_add(u32::from(task) * 2);
            let bits = match cpu.read16(event_addr) { Ok(v) => v, Err(_) => return false };
            if bits == 0 { continue; }
            let handler = match cpu.read32(tasks.wrapping_add(u32::from(task) * 4)) {
                Ok(v) if v & 1 == 1 => v,
                _ => return false,
            };
            if cpu.write16(event_addr, 0).is_err() { return false; }
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
        if task >= self.count { return None; }
        self.events.map(|p| p + u32::from(task) * 2)
    }

    fn post(&self, cpu: &mut Processor, task: u8, event: u16) -> bool {
        let Some(addr) = self.event_addr(task) else { return false; };
        let current = match cpu.read16(addr) { Ok(v) => v, Err(_) => return false };
        cpu.write16(addr, current | event).is_ok()
    }

    fn set_event_call(&mut self, cpu: &mut Processor) -> bool {
        if !self.resolve(cpu) { return false; }
        let task = cpu.get_r(Reg::R0) as u8;
        let event = cpu.get_r(Reg::R1) as u16;
        if !self.post(cpu, task, event) { return false; }
        self.once(ROM_SET_EVENT, || eprintln!("OSAL host set_event -> guest event bitmap"));
        cpu.set_r(Reg::R0, 0); ret(cpu); true
    }

    fn clear_event_call(&mut self, cpu: &mut Processor) -> bool {
        if !self.resolve(cpu) { return false; }
        let task = cpu.get_r(Reg::R0) as u8;
        let event = cpu.get_r(Reg::R1) as u16;
        let Some(addr) = self.event_addr(task) else { return false; };
        let current = match cpu.read16(addr) { Ok(v) => v, Err(_) => return false };
        if cpu.write16(addr, current & !event).is_err() { return false; }
        self.once(ROM_CLEAR_EVENT, || eprintln!("OSAL host clear_event -> guest event bitmap"));
        cpu.set_r(Reg::R0, 0); ret(cpu); true
    }

    fn self_call(&mut self, cpu: &mut Processor) -> bool {
        cpu.set_r(Reg::R0, u32::from(self.running.unwrap_or(INVALID_TASK)));
        ret(cpu); true
    }

    fn clock(&mut self, cpu: &mut Processor, now: u32) -> bool {
        self.once(ROM_CLOCK, || eprintln!("OSAL host system clock unit=ms"));
        cpu.set_r(Reg::R0, now); ret(cpu); true
    }

    fn timer_call(&mut self, cpu: &mut Processor, now: u32, reload: bool) -> bool {
        if !self.resolve(cpu) { return false; }
        let task = cpu.get_r(Reg::R0) as u8;
        let event = cpu.get_r(Reg::R1) as u16;
        let ms = cpu.get_r(Reg::R2);
        if task >= self.count || event == 0 { return false; }
        self.timers.retain(|t| !(t.task == task && t.event == event));
        if ms == 0 {
            if !self.post(cpu, task, event) { return false; }
        } else {
            self.timers.push(Timer { task, event, deadline: now.wrapping_add(ms), reload: if reload { ms } else { 0 } });
        }
        let entry = if reload { ROM_RELOAD_TIMER } else { ROM_START_TIMER };
        self.once(entry, || eprintln!("OSAL host {} timer", if reload { "reload" } else { "one-shot" }));
        cpu.set_r(Reg::R0, 0); ret(cpu); true
    }

    fn stop_timer(&mut self, cpu: &mut Processor) -> bool {
        let task = cpu.get_r(Reg::R0) as u8;
        let event = cpu.get_r(Reg::R1) as u16;
        self.timers.retain(|t| !(t.task == task && t.event == event));
        self.once(ROM_STOP_TIMER, || eprintln!("OSAL host stop_timer"));
        cpu.set_r(Reg::R0, 0); ret(cpu); true
    }

    fn expire(&mut self, cpu: &mut Processor, now: u32) {
        let mut due = Vec::new();
        for (i, t) in self.timers.iter().enumerate() { if reached(now, t.deadline) { due.push(i); } }
        for i in due.into_iter().rev() {
            let mut t = self.timers.remove(i);
            let _ = self.post(cpu, t.task, t.event);
            if t.reload != 0 {
                loop { t.deadline = t.deadline.wrapping_add(t.reload); if !reached(now, t.deadline) { break; } }
                self.timers.push(t);
            }
        }
    }

    fn heap(&mut self, cpu: &mut Processor) -> bool {
        if self.heap_next.is_some() { return true; }
        let base = match cpu.read32(EMU_HEAP_BASE) { Ok(v) if v != 0 => v, _ => return false };
        let size = match cpu.read32(EMU_HEAP_SIZE) { Ok(v) if v != 0 => v, _ => return false };
        let Some(end) = base.checked_add(size) else { return false; };
        self.heap_next = Some(align4(base)); self.heap_end = end;
        eprintln!("OSAL host heap base={base:#010x} size={size:#x}");
        true
    }

    fn alloc_block(&mut self, cpu: &mut Processor, requested: u32) -> u32 {
        if requested == 0 || !self.heap(cpu) { return 0; }
        let size = align4(requested);
        if let Some((index, &(ptr, available))) = self.free.iter().enumerate().find(|(_, block)| block.1 >= size) {
            self.free.swap_remove(index);
            if available > size { self.free.push((ptr + size, available - size)); }
            self.allocs.insert(ptr, size);
            return ptr;
        }
        let start = self.heap_next.unwrap();
        let Some(end) = start.checked_add(size) else { return 0; };
        if end > self.heap_end { return 0; }
        self.heap_next = Some(end);
        self.allocs.insert(start, size);
        start
    }

    fn free_block(&mut self, ptr: u32) -> bool {
        if ptr == 0 { return true; }
        let Some(size) = self.allocs.remove(&ptr) else { return false; };
        self.free.push((ptr, size));
        true
    }

    fn alloc_call(&mut self, cpu: &mut Processor) -> bool {
        let ptr = self.alloc_block(cpu, cpu.get_r(Reg::R0));
        self.once(ROM_ALLOC, || eprintln!("OSAL host reusable allocator"));
        cpu.set_r(Reg::R0, ptr); ret(cpu); true
    }

    fn free_call(&mut self, cpu: &mut Processor) -> bool {
        let ptr = cpu.get_r(Reg::R0);
        if !self.free_block(ptr) { eprintln!("OSAL strict free unknown ptr={ptr:#010x}"); return false; }
        self.once(ROM_FREE, || eprintln!("OSAL host mem_free"));
        ret(cpu); true
    }

    fn memcpy_bytes(cpu: &mut Processor, dst: u32, src: u32, len: u32, reverse: bool) -> bool {
        let mut bytes = Vec::with_capacity(len as usize);
        for i in 0..len {
            let at = if reverse { len - 1 - i } else { i };
            let Ok(v) = cpu.read8(src.wrapping_add(at)) else { return false; };
            bytes.push(v);
        }
        for (i, byte) in bytes.into_iter().enumerate() {
            let at = if reverse { len - 1 - i as u32 } else { i as u32 };
            if cpu.write8(dst.wrapping_add(at), byte).is_err() { return false; }
        }
        true
    }

    fn memcpy_call(&mut self, cpu: &mut Processor) -> bool {
        let dst = cpu.get_r(Reg::R0); let src = cpu.get_r(Reg::R1); let len = cpu.get_r(Reg::R2);
        if !Self::memcpy_bytes(cpu, dst, src, len, false) { return false; }
        self.once(ROM_MEMCPY, || eprintln!("OSAL host memcpy"));
        cpu.set_r(Reg::R0, dst); ret(cpu); true
    }

    fn revmemcpy_call(&mut self, cpu: &mut Processor) -> bool {
        let dst = cpu.get_r(Reg::R0); let src = cpu.get_r(Reg::R1); let len = cpu.get_r(Reg::R2);
        if !Self::memcpy_bytes(cpu, dst, src, len, true) { return false; }
        self.once(ROM_REVMEMCPY, || eprintln!("OSAL host revmemcpy"));
        cpu.set_r(Reg::R0, dst); ret(cpu); true
    }

    fn memdup_call(&mut self, cpu: &mut Processor) -> bool {
        let src = cpu.get_r(Reg::R0); let len = cpu.get_r(Reg::R1);
        let dst = self.alloc_block(cpu, len);
        if dst != 0 && !Self::memcpy_bytes(cpu, dst, src, len, false) { return false; }
        self.once(ROM_MEMDUP, || eprintln!("OSAL host memdup"));
        cpu.set_r(Reg::R0, dst); ret(cpu); true
    }

    fn memset(&mut self, cpu: &mut Processor) -> bool {
        let dst = cpu.get_r(Reg::R0); let value = cpu.get_r(Reg::R1) as u8; let len = cpu.get_r(Reg::R2);
        for i in 0..len { if cpu.write8(dst.wrapping_add(i), value).is_err() { return false; } }
        self.once(ROM_MEMSET, || eprintln!("OSAL host memset"));
        cpu.set_r(Reg::R0, dst); ret(cpu); true
    }

    fn strlen_call(&mut self, cpu: &mut Processor) -> bool {
        let ptr = cpu.get_r(Reg::R0); let mut len = 0u32;
        loop {
            match cpu.read8(ptr.wrapping_add(len)) {
                Ok(0) => break,
                Ok(_) if len < 0x10000 => len += 1,
                _ => return false,
            }
        }
        self.once(ROM_STRLEN, || eprintln!("OSAL host strlen"));
        cpu.set_r(Reg::R0, len); ret(cpu); true
    }

    fn msg_alloc(&mut self, cpu: &mut Processor) -> bool {
        let len = cpu.get_r(Reg::R0);
        let hdr = self.alloc_block(cpu, len.saturating_add(MSG_HDR));
        let payload = if hdr == 0 { 0 } else { hdr + MSG_HDR };
        if hdr != 0 {
            if cpu.write32(hdr, 0).is_err() || cpu.write16(hdr + MSG_LEN_OFF, len as u16).is_err() || cpu.write8(hdr + MSG_DEST_OFF, INVALID_TASK).is_err() { return false; }
        }
        self.once(ROM_MSG_ALLOC, || eprintln!("OSAL host message allocation"));
        cpu.set_r(Reg::R0, payload); ret(cpu); true
    }

    fn msg_dealloc(&mut self, cpu: &mut Processor) -> bool {
        let payload = cpu.get_r(Reg::R0);
        let ok = payload >= MSG_HDR && self.free_block(payload - MSG_HDR);
        self.once(ROM_MSG_DEALLOC, || eprintln!("OSAL host message deallocation"));
        cpu.set_r(Reg::R0, if ok { 0 } else { 1 }); ret(cpu); true
    }

    fn msg_send(&mut self, cpu: &mut Processor) -> bool {
        if !self.resolve(cpu) { return false; }
        let task = cpu.get_r(Reg::R0) as u8; let payload = cpu.get_r(Reg::R1);
        if task >= self.count || payload < MSG_HDR { cpu.set_r(Reg::R0, 1); ret(cpu); return true; }
        let hdr = payload - MSG_HDR;
        if !self.allocs.contains_key(&hdr) || cpu.write8(hdr + MSG_DEST_OFF, task).is_err() || cpu.write32(hdr, 0).is_err() { cpu.set_r(Reg::R0, 1); ret(cpu); return true; }
        self.messages.push_back(payload);
        if !self.post(cpu, task, SYS_EVENT_MSG) { return false; }
        self.once(ROM_MSG_SEND, || eprintln!("OSAL host message queue send + SYS_EVENT_MSG"));
        cpu.set_r(Reg::R0, 0); ret(cpu); true
    }

    fn msg_receive(&mut self, cpu: &mut Processor) -> bool {
        let task = cpu.get_r(Reg::R0) as u8;
        let pos = self.messages.iter().position(|payload| cpu.read8(*payload - MSG_HDR + MSG_DEST_OFF).ok() == Some(task));
        let payload = pos.and_then(|i| self.messages.remove(i)).unwrap_or(0);
        if payload != 0 { let _ = cpu.write32(payload - MSG_HDR, 0); }
        self.once(ROM_MSG_RECEIVE, || eprintln!("OSAL host message queue receive"));
        cpu.set_r(Reg::R0, payload); ret(cpu); true
    }
}

fn ret(cpu: &mut Processor) { cpu.set_pc(cpu.get_r(Reg::LR) & !1); }
fn align4(v: u32) -> u32 { v.saturating_add(3) & !3 }
fn simulated_ms(cpu: &Processor) -> u32 { (cpu.cycle_count / 16_000) as u32 }
fn reached(now: u32, deadline: u32) -> bool { now.wrapping_sub(deadline) < 0x8000_0000 }

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn alignment() { assert_eq!(align4(1), 4); assert_eq!(align4(5), 8); }
    #[test] fn wrap_deadline() { assert!(!reached(9, 10)); assert!(reached(10, 10)); assert!(reached(1, 0xffff_fffe)); }
    #[test] fn message_header_matches_arm_osal_layout() { assert_eq!(MSG_HDR, 8); assert_eq!(MSG_LEN_OFF, 4); assert_eq!(MSG_DEST_OFF, 6); }
    #[test] fn timer_key_is_task_and_event() {
        let a = Timer { task: 1, event: 2, deadline: 3, reload: 0 };
        let b = Timer { task: 2, event: 2, deadline: 3, reload: 0 };
        assert_ne!(a.task, b.task); assert_eq!(a.event, b.event);
    }
    #[test] fn free_block_can_be_reused() {
        let mut h = HostOsal { heap_next: Some(0x1000), heap_end: 0x1100, ..HostOsal::default() };
        h.allocs.insert(0x1000, 16); h.free_block(0x1000); assert_eq!(h.free, vec![(0x1000, 16)]);
    }
}
