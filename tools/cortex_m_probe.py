#!/usr/bin/env python3
"""Run a Cortex-M ELF in QEMU and measure its real stack high-water mark.

This is the first executable Firmverse backend. It deliberately keeps the
contract small: a firmware image exposes a non-inlined completion function,
Firmverse breaks at that symbol, and a pre-painted RAM region reveals the
maximum stack depth reached by the complete scenario.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable

PATTERN_BYTE = 0xA5


@dataclass(frozen=True)
class Section:
    name: str
    size: int
    address: int


@dataclass(frozen=True)
class ProbeReport:
    schema: int
    status: str
    backend: str
    target: str
    machine: str
    cpu: str
    elf_sha256: str
    completion_symbol: str
    completion_address: int
    flash_bytes: int
    static_ram_bytes: int
    stack_region_start: int
    stack_top: int
    peak_stack_bytes: int
    final_sp: int | None
    stack_limit_bytes: int
    stack_headroom_bytes: int
    recommended_stack_bytes: int
    wall_time_ms: int
    cycle_count: int | None
    cycle_count_status: str
    qemu_version: str
    gdb_version: str


def parse_int(value: str) -> int:
    return int(value, 0)


def parse_number(value: str) -> int:
    value = value.strip()
    if value.lower().startswith("0x"):
        return int(value, 16)
    if any(character in "abcdefABCDEF" for character in value):
        return int(value, 16)
    return int(value, 10)


def align_up(value: int, alignment: int) -> int:
    if alignment <= 0:
        raise ValueError("alignment must be positive")
    return ((value + alignment - 1) // alignment) * alignment


def parse_llvm_size(output: str) -> list[Section]:
    sections: list[Section] = []
    for raw_line in output.splitlines():
        fields = raw_line.split()
        if len(fields) != 3 or fields[0] in {"section", "Total"}:
            continue
        try:
            size = parse_number(fields[1])
            address = parse_number(fields[2])
        except ValueError:
            continue
        sections.append(Section(fields[0], size, address))
    if not sections:
        raise ValueError("llvm-size produced no parseable sections")
    return sections


def in_range(address: int, origin: int, length: int) -> bool:
    return origin <= address < origin + length


def classify_sections(
    sections: Iterable[Section],
    flash_origin: int,
    flash_length: int,
    ram_origin: int,
    ram_length: int,
) -> tuple[int, int, int]:
    flash = 0
    static_ram = 0
    static_ram_end = ram_origin
    data_load_image = 0
    for section in sections:
        if in_range(section.address, flash_origin, flash_length):
            flash += section.size
        if in_range(section.address, ram_origin, ram_length):
            static_ram += section.size
            static_ram_end = max(static_ram_end, section.address + section.size)
            if section.name == ".data" or section.name.startswith(".data."):
                data_load_image += section.size
    return flash + data_load_image, static_ram, static_ram_end


def find_symbol(llvm_nm: Path, elf: Path, needle: str) -> tuple[int, str]:
    completed = subprocess.run(
        [str(llvm_nm), "--defined-only", "--numeric-sort", "--demangle", str(elf)],
        check=True,
        capture_output=True,
        text=True,
    )
    matches: list[tuple[int, str]] = []
    for line in completed.stdout.splitlines():
        match = re.match(r"^([0-9a-fA-F]+)\s+\S\s+(.+)$", line.strip())
        if match and needle in match.group(2):
            matches.append((int(match.group(1), 16), match.group(2)))
    if not matches:
        raise ValueError(f"completion symbol containing {needle!r} was not found")
    exact = [item for item in matches if item[1] == needle or item[1].endswith(f"::{needle}")]
    candidates = exact or matches
    if len(candidates) != 1:
        names = ", ".join(name for _, name in candidates)
        raise ValueError(f"completion symbol is ambiguous: {names}")
    return candidates[0]


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def wait_for_port(port: int, process: subprocess.Popen[str], timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            stdout, stderr = process.communicate()
            raise RuntimeError(
                f"QEMU exited before GDB connected\nstdout:\n{stdout}\nstderr:\n{stderr}"
            )
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return
        except OSError:
            time.sleep(0.05)
    raise TimeoutError("QEMU GDB endpoint did not become ready")


def quote_gdb_path(path: Path) -> str:
    return str(path.resolve()).replace("\\", "\\\\").replace('"', '\\"')


def parse_register(output: str, register: str) -> int | None:
    match = re.search(rf"(?m)^{re.escape(register)}\s+0x([0-9a-fA-F]+)\b", output)
    return int(match.group(1), 16) if match else None


def scan_stack_watermark(data: bytes, pattern: int = PATTERN_BYTE) -> tuple[int, int]:
    changed = [index for index, value in enumerate(data) if value != pattern]
    if not changed:
        raise ValueError("stack pattern was untouched; firmware probably did not execute")
    lowest_changed = min(changed)
    return len(data) - lowest_changed, lowest_changed


def first_line(command: list[str]) -> str:
    completed = subprocess.run(command, check=True, capture_output=True, text=True)
    return completed.stdout.splitlines()[0].strip()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def run_probe(args: argparse.Namespace) -> ProbeReport:
    elf = args.elf.resolve()
    if not elf.is_file():
        raise FileNotFoundError(elf)

    llvm_nm = resolve_tool(args.llvm_nm, "llvm-nm")
    llvm_size = resolve_tool(args.llvm_size, "llvm-size")
    qemu = resolve_tool(args.qemu, "qemu-system-arm")
    gdb = resolve_tool(args.gdb, "gdb-multiarch")

    size_output = subprocess.run(
        [str(llvm_size), "--format=sysv", str(elf)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    sections = parse_llvm_size(size_output)
    flash_bytes, static_ram_bytes, static_ram_end = classify_sections(
        sections,
        args.flash_origin,
        args.flash_length,
        args.ram_origin,
        args.ram_length,
    )
    completion_address, completion_symbol = find_symbol(llvm_nm, elf, args.done_symbol)

    stack_top = args.ram_origin + args.ram_length
    pattern_start = align_up(static_ram_end, 32)
    if pattern_start >= stack_top:
        raise ValueError("static RAM leaves no region for the stack")
    pattern_length = stack_top - pattern_start

    with tempfile.TemporaryDirectory(prefix="firmverse-") as temporary:
        temp = Path(temporary)
        pattern_path = temp / "stack-pattern.bin"
        dump_path = temp / "stack-after.bin"
        gdb_script = temp / "probe.gdb"
        pattern_path.write_bytes(bytes([PATTERN_BYTE]) * pattern_length)

        port = find_free_port()
        script = "\n".join(
            [
                "set pagination off",
                "set confirm off",
                "set remotetimeout 10",
                f"target remote 127.0.0.1:{port}",
                (
                    f'restore "{quote_gdb_path(pattern_path)}" binary '
                    f"0x{pattern_start:x}"
                ),
                f"hbreak *0x{completion_address:x}",
                "continue",
                (
                    f'dump binary memory "{quote_gdb_path(dump_path)}" '
                    f"0x{pattern_start:x} 0x{stack_top:x}"
                ),
                "info registers sp pc lr",
                "detach",
                "quit",
                "",
            ]
        )
        gdb_script.write_text(script, encoding="utf-8")

        command = [
            str(qemu),
            "-M",
            args.machine,
            "-nographic",
            "-monitor",
            "none",
            "-serial",
            "none",
            "-S",
            "-gdb",
            f"tcp:127.0.0.1:{port}",
            "-kernel",
            str(elf),
        ]
        if args.cpu:
            command[3:3] = ["-cpu", args.cpu]

        started = time.monotonic()
        qemu_process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            wait_for_port(port, qemu_process, min(10.0, args.timeout_seconds))
            gdb_result = subprocess.run(
                [str(gdb), "--batch", "--quiet", "-x", str(gdb_script), str(elf)],
                capture_output=True,
                text=True,
                timeout=args.timeout_seconds,
                check=False,
            )
        finally:
            if qemu_process.poll() is None:
                qemu_process.terminate()
                try:
                    qemu_process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    qemu_process.kill()
                    qemu_process.wait(timeout=3)
        elapsed_ms = round((time.monotonic() - started) * 1000)

        combined_gdb = f"{gdb_result.stdout}\n{gdb_result.stderr}"
        if gdb_result.returncode != 0 or "Breakpoint" not in combined_gdb:
            raise RuntimeError(
                "firmware did not reach the Firmverse completion symbol\n"
                f"GDB output:\n{combined_gdb}"
            )
        if not dump_path.is_file():
            raise RuntimeError("GDB reached completion but produced no RAM dump")

        peak_stack_bytes, lowest_changed = scan_stack_watermark(dump_path.read_bytes())
        if lowest_changed == 0:
            raise RuntimeError(
                "stack reached the bottom of the painted region; measurement overflowed"
            )
        final_sp = parse_register(combined_gdb, "sp")

    recommended_stack = align_up(
        math.ceil(peak_stack_bytes * (100 + args.stack_margin_percent) / 100)
        + args.exception_reserve_bytes,
        1024,
    )
    headroom = args.stack_limit_bytes - peak_stack_bytes
    status = "pass" if headroom >= 0 else "fail"

    return ProbeReport(
        schema=1,
        status=status,
        backend="qemu-gdb-stack-watermark",
        target=args.target,
        machine=args.machine,
        cpu=args.cpu,
        elf_sha256=sha256(elf),
        completion_symbol=completion_symbol,
        completion_address=completion_address,
        flash_bytes=flash_bytes,
        static_ram_bytes=static_ram_bytes,
        stack_region_start=pattern_start,
        stack_top=stack_top,
        peak_stack_bytes=peak_stack_bytes,
        final_sp=final_sp,
        stack_limit_bytes=args.stack_limit_bytes,
        stack_headroom_bytes=headroom,
        recommended_stack_bytes=recommended_stack,
        wall_time_ms=elapsed_ms,
        cycle_count=None,
        cycle_count_status=(
            "not available from the initial QEMU backend; Firmverse instruction/cycle "
            "accounting or a target DWT counter is required before selecting clock MHz"
        ),
        qemu_version=first_line([str(qemu), "--version"]),
        gdb_version=first_line([str(gdb), "--version"]),
    )


def resolve_tool(explicit: str | None, fallback: str) -> Path:
    candidate = explicit or shutil.which(fallback)
    if not candidate:
        raise FileNotFoundError(f"required tool {fallback!r} was not found")
    path = Path(candidate)
    if not path.exists():
        raise FileNotFoundError(path)
    return path


def kib(value: int) -> str:
    return f"{value / 1024:.1f} KiB"


def render_markdown(report: ProbeReport) -> str:
    result = "PASS" if report.status == "pass" else "FAIL"
    final_sp = f"`0x{report.final_sp:08x}`" if report.final_sp is not None else "unavailable"
    return "\n".join(
        [
            f"# Firmverse Cortex-M probe — {result}",
            "",
            "| Metric | Result |",
            "| --- | ---: |",
            f"| Target | `{report.target}` |",
            f"| Virtual board | `{report.machine}` / `{report.cpu}` |",
            f"| Linked Flash | {kib(report.flash_bytes)} |",
            f"| Static RAM | {kib(report.static_ram_bytes)} |",
            f"| Measured peak stack | **{kib(report.peak_stack_bytes)}** |",
            f"| Current stack gate | {kib(report.stack_limit_bytes)} |",
            f"| Stack headroom | {kib(report.stack_headroom_bytes)} |",
            f"| Recommended stack | {kib(report.recommended_stack_bytes)} |",
            f"| Final SP | {final_sp} |",
            f"| Emulator wall time | {report.wall_time_ms} ms |",
            "",
            "The stack value is measured by painting the available RAM before reset,",
            "running the real Cortex-M ELF to its completion symbol, and scanning the",
            "remaining pattern. It includes compiler, crypto and parser stack frames.",
            "",
            f"Cycle status: {report.cycle_count_status}.",
            "",
            f"ELF SHA-256: `{report.elf_sha256}`.",
            "",
        ]
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--elf", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--machine", default="mps2-an505")
    parser.add_argument("--cpu", default="cortex-m33")
    parser.add_argument("--ram-origin", type=parse_int, default=0x20000000)
    parser.add_argument("--ram-length", type=parse_int, default=256 * 1024)
    parser.add_argument("--flash-origin", type=parse_int, default=0x00000000)
    parser.add_argument("--flash-length", type=parse_int, default=1024 * 1024)
    parser.add_argument("--done-symbol", default="firmverse_done")
    parser.add_argument("--stack-limit-bytes", type=parse_int, default=32 * 1024)
    parser.add_argument("--stack-margin-percent", type=int, default=50)
    parser.add_argument("--exception-reserve-bytes", type=parse_int, default=512)
    parser.add_argument("--timeout-seconds", type=float, default=120.0)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--llvm-nm")
    parser.add_argument("--llvm-size")
    parser.add_argument("--qemu")
    parser.add_argument("--gdb")
    args = parser.parse_args()

    report = run_probe(args)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    json_path = args.output_dir / "firmverse-report.json"
    markdown_path = args.output_dir / "firmverse-report.md"
    json_path.write_text(json.dumps(asdict(report), indent=2) + "\n", encoding="utf-8")
    markdown = render_markdown(report)
    markdown_path.write_text(markdown, encoding="utf-8")
    print(markdown)
    return 0 if report.status == "pass" else 2


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"firmverse probe failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
