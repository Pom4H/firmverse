from pathlib import Path

path = Path('src/discovery.rs')
text = path.read_text()
marker = 'const ROM_SHIMS: &[RomShim] = &[\n'
entry = '''    RomShim {\n        entry: 0x0001_6DC4,\n        name: "spif_config",\n        behavior: "noop-return (host XIP backend already configured)",\n        code: DRV_IRQ_INIT_CODE,\n    },\n'''
if 'entry: 0x0001_6DC4' not in text:
    if marker not in text:
        raise SystemExit('ROM_SHIMS marker not found')
    text = text.replace(marker, marker + entry, 1)

needle = '        assert_eq!(bus.read16(0x0000_8C00).unwrap(), THUMB_BX_LR);\n'
check = '        assert_eq!(bus.read16(0x0001_6DC4).unwrap(), THUMB_BX_LR);\n'
if 'bus.read16(0x0001_6DC4)' not in text:
    if needle not in text:
        raise SystemExit('ROM shim test marker not found')
    text = text.replace(needle, needle + check, 1)
path.write_text(text)
