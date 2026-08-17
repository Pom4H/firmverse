from pathlib import Path

# Keep the observed SPIF bootstrap register set exact: +0x7C in the firmware
# disassembly was an NVIC write based at 0xE000E104, not AP_SPIF.
discovery = Path('src/discovery.rs')
d = discovery.read_text()
d = d.replace('            0x7C => Some("SPIF.INDIRECT_WR_CNT"),\n', '')
d = d.replace('            (0x7C, 0x0004_0000),\n', '')
discovery.write_text(d)

emu = Path('src/emu.rs')
e = emu.read_text()
e = e.replace(
    'use zmu_cortex_m::core::register::BaseReg;\n',
    'use zmu_cortex_m::core::register::{BaseReg, Reg};\n',
    1,
)
e = e.replace(
    'const ROM_DRV_ENABLE_IRQ: u32 = 0x0000_A99C;\n',
    'const ROM_DRV_ENABLE_IRQ: u32 = 0x0000_A99C;\nconst ROM_SPIF_READ_ID: u32 = 0x0001_7208;\n',
    1,
)
old = '''fn redirect_cpu_rom_abi(processor: &mut Processor, seen: &mut u8) {\n    let pc = processor.get_pc();\n    let (thunk, bit, name, behavior) = match pc {\n        ROM_DRV_DISABLE_IRQ => (CPU_THUNK_DISABLE_IRQ, 1u8, "drv_disable_irq", "CPSID i / PRIMASK=1"),\n        ROM_DRV_ENABLE_IRQ => (CPU_THUNK_ENABLE_IRQ, 2u8, "drv_enable_irq", "CPSIE i / PRIMASK=0"),\n        _ => return,\n    };\n    if *seen & bit == 0 {\n        eprintln!("ROM CPU shim {name} entry={pc:#010x} behavior={behavior}");\n        *seen |= bit;\n    }\n    processor.set_pc(thunk);\n}\n'''
new = '''fn redirect_cpu_rom_abi(processor: &mut Processor, seen: &mut u8) {\n    let pc = processor.get_pc();\n    if pc == ROM_SPIF_READ_ID {\n        let pid_ptr = processor.get_r(Reg::R0);\n        if pid_ptr != 0 {\n            if *seen & 4 == 0 {\n                eprintln!(\n                    "ROM CPU strict spif_read_id entry={pc:#010x} pid={pid_ptr:#010x} -- flash identity profile not configured"\n                );\n                *seen |= 4;\n            }\n            return;\n        }\n        if *seen & 8 == 0 {\n            eprintln!(\n                "ROM CPU shim spif_read_id entry={pc:#010x} behavior=NULL-probe-success (no JEDEC ID invented)"\n            );\n            *seen |= 8;\n        }\n        processor.set_r(Reg::R0, 0); // PPlus_SUCCESS\n        let lr = processor.get_r(Reg::LR);\n        processor.set_pc(lr & !1);\n        return;\n    }\n\n    let (thunk, bit, name, behavior) = match pc {\n        ROM_DRV_DISABLE_IRQ => (CPU_THUNK_DISABLE_IRQ, 1u8, "drv_disable_irq", "CPSID i / PRIMASK=1"),\n        ROM_DRV_ENABLE_IRQ => (CPU_THUNK_ENABLE_IRQ, 2u8, "drv_enable_irq", "CPSIE i / PRIMASK=0"),\n        _ => return,\n    };\n    if *seen & bit == 0 {\n        eprintln!("ROM CPU shim {name} entry={pc:#010x} behavior={behavior}");\n        *seen |= bit;\n    }\n    processor.set_pc(thunk);\n}\n'''
if 'ROM CPU shim spif_read_id' not in e:
    if old not in e:
        raise SystemExit('redirect_cpu_rom_abi marker not found')
    e = e.replace(old, new, 1)
emu.write_text(e)
