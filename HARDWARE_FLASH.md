# Flashing a real PHY6252 over USB-UART

`phy6252-emu` contains a host-side flasher for real PHY62xx silicon:

```text
tools/phy6252_flash.py
```

It talks directly to the serial monitor in the PHY62xx ROM. It does **not** use
emulator ROM shims and does not require PhyPlusKit.

## BLE firmware must use the real SDK stack

The flasher cannot inject a radio driver into an already-linked firmware image.
On real PHY6252 silicon the RF/controller and BLE host code must be linked into
the firmware before it is programmed.

For PHY62XX SDK 3.1.2 the repository verifies both vendor toolchains:

- Keil: `lib/rf.lib`, `lib/ble_host.lib`;
- GCC: `lib/libphy6222_rf.a`, `lib/libphy6222_host.a`,
  `lib/libphy6222_sec_boot.a`.

The SDK 3.1.2 GCC `simpleBlePeripheral` example includes
`components/gcc/components.mk`, whose linker flags select the vendor RF and BLE
host libraries. The flasher can build this example itself and verify the
resulting linker map before touching the board.

## Recommended first hardware test: vendor BLE smoke

Install an Arm Embedded GCC toolchain (`arm-none-eabi-gcc`) and point the tool
at an unmodified PHY62XX SDK 3.1.2 tree:

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install -r tools/requirements.txt

python tools/phy6252_flash.py \
  --sdk-root ../PHY6252_6222_SDK \
  --vendor-ble-smoke \
  --port /dev/ttyUSB0
```

This mode performs one controlled workflow:

1. verifies that the tree identifies `PHY62XX_SDK_3.1.2`;
2. verifies the Keil and GCC vendor RF/BLE libraries;
3. builds the SDK's own GCC `simpleBlePeripheral` example;
4. checks `build.map` for `libphy6222_rf.a` and `libphy6222_host.a`;
5. prepares the PHY62xx flash layout;
6. enters the chip ROM UART monitor;
7. erases each affected flash sector only once;
8. writes the image through the ROM `cpbin` protocol;
9. resets into the real vendor BLE firmware.

Use `--dry-run` to stop after build, provenance checks and flash-plan generation.

## Flashing your own BLE firmware

For a custom image, pass its linker map as evidence that the real SDK RF/BLE
libraries were linked:

```sh
python tools/phy6252_flash.py \
  --sdk-root ../PHY6252_6222_SDK \
  --link-map build.map \
  --port /dev/ttyUSB0 \
  build/firmware.ihex
```

Without `--link-map`, the tool refuses a custom image by default. For a
non-BLE bring-up image you can explicitly bypass provenance checking with
`--allow-unverified-image`.

## Wiring

Use a **3.3 V USB-UART adapter**. Do not drive PHY6252 UART pins with 5 V logic.

Typical wiring:

```text
USB-UART     PHY6252 board
GND       -> GND
TX        -> RX
RX        -> TX
RTS       -> RST_N
DTR       -> TM / test-mode control
3.3 V     -> VCC, only if the adapter is intended to power the board
```

With RTS/DTR connected, the utility toggles reset/test mode automatically.
If your adapter only exposes TX/RX, put the target into the PHY62xx ROM UART
boot mode manually and add `--manual-boot`.

## Safety properties

The tool deliberately does not expose a full-chip erase command. A full erase
can destroy factory data such as MAC/chip information and is unnecessary for
normal application flashing.

Before programming it validates Intel HEX checksums, rejects unsupported load
regions, rejects boot-header overlaps, coalesces erase ranges so segments in a
shared 4 KiB sector cannot erase each other, and checks the detected flash
capacity before each write.

## SDK 3.1.2 source of truth

The emulator repository does not redistribute vendor libraries. Keep the SDK
3.1.2 tree outside this public repository and point `--sdk-root` at it.

For reproducible development, pin the exact SDK revision used to build release
firmware and archive its linker map alongside the HEX artifact.
