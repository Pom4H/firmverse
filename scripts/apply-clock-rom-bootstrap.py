from pathlib import Path

path = Path("src/discovery.rs")
text = path.read_text()

marker = "const ROM_SHIMS: &[RomShim] = &[\n"
entries = (
    '    RomShim { entry: 0x0000_8AA8, name: "clk_init ROM helper 0x8AA9", behavior: "identity-r0 (observed RC32M->XTAL16M boot path)", code: DRV_IRQ_INIT_CODE },\n'
    '    RomShim { entry: 0x0000_8C00, name: "clk_init ROM helper 0x8C01", behavior: "identity-r0 (observed RC32M->XTAL16M boot path)", code: DRV_IRQ_INIT_CODE },\n'
)
if "entry: 0x0000_8C00" not in text:
    if marker not in text:
        raise SystemExit("ROM_SHIMS marker not found")
    text = text.replace(marker, marker + entries, 1)

needle = "        assert_eq!(bus.read16(0x0000_A9C8).unwrap(), THUMB_BX_LR);\n"
checks = (
    "        assert_eq!(bus.read16(0x0000_8AA8).unwrap(), THUMB_BX_LR);\n"
    "        assert_eq!(bus.read16(0x0000_8C00).unwrap(), THUMB_BX_LR);\n"
    "        assert!(matches!(bus.read16(0x0000_8C04), Err(Fault::DAccViol)));\n"
)
if "bus.read16(0x0000_8C00)" not in text:
    if needle not in text:
        raise SystemExit("ROM thunk test marker not found")
    text = text.replace(needle, checks + needle, 1)

path.write_text(text)
