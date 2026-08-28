from __future__ import annotations

import base64
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"missing patch anchor in {path}: {old[:120]!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def apply_base_migration() -> None:
    encoded = "".join(
        part.read_text(encoding="utf-8").strip()
        for part in sorted((ROOT / ".migration").glob("fv.part*"))
    )
    source = base64.b64decode(encoded).decode("utf-8")

    slash = chr(92)
    pairs = [
        (
            "'''        script = \"" + slash + "n\".join(",
            "'''        script = \"" + slash + slash + "n\".join(",
        ),
        (
            '        script = "' + slash + 'n".join(commands)',
            '        script = "' + slash + slash + 'n".join(commands)',
        ),
    ]
    for old, new in pairs:
        if old not in source:
            raise SystemExit(f"base migration newline anchor missing: {old!r}")
        source = source.replace(old, new, 1)

    code = compile(source, "<firmverse-fvd1-base-migration>", "exec")
    exec(code, {"__name__": "__main__"})


def patch_probe() -> None:
    path = ROOT / "tools/cortex_m_probe.py"
    text = path.read_text(encoding="utf-8")

    replacements = [
        (
            'with tempfile.TemporaryDirectory(prefix="firmverse-") as temporary:',
            'with tempfile.TemporaryDirectory(prefix=".firmverse-", dir=Path.cwd()) as temporary:',
        ),
        (
            "f'restore \"{quote_gdb_path(pattern_path)}\" binary '",
            "f'restore {quote_gdb_path(pattern_path)} binary '",
        ),
        (
            "f'dump binary memory \"{quote_gdb_path(dump_path)}\" '",
            "f'dump binary memory {quote_gdb_path(dump_path)} '",
        ),
        (
            "f'dump binary memory \"{quote_gdb_path(trace_path)}\" '",
            "f'dump binary memory {quote_gdb_path(trace_path)} '",
        ),
    ]
    for old, new in replacements:
        if old not in text:
            raise SystemExit(f"probe anchor missing: {old}")
        text = text.replace(old, new, 1)

    remote = '            f"target remote 127.0.0.1:{port}",\n'
    if remote not in text:
        raise SystemExit("target remote anchor missing")
    text = text.replace(remote, remote + '            "load",\n', 1)

    breakpoint = '            f"hbreak *0x{completion_address:x}",\n'
    startup = (
        '            f"set *(unsigned int*)0xe000ed08 = 0x{args.flash_origin:x}",\n'
        + '            "set $control = 0",\n'
        + '            "set $msplim = 0",\n'
        + '            f"set $msp = *(unsigned int*)0x{args.flash_origin:x}",\n'
        + '            f"set $sp = *(unsigned int*)0x{args.flash_origin:x}",\n'
        + '            f"set $pc = (*(unsigned int*)0x{args.flash_origin + 4:x}) & 0xfffffffe",\n'
        + '            "set $xpsr = 0x01000000",\n'
        + breakpoint
    )
    if breakpoint not in text:
        raise SystemExit("breakpoint anchor missing")
    text = text.replace(breakpoint, startup, 1)
    path.write_text(text, encoding="utf-8")


def patch_action() -> None:
    path = ROOT / "actions/cortex-m-probe/action.yml"
    text = path.read_text(encoding="utf-8")
    old_ram = 'default: "0x20000000"'
    old_flash = 'default: "0x00000000"'
    if old_ram not in text or old_flash not in text:
        raise SystemExit("action memory-map anchors missing")
    text = text.replace(old_ram, 'default: "0x38000000"', 1)
    text = text.replace(old_flash, 'default: "0x10000000"', 1)
    path.write_text(text, encoding="utf-8")


