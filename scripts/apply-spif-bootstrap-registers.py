from pathlib import Path

path = Path('src/discovery.rs')
text = path.read_text()

const_marker = 'const PWM_BASE: u32 = 0x4000_E000;\n'
consts = 'const SPIF_BASE: u32 = 0x4000_C800;\n'
if 'const SPIF_BASE:' not in text:
    text = text.replace(const_marker, const_marker + consts, 1)

func_marker = '''    fn pwm_write_known(addr: u32) -> bool {\n        let aligned = addr & !3;\n        (0..PWM_CHANNELS as u32).any(|ch| aligned == PWM_BASE + ch * 16 + 8)\n    }\n\n'''
func = '''    fn spif_bootstrap_write_name(addr: u32) -> Option<&'static str> {\n        match (addr & !3).wrapping_sub(SPIF_BASE) {\n            0x38 => Some("SPIF.WR_COMPLETION_CTRL"),\n            0x50 => Some("SPIF.LOW_WR_PROTECTION"),\n            0x54 => Some("SPIF.UP_WR_PROTECTION"),\n            0x58 => Some("SPIF.WR_PROTECTION"),\n            0x7C => Some("SPIF.INDIRECT_WR_CNT"),\n            _ => None,\n        }\n    }\n\n'''
if 'fn spif_bootstrap_write_name' not in text:
    if func_marker not in text:
        raise SystemExit('pwm marker not found')
    text = text.replace(func_marker, func_marker + func, 1)

write_marker = '''        if self.strict && Self::is_unmodeled_rom(addr) {\n            return self.rom_unknown("write32", addr);\n        }\n        if !Self::is_mmio(addr) || Self::functional_write(addr) {\n'''
write_repl = '''        if self.strict && Self::is_unmodeled_rom(addr) {\n            return self.rom_unknown("write32", addr);\n        }\n        if let Some(name) = Self::spif_bootstrap_write_name(addr) {\n            eprintln!("SPIF config {name}={value:#010x}");\n            self.sparse_write(addr, value, 4);\n            return Ok(());\n        }\n        if !Self::is_mmio(addr) || Self::functional_write(addr) {\n'''
if 'SPIF config {name}' not in text:
    if write_marker not in text:
        raise SystemExit('write32 marker not found')
    text = text.replace(write_marker, write_repl, 1)

test_marker = '''    #[test]\n    fn exact_cache_controller_storage_is_visible_to_strict_mode() {\n'''
test = '''    #[test]\n    fn spif_bootstrap_accepts_only_observed_word_writes() {\n        let mut bus = bus(true);\n        for (offset, value) in [\n            (0x38, 0xFF01_0005),\n            (0x50, 0),\n            (0x54, 0x10),\n            (0x58, 2),\n            (0x7C, 0x0004_0000),\n        ] {\n            let addr = SPIF_BASE + offset;\n            bus.write32(addr, value).unwrap();\n            assert_eq!(bus.sparse_mmio.borrow().get(&addr), Some(&value));\n            assert!(matches!(bus.read32(addr), Err(Fault::DAccViol)));\n        }\n        assert!(matches!(bus.write32(SPIF_BASE + 0x3C, 1), Err(Fault::DAccViol)));\n        assert!(matches!(bus.write16(SPIF_BASE + 0x38, 1), Err(Fault::DAccViol)));\n    }\n\n'''
if 'spif_bootstrap_accepts_only_observed_word_writes' not in text:
    if test_marker not in text:
        raise SystemExit('test marker not found')
    text = text.replace(test_marker, test + test_marker, 1)

path.write_text(text)
