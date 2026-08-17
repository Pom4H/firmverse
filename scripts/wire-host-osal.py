from pathlib import Path

p = Path('src/emu.rs')
s = p.read_text()

if 'use crate::osal::HostOsal;' not in s:
    s = s.replace('use crate::mailbox;\n', 'use crate::mailbox;\nuse crate::osal::HostOsal;\n', 1)

if 'let mut host_osal = HostOsal::new();' not in s:
    s = s.replace(
        '    let mut cpu_rom_seen = 0u8;\n',
        '    let mut cpu_rom_seen = 0u8;\n    let mut host_osal = HostOsal::new();\n',
        1,
    )

s = s.replace(
    'redirect_cpu_rom_abi(&mut processor, &mut cpu_rom_seen);',
    'redirect_cpu_rom_abi(&mut processor, &mut cpu_rom_seen, &mut host_osal);',
)

old = 'fn redirect_cpu_rom_abi(processor: &mut Processor, seen: &mut u8) {\n    let pc = processor.get_pc();\n'
new = 'fn redirect_cpu_rom_abi(processor: &mut Processor, seen: &mut u8, host_osal: &mut HostOsal) {\n    if host_osal.handle(processor) {\n        return;\n    }\n    let pc = processor.get_pc();\n'
if old in s:
    s = s.replace(old, new, 1)
elif 'host_osal: &mut HostOsal' not in s:
    raise SystemExit('redirect_cpu_rom_abi signature marker not found')

p.write_text(s)
