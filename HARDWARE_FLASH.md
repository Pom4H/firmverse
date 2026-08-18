# Flashing a real PHY6252 over USB-UART

`phy6252-emu` also contains a small host-side flasher for real PHY62xx silicon:

```text
tools/phy6252_flash.py
```

It talks directly to the serial monitor in the PHY62xx ROM. It does **not** use
emulator ROM shims and does not require PhyPlusKit.

## BLE firmware must use the vendor SDK

The flasher does not install a radio driver separately. On PHY6252 the real BLE
controller/host implementation is linked into the firmware image.

For an SDK 3.1.2 build, keep the genuine SDK libraries in the SDK tree and link
the application against its normal vendor targets, in particular:

- `lib/rf.lib` — PHY radio/controller implementation;
- `lib/ble_host.lib` — BLE host stack.

The public SDK release notes identify `PHY62XX_SDK_3.1.2`, and its
`_bld_script/bld_v312.yml` build recipe generates/uses those two libraries.
Do not replace them with emulator code when building firmware for hardware.

The flasher requires `--sdk-root` and refuses to program unless it can verify
all of these files:

```text
release_note.md
_bld_script/bld_v312.yml
lib/rf.lib
lib/ble_host.lib
```

This prevents accidentally flashing an image while pointing the workflow at a
wrong or incomplete SDK tree. It cannot prove which static library was linked
into an already-built arbitrary HEX file; build and flash should therefore be
one controlled workflow.

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

The tool asserts reset/test mode, releases reset and sends the PHY62xx ROM
activation sequence. If RTS/DTR are not wired, bootloader entry has to be done
manually and timing is less reliable.

## Install

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install -r tools/requirements.txt
```

## Flash an SDK HEX image

```sh
python tools/phy6252_flash.py \
  --port /dev/ttyUSB0 \
  --sdk-root ../PHY6252_6222_SDK \
  build/firmware.hex
```

On Windows the serial device can be `COM5` etc. The protocol itself is not
platform-specific.

The utility:

1. verifies the SDK 3.1.2 tree and the real BLE/RF libraries;
2. resets the target into the PHY62xx ROM serial monitor;
3. detects the flash capacity from the ROM revision/JEDEC information;
4. parses the SDK Intel HEX load regions;
5. creates the PHY62xx ROM boot segment table at flash `0x2000`;
6. maps SRAM load sections into flash backing storage;
7. erases only the sectors that will be rewritten;
8. streams blocks through the ROM `cpbin` protocol and validates its checksum
   handshake;
9. resets into the newly programmed firmware.

By default the utility deliberately does **not** expose a full-chip erase
command. A full erase can destroy factory data such as MAC/chip information and
is not needed for normal application flashing.

## SDK 3.1.2 source of truth

The repository does not redistribute PhyPlus proprietary libraries. Keep your
licensed/original SDK 3.1.2 tree outside this public repository and point
`--sdk-root` at it.

For reproducible development, pin the SDK source/release used by your build and
record its revision next to the firmware build artifacts.
