from pathlib import Path

p = Path('src/discovery.rs')
s = p.read_text()

marker = 'const SPIF_BASE: u32 = 0x4000_C800;\n'
consts = 'const WAKEUP_MASK_31_0: u32 = 0x4000_F0A0;\nconst WAKEUP_MASK_34_32: u32 = 0x4000_F0A4;\n'
if 'const WAKEUP_MASK_31_0' not in s:
    if marker not in s:
        raise SystemExit('SPIF const marker not found')
    s = s.replace(marker, marker + consts, 1)

func_marker = '''    fn spif_bootstrap_write_name(addr: u32) -> Option<&'static str> {\n        match (addr & !3).wrapping_sub(SPIF_BASE) {\n            0x38 => Some("SPIF.WR_COMPLETION_CTRL"),\n            0x50 => Some("SPIF.LOW_WR_PROTECTION"),\n            0x54 => Some("SPIF.UP_WR_PROTECTION"),\n            0x58 => Some("SPIF.WR_PROTECTION"),\n            _ => None,\n        }\n    }\n\n'''
func = '''    fn wakeup_bootstrap_write_name(addr: u32) -> Option<&'static str> {\n        match addr & !3 {\n            WAKEUP_MASK_31_0 => Some("WAKEUP.io_wu_mask_31_0"),\n            WAKEUP_MASK_34_32 => Some("WAKEUP.io_wu_mask_34_32"),\n            _ => None,\n        }\n    }\n\n'''
if 'fn wakeup_bootstrap_write_name' not in s:
    if func_marker not in s:
        raise SystemExit('SPIF helper marker not found')
    s = s.replace(func_marker, func_marker + func, 1)

write_marker = '''        if let Some(name) = Self::spif_bootstrap_write_name(addr) {\n            eprintln!("SPIF config {name}={value:#010x}");\n            self.sparse_write(addr, value, 4);\n            return Ok(());\n        }\n'''
write_add = write_marker + '''        if let Some(name) = Self::wakeup_bootstrap_write_name(addr) {\n            eprintln!("GPIO bootstrap {name}={value:#010x}");\n            self.sparse_write(addr, value, 4);\n            return Ok(());\n        }\n'''
if 'GPIO bootstrap {name}' not in s:
    if write_marker not in s:
        raise SystemExit('write32 SPIF marker not found')
    s = s.replace(write_marker, write_add, 1)

# Exact startup behavior: word writes accepted, reads and neighbors remain strict.
test_marker = '''    #[test]\n    fn spif_bootstrap_accepts_only_observed_word_writes() {\n'''
test = '''    #[test]\n    fn gpio_wakeup_bootstrap_is_write_only_and_exact() {\n        let mut bus = bus(true);\n        bus.write32(WAKEUP_MASK_31_0, 0).unwrap();\n        bus.write32(WAKEUP_MASK_34_32, 0).unwrap();\n        assert_eq!(bus.sparse_mmio.borrow().get(&WAKEUP_MASK_31_0), Some(&0));\n        assert_eq!(bus.sparse_mmio.borrow().get(&WAKEUP_MASK_34_32), Some(&0));\n        assert!(matches!(bus.read32(WAKEUP_MASK_31_0), Err(Fault::DAccViol)));\n        assert!(matches!(bus.write32(0x4000_F0A8, 0), Err(Fault::DAccViol)));\n    }\n\n'''
if 'gpio_wakeup_bootstrap_is_write_only_and_exact' not in s:
    if test_marker not in s:
        raise SystemExit('SPIF test marker not found')
    s = s.replace(test_marker, test + test_marker, 1)

p.write_text(s)
