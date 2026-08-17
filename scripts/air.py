#!/usr/bin/env python3
"""Bridge Mac BLE air ↔ phy6252-emu stdin/stdout.

The PHY6252 link layer is not in this tree (no vendor BLE ROM). The laptop
radio is the RF PHY: GAP/GATT on CoreBluetooth, ATT payloads in the hex mailbox.
"""
from __future__ import annotations

import os
import select
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EMU = ROOT / "target" / "release" / "phy6252-emu"
DEBUG = ROOT / "target" / "debug" / "phy6252-emu"
BLE = ROOT / "host" / "ble" / "phy6252-ble"
HEX = Path(os.environ.get("PHY6252_HEX", ROOT / "firmware" / "kit-demo.hex"))
NAME = os.environ.get("PHY6252_BLE_NAME", "PB03FKIT")[:8]


def find_emu() -> Path:
    if EMU.is_file():
        return EMU
    if DEBUG.is_file():
        return DEBUG
    sys.exit("build the emulator first: cargo build --release")


def spawn(cmd: list[str]) -> subprocess.Popen[str]:
    return subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )


def put(proc: subprocess.Popen[str], line: str) -> None:
    if proc.stdin is None:
        return
    proc.stdin.write(line + "\n")
    proc.stdin.flush()


def main() -> int:
    if not BLE.is_file():
        sys.exit("build the BLE host first: bash host/ble/build.sh")
    if not HEX.is_file():
        sys.exit(f"no hex at {HEX}")

    emu = spawn([str(find_emu()), "--live", str(HEX)])
    ble = spawn([str(BLE), "--name", NAME])
    assert emu.stdout and emu.stderr and ble.stdout and ble.stderr
    print(f"air name={NAME} hex={HEX}", flush=True)

    streams = {
        emu.stdout: "emu",
        emu.stderr: "emu-err",
        ble.stdout: "ble",
        ble.stderr: "ble-err",
    }
    try:
        while True:
            if emu.poll() is not None and ble.poll() is not None:
                break
            ready, _, _ = select.select(list(streams), [], [], 0.2)
            for pipe in ready:
                line = pipe.readline()
                if line == "":
                    continue
                text = line.rstrip("\n")
                tag = streams[pipe]
                if tag == "emu":
                    if text.startswith("FRAME "):
                        put(ble, "TX " + text[6:].strip())
                    print(text, flush=True)
                elif tag == "ble":
                    if text.startswith("RX "):
                        put(emu, "WRITE " + text[3:].strip())
                    elif text == "CONNECTED":
                        put(emu, "CONNECT")
                    elif text == "SUBSCRIBED":
                        put(emu, "CCCD 1")
                    elif text == "DISCONNECTED":
                        put(emu, "DISCONNECT")
                    print("BLE " + text, flush=True)
                else:
                    print(text, file=sys.stderr, flush=True)
    except KeyboardInterrupt:
        pass
    finally:
        for proc in (emu, ble):
            if proc.poll() is None:
                proc.terminate()
        for proc in (emu, ble):
            try:
                proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                proc.kill()
    return 0


if __name__ == "__main__":
    if shutil.which("python3") is None:
        sys.exit("python3 required")
    raise SystemExit(main())
