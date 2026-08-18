#!/usr/bin/env python3
"""Turn PHY62x2 bb_rom_sym_m0.txt into Thumb veneers GNU ld 2.4x will accept."""
from pathlib import Path
import sys

src = Path(sys.argv[1])
asm_path = Path(sys.argv[2])
ld_path = Path(sys.argv[3])

lines = []
data = []
for raw in src.read_text().splitlines():
    parts = raw.split()
    if len(parts) < 3 or not parts[0].startswith("0x"):
        continue
    addr, kind, name = parts[0], parts[1], parts[2]
    if not name[0].isalpha() and name[0] not in "._":
        continue
    if kind == "T":
        thumb = int(addr, 16) | 1
        lines.append((name, f"{thumb:#010x}"))
    else:
        data.append((name, addr))

out = [".syntax unified", ".cpu cortex-m0", ".thumb", ""]
for name, addr in lines:
    out.append(f"    .global {name}")
    out.append(f"    .thumb_func")
    out.append(f"{name}:")
    out.append("    push {r3}")
    out.append(f"    ldr r3, ={addr}")
    out.append("    mov ip, r3")
    out.append("    pop {r3}")
    out.append("    bx ip")
    out.append(f"    .global _symrom_{name}")
    out.append(f"    .thumb_set _symrom_{name}, {name}")
    out.append("    .ltorg")
    out.append("")
asm_path.write_text("\n".join(out) + "\n")

ld = []
for name, addr in data:
    ld.append(f"PROVIDE({name} = {addr});")
    ld.append(f"PROVIDE(_symrom_{name} = {addr});")
ld_path.write_text("\n".join(ld) + "\n")
