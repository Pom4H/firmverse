# PHY6252 emulator

Cortex-M0 emulator for **PHY6252** and the AI-Thinker **PB-03F-Kit**. The project is intentionally small: a Rust emulator, a UART flasher, one raw line protocol, an optional terminal UI, and an optional Linux BlueZ bridge.

```sh
git clone --recurse-submodules https://github.com/Pom4H/phy6252-emu.git
cd phy6252-emu
cargo run --release
```

## Run

```sh
phy6252 firmware/kit-demo.hex
phy6252 --tui firmware/kit-demo.hex
phy6252 --raw firmware/kit-demo.hex
phy6252 --strict --once firmware/kit-demo.hex
phy6252 --ble firmware/kit-demo.hex
phy6252 firmware/build/rssi-rank.hex
phy6252 sim --node a=firmware/build/rssi-rank.hex --node b=firmware/build/rssi-rank.hex
phy6252 sim --world crowd firmware/build/rssi-rank.hex
phy6252 worlds
```

`--strict-mmio` is kept as an alias for `--strict`.

### RSSI rank demo

`firmware/build/rssi-rank.hex` keeps the five strongest advertisers and gives each a sticky LED colour. Unused kit LEDs stay off. Closer devices blink faster (same curve as the silicon image: −35 dBm ≈ 12 Hz, −90 dBm ≈ 0.6 Hz). If a device drops out of the top 5 or disappears, that colour is freed for the next newcomer.

| UART | LED |
|---|---|
| R | P7 red |
| G | P11 green |
| B | P18 blue |
| Y | P0 yellow (warm) |
| W | P34 white (cool) |

Restore is P15. The emulator mailbox supplies `scan`/`gone`; the silicon HEX scans the air. GPIO, LEDs, Restore and the DIP-30 pinout are the same board.

Inject advertisers from the REPL / `--raw` / TUI:

```text
scan aa:bb:cc:dd:ee:01 -40
scan aa:bb:cc:dd:ee:02 -55
gone aa:bb:cc:dd:ee:01
```

A device that is not refreshed for 4 s is treated as gone.

### Mesh / multi-chip simulation

`phy6252 sim` runs several guests on one 1 ms clock in a shared RF world. Each `--node` loads its own HEX and gets a local MAC from the node id. Chips advertise to each other (and to optional virtual walkers) as `scan` / `gone` mailbox reports, with RSSI from distance.

```sh
phy6252 sim --node a=firmware/build/rssi-rank.hex --node b@3,0=firmware/build/rssi-rank.hex
phy6252 sim --world crowd firmware/build/rssi-rank.hex
phy6252 sim --once --ticks 2000 --raw --world mesh \
  --node a=firmware/build/rssi-rank.hex \
  --node b=firmware/build/rssi-rank.hex
```

`--node` is `id[@x,y]=path`. Default spacing is 3 m on X, which is inside radio range. Two or more nodes default to world `mesh` (chips only); a single node defaults to `crowd` (six looping walkers). `phy6252 worlds` lists them.

The world timeline wraps while the sim is live. `--once --ticks N` is the scripted run: no sleep, optional `--loop` to wrap the walkers anyway. With several chips, `--raw` prefixes lines as `[id] GPIO` / `[id] UART`. Unprefixed stdin commands go to every chip; `a scan …` or `[b] gone …` target one node. The TUI can attach to a one-chip sim (`phy6252 sim --tui --world crowd firmware.hex`).

This is a host-side coupling of the scan mailbox, not a cycle-accurate BLE radio. Firmware that blindly echoes RX into TX will not be auto-relayed as a mesh flood.

### Flash a PB-03F-Kit

The emulator image `rssi-rank.hex` ranks mailbox `scan` reports. For a kit that scans the air, build the SDK silicon image (needs PHY62XX SDK 3.1.2) and flash that:

```sh
make -C firmware/silicon
cargo run --release --bin phy6252-flash -- firmware/build/rssi-rank-ble.hex
```

`PHY62XX_SDK` overrides the SDK path. The silicon image advertises as `rssi-rank` (non-connectable) and maps the five strongest advertisers onto the same LEDs as the emulator demo. UART0 is P9/P10 at 115200.

Hold **KEY1** (RST/PROG), start the tool, release the button when it prints `bootloader`. `--port` and `PHY6252_PORT` override auto-detect of the CH340 USB-UART. `--erase` wipes the whole chip, including NVRAM.

The Python ROM flasher (`tools/phy6252_flash.py`) programs NOR with an SDK layout and can build/check the vendor `simpleBlePeripheral` example. See `HARDWARE_FLASH.md`. It is a different path from `phy6252-flash` (CH340 + KEY1, used for `rssi-rank-ble.hex`).

Install locally with:

```sh
cargo install --path .
```

The default REPL accepts the same commands as the raw/TUI frontends:

```text
connect
cccd 1
write 01020304
adc 3.3 1.65 2.5 3.3
p34 on
help
```

## What is modeled

The emulator currently executes real Cortex-M0 HEX images and models the PHY6252 surfaces needed by the bundled strict capability regression:

