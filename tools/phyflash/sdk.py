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
    ppsp_impl_source: pathlib.Path


_REQUIRED_MARKER = "PHY62XX_SDK_3.1.2"
_GCC13_BAD_PPSP_LOG_CALL = 'logs_war("!! MSGS LOSS, ALLS DROP !! \\r\\n",);'
_GCC13_FIXED_PPSP_LOG_CALL = 'logs_war("!! MSGS LOSS, ALLS DROP !! \\r\\n");'


def verify_sdk_312(root: pathlib.Path) -> Sdk312Info:
    """Fail closed unless *root* is the SDK 3.1.2 tree with real BLE/RF libs."""
    root = root.expanduser().resolve()
    note = root / "release_note.md"
    recipe = root / "_bld_script" / "bld_v312.yml"
    components = root / "components" / "gcc" / "components.mk"
    example = root / "example" / "ble_peripheral" / "simpleBlePeripheral" / "gcc" / "Makefile"
    ppsp_impl = root / "components" / "profiles" / "ppsp" / "ppsp_impl.c"
    info = Sdk312Info(
        root=root,
        keil_rf=root / "lib" / "rf.lib",
        keil_host=root / "lib" / "ble_host.lib",
        gcc_rf=root / "lib" / "libphy6222_rf.a",
        gcc_host=root / "lib" / "libphy6222_host.a",
        gcc_sec_boot=root / "lib" / "libphy6222_sec_boot.a",
        ble_example_makefile=example,
        ppsp_impl_source=ppsp_impl,
    )

    required = [
        note,
        recipe,
        components,
        example,
        ppsp_impl,
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


def apply_sdk_312_gcc_compat(info: Sdk312Info) -> bool:
    """Patch the one known SDK 3.1.2 C syntax bug rejected by modern GCC.

    The upstream SDK contains a trailing empty variadic argument in ppsp_impl.c:
    ``logs_war("...",);``. GCC 13 rejects the resulting expansion. Keep this
    compatibility shim fail-closed so an upstream source change cannot silently
    mutate arbitrary vendor code.
    """
    path = info.ppsp_impl_source
    text = path.read_text(encoding="utf-8", errors="strict")
    bad_count = text.count(_GCC13_BAD_PPSP_LOG_CALL)
    fixed_count = text.count(_GCC13_FIXED_PPSP_LOG_CALL)

    if bad_count == 0 and fixed_count == 1:
        return False
    if bad_count != 1 or fixed_count != 0:
        raise SdkError(
            "SDK 3.1.2 PPSP GCC compatibility signature changed; refusing to patch vendor source"
        )

    path.write_text(
        text.replace(_GCC13_BAD_PPSP_LOG_CALL, _GCC13_FIXED_PPSP_LOG_CALL, 1),
        encoding="utf-8",
    )
    return True


def verify_ble_link_map(path: pathlib.Path) -> str:
    """Verify that a linker map names the vendor RF and BLE host libraries."""
    if not path.is_file():
        raise SdkError(f"linker map not found: {path}")
    text = path.read_text(encoding="utf-8", errors="ignore").lower()
    gcc = "libphy6222_rf.a" in text and "libphy6222_host.a" in text
    keil = "rf.lib" in text and "ble_host.lib" in text
    if gcc:
        return "GCC vendor RF + BLE host"
    if keil:
        return "Keil vendor RF + BLE host"
    raise SdkError(
        "linker map does not prove that PHY62XX SDK RF and BLE host libraries were linked"
    )


def build_vendor_ble_example(info: Sdk312Info) -> tuple[pathlib.Path, pathlib.Path]:
    """Build the SDK's own BLE peripheral with its vendor RF/host libraries."""
    if shutil.which("arm-none-eabi-gcc") is None:
        raise SdkError("arm-none-eabi-gcc is required to build the SDK 3.1.2 BLE example")
    if shutil.which("make") is None:
        raise SdkError("make is required to build the SDK 3.1.2 BLE example")

    apply_sdk_312_gcc_compat(info)
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
    link_map = workdir / "build.map"
    if not image.is_file() or image.stat().st_size == 0:
        raise SdkError("SDK BLE build completed without output/sbp.ihex")
    if not link_map.is_file() or link_map.stat().st_size == 0:
        raise SdkError("SDK BLE build completed without build.map")
    verify_ble_link_map(link_map)
    return image, link_map
