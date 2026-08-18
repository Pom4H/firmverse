#!/usr/bin/env python3
"""Concatenate Intel HEX files. Optional last arg is the start address."""
from pathlib import Path
import sys


def records(path: Path) -> tuple[list[str], int | None]:
    lines: list[str] = []
    entry = None
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line.startswith(":"):
            continue
        kind = int(line[7:9], 16)
        if kind == 1:
            continue
        if kind == 5:
            entry = int(line[9:17], 16)
            continue
        lines.append(line)
    return lines, entry


def type5(addr: int) -> str:
    payload = bytes(
        [
            4,
            0,
            0,
            5,
            (addr >> 24) & 0xFF,
            (addr >> 16) & 0xFF,
            (addr >> 8) & 0xFF,
            addr & 0xFF,
        ]
    )
    csum = (~sum(payload) + 1) & 0xFF
    return ":" + payload.hex().upper() + f"{csum:02X}"


def parse_addr(text: str) -> int | None:
    t = text.strip().lower()
    if t.startswith("0x"):
        return int(t, 16)
    return None


def main() -> None:
    args = sys.argv[1:]
    entry_override = None
    if args:
        maybe = parse_addr(args[-1])
        if maybe is not None:
            entry_override = maybe
            args = args[:-1]
    out = Path(args[-1])
    parts = [Path(p) for p in args[:-1]]
    merged: list[str] = []
    entry = entry_override
    for part in parts:
        recs, rec_entry = records(part)
        merged.extend(recs)
        if entry_override is None and rec_entry is not None:
            entry = rec_entry
    if entry is not None:
        merged.append(type5(entry))
    merged.append(":00000001FF")
    out.write_text("\n".join(merged) + "\n")


if __name__ == "__main__":
    main()
