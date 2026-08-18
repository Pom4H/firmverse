#!/usr/bin/env python3
"""Minimal PHY62xx ROM USB-UART flash utility.

The PHY6252 contains a ROM serial monitor used by vendor tooling.  This tool
speaks that monitor directly; it does not depend on PhyPlusKit.

Typical use:
    python -m pip install -r tools/requirements.txt
    python tools/phy6252_flash.py --port /dev/ttyUSB0 --sdk-root ../PHY6252_6222_SDK firmware.hex

The firmware image itself must be built with the real PHY62XX SDK libraries.
For BLE firmware use SDK 3.1.2 rf.lib + ble_host.lib; this utility only writes
the resulting image to flash.
"""

from __future__ import annotations

import argparse
import io
import pathlib
import struct
import sys
import time
from dataclasses import dataclass

import serial

START_BAUD = 9_600
DEFAULT_BAUD = 115_200
FLASH_BASE = 0x1100_0000
SRAM_BASE = 0x1FFF_0000
MAX_FLASH_SIZE = 0x20_0000
SECTOR_SIZE = 0x1000
WRITE_BLOCK = 0x2000
BOOT_HEADER_ADDR = 0x2000
SRAM_IMAGE_ADDR = 0x5000
DEFAULT_ENTRY = 0x1FFF_1838


class FlashError(RuntimeError):
    pass


@dataclass
class Segment:
    load_addr: int
    data: bytes
    flash_addr: int = 0


def _hex_byte(line: str, start: int, end: int) -> int:
    return int(line[start:end], 16)


def parse_intel_hex(path: pathlib.Path) -> list[Segment]:
    """Parse Intel HEX into contiguous absolute-address segments."""
    upper = 0
    segments: list[Segment] = []
    current_addr: int | None = None
    current = bytearray()

    def flush() -> None:
        nonlocal current_addr, current
        if current_addr is not None and current:
            segments.append(Segment(current_addr, bytes(current)))
        current_addr = None
        current = bytearray()

    for number, raw in enumerate(path.read_text(encoding="ascii").splitlines(), 1):
        line = raw.strip()
        if not line:
            continue
        if not line.startswith(":") or len(line) < 11:
            raise FlashError(f"invalid Intel HEX record at line {number}")
        count = _hex_byte(line, 1, 3)
        offset = _hex_byte(line, 3, 7)
        kind = _hex_byte(line, 7, 9)
        payload = bytes.fromhex(line[9 : 9 + count * 2])
        checksum = _hex_byte(line, 9 + count * 2, 11 + count * 2)
        record = bytes.fromhex(line[1 : 9 + count * 2]) + bytes([checksum])
        if sum(record) & 0xFF:
            raise FlashError(f"bad Intel HEX checksum at line {number}")

        if kind == 0x00:
            absolute = upper + offset
            if current_addr is None:
                current_addr = absolute
            expected = current_addr + len(current)
            if absolute != expected:
                flush()
                current_addr = absolute
            current.extend(payload)
        elif kind == 0x01:
            flush()
            break
        elif kind == 0x04:
            flush()
            if len(payload) != 2:
                raise FlashError(f"bad extended linear address at line {number}")
            upper = int.from_bytes(payload, "big") << 16
        elif kind in (0x03, 0x05):
            # Entry records are not used by the PHY62xx flash header.
            continue
        else:
            raise FlashError(f"unsupported Intel HEX record type 0x{kind:02x}")

    flush()
    if not segments:
        raise FlashError("HEX file contains no data")
    return segments


def prepare_phy_hex(segments: list[Segment], entry: int) -> list[Segment]:
    """Map SDK HEX load regions to PHY62xx flash and prepend ROM boot table."""
    flash_end = 0
    sram_total = 0
    for seg in segments:
        if FLASH_BASE <= seg.load_addr < FLASH_BASE + MAX_FLASH_SIZE:
            start = seg.load_addr - FLASH_BASE
            seg.flash_addr = start
            flash_end = max(flash_end, start + len(seg.data))
        elif SRAM_BASE <= seg.load_addr < SRAM_BASE + 0x10000:
            sram_total += len(seg.data)
        else:
            raise FlashError(
                f"HEX segment 0x{seg.load_addr:08x} is neither PHY62xx flash nor SRAM"
            )

    ram_cursor = SRAM_IMAGE_ADDR
    if ram_cursor + sram_total >= flash_end and ram_cursor < flash_end:
        ram_cursor = (flash_end + 3) & ~3

    for seg in segments:
        if SRAM_BASE <= seg.load_addr < SRAM_BASE + 0x10000:
            seg.flash_addr = ram_cursor
            ram_cursor = (ram_cursor + len(seg.data) + 3) & ~3

    header = bytearray(b"\xff" * 0x100)
    header[0:4] = len(segments).to_bytes(4, "little")
    header[8:12] = entry.to_bytes(4, "little")
    pos = 16
    for seg in segments:
        if pos + 16 > len(header):
            raise FlashError("too many HEX segments for PHY62xx boot header")
        header[pos : pos + 16] = struct.pack(
            "<IIII", seg.flash_addr, len(seg.data), seg.load_addr, 0xFFFF_FFFF
        )
        pos += 16

    return [Segment(0, bytes(header), BOOT_HEADER_ADDR), *segments]