- relocated Cortex-M0 vectors, SRAM and 256 KiB XIP flash;
- GPIO, UART0/UART1, ADC, six PWM channels and timers;
- exact/narrow PCR, AON, cache, clock, SPI-flash and DMAC register behavior used by firmware;
- NOR erase/program semantics and optional persistent flash state;
- eFuse/AES bootstrap behavior and ARM EABI helpers;
- OSAL heap, memory, linked message queues, events, timers and cooperative task dispatch;
- the exercised PHY6252 HCI/LL/GAP/GATT/security ROM ABI at a host-controller boundary;
- generic ATT RX/TX mailbox transport;
- Linux BlueZ advertising/GATT bridge;
- strict discovery: unknown MMIO or vendor-ROM behavior stops instead of silently succeeding.

The project is **not a cycle-accurate RF simulator**. `phy6252 sim` approximates advertising as distance-based scan reports between chips and virtual walkers. Over-the-air scheduling and the physical BLE radio are delegated to the host Bluetooth controller when `--ble` is used (BlueZ on Linux, the system adapter on macOS). Unknown vendor behavior remains a strict fault until it is modeled explicitly.

## Persistent NOR

Set `PHY6252_FLASH_STATE` to preserve the complete 256 KiB NOR image across emulator restarts:

```sh
PHY6252_FLASH_STATE=.state/device.flash \
  phy6252 --strict firmware.hex
```

The state file is tied to the baseline firmware image. An incompatible snapshot is ignored instead of silently replacing a different firmware image. This sits below any guest filesystem/SNV format, so firmware persistence uses the same flash path as ordinary code.

## Terminal UI

```sh
phy6252 --tui firmware.hex
```

The TUI is only a frontend over `--raw`; it does not create a second emulator path. It shows:

- run/strict state and image name;
- BLE link/notify state;
- live ADC voltages;
- the PB-03F-Kit bottom-view pinout with GPIO direction/level;
- RGB / yellow (P0) / white (P34) LED state, Restore on P15, and all six PWM channels;
- a rolling UART/ATT/ROM/MMIO diagnostic log;
- one command line with Up/Down history.

For the full physical pinout use a terminal of at least `68x26`. Smaller terminals automatically switch to the compact status/log view.

Keys: `Enter` sends, `Up/Down` browse history, `Esc` or `Ctrl-C` exits.

## Host Bluetooth LE

`--ble` exposes the generic ATT mailbox through the host Bluetooth adapter. The firmware remains the source of application RX/TX data; the host supplies the real radio, advertising and GATT transport. `--tui` cannot be combined with `--ble`.

Linux:

- BlueZ / `bluetoothd`;
- `python3-dbus` and `python3-gi`;
- an adapter exposing `GattManager1` and `LEAdvertisingManager1`.

macOS:

- Xcode Command Line Tools (`xcrun swiftc`); Bluetooth permission for `phy6252-ble`.
- First `--ble` compiles `host/ble/darwin.swift` into `$TMPDIR/phy6252-ble-<version>/Phy6252Ble.app`.
- If advertising never starts: System Settings → Privacy & Security → Bluetooth, allow `phy6252-ble`.

Default public test profile:

| Setting | Value |
|---|---|
| Local name | `PB03FKIT` |
| Service | `6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001` |
| RX write | `6B1D0002-7C8E-4A91-9F2B-E3A14C5B0001` |
| TX notify | `6B1D0003-7C8E-4A91-9F2B-E3A14C5B0001` |

All values are runtime-configurable:

```sh
phy6252 --ble \
  --ble-name DEVICE \
  --ble-service 12345678-1234-5678-1234-56789abcdef0 \
  --ble-rx      12345678-1234-5678-1234-56789abcdef1 \
  --ble-tx      12345678-1234-5678-1234-56789abcdef2 \
  firmware.hex
```

## Strict discovery

Use a firmware image as the executable specification for the next missing chip behavior:

```sh
phy6252 --strict --once firmware.hex
```

Unknown MMIO stops with a `DAccViol`; unknown vendor-ROM entrypoints stop as unmodeled ROM ABI. Confirmed functions are then implemented narrowly from the public chip/SDK contract. Blanket `BX LR` stubs are intentionally avoided.

This keeps one source of truth:

```text
firmware
  -> Cortex-M0 / OSAL / vendor ABI
  -> generic host-controller boundary
  -> Linux BlueZ (optional)
  -> real Bluetooth adapter
```

## Demo and regression firmware

```sh
make -C firmware clean all
```

Requires `arm-none-eabi-gcc`.

Freestanding images (`make -C firmware`):

- `kit-demo.hex` - small interactive board demo;
- `rssi-rank.hex` - top-5 BLE advertisers mapped onto the five kit LEDs (emulator mailbox);
- `capability-demo.hex` - strict regression covering OSAL memory/queues, NOR, AES, controller ABI, GPIO/ADC/PWM/UART and DMAC paths.

`make -C firmware/silicon` additionally builds `rssi-rank-ble.hex` when PHY62XX SDK 3.1.2 is present. That image talks to the real 2.4 GHz radio.

The CI runs Rust tests, both strict firmware smokes, and a two-run persistent-NOR restore check. CI installs Node via `actions/setup-node` with `lts/*`; GitHub JavaScript actions used by the workflow target the current Node 24 action runtime.

Machine protocol: [PROTOCOL.md](PROTOCOL.md).
