#!/usr/bin/env python3
"""Record deterministic sensitive-data lifetime evidence from a Cortex-M test ELF.

The firmware uses a public test canary. GDB pauses once while the canary-backed
test data is intentionally live and once after teardown, then records configured
RAM/Flash regions and reports exact canary matches. This is a reproducible
memory-hygiene test and never uses production credentials.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path


@dataclass(frozen=True)
class Match:
    region: str
    offset: int
    address: int
    excerpt_address: int
    excerpt_hex: str


@dataclass(frozen=True)
class Report:
    schema: int
    status: str
    backend: str
    machine: str
    cpu: str
    live_symbol: str
    live_address: int
    done_symbol: str
    done_address: int
    canary_hex: str
    live_ram_matches: tuple[Match, ...]
    live_flash_matches: tuple[Match, ...]
    after_ram_matches: tuple[Match, ...]
    after_flash_matches: tuple[Match, ...]
    live_present: bool
    after_present: bool
    qemu_version: str
    gdb_version: str


def parse_int(value: str) -> int:
    return int(value, 0)


def parse_canary(value: str) -> bytes:
    compact = re.sub(r"[^0-9a-fA-F]", "", value)
    if not compact or len(compact) % 2:
        raise ValueError("canary must contain an even number of hexadecimal digits")
    return bytes.fromhex(compact)


def resolve_tool(explicit: str | None, fallback: str) -> Path:
    candidate = explicit or shutil.which(fallback)
    if not candidate:
        raise FileNotFoundError(f"required tool {fallback!r} was not found")
    path = Path(candidate)
    if not path.exists():
        raise FileNotFoundError(path)
    return path


def find_symbol(llvm_nm: Path, elf: Path, needle: str) -> tuple[int, str]:
    output = subprocess.run(
        [str(llvm_nm), "--defined-only", "--numeric-sort", "--demangle", str(elf)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    matches: list[tuple[int, str]] = []
    for line in output.splitlines():
        match = re.match(r"^([0-9a-fA-F]+)\s+\S\s+(.+)$", line.strip())
        if match and needle in match.group(2):
            matches.append((int(match.group(1), 16), match.group(2)))
    if not matches:
        raise ValueError(f"symbol containing {needle!r} was not found")
    exact = [item for item in matches if item[1] == needle or item[1].endswith(f"::{needle}")]
    candidates = exact or matches
    if len(candidates) != 1:
        raise ValueError("symbol is ambiguous")
    return candidates[0]


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def wait_for_port(port: int, process: subprocess.Popen[str], timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            stdout, stderr = process.communicate()
            raise RuntimeError(f"QEMU exited early\n{stdout}\n{stderr}")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return
        except OSError:
            time.sleep(0.05)
    raise TimeoutError("QEMU GDB endpoint did not become ready")


def quote(path: Path) -> str:
    return str(path.resolve()).replace("\\", "\\\\").replace('"', '\\"')


def first_line(command: list[str]) -> str:
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout.splitlines()[0].strip()


def offsets(data: bytes, needle: bytes) -> list[int]:
    found: list[int] = []
    cursor = 0
    while True:
        position = data.find(needle, cursor)
        if position < 0:
            return found
        found.append(position)
        cursor = position + 1


def matches(region: str, origin: int, data: bytes, canary: bytes) -> tuple[Match, ...]:
    result: list[Match] = []
    for offset in offsets(data, canary):
        start = max(0, offset - 16)
        end = min(len(data), offset + len(canary) + 16)
        result.append(
            Match(
                region=region,
                offset=offset,
                address=origin + offset,
                excerpt_address=origin + start,
                excerpt_hex=" ".join(f"{byte:02X}" for byte in data[start:end]),
            )
        )
    return tuple(result)


def dump(path: Path, start: int, length: int) -> str:
    return f'dump binary memory {quote(path)} 0x{start:x} 0x{start + length:x}'


def run(args: argparse.Namespace) -> Report:
    elf = args.elf.resolve()
    if not elf.is_file():
        raise FileNotFoundError(elf)
    canary = parse_canary(args.canary_hex)
    llvm_nm = resolve_tool(args.llvm_nm, "llvm-nm")
    qemu = resolve_tool(args.qemu, "qemu-system-arm")
    gdb = resolve_tool(args.gdb, "gdb-multiarch")
    live_address, live_symbol = find_symbol(llvm_nm, elf, args.live_symbol)
    done_address, done_symbol = find_symbol(llvm_nm, elf, args.done_symbol)

    args.output_dir.mkdir(parents=True, exist_ok=True)
    paths = {
        "live_ram": args.output_dir / "live-ram.bin",
        "live_flash": args.output_dir / "live-flash.bin",
        "after_ram": args.output_dir / "after-ram.bin",
        "after_flash": args.output_dir / "after-flash.bin",
    }

    with tempfile.TemporaryDirectory(prefix=".firmverse-memory-", dir=Path.cwd()) as temporary:
        script = Path(temporary) / "memory-evidence.gdb"
        port = free_port()
        commands = [
            "set pagination off",
            "set confirm off",
            "set remotetimeout 10",
            f"target remote 127.0.0.1:{port}",
            "load",
            f"set *(unsigned int*)0xe000ed08 = 0x{args.flash_origin:x}",
            "set $control = 0",
            "set $msplim = 0",
            f"set $msp = *(unsigned int*)0x{args.flash_origin:x}",
            f"set $sp = *(unsigned int*)0x{args.flash_origin:x}",
            f"set $pc = (*(unsigned int*)0x{args.flash_origin + 4:x}) & 0xfffffffe",
            "set $xpsr = 0x01000000",
            f"hbreak *0x{live_address:x}",
            "continue",
            dump(paths["live_ram"], args.ram_origin, args.ram_length),
            dump(paths["live_flash"], args.flash_origin, args.flash_length),
            "delete breakpoints",
            f"hbreak *0x{done_address:x}",
            "continue",
            dump(paths["after_ram"], args.ram_origin, args.ram_length),
            dump(paths["after_flash"], args.flash_origin, args.flash_length),
            "detach",
            "quit",
            "",
        ]
        script.write_text("\n".join(commands), encoding="utf-8")

        command = [
            str(qemu), "-M", args.machine, "-nographic", "-monitor", "none",
            "-serial", "none", "-S", "-gdb", f"tcp:127.0.0.1:{port}",
            "-kernel", str(elf),
        ]
        if args.cpu:
            command[3:3] = ["-cpu", args.cpu]
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            wait_for_port(port, process, min(10.0, args.timeout_seconds))
            result = subprocess.run(
                [str(gdb), "--batch", "--quiet", "-x", str(script), str(elf)],
                capture_output=True,
                text=True,
                timeout=args.timeout_seconds,
                check=False,
            )
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=3)
        combined = f"{result.stdout}\n{result.stderr}"
        if result.returncode != 0 or combined.count("Breakpoint") < 2:
            raise RuntimeError(f"both memory checkpoints were not reached\n{combined}")

    for path in paths.values():
        if not path.is_file():
            raise RuntimeError(f"missing dump {path}")

    live_ram = matches("ram", args.ram_origin, paths["live_ram"].read_bytes(), canary)
    live_flash = matches("flash", args.flash_origin, paths["live_flash"].read_bytes(), canary)
    after_ram = matches("ram", args.ram_origin, paths["after_ram"].read_bytes(), canary)
    after_flash = matches("flash", args.flash_origin, paths["after_flash"].read_bytes(), canary)
    return Report(
        schema=1,
        status="pass",
        backend="qemu-gdb-memory-canary",
        machine=args.machine,
        cpu=args.cpu,
        live_symbol=live_symbol,
        live_address=live_address,
        done_symbol=done_symbol,
        done_address=done_address,
        canary_hex=canary.hex(),
        live_ram_matches=live_ram,
        live_flash_matches=live_flash,
        after_ram_matches=after_ram,
        after_flash_matches=after_flash,
        live_present=bool(live_ram or live_flash),
        after_present=bool(after_ram or after_flash),
        qemu_version=first_line([str(qemu), "--version"]),
        gdb_version=first_line([str(gdb), "--version"]),
    )


def markdown(report: Report) -> str:
    lines = [
        "# Firmverse Cortex-M memory hygiene evidence",
        "",
        "The canary is public deterministic test data, not a production credential.",
        "",
        "| Checkpoint | RAM matches | Flash matches |",
        "| --- | ---: | ---: |",
        f"| Test data live | {len(report.live_ram_matches)} | {len(report.live_flash_matches)} |",
        f"| After teardown | {len(report.after_ram_matches)} | {len(report.after_flash_matches)} |",
        "",
        f"Live canary present: **{'YES' if report.live_present else 'NO'}**.",
        f"After-teardown canary present: **{'YES' if report.after_present else 'NO'}**.",
        "",
        f"Canary: `{report.canary_hex}`",
    ]
    if report.live_ram_matches:
        item = report.live_ram_matches[0]
        lines.extend([
            "", "## First live RAM match", "",
            f"Address: `0x{item.address:08x}`", "",
            f"```text\n0x{item.excerpt_address:08x}: {item.excerpt_hex}\n```",
        ])
    lines.extend([
        "",
        "A zero-match result is checkpoint evidence only. It does not establish resistance",
        "to side channels, fault injection, debug bypass, invasive probing, or other",
        "hardware attacks outside the emulated memory model.",
        "",
    ])
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--elf", type=Path, required=True)
    parser.add_argument("--machine", default="mps2-an505")
    parser.add_argument("--cpu", default="cortex-m33")
    parser.add_argument("--ram-origin", type=parse_int, default=0x38000000)
    parser.add_argument("--ram-length", type=parse_int, default=256 * 1024)
    parser.add_argument("--flash-origin", type=parse_int, default=0x10000000)
    parser.add_argument("--flash-length", type=parse_int, default=1024 * 1024)
    parser.add_argument("--live-symbol", required=True)
    parser.add_argument("--done-symbol", default="firmverse_done")
    parser.add_argument("--canary-hex", required=True)
    parser.add_argument("--timeout-seconds", type=float, default=120.0)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--llvm-nm")
    parser.add_argument("--qemu")
    parser.add_argument("--gdb")
    args = parser.parse_args()

    report = run(args)
    (args.output_dir / "memory-evidence.json").write_text(
        json.dumps(asdict(report), indent=2) + "\n", encoding="utf-8"
    )
    rendered = markdown(report)
    (args.output_dir / "memory-evidence.md").write_text(rendered, encoding="utf-8")
    print(rendered)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"firmverse memory evidence failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
