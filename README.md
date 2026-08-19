# Firmverse

**A virtual embedded systems lab for real firmware, SoCs, boards and multi-node worlds.**

Firmverse executes firmware, models chip/board behavior, runs several devices in a shared environment, bridges selected peripherals to the host, and keeps hardware flashing tools beside the virtual model.

The project started with PHY6252 / AI-Thinker PB-03F-Kit and is being generalized without throwing away the strict firmware regressions that made that model useful.

```text
Firmware
   ↓
CPU backend
   ↓
SoC
   ↓
Board
   ↓
World
```

Today PHY6252 executes on the vendored [jjkt/zmu](https://github.com/jjkt/zmu) Cortex-M engine (`armv6m` / Cortex-M0). The SoC registry keeps the same zmu backend available for future Cortex-M0+/M3/M4/M4F/M7 SoCs. CH592F / WeAct is registered separately as a future RISC-V QingKe V4C backend; Firmverse deliberately refuses to pretend it is another PHY6252 board.

## Build

```sh
git clone --recurse-submodules https://github.com/Pom4H/firmverse.git
cd firmverse
cargo build --release
```

The main binary is `firmverse`.

```sh
firmverse socs
firmverse boards
firmverse worlds
```

## Run firmware

The current executable SoC is PHY6252 and accepts Intel HEX images.

```sh
firmverse firmware/kit-demo.hex
firmverse --raw firmware/kit-demo.hex
firmverse --strict --once firmware/kit-demo.hex
firmverse --tui firmware/kit-demo.hex
```

`--strict-mmio` remains an alias for `--strict`.

Select a board profile independently of the SoC implementation:

```sh
firmverse --board pb03f-kit firmware/kit-demo.hex
firmverse --board headless firmware/kit-demo.hex
```

`headless` removes PB-03F-specific LED naming while keeping the same PHY6252 SoC execution path.

## Multi-node simulation

`firmverse sim` runs one or more firmware nodes on one 1 ms virtual clock. Each node has its own firmware instance, MAC address, position and board profile. `World` owns what happens between the nodes.

```sh
firmverse sim \
  --world mesh \
  --node a=firmware/build/rssi-rank.hex \
  --node b@3,0=firmware/build/rssi-rank.hex
```

For deterministic CI runs:

```sh
firmverse sim \
  --strict \
  --once \
  --ticks 2000 \
  --raw \
  --world mesh \
  --node a=firmware/build/rssi-rank.hex \
  --node b=firmware/build/rssi-rank.hex
```

Current built-in Worlds:

- `mesh` — firmware nodes only; every node can hear the others when RF distance allows it;
- `still` — five static virtual BLE advertisers;
- `crowd` — six moving virtual advertisers.

The current RF model is intentionally lightweight: distance becomes RSSI, advertisers become `scan`/`gone` observations, and the firmware reacts through the same mailbox/controller boundary as the single-node emulator. It is not a cycle-accurate 2.4 GHz PHY.

The runtime already stores a board per node. The CLI currently applies one `sim --board` profile to every node; per-node board syntax can be added without changing the World model.

## PHY6252 model

The PHY6252 implementation executes real Cortex-M0 HEX images and models the surfaces exercised by the strict capability regression:

- relocated Cortex-M0 vectors, SRAM and 256 KiB XIP flash;
- GPIO, UART0/UART1, ADC, six PWM channels and timers;
- PCR, AON, cache, clock, SPI-flash and DMAC register behavior used by firmware;
- NOR erase/program semantics and optional persistent flash state;
- eFuse/AES bootstrap behavior and ARM EABI helpers;
- OSAL heap, memory, linked message queues, events, timers and cooperative task dispatch;
- exercised PHY6252 HCI/LL/GAP/GATT/security ROM ABI at a host-controller boundary;
- generic ATT RX/TX mailbox transport;
- strict discovery for unknown MMIO or vendor-ROM behavior.

Unknown behavior does not silently become success in strict mode. The firmware stops at the missing MMIO/ROM surface so the model can be extended from an observable requirement instead of adding blanket no-op stubs.

```sh
firmverse --strict --once firmware.hex
```

That fail-closed workflow is one of the main design rules of Firmverse.

## RSSI / BLE demo

`firmware/build/rssi-rank.hex` keeps the five strongest advertisers and assigns sticky PB-03F LEDs.

```text
scan aa:bb:cc:dd:ee:01 -40
scan aa:bb:cc:dd:ee:02 -55
gone aa:bb:cc:dd:ee:01
```

PB-03F mappings used by the board profile:

| Function | Pin |
|---|---|
| red | P7 |
| green | P11 |
| blue | P18 |
| yellow | P0 |
| white | P34 |
| Restore | P15 |

Package-pad → AP_GPIO mapping remains a PHY6252 SoC fact; LED/Restore meanings are board facts.

## Host Bluetooth LE

`--ble` exposes the generic ATT mailbox through the host Bluetooth adapter. Firmware remains the source of application RX/TX data; the host provides the real BLE transport.

```sh
firmverse --ble firmware/kit-demo.hex
```

Linux uses BlueZ. macOS uses the system Bluetooth stack through the Swift helper under `host/ble/`.

Default public test profile:

| Setting | Value |
|---|---|
| Local name | `PB03FKIT` |
| Service | `6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001` |
| RX write | `6B1D0002-7C8E-4A91-9F2B-E3A14C5B0001` |
| TX notify | `6B1D0003-7C8E-4A91-9F2B-E3A14C5B0001` |

Values are runtime-configurable with `--ble-name`, `--ble-service`, `--ble-rx`, and `--ble-tx`.

## Persistent NOR

Set `PHY6252_FLASH_STATE` to preserve the complete 256 KiB PHY6252 NOR image between runs:

```sh
PHY6252_FLASH_STATE=.state/device.flash \
  firmverse --strict firmware.hex
```

The state file is tied to the baseline firmware image so an incompatible snapshot is not silently restored over another image.

## Flash real PHY6252 hardware

Firmware execution and real-hardware tooling live in the same repository, but the flasher remains SoC-specific:

```sh
make -C firmware/silicon
cargo run --release --bin phy6252-flash -- firmware/build/rssi-rank-ble.hex
```

The SDK-backed silicon image requires PHY62XX SDK 3.1.2. `PHY62XX_SDK` overrides the SDK path. See `HARDWARE_FLASH.md` for the CH340 / ROM bootloader path and the Python tooling.

This distinction is intentional: **Firmverse is the lab; `phy6252-flash` is one hardware adapter inside it.** Future SoCs can add their own flashing transport without polluting the CPU or World layers.

## Demo and regression firmware

```sh
make -C firmware clean all
```

Freestanding images include:

- `kit-demo.hex` — interactive PB-03F board demo;
- `rssi-rank.hex` — strongest BLE advertisers mapped to board LEDs;
- `capability-demo.hex` — strict regression covering OSAL, NOR, AES, controller ABI, GPIO/ADC/PWM/UART and DMAC paths.

CI additionally builds and validates a real vendor PHY62XX SDK 3.1.2 BLE image.

## Architecture

See `ARCHITECTURE.md` for the ownership rules and migration plan.

The short version:

> **SoC defines how the device works. Board defines how it is physically assembled. World defines the reality around it.**

This lets the same Firmverse runtime grow from one PHY6252 board into mixed SoC/board simulations without turning the environment, UI or CI into chip-specific code.
