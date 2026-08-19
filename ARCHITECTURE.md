# Firmverse architecture

Firmverse is a virtual embedded systems lab. It combines firmware execution, board wiring, multi-node environments and hardware tooling without making any one chip or development board the root abstraction.

## Layers

```text
Firmware
  |
  v
CPU backend
  |
  +-- jjkt/zmu for Cortex-M
  +-- future RISC-V backend(s)
  |
  v
SoC
  +-- CPU integration
  +-- memory map / MMIO
  +-- ROM ABI
  +-- GPIO / UART / ADC / timers / flash / radio controller
  |
  v
Board
  +-- connector/pin wiring
  +-- LEDs, buttons and external devices
  +-- board-specific defaults
  |
  v
World
  +-- positions and movement
  +-- RF peers / RSSI
  +-- scripted environment
  +-- multi-node simulation
  |
  +-- host bridges (for example real BLE)
  +-- test / CI frontends
  +-- hardware flashing tools
```

The ownership rule is strict:

- **CPU backend** executes instructions.
- **SoC** defines what the firmware-visible chip does.
- **Board** defines how a concrete product/development board wires things around that SoC.
- **World** defines the external reality shared by one or more nodes.

Firmware sees the SoC. A Board may connect to SoC pins/peripherals but must not redefine CPU/MMIO/ROM behavior. A World may influence nodes through radio, GPIO or environmental inputs but must not know PB-03F-specific wiring.

## CPU backends

`firmverse socs` prints the SoC registry and selected CPU backend.

PHY6252 currently executes through the vendored `jjkt/zmu` Cortex-M engine using its ARMv6-M profile. The registry also records the upstream zmu Cortex-M profiles used by the project architecture: M0/M0+, M3, M4/M4F and M7. A SoC selects the concrete profile it requires; supporting a Cortex-M4 SoC should not require a second CPU abstraction.

CH592F is deliberately different. The WeAct board uses a WCH CH592F RISC-V SoC, so the registry reserves a `riscv/qingke-v4c` backend and fails closed until that execution path exists. It is not modeled as a PHY6252 board variant.

## Current SoCs

- `phy6252` — implemented; `zmu/cortex-m0`.
- `ch592f` — registered/planned; RISC-V QingKe V4C execution not implemented yet.

## Current boards

`firmverse boards` lists board profiles.

- `pb03f-kit` — AI-Thinker PB-03F-Kit on PHY6252. Default compatibility profile.
- `headless` — bare PHY6252 with no PB-03F LED naming assumptions.
- `weact-ch592f` — WeAct Studio CH592F Core Board, reserved until the CH592F SoC backend can execute firmware.

PHY6252 package-pad to AP_GPIO-bit mapping is a SoC/package fact. PB-03F meanings such as `red`, `white`, or `Restore` are board facts.

## World

A World owns relationships between nodes and their environment. The current RF implementation keeps node positions, creates virtual advertisers, derives RSSI from distance and injects `scan`/`gone` observations into each node. Built-ins are:

- `mesh` — firmware nodes only;
- `still` — static virtual BLE beacons;
- `crowd` — moving virtual BLE advertisers.

The important boundary is that the World works with node identity, position and radio observations. It does not care whether a node is PB-03F-Kit or another board. This is the basis for future walls/attenuation, interference, sensors, power conditions and mixed-SoC simulations.

## Multi-node runtime

Each simulation node now carries its own `BoardKind`, even though the CLI currently applies one `--board` value to all nodes in a run. Output formatting uses that node board while the World snapshot remains board-agnostic. The next parser extension can allow a board per `--node` without changing the runtime model.

## Migration plan

1. Move PHY6252-specific implementation modules under `soc/phy6252/` without behavior changes.
2. Replace the large discovery/bus responsibilities with peripheral-oriented PHY6252 modules and explicit ROM/HLE dispatch.
3. Move PB-03F TUI pinout/connector rows into board profile data.
4. Add declarative World configuration (nodes, walls, RF loss, movement, environment inputs).
5. Add the CH592F RISC-V execution/memory/MMIO skeleton and only then enable `weact-ch592f`.
6. Add differential tests that compare emulator observations with real hardware traces.

The first constraint of every refactor is preserving the existing strict PHY6252 firmware regressions and real SDK 3.1.2 build path.
