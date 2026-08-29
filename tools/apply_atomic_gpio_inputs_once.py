#!/usr/bin/env python3
"""One-shot migration adding atomic GPIO input masks to the browser runtime."""

from pathlib import Path


def replace_once(path: Path, old: str, new: str, label: str) -> None:
    source = path.read_text()
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one anchor, found {count}")
    path.write_text(source.replace(old, new, 1))


runtime = Path("src/web_runtime.rs")
replace_once(
    runtime,
    """    pub fn pin(&mut self, id: &str, pin: &str, high: bool) -> Result<(), String> {
        let bit = pins::gpio_bit(pin).ok_or_else(|| format!("unknown PHY6252 pin {pin:?}"))?;
        let index = self.node_index(id)?;
        self.nodes[index].chip.apply(ChipCmd::Pin { bit, high })?;
        Ok(())
    }

""",
    """    pub fn pin(&mut self, id: &str, pin: &str, high: bool) -> Result<(), String> {
        let bit = pins::gpio_bit(pin).ok_or_else(|| format!("unknown PHY6252 pin {pin:?}"))?;
        let index = self.node_index(id)?;
        self.nodes[index].chip.apply(ChipCmd::Pin { bit, high })?;
        Ok(())
    }

    pub fn inputs(&mut self, id: &str, mask: u32) -> Result<(), String> {
        let index = self.node_index(id)?;
        self.nodes[index].chip.apply(ChipCmd::In(mask))?;
        Ok(())
    }

""",
    "browser input method",
)
replace_once(
    runtime,
    """                "adc" => {
""",
    """                "inputs" => {
                    let mask = request
                        .get("mask")
                        .and_then(Value::as_u64)
                        .filter(|value| *value <= u64::from(u32::MAX))
                        .ok_or_else(|| "field mask must fit u32".to_string())?;
                    lab.inputs(string(&request, "id")?, mask as u32)?;
                    Ok(json!({ "ok": true, "snapshot": lab.snapshot() }))
                }
                "adc" => {
""",
    "browser input dispatch",
)

worker = Path("web/src/engine-worker.js")
replace_once(
    worker,
    """      case 'adc':
""",
    """      case 'inputs':
        publishSnapshot(call({ op: 'inputs', id: message.id, mask: Number(message.mask ?? 0) >>> 0 }));
        return;
      case 'adc':
""",
    "worker input message",
)
