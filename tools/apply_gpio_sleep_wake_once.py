#!/usr/bin/env python3
"""One-shot source migration for GPIO-driven Cortex-M WFI wake semantics."""

from __future__ import annotations

import re
from pathlib import Path


CHIP = Path("src/soc/phy6252/chip.rs")
WEB_RUNTIME = Path("src/web_runtime.rs")


def replace_once(source: str, old: str, new: str, label: str) -> str:
    if source.count(old) != 1:
        raise SystemExit(f"{label}: expected one anchor, found {source.count(old)}")
    return source.replace(old, new, 1)


def main() -> None:
    source = CHIP.read_text()

    source, count = re.subn(
        r"(\n\s*ext_in: Arc<AtomicU32>,\n)",
        r"\1    sleep_entries: u32,\n    wake_count: u32,\n    last_wake_pin: Option<u32>,\n",
        source,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"chip fields: expected one anchor, found {count}")

    source, count = re.subn(
        r"(\n\s*ext_in: Arc::new\(AtomicU32::new\(0\)\),\n)",
        r"\1            sleep_entries: 0,\n            wake_count: 0,\n            last_wake_pin: None,\n",
        source,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"chip initialization: expected one anchor, found {count}")

    method_anchor = """    pub fn gpio_bank(&self) -> Rc<RefCell<GpioBank>> {
        Rc::clone(&self.gpio)
    }
"""
    method_replacement = method_anchor + """
    pub fn sleeping(&self) -> bool {
        self.processor.sleeping
    }

    pub fn sleep_entries(&self) -> u32 {
        self.sleep_entries
    }

    pub fn wake_count(&self) -> u32 {
        self.wake_count
    }

    pub fn last_wake_pin(&self) -> Option<u32> {
        self.last_wake_pin
    }

    fn set_external_inputs(&mut self, next: u32) {
        let next = next & GPIO_PIN_MASK;
        let current = self.ext_in.load(Ordering::Relaxed);
        let rising = next & !current;
        self.ext_in.store(next, Ordering::Relaxed);
        if rising != 0 && self.processor.sleeping {
            self.processor.sleeping = false;
            self.wake_count = self.wake_count.saturating_add(1);
            self.last_wake_pin = Some(rising.trailing_zeros());
        }
    }
"""
    source = replace_once(source, method_anchor, method_replacement, "gpio methods")

    command_pattern = re.compile(
        r"            ChipCmd::In\(value\) => \{.*?            ChipCmd::Write\(bytes\) => \{",
        re.S,
    )
    command_replacement = """            ChipCmd::In(value) => {
                self.set_external_inputs(value);
                Ok(Apply::Continue)
            }
            ChipCmd::Pin { bit, high } => {
                let mask = 1u32 << bit;
                let current = self.ext_in.load(Ordering::Relaxed);
                let next = if high { current | mask } else { current & !mask };
                self.set_external_inputs(next);
                Ok(Apply::Continue)
            }
            ChipCmd::Write(bytes) => {"""
    source, count = command_pattern.subn(command_replacement, source, count=1)
    if count != 1:
        raise SystemExit(f"external input commands: expected one anchor, found {count}")

    source = replace_once(
        source,
        """        if self.processor.sleeping {
            self.processor.sleeping = false;
        }
""",
        """        if self.processor.sleeping {
            self.collect(&mut delta);
            self.collect_bootrom(&mut delta);
            if live_clock && !self.bootrom.active() {
                self.clock_ms = self.clock_ms.wrapping_add(1);
                let _ = mailbox::set_tick(&mut self.processor, self.clock_ms);
            }
            return delta;
        }
""",
        "unconditional wake",
    )

    source = replace_once(
        source,
        """            self.processor.step();
            self.insn += 1;
            if self.reset_requested.replace(false) {
""",
        """            self.processor.step();
            self.insn += 1;
            if self.processor.sleeping {
                self.sleep_entries = self.sleep_entries.saturating_add(1);
                break;
            }
            if self.reset_requested.replace(false) {
""",
        "WFI entry observation",
    )
    CHIP.write_text(source)

    source = WEB_RUNTIME.read_text()
    source = replace_once(
        source,
        """            "insns": node.chip.insn,
            "stopped": node.chip.stopped(),
            "gpio": {
""",
        """            "insns": node.chip.insn,
            "stopped": node.chip.stopped(),
            "power": {
                "sleeping": node.chip.sleeping(),
                "sleepEntries": node.chip.sleep_entries(),
                "wakeCount": node.chip.wake_count(),
                "lastWakePin": node.chip.last_wake_pin(),
            },
            "gpio": {
""",
        "browser snapshot",
    )
    WEB_RUNTIME.write_text(source)


if __name__ == "__main__":
    main()
