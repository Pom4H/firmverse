"""Command-line entry point for the PHY6252 USB-UART flasher."""

from __future__ import annotations

import argparse
import pathlib
import sys

import serial

from .image import DEFAULT_ENTRY, ImageError, Segment, parse_intel_hex, prepare_phy_hex
from .rom import DEFAULT_BAUD, SECTOR_SIZE, RomError, RomMonitor
from .sdk import SdkError, build_vendor_ble_example, verify_ble_link_map, verify_sdk_312


def _erase_plan(image: list[Segment]) -> list[tuple[int, int]]:
    ranges: list[tuple[int, int]] = []
    for seg in image:
        if not seg.data:
            continue
        start = seg.flash_addr & ~(SECTOR_SIZE - 1)
        end = (seg.flash_addr + len(seg.data) + SECTOR_SIZE - 1) & ~(SECTOR_SIZE - 1)
        ranges.append((start, end))
    ranges.sort()

    merged: list[tuple[int, int]] = []
    for start, end in ranges:
        if merged and start <= merged[-1][1]:
            merged[-1] = (merged[-1][0], max(merged[-1][1], end))
        else:
            merged.append((start, end))
    return merged


def _show_plan(image: list[Segment]) -> None:
    for start, end in _erase_plan(image):
        print(f"  erase 0x{start:05x}..0x{end:05x} ({end - start} bytes)")
    for seg in image:
        print(
            f"  write 0x{seg.flash_addr:05x} <- load 0x{seg.load_addr:08x}, "
            f"{len(seg.data)} bytes"
        )


def _flash_prepared(mon: RomMonitor, image: list[Segment]) -> None:
    mon.init_flash_writer()
    for start, end in _erase_plan(image):
        mon.erase_region(start, end - start)
    for seg in image:
        mon.write_region(seg.flash_addr, seg.data, erase=False)


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Flash PHY6252/PHY6222 through the chip ROM USB-UART monitor"
    )
    parser.add_argument("image", nargs="?", type=pathlib.Path, help="SDK Intel HEX image")
    parser.add_argument("--port", "-p", help="serial device, e.g. /dev/ttyUSB0 or COM5")
    parser.add_argument("--baud", type=int, default=DEFAULT_BAUD, help="transfer baud")
    parser.add_argument("--entry", type=lambda value: int(value, 0), default=DEFAULT_ENTRY)
    parser.add_argument(
        "--sdk-root",
        type=pathlib.Path,
        required=True,
        help="PHY62XX SDK 3.1.2 root",
    )
    parser.add_argument(
        "--vendor-ble-smoke",
        action="store_true",
        help="build and flash the SDK 3.1.2 simpleBlePeripheral GCC example",
    )
    parser.add_argument(
        "--link-map",
        type=pathlib.Path,
        help="linker map proving that vendor RF and BLE host libraries are in a custom image",
    )
    parser.add_argument(
        "--allow-unverified-image",
        action="store_true",
        help="flash a custom HEX without BLE library provenance (unsafe for BLE bring-up)",
    )
    parser.add_argument("--dry-run", action="store_true", help="validate and print the flash plan only")
    parser.add_argument("--no-reset", action="store_true", help="do not reset MCU after programming")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)
    try:
        sdk = verify_sdk_312(args.sdk_root)
        print("SDK 3.1.2 verified: Keil and GCC vendor RF/BLE libraries are present")

        if args.vendor_ble_smoke:
            if args.image is not None or args.link_map is not None:
                raise SdkError("--vendor-ble-smoke selects its own SDK image and linker map")
            image_path, link_map = build_vendor_ble_example(sdk)
            provenance = verify_ble_link_map(link_map)
            print(f"Vendor BLE smoke image: {image_path}")
            print(f"BLE provenance: {provenance}")
        else:
            if args.image is None:
                raise ImageError("provide an Intel HEX image or use --vendor-ble-smoke")
            image_path = args.image
            if image_path.suffix.lower() not in (".hex", ".ihex"):
                raise ImageError("use an SDK-produced Intel HEX image; raw .bin is not accepted")
            if args.link_map is not None:
                provenance = verify_ble_link_map(args.link_map)
                print(f"BLE provenance: {provenance}")
            elif not args.allow_unverified_image:
                raise SdkError(
                    "custom HEX requires --link-map so the flasher can verify real SDK RF/BLE "
                    "libraries; use --allow-unverified-image only for non-BLE bring-up"
                )
            else:
                print("WARNING: custom image BLE library provenance was not verified", file=sys.stderr)

        prepared = prepare_phy_hex(parse_intel_hex(image_path), args.entry)
        _show_plan(prepared)
        if args.dry_run:
            print("Dry run complete; device was not touched")
            return 0
        if not args.port:
            raise RomError("--port is required unless --dry-run is used")

        with RomMonitor(args.port, args.baud) as mon:
            revision = mon.connect()
            print(f"ROM connected: {revision or 'PHY62xx'}; flash={mon.flash_size // 1024} KiB")
            _flash_prepared(mon, prepared)
            if not args.no_reset:
                mon.reset()
        print("Flash complete")
        return 0
    except (ImageError, SdkError, RomError, OSError, serial.SerialException) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
