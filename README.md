# PHY6252 emulator

Cortex-M0 chip emulator for **PHY6252** (AI-Thinker **PB-03F-Kit**). Runs an Intel HEX image on [zmu](https://github.com/jjkt/zmu) with the chip memory map: SRAM, XIP, `AP_GPIO`, UART, timers, ADC, PWM, and BLE ATT via a host radio.

The public API is a **line protocol** on stdin/stdout. See [PROTOCOL.md](PROTOCOL.md).

## BLE

The kit's 2.4 GHz radio is not a vendor BLE ROM in this repo. **The laptop is the RF PHY.** The hex still owns GATT behavior: connection, writes, notifies.

```sh
bash scripts/air.sh
```

macOS advertises **PB03FKIT**. Phone/laptop central:

| | UUID |
|---|---|
| Service | `6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001` |
| RX (write) | `6B1D0002-7C8E-4A91-9F2B-E3A14C5B0001` |
| TX (notify / indicate) | `6B1D0003-7C8E-4A91-9F2B-E3A14C5B0001` |

Write to RX → hex mailbox → UART `ble up` / `rx …` and an echo notify on TX. Subscribe to TX for GPIO/ADC snapshots. Green LED stays on while connected.

Without a radio, the same ATT path is stdin: `CONNECT`, `CCCD 1`, `WRITE 4849` → `FRAME`.

## Quick start

```sh
git clone --recurse-submodules https://github.com/Pom4H/phy6252-emu.git
cd phy6252-emu
make -C firmware
cargo run --release -- --live firmware/build/kit-demo.hex
```

In another terminal (or type into stdin):

```text
ADC 3300 1650 2500 3300
IN 00400000
CONNECT
CCCD 1
WRITE 4849
```

`IN 00400000` drives silkscreen **P34** high (gpio bit 22). The demo firmware lights every RGB LED while that input is set.

## What the demo firmware touches

Bare-metal C, no vendor SDK. Linked into SRAM at `0x1FFF0000`.

| Block | Kit / silicon | Demo |
|---|---|---|
| RGB | P7 / P11 / P18 | color chase |
| Warm LED | P0 | fourth RGB phase |
| Header GPIO | P14 P16 P17 P31 P32 P33 | walking one |
| Button | P34 | all LEDs on when high |
| ADC | P20 P15 P24 P23 | millivolts from host `ADC` |
| UART0 / UART1 | TX0 / TX1 | status log / heartbeat |
| PWM | `0x4000E000` | duty from tick + ADC |
| Timer | `0x40001000` | free-running count |
| WDT | `0x40002000` | periodic feed |
| I2C0 / SPI0 | `0x40005000` / `0x40006000` | register poke |
| PCR / AON | `0x40000000` / `0x4000F000` | wakeup-side windows |
| Radio mailbox | SRAM `0x20000000` | ATT payloads (BLE without vendor LL) |

## Build

- Rust (`rustup`)
- `arm-none-eabi-gcc` for the demo HEX
- zmu Cortex-M0 crate (git submodule `third_party/zmu`)

```sh
./scripts/fetch-zmu.sh    # if the submodule is empty
make -C firmware
cargo test
cargo run --release -- --live
```

## Layout

| Path | |
|---|---|
| `src/` | emulator |
| `firmware/` | PB-03F-Kit demo image |
| `host/ble/` | macOS CoreBluetooth peripheral (GAP/GATT on the laptop radio) |
| `scripts/air.sh` | emu + BLE air bridge |
| `third_party/zmu` | [zmu](https://github.com/jjkt/zmu) Cortex-M0 core |

Pass any other Intel HEX as the first argument. Vector table is the first eight bytes of SRAM (`0x1FFF0000`).
