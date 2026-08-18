from __future__ import annotations

import pathlib
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

from phyflash.cli import _erase_plan
from phyflash.image import BOOT_HEADER_ADDR, ImageError, Segment, parse_intel_hex, prepare_phy_hex
from phyflash.sdk import SdkError, apply_sdk_312_gcc_compat, verify_ble_link_map, verify_sdk_312


def hex_record(address: int, kind: int, data: bytes = b"") -> str:
    body = bytes([len(data), (address >> 8) & 0xFF, address & 0xFF, kind]) + data
    checksum = (-sum(body)) & 0xFF
    return ":" + (body + bytes([checksum])).hex().upper()


class ImageTests(unittest.TestCase):
    def test_parse_extended_linear_hex(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "fw.hex"
            path.write_text(
                "\n".join(
                    [
                        hex_record(0, 4, bytes.fromhex("1102")),
                        hex_record(0x0010, 0, b"ABCD"),
                        hex_record(0, 1),
                    ]
                )
                + "\n",
                encoding="ascii",
            )
            segments = parse_intel_hex(path)
            self.assertEqual(segments, [Segment(0x11020010, b"ABCD")])

    def test_rejects_missing_eof(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "fw.hex"
            path.write_text(hex_record(0, 0, b"x") + "\n", encoding="ascii")
            with self.assertRaisesRegex(ImageError, "missing EOF"):
                parse_intel_hex(path)

    def test_layout_keeps_sdk_flash_and_stages_sram(self) -> None:
        prepared = prepare_phy_hex(
            [
                Segment(0x11020000, b"F" * 32),
                Segment(0x1FFF1880, b"R" * 16),
            ]
        )
        self.assertEqual(prepared[0].flash_addr, BOOT_HEADER_ADDR)
        by_load = {seg.load_addr: seg for seg in prepared[1:]}
        self.assertEqual(by_load[0x11020000].flash_addr, 0x20000)
        self.assertEqual(by_load[0x1FFF1880].flash_addr, 0x5000)

    def test_layout_rejects_boot_header_overlap(self) -> None:
        with self.assertRaisesRegex(ImageError, "boot header"):
            prepare_phy_hex([Segment(0x11002000, b"X" * 32)])

    def test_erase_plan_merges_shared_sectors(self) -> None:
        plan = _erase_plan(
            [
                Segment(0, b"A" * 100, 0x2100),
                Segment(0, b"B" * 100, 0x2F00),
                Segment(0, b"C" * 100, 0x4000),
            ]
        )
        self.assertEqual(plan, [(0x2000, 0x3000), (0x4000, 0x5000)])


class SdkTests(unittest.TestCase):
    def _fake_sdk(self, root: pathlib.Path) -> None:
        files = {
            "release_note.md": "Version PHY62XX_SDK_3.1.2\n",
            "_bld_script/bld_v312.yml": "rf.lib ble_host.lib\n",
            "components/gcc/components.mk": (
                "LIBS += -lphy6222_rf\n"
                "LIBS += -lphy6222_host\n"
                "LIBS += -lphy6222_sec_boot\n"
            ),
            "components/profiles/ppsp/ppsp_impl.c": (
                'logs_war("!! MSGS LOSS, ALLS DROP !! \\r\\n",);\n'
            ),
            "example/ble_peripheral/simpleBlePeripheral/gcc/Makefile": (
                "include $(ROOT)/components/gcc/components.mk\narm-none-eabi-gcc\n"
            ),
            "lib/rf.lib": "x",
            "lib/ble_host.lib": "x",
            "lib/libphy6222_rf.a": "x",
            "lib/libphy6222_host.a": "x",
            "lib/libphy6222_sec_boot.a": "x",
        }
        for relative, content in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")

    def test_sdk_312_validation_checks_both_toolchains(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            self._fake_sdk(root)
            info = verify_sdk_312(root)
            self.assertEqual(info.root, root.resolve())

    def test_sdk_312_gcc_compat_patches_known_ppsp_bug(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            self._fake_sdk(root)
            info = verify_sdk_312(root)
            self.assertTrue(apply_sdk_312_gcc_compat(info))
            self.assertIn(
                'logs_war("!! MSGS LOSS, ALLS DROP !! \\r\\n");',
                info.ppsp_impl_source.read_text(encoding="utf-8"),
            )
            self.assertFalse(apply_sdk_312_gcc_compat(info))

    def test_sdk_312_gcc_compat_fails_closed_on_unknown_source(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            self._fake_sdk(root)
            info = verify_sdk_312(root)
            info.ppsp_impl_source.write_text("unexpected vendor source\n", encoding="utf-8")
            with self.assertRaisesRegex(SdkError, "signature changed"):
                apply_sdk_312_gcc_compat(info)

    def test_link_map_proves_gcc_ble_libraries(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "build.map"
            path.write_text(
                "lib/libphy6222_rf.a(foo.o)\nlib/libphy6222_host.a(bar.o)\n",
                encoding="utf-8",
            )
            self.assertIn("GCC", verify_ble_link_map(path))

    def test_link_map_rejects_unproven_image(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "build.map"
            path.write_text("main.o\n", encoding="utf-8")
            with self.assertRaisesRegex(SdkError, "does not prove"):
                verify_ble_link_map(path)


if __name__ == "__main__":
    unittest.main()
