"""PHY62XX SDK 3.1.2 validation and vendor BLE smoke build."""

from __future__ import annotations

import pathlib
import shutil
import subprocess
from dataclasses import dataclass


class SdkError(RuntimeError):
    """SDK tree is missing, mismatched, or cannot be built."""


@dataclass(frozen=True)
class Sdk312Info:
    root: pathlib.Path
    keil_rf: pathlib.Path
    keil_host: pathlib.Path
    gcc_rf: pathlib.Path
    gcc_host: pathlib.Path
    gcc_sec_boot: pathlib.Path
    ble_example_makefile: pathlib.Path


_REQUIRED_MARKER = "PHY62XX_SDK_3.1.2"


def verify_sdk_312(root: pathlib.Path) -> Sdk312Info:
    """Fail closed unless *root* is the SDK 3.1.2 tree with real BLE/RF libs."""
    root = root.expanduser().resolve()
    note = root / "release_note.md"
    recipe = root / "_bld_script" / "bld_v312.yml"
    components = root / "components" / "gcc" / "components.mk"
    example = root / "example" / "ble_peripheral" / "simpleBlePeripheral" / "gcc" / "Makefile"
    info = Sdk312Info(
        root=root,
        keil_rf=root / "lib" / "rf.lib",
        keil_host=root / "lib" / "ble_host.lib",
        gcc_rf=root / "lib" / "libphy6222_rf.a",
        gcc_host=root / "lib" / "libphy6222_host.a",
        gcc_sec_boot=root / "lib" / "libphy6222_sec_boot.a",
        ble_example_makefile=example,
    )

    required = [
        note,
        recipe,
        components,
        example,
        info.keil_rf,
        info.keil_host,
        info.gcc_rf,
        info.gcc_host,
        info.gcc_sec_boot,
    ]
    missing = [path for path in required if not path.is_file()]
    if missing:
        joined = ", ".join(str(path.relative_to(root)) for path in missing)
        raise SdkError(f"SDK 3.1.2 check failed; missing: {joined}")

    notes = note.read_text(encoding="utf-8", errors="ignore")
    if _REQUIRED_MARKER not in notes:
        raise SdkError(f"release_note.md does not identify {_REQUIRED_MARKER}")

    recipe_text = recipe.read_text(encoding="utf-8", errors="ignore")
    if "rf.lib" not in recipe_text or "ble_host.lib" not in recipe_text:
        raise SdkError("SDK 3.1.2 release recipe does not build rf.lib + ble_host.lib")

    gcc_text = components.read_text(encoding="utf-8", errors="ignore")
    for flag in ("-lphy6222_rf", "-lphy6222_host", "-lphy6222_sec_boot"):
        if flag not in gcc_text:
            raise SdkError(f"SDK GCC components do not link required vendor library {flag}")

    make_text = example.read_text(encoding="utf-8", errors="ignore")
    if "components.mk" not in make_text or "arm-none-eabi-gcc" not in make_text:
        raise SdkError("SDK BLE GCC example is not the expected vendor build")

    return info


def build_vendor_ble_example(info: Sdk312Info) -> pathlib.Path:
    """Build the SDK's own BLE peripheral with its vendor RF/host libraries."""
    if shutil.which("arm-none-eabi-gcc") is None:
        raise SdkError("arm-none-eabi-gcc is required to build the SDK 3.1.2 BLE example")
    if shutil.which("make") is None:
        raise SdkError("make is required to build the SDK 3.1.2 BLE example")

    workdir = info.ble_example_makefile.parent
    result = subprocess.run(
        ["make", "clean", "all"],
        cwd=workdir,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if result.returncode != 0:
        tail = "\n".join(result.stdout.splitlines()[-40:])
        raise SdkError(f"SDK 3.1.2 BLE example build failed:\n{tail}")

    image = workdir / "output" / "sbp.ihex"
    if not image.is_file() or image.stat().st_size == 0:
        raise SdkError("SDK BLE build completed without output/sbp.ihex")
    return image
