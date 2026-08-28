from __future__ import annotations

import sys
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))

import cortex_m_probe


class CortexMProbeTests(unittest.TestCase):
    def test_parse_and_classify_sections(self) -> None:
        output = """
section             size       addr
.vector_table       1984       0
.text               169300     1984
.rodata             61524      171284
.data               128        536870912
.bss                256        536871040
.uninit             64         536871296
Total               233256
"""
        sections = cortex_m_probe.parse_llvm_size(output)
        flash, static_ram, static_end = cortex_m_probe.classify_sections(
            sections,
            flash_origin=0,
            flash_length=1024 * 1024,
            ram_origin=0x20000000,
            ram_length=256 * 1024,
        )
        self.assertEqual(flash, 1984 + 169300 + 61524 + 128)
        self.assertEqual(static_ram, 128 + 256 + 64)
        self.assertEqual(static_end, 0x20000000 + 128 + 256 + 64)

    def test_stack_watermark_finds_deepest_write(self) -> None:
        data = bytearray([cortex_m_probe.PATTERN_BYTE] * 4096)
        data[4096 - 768] = 0x42
        data[-4:] = b"used"
        peak, lowest = cortex_m_probe.scan_stack_watermark(bytes(data))
        self.assertEqual(peak, 768)
        self.assertEqual(lowest, 4096 - 768)

    def test_untouched_stack_is_rejected(self) -> None:
        data = bytes([cortex_m_probe.PATTERN_BYTE] * 128)
        with self.assertRaisesRegex(ValueError, "probably did not execute"):
            cortex_m_probe.scan_stack_watermark(data)

    def test_alignment_and_integer_input(self) -> None:
        self.assertEqual(cortex_m_probe.align_up(1025, 1024), 2048)
        self.assertEqual(cortex_m_probe.parse_int("0x20000000"), 0x20000000)
        self.assertEqual(cortex_m_probe.parse_int("262144"), 262144)


    def test_parses_fvd1_device_trace(self) -> None:
        words = [
            cortex_m_probe.DEVICE_TRACE_MAGIC,
            1,
            cortex_m_probe.DEVICE_TRACE_BYTES,
            1,
            0x1F,
            3,
            2,
            0x12345678,
            32,
            0x87654321,
            2,
            2,
            0xAABBCCDD,
            1,
            0xDDCCBBAA,
            0,
        ]
        data = __import__("struct").pack("<16I", *words)
        trace = cortex_m_probe.parse_device_trace(
            data, "trace", 0x20000100
        )
        self.assertEqual(trace.status, "pass")
        self.assertEqual(trace.capabilities, 0x1F)
        self.assertEqual(trace.storage_generation, 2)
        self.assertEqual(
            cortex_m_probe.validate_device_trace(trace, 0x1F), ()
        )

    def test_device_trace_fails_closed(self) -> None:
        words = [
            cortex_m_probe.DEVICE_TRACE_MAGIC,
            1,
            64,
            2,
            1,
        ] + [0] * 11
        data = __import__("struct").pack("<16I", *words)
        trace = cortex_m_probe.parse_device_trace(
            data, "trace", 0x20000100
        )
        errors = cortex_m_probe.validate_device_trace(trace, 0x1F)
        self.assertTrue(any("status" in error for error in errors))
        self.assertTrue(any("capability" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