def verify_sdk_312(root: pathlib.Path) -> None:
    """Fail closed unless the selected SDK is demonstrably the 3.1.2 tree."""
    note = root / "release_note.md"
    build = root / "_bld_script" / "bld_v312.yml"
    rf = root / "lib" / "rf.lib"
    ble = root / "lib" / "ble_host.lib"

    missing = [p for p in (note, build, rf, ble) if not p.exists()]
    if missing:
        joined = ", ".join(str(p.relative_to(root)) for p in missing)
        raise FlashError(f"SDK 3.1.2 check failed; missing: {joined}")
    text = note.read_text(encoding="utf-8", errors="ignore")
    if "PHY62XX_SDK_3.1.2" not in text:
        raise FlashError("release_note.md does not identify PHY62XX_SDK_3.1.2")
    recipe = build.read_text(encoding="utf-8", errors="ignore")
    if "rf.lib" not in recipe or "ble_host.lib" not in recipe:
        raise FlashError("SDK 3.1.2 build recipe does not reference rf.lib + ble_host.lib")


class RomMonitor:
    def __init__(self, port: str, baud: int):
        self.port_name = port
        self.run_baud = baud
        self.port = serial.Serial(port, START_BAUD, timeout=1)
        self.flash_size = 0x40000
        self.block_no = 0

    def close(self) -> None:
        self.port.close()

    def _expect_ok(self, command: str, timeout: float = 1.0) -> None:
        old = self.port.timeout
        self.port.timeout = timeout
        self.port.write(command.encode("ascii"))
        reply = self.port.read(6)
        self.port.timeout = old
        if reply != b"#OK>>:":
            raise FlashError(f"ROM command failed: {command!r}, reply={reply!r}")

    def connect(self) -> str:
        # Common USB-UART adapters expose RTS->RST_N and DTR->TM.
        self.port.rts = True
        self.port.dtr = True
        time.sleep(0.1)
        self.port.reset_input_buffer()
        self.port.reset_output_buffer()
        self.port.dtr = False
        self.port.rts = False
        self.port.timeout = 0.04

        reply = b""
        for _ in range(250):
            self.port.write(b"UXTDWU")
            reply = self.port.read(6)
            if reply == b"cmd>>:":
                break
            if reply == b"fct>>:":
                raise FlashError("chip is in FCT mode; erase/recover it with vendor tooling")
        else:
            raise FlashError(
                "ROM monitor did not answer; check TX/RX, GND, 3.3V, RTS->RST_N and DTR->TM"
            )

        # The ROM monitor switches to its normal command baud after activation.
        self.port.baudrate = DEFAULT_BAUD
        self.port.timeout = 0.2
        revision = self.read_revision()
        self._unlock_flash()
        self.write_reg(0x4000_F054, 0)
        self.write_reg(0x4000_F140, 0)
        self.write_reg(0x4000_F144, 0)
        self.set_baud(self.run_baud)
        return revision

    def read_revision(self) -> str:
        self.port.write(b"rdrev+ ")
        data = self.port.read(26)
        if not data.endswith(b"#OK>>:"):
            raise FlashError(f"cannot read chip revision: {data!r}")
        body = data[:-6].decode("ascii", errors="replace")
        if body.startswith("0x"):
            body = body[2:]
        # PHY62xx ROM reports the JEDEC ID before the family string.  Capacity
        # is encoded in the JEDEC density byte (e.g. 0x12=>256 KiB, 0x13=>512 KiB).
        try:
            flash_id = int(body[:8], 16)
            density = (flash_id >> 16) & 0xFF
            if 0x10 <= density <= 0x18:
                self.flash_size = 1 << density
        except ValueError:
            pass
        return body.strip()

    def read_reg(self, addr: int) -> int:
        self.port.write(f"rdreg{addr:08x}".encode("ascii"))
        data = self.port.read(17)
        if len(data) != 17 or not data.startswith(b"=0x") or not data.endswith(b"#OK>>:"):
            raise FlashError(f"rdreg 0x{addr:08x} failed: {data!r}")
        return int(data[1:11], 16)

    def write_reg(self, addr: int, value: int) -> None:
        self._expect_ok(f"wrreg{addr:08x} {value:08x} ")

    def _flash_command(
        self, opcode: int, *, data: int = 0, write_len: int = 0,
        addr: int = 0, addr_len: int = 0, read_len: int = 0
    ) -> None:
        reg = opcode << 24
        if write_len:
            self.write_reg(0x4000_C8A8, data)
            reg |= 0x8000 | ((write_len - 1) << 12)
        if addr_len:
            self.write_reg(0x4000_C894, addr)
            reg |= 0x80000 | ((addr_len - 1) << 16)
        if read_len:
            reg |= 0x800000 | ((read_len - 1) << 20)
        self.write_reg(0x4000_C890, reg | 1)

    def _unlock_flash(self) -> None:
        self._flash_command(0x06)
        self._flash_command(0x01, data=0, write_len=1)

    def set_baud(self, baud: int) -> None:
        if baud == self.port.baudrate:
            return
        self.port.timeout = 0.7
        self.port.write(f"uarts{baud}".encode("ascii"))
        reply = self.port.read(3)
        self.port.baudrate = baud
        self.port.timeout = 0.2
        time.sleep(0.05)
        self.port.reset_input_buffer()
        self.port.reset_output_buffer()
        if reply != b"#OK":
            # Some ROM revisions change speed before the acknowledgement is read.
            self.read_reg(SRAM_BASE)

    def init_flash_writer(self) -> None:
        self._expect_ok("spifs 0 1 3 0 ")
        self._expect_ok("sfmod 2 2 ")
        self._expect_ok("cpnum ffffffff ")
        self.write_reg(0x1FFF_0898, 0x0040_0000)

    def erase_region(self, offset: int, size: int) -> None:
        first = offset & ~(SECTOR_SIZE - 1)
        end = (offset + size + SECTOR_SIZE - 1) & ~(SECTOR_SIZE - 1)
        cursor = first
        while cursor < end:
            left = end - cursor
            if cursor % 0x10000 == 0 and left >= 0x10000:
                self._expect_ok(f"er64k {cursor | self.flash_size:X}", timeout=2.0)
                cursor += 0x10000
            else:
                self._expect_ok(f"era4k {cursor | self.flash_size:X}", timeout=0.7)
                cursor += SECTOR_SIZE

    def write_region(self, offset: int, data: bytes, *, erase: bool = True) -> None:
        if offset < 0 or offset + len(data) > self.flash_size:
            raise FlashError(
                f"write 0x{offset:x}..0x{offset + len(data):x} exceeds detected flash "
                f"size 0x{self.flash_size:x}"
            )
        if erase:
            self.erase_region(offset, len(data))
        stream = io.BytesIO(data)
        cursor = offset
        remaining = len(data)
        while remaining:
            size = min(WRITE_BLOCK, remaining)
            chunk = stream.read(size)
            command = f"cpbin c{self.block_no} {cursor | self.flash_size:X} {size:X} {cursor:X}"
            self.port.write(command.encode("ascii"))
            if self.port.read(12) != b"by hex mode:":
                raise FlashError(f"cpbin was not accepted at flash offset 0x{cursor:x}")
            self.port.write(chunk)
            checksum_reply = self.port.read(23)
            prefix = b"checksum is: 0x"
            if not checksum_reply.startswith(prefix) or len(checksum_reply) < len(prefix) + 8:
                raise FlashError(f"bad checksum challenge: {checksum_reply!r}")
            self.port.write(checksum_reply[len(prefix) : len(prefix) + 8])
            if self.port.read(6) != b"#OK>>:":
                raise FlashError(f"ROM rejected data block at flash offset 0x{cursor:x}")
            self.block_no += 1
            cursor += size
            remaining -= size

    def reset(self) -> None:
        self.port.write(b"reset ")