def write_durable_workflow() -> None:
    path = ROOT / ".github/workflows/cortex-m-probe.yml"
    path.write_text(
        """name: Cortex-M probe

on:
  push:
    paths:
      - .github/workflows/cortex-m-probe.yml
      - actions/cortex-m-probe/**
      - tools/cortex_m_probe.py
      - tools/test_cortex_m_probe.py
      - docs/DEVICE_TRACE_ABI.md
      - tests/fixtures/cortex-m33-fvd1.S
      - tests/fixtures/cortex-m33-secure.ld
  pull_request:
    paths:
      - .github/workflows/cortex-m-probe.yml
      - actions/cortex-m-probe/**
      - tools/cortex_m_probe.py
      - tools/test_cortex_m_probe.py
      - docs/DEVICE_TRACE_ABI.md
      - tests/fixtures/cortex-m33-fvd1.S
      - tests/fixtures/cortex-m33-secure.ld
  workflow_dispatch:

permissions:
  contents: read

jobs:
  probe:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262

      - name: Test report accounting
        run: python3 -m unittest tools/test_cortex_m_probe.py

      - name: Install fixture compiler
        run: |
          sudo apt-get update -qq
          sudo apt-get install -y --no-install-recommends clang lld

      - name: Install LLVM inspection tools
        uses: dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c
        with:
          toolchain: 1.98.0
          components: llvm-tools-preview

      - name: Build deterministic Cortex-M33 fixture
        shell: bash
        run: |
          set -euo pipefail
          mkdir -p target/fixture
          clang --target=arm-none-eabi -mcpu=cortex-m33 -mthumb \
            -nostdlib -fuse-ld=lld \
            -Wl,-T,tests/fixtures/cortex-m33-secure.ld \
            -o target/fixture/fixture.elf \
            tests/fixtures/cortex-m33-fvd1.S

      - name: Execute fixture through local Firmverse action
        id: firmverse
        uses: ./actions/cortex-m-probe
        with:
          elf: target/fixture/fixture.elf
          target: thumbv8m.main-none-eabi
          machine: mps2-an505
          cpu: cortex-m33
          ram-origin: "0x38000000"
          ram-length: "262144"
          flash-origin: "0x10000000"
          flash-length: "1048576"
          device-trace-symbol: firmverse_device_trace
          required-device-capabilities: "0x1f"
          stack-limit-bytes: "4096"
          output-dir: target/firmverse-report

      - name: Verify stack and virtual-device evidence
        shell: bash
        env:
          PEAK_STACK: ${{ steps.firmverse.outputs.peak-stack-bytes }}
          DEVICE_STATUS: ${{ steps.firmverse.outputs.device-status }}
          DEVICE_CAPABILITIES: ${{ steps.firmverse.outputs.device-capabilities }}
          REPORT_JSON: ${{ steps.firmverse.outputs.report-json }}
        run: |
          set -euo pipefail
          test "$PEAK_STACK" -ge 768
          test "$PEAK_STACK" -le 1024
          test "$DEVICE_STATUS" = pass
          test "$DEVICE_CAPABILITIES" = 31
          python3 - "$REPORT_JSON" <<'PY'
          import json
          import sys

          trace = json.load(open(sys.argv[1], encoding="utf-8"))["device_trace"]
          assert trace["button_events"] == 3
          assert trace["display_frames"] == 2
          assert trace["trng_bytes"] == 32
          assert trace["storage_generation"] == 2
          assert trace["secure_element_ops"] == 1
          PY

      - name: Upload execution evidence
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
        with:
          name: cortex-m-probe-fixture
          path: target/firmverse-report
          if-no-files-found: error
""",
        encoding="utf-8",
    )


def update_docs() -> None:
    path = ROOT / "docs/DEVICE_TRACE_ABI.md"
    text = path.read_text(encoding="utf-8")
    note = (
        "\nFor the default `mps2-an505` backend, Firmverse links Secure code at "
        "`0x10000000` and uses Secure SRAM at `0x38000000`, matching the board "
        "model's TrustZone aliases. The FVD1 block is writable RAM and must be "
        "published by executing firmware before `firmverse_done`.\n"
    )
    if note.strip() not in text:
        path.write_text(text.rstrip() + "\n" + note, encoding="utf-8")


if __name__ == "__main__":
    apply_base_migration()
    patch_probe()
    patch_action()
    write_durable_workflow()
    update_docs()
