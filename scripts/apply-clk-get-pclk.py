from pathlib import Path

p = Path('src/emu.rs')
s = p.read_text()

s = s.replace(
    'use zmu_cortex_m::core::fault::FaultTrapMode;\n',
    'use zmu_cortex_m::bus::Bus;\nuse zmu_cortex_m::core::fault::FaultTrapMode;\n',
    1,
)
s = s.replace(
    'const ROM_SPIF_READ_ID: u32 = 0x0001_7208;\n',
    'const ROM_SPIF_READ_ID: u32 = 0x0001_7208;\nconst ROM_CLK_GET_PCLK: u32 = 0x0000_A5D0;\nconst PHY6252_G_HCLK: u32 = 0x1FFF_0874;\n',
    1,
)
needle = '''fn redirect_cpu_rom_abi(processor: &mut Processor, seen: &mut u8) {\n    let pc = processor.get_pc();\n'''
insert = '''fn redirect_cpu_rom_abi(processor: &mut Processor, seen: &mut u8) {\n    let pc = processor.get_pc();\n    if pc == ROM_CLK_GET_PCLK {\n        // The observed SDK boot path does not call clk_set_pclk_div before UART init.\n        // clk_init has already written g_hclk from g_hclk_table, so PCLK is the\n        // current HCLK while the APB divider remains at its reset /1 setting.\n        // A future clk_set_pclk_div call is still unknown ROM and therefore strict-faults.\n        let pclk = match processor.read32(PHY6252_G_HCLK) {\n            Ok(value) if value != 0 => value,\n            Ok(_) => {\n                if *seen & 16 == 0 {\n                    eprintln!(\n                        "ROM CPU strict clk_get_pclk entry={pc:#010x} -- g_hclk is zero before clock init"\n                    );\n                    *seen |= 16;\n                }\n                return;\n            }\n            Err(fault) => {\n                eprintln!(\n                    "ROM CPU strict clk_get_pclk entry={pc:#010x} -- cannot read g_hclk: {fault}"\n                );\n                return;\n            }\n        };\n        if *seen & 32 == 0 {\n            eprintln!(\n                "ROM CPU shim clk_get_pclk entry={pc:#010x} behavior=g_hclk/default-divider pclk={pclk}Hz"\n            );\n            *seen |= 32;\n        }\n        processor.set_r(Reg::R0, pclk);\n        let lr = processor.get_r(Reg::LR);\n        processor.set_pc(lr & !1);\n        return;\n    }\n'''
if 'ROM CPU shim clk_get_pclk' not in s:
    if needle not in s:
        raise SystemExit('redirect marker not found')
    s = s.replace(needle, insert, 1)

p.write_text(s)
