"""PHY62xx ROM UART monitor transport."""

from __future__ import annotations

import io
import time

import serial

START_BAUD = 9_600
DEFAULT_BAUD = 115_200
SRAM_BASE = 0x1FFF_0000
SECTOR_SIZE = 0x1000
WRITE_BLOCK = 0x2000


class RomError(RuntimeError):
    """PHY62xx ROM monitor communication failed."""


class RomMonitor:
    def __init__(self, port: str, baud: int = DEFAULT_BAUD, *, auto_boot: bool = True):
        self.port_name = port
        self.run_baud = baud
        self.auto_boot = auto_boot
        self.port = serial.Serial(port, START_BAUD, timeout=1)
        self.flash_size = 0x40000
        self.block_no = 0

    def __enter__(self) -> "RomMonitor":
        return self

    def __exit__(self, _exc_type, _exc, _tb) -> None:
        self.close()

    def close(self) -> None:
        self.port.close()

    def _expect_ok(self, command: str, timeout: float = 1.0) -> None:
        old_timeout = self.port.timeout
        self.port.timeout = timeout
        try:
            self.port.write(command.encode("ascii"))
            reply = self.port.read(6)
        finally:
            self.port.timeout = old_timeout
        if reply != b"#OK>>:":
            raise RomError(f"ROM command failed: {command!r}, reply={reply!r}")

    def _enter_monitor(self) -> None:
        if self.auto_boot:
            # Common adapters: RTS -> RST_N and DTR -> TM/test-mode control.
            self.port.rts = True
            self.port.dtr = True
            time.sleep(0.1)
            self.port.dtr = False
            self.port.rts = False
        self.port.reset_input_buffer()
        self.port.reset_output_buffer()
        self.port.timeout = 0.04

    def connect(self) -> str:
        self._enter_monitor()
        for _ in range(250):
            self.port.write(b"UXTDWU")
            reply = self.port.read(6)
            if reply == b"cmd>>:":
                break
            if reply == b"fct>>:":
                raise RomError("chip is in FCT mode; recover it with vendor tooling first")
        else:
            hint = (
                "check TX/RX, GND, 3.3V, RTS->RST_N and DTR->TM"
                if self.auto_boot
                else "put the chip in ROM UART boot mode, then check TX/RX, GND and 3.3V"
            )
            raise RomError(f"ROM monitor did not answer; {hint}")

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
            raise RomError(f"cannot read chip revision: {data!r}")
        body = data[:-6].decode("ascii", errors="replace")
        if body.startswith("0x"):
            body = body[2:]
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
            raise RomError(f"rdreg 0x{addr:08x} failed: {data!r}")
        return int(data[1:11], 16)

    def write_reg(self, addr: int, value: int) -> None:
        self._expect_ok(f"wrreg{addr:08x} {value:08x} ")

    def _flash_command(
        self,
        opcode: int,
        *,
        data: int = 0,
        write_len: int = 0,
        addr: int = 0,
        addr_len: int = 0,
        read_len: int = 0,
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
            # Some ROM revisions switch speed before the acknowledgement is read.
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
            encoded = cursor | self.flash_size
            if cursor % 0x10000 == 0 and left >= 0x10000:
                self._expect_ok(f"er64k {encoded:X}", timeout=2.0)
                cursor += 0x10000
            else:
                self._expect_ok(f"era4k {encoded:X}", timeout=0.7)
                cursor += SECTOR_SIZE

    def write_region(self, offset: int, data: bytes, *, erase: bool = True) -> None:
        if offset < 0 or offset + len(data) > self.flash_size:
            raise RomError(
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
                raise RomError(f"cpbin was not accepted at flash offset 0x{cursor:x}")
            self.port.write(chunk)
            checksum_reply = self.port.read(23)
            prefix = b"checksum is: 0x"
            if not checksum_reply.startswith(prefix) or len(checksum_reply) < len(prefix) + 8:
                raise RomError(f"bad checksum challenge: {checksum_reply!r}")
            self.port.write(checksum_reply[len(prefix) : len(prefix) + 8])
            if self.port.read(6) != b"#OK>>:":
                raise RomError(f"ROM rejected data block at flash offset 0x{cursor:x}")
            self.block_no += 1
            cursor += size
            remaining -= size

    def reset(self) -> None:
        self.port.write(b"reset ")
