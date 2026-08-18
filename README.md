# PHY6252 emulator

Cortex-M0 emulator for **PHY6252** and the AI-Thinker **PB-03F-Kit**. The project is intentionally small: one Rust binary, one raw line protocol, an optional terminal UI, and an optional Linux BlueZ bridge.

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
```

`--strict-mmio` is kept as an alias for `--strict`.

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

The project is **not a cycle-accurate RF simulator**. Over-the-air scheduling and the physical BLE radio are delegated to the Linux Bluetooth controller when `--ble` is used. Unknown vendor behavior remains a strict fault until it is modeled explicitly.

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
- RGB/W LED state and all six PWM channels;
- a rolling UART/ATT/ROM/MMIO diagnostic log;
- one command line with Up/Down history.

For the full physical pinout use a terminal of at least `68x26`. Smaller terminals automatically switch to the compact status/log view.

Keys: `Enter` sends, `Up/Down` browse history, `Esc` or `Ctrl-C` exits.

## Linux Bluetooth LE

`--ble` exposes the generic ATT mailbox through the host Bluetooth adapter using BlueZ. The firmware remains the source of application RX/TX data; BlueZ supplies the real radio, advertising and GATT transport.

Requirements:

- Linux + BlueZ / `bluetoothd`;
- `python3-dbus`;
- `python3-gi`;
- an adapter exposing `GattManager1` and `LEAdvertisingManager1`.

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

Two freestanding images are built:

- `kit-demo.hex` - small interactive board demo;
- `capability-demo.hex` - strict regression covering OSAL memory/queues, NOR, AES, controller ABI, GPIO/ADC/PWM/UART and DMAC paths.

The CI runs Rust tests, both strict firmware smokes, and a two-run persistent-NOR restore check. CI installs Node via `actions/setup-node` with `lts/*`; GitHub JavaScript actions used by the workflow target the current Node 24 action runtime.

Machine protocol: [PROTOCOL.md](PROTOCOL.md).