def flash_hex(mon: RomMonitor, path: pathlib.Path, entry: int) -> None:
    image = prepare_phy_hex(parse_intel_hex(path), entry)
    mon.init_flash_writer()
    for seg in image:
        print(
            f"  flash 0x{seg.flash_addr:05x} <- load 0x{seg.load_addr:08x}, "
            f"{len(seg.data)} bytes"
        )
        mon.write_region(seg.flash_addr, seg.data)


def main() -> int:
    parser = argparse.ArgumentParser(description="Flash PHY6252/PHY6222 through ROM USB-UART")
    parser.add_argument("image", type=pathlib.Path, help="SDK Intel HEX image")
    parser.add_argument("--port", "-p", required=True, help="serial device, e.g. /dev/ttyUSB0 or COM5")
    parser.add_argument("--baud", type=int, default=DEFAULT_BAUD, help="transfer baud (default: 115200)")
    parser.add_argument("--entry", type=lambda x: int(x, 0), default=DEFAULT_ENTRY)
    parser.add_argument(
        "--sdk-root",
        type=pathlib.Path,
        required=True,
        help="PHY62XX SDK 3.1.2 root; rf.lib and ble_host.lib are verified before flashing",
    )
    parser.add_argument("--no-reset", action="store_true", help="do not reset MCU after programming")
    args = parser.parse_args()

    try:
        verify_sdk_312(args.sdk_root)
        if args.image.suffix.lower() not in (".hex", ".ihex"):
            raise FlashError("use the SDK-produced Intel HEX image; raw .bin is intentionally not accepted")
        print("SDK 3.1.2: rf.lib + ble_host.lib present")
        mon = RomMonitor(args.port, args.baud)
        try:
            revision = mon.connect()
            print(f"ROM connected: {revision or 'PHY62xx'}; flash={mon.flash_size // 1024} KiB")
            flash_hex(mon, args.image, args.entry)
            if not args.no_reset:
                mon.reset()
            print("Flash complete")
        finally:
            mon.close()
    except (FlashError, OSError, serial.SerialException) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
