"""Intel HEX parsing and PHY62xx boot-image layout."""

from __future__ import annotations

import pathlib
import struct
from dataclasses import dataclass

FLASH_BASE = 0x1100_0000
SRAM_BASE = 0x1FFF_0000
MAX_FLASH_SIZE = 0x20_0000
BOOT_HEADER_ADDR = 0x2000
SRAM_IMAGE_ADDR = 0x5000
DEFAULT_ENTRY = 0x1FFF_1838


class ImageError(RuntimeError):
    """Invalid or unsupported firmware image."""


@dataclass(frozen=True)
class Segment:
    load_addr: int
    data: bytes
    flash_addr: int = 0


def _hex_byte(line: str, start: int, end: int) -> int:
    try:
        return int(line[start:end], 16)
    except ValueError as exc:
        raise ImageError("invalid hexadecimal field") from exc


def parse_intel_hex(path: pathlib.Path) -> list[Segment]:
    """Parse Intel HEX into contiguous absolute-address segments."""
    upper = 0
    segments: list[Segment] = []
    current_addr: int | None = None
    current = bytearray()
    eof_seen = False

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
        if eof_seen:
            raise ImageError(f"data found after EOF record at line {number}")
        if not line.startswith(":") or len(line) < 11 or len(line) % 2 == 0:
            raise ImageError(f"invalid Intel HEX record at line {number}")

        count = _hex_byte(line, 1, 3)
        expected_len = 11 + count * 2
        if len(line) != expected_len:
            raise ImageError(f"invalid Intel HEX record length at line {number}")

        offset = _hex_byte(line, 3, 7)
        kind = _hex_byte(line, 7, 9)
        try:
            payload = bytes.fromhex(line[9 : 9 + count * 2])
            record = bytes.fromhex(line[1:])
        except ValueError as exc:
            raise ImageError(f"invalid Intel HEX data at line {number}") from exc
        if sum(record) & 0xFF:
            raise ImageError(f"bad Intel HEX checksum at line {number}")

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
            if count != 0:
                raise ImageError(f"bad EOF record at line {number}")
            flush()
            eof_seen = True
        elif kind == 0x04:
            flush()
            if len(payload) != 2:
                raise ImageError(f"bad extended linear address at line {number}")
            upper = int.from_bytes(payload, "big") << 16
        elif kind in (0x03, 0x05):
            # Entry records are not used by the PHY62xx flash header.
            continue
        else:
            raise ImageError(f"unsupported Intel HEX record type 0x{kind:02x}")

    flush()
    if not eof_seen:
        raise ImageError("Intel HEX is missing EOF record")
    if not segments:
        raise ImageError("HEX file contains no data")
    return segments


def _overlaps(start: int, end: int, ranges: list[tuple[int, int]]) -> bool:
    return any(start < other_end and end > other_start for other_start, other_end in ranges)


def prepare_phy_hex(segments: list[Segment], entry: int = DEFAULT_ENTRY) -> list[Segment]:
    """Map SDK HEX load regions to PHY62xx flash and prepend ROM boot table."""
    mapped: list[Segment] = []
    direct_ranges: list[tuple[int, int]] = []
    sram_segments: list[Segment] = []

    for seg in segments:
        if not seg.data:
            continue
        end = seg.load_addr + len(seg.data)
        if end < seg.load_addr:
            raise ImageError("segment address overflow")
        if FLASH_BASE <= seg.load_addr and end <= FLASH_BASE + MAX_FLASH_SIZE:
            flash_addr = seg.load_addr - FLASH_BASE
            flash_end = flash_addr + len(seg.data)
            if _overlaps(flash_addr, flash_end, direct_ranges):
                raise ImageError(
                    f"HEX flash ranges overlap at 0x{flash_addr:x}..0x{flash_end:x}"
                )
            direct_ranges.append((flash_addr, flash_end))
            mapped.append(Segment(seg.load_addr, seg.data, flash_addr))
        elif SRAM_BASE <= seg.load_addr and end <= SRAM_BASE + 0x10000:
            sram_segments.append(seg)
        else:
            raise ImageError(
                f"HEX segment 0x{seg.load_addr:08x}..0x{end:08x} is neither PHY62xx flash nor SRAM"
            )

    sram_total = sum((len(seg.data) + 3) & ~3 for seg in sram_segments)
    ram_cursor = SRAM_IMAGE_ADDR
    if sram_total and _overlaps(ram_cursor, ram_cursor + sram_total, direct_ranges):
        ram_cursor = max((end for _start, end in direct_ranges), default=ram_cursor)
        ram_cursor = (ram_cursor + 3) & ~3

    finalized = list(mapped)
    ranges = list(direct_ranges)
    for seg in sram_segments:
        flash_addr = ram_cursor
        flash_end = flash_addr + len(seg.data)
        if flash_end > MAX_FLASH_SIZE:
            raise ImageError("prepared image exceeds maximum supported flash size")
        if _overlaps(flash_addr, flash_end, ranges):
            raise ImageError(
                f"prepared flash ranges overlap at 0x{flash_addr:x}..0x{flash_end:x}"
            )
        ranges.append((flash_addr, flash_end))
        finalized.append(Segment(seg.load_addr, seg.data, flash_addr))
        ram_cursor = (flash_end + 3) & ~3

    finalized.sort(key=lambda seg: seg.flash_addr)
    header = bytearray(b"\xff" * 0x100)
    header[0:4] = len(finalized).to_bytes(4, "little")
    header[8:12] = entry.to_bytes(4, "little")
    pos = 16
    for seg in finalized:
        if pos + 16 > len(header):
            raise ImageError("too many HEX segments for PHY62xx boot header")
        header[pos : pos + 16] = struct.pack(
            "<IIII", seg.flash_addr, len(seg.data), seg.load_addr, 0xFFFF_FFFF
        )
        pos += 16

    header_end = BOOT_HEADER_ADDR + len(header)
    if _overlaps(BOOT_HEADER_ADDR, header_end, ranges):
        raise ImageError("firmware data overlaps PHY62xx boot header at flash 0x2000")

    return [Segment(0, bytes(header), BOOT_HEADER_ADDR), *finalized]
