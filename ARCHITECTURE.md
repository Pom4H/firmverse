# Firmverse architecture

Firmverse is composed around hardware ownership instead of around one development board or one frontend.

```text
Firmware image
     │
     ▼
CPU backend
     │
     ▼
SoC
     │
     ▼
Board
     │
     ▼
World
```

A frontend such as CLI/TUI, a browser Worker, a host bridge, or CI observes and drives this stack. It does not get to redefine the stack.

## Ownership

### CPU backend

Executes instructions and exposes the architectural CPU state required by the SoC integration.

Current backend family:

- `jjkt/zmu` for Cortex-M;
- PHY6252 selects `cortex-m0` / ARMv6-M;
- the registry keeps M0+, M3, M4, M4F and M7 profiles visible for future SoCs.

A CPU backend does **not** own GPIO, flash layout, BLE controller semantics, board LEDs, or RF distance.

### SoC

Owns everything firmware can observe as part of the chip:

- memory map;
- MMIO registers;
- ROM/HLE ABI;
- interrupt/peripheral behavior;
- GPIO pad mapping;
- ADC channels;
- timers/PWM/UART/DMA;
- flash semantics;
- radio/controller boundary;
- power state that belongs to the silicon.

For PHY6252 the package pad metadata lives in `src/soc/phy6252/pins.rs`. `P15 -> AP_GPIO bit 9` is a SoC fact. `P15 -> Restore` is not.

### Board

Owns physical assembly around a SoC:

- connector layout;
- LED/button meaning;
- pin aliases visible on the PCB;
- external devices wired to SoC pins;
- board-specific defaults.

`BoardProfile` is data consumed by every frontend. The PB-03F DIP-30 rows, LED colours and Restore meaning live there instead of inside TUI or browser JavaScript.

A Board must not redefine CPU instructions, SoC MMIO or the SoC package pad map.

### World

Owns the reality shared by nodes:

- position and movement;
- RF visibility / RSSI;
- virtual peers;
- environment inputs;
- eventually walls, attenuation, interference, temperature, power conditions, etc.

A World sees node identity and externally observable interfaces. It must not contain checks like `if board == PB03F`.

## Composition

Single node:

```text
firmware.hex
    │
    ▼
PHY6252 SoC ── zmu/cortex-m0
    │
    ▼
PB-03F-Kit or headless board
    │
    ├── CLI / raw / TUI / host BLE
    └── browser WASM Worker
```

Multi-node:

```text
            ┌─ firmware A → SoC → Board ─┐
World  ◄────┼─ firmware B → SoC → Board ─┼────► observations
            └─ firmware C → SoC → Board ─┘
```

`sim::NodeSpec` carries its Board profile on native. `BrowserLab` carries the same Board + `Chip` composition in WebAssembly. Both feed node identity/pose/radio state into the same `World` implementation.

## Native and WebAssembly share one core

The crate now has a library composition root in `src/lib.rs`. The native `firmverse` binary imports that library instead of declaring its own copy of the modules.

```text
                    src/lib.rs
                       │
      ┌────────────────┴────────────────┐
      │                                 │
 src/main.rs                     src/web_runtime.rs
 native adapters                     raw WASM ABI
      │                                 │
 CLI/TUI/BLE host                    Web Worker
                                        │
                             web custom elements
```

Browser-only integration must therefore be an adapter around the library, not another emulator written in JavaScript.

The WASM build excludes native-only terminal/serial dependencies with target-specific Cargo dependencies. Firmware is loaded through `HexImage::parse()` / `Chip::load_text()` so a browser `File` can feed the same PHY6252/zmu runtime without inventing a virtual filesystem.

## Browser ABI

`src/web_runtime.rs` exposes a small JSON-over-linear-memory ABI on `wasm32`:

```text
firmverse_input_reserve
firmverse_call
firmverse_result_ptr
firmverse_result_len
```

The Web Worker owns a `BrowserLab`. The main UI thread never manipulates Rust memory layout and never executes CPU bursts directly.

Current JSON operations cover registry discovery, lab creation, node add/remove/move, World selection, GPIO/ADC inputs, ticking and snapshots. See [`docs/WEB.md`](docs/WEB.md).

## Registry-driven UI

The browser follows the same ownership rule as native frontends and the metadata-first approach used by `Pom4H/elements`:

```text
BoardProfile ─────┐
SoC profile ──────┼──► registry JSON ─► <firmverse-board>
SoC pins ─────────┤                   └► inspector
World::list() ────┘                   └► <firmverse-world>
```

`<firmverse-board>` may decide how to draw a connector, but it may not contain a PB-03F pinout table. `<firmverse-world>` may decide how to draw an RF link, but the link exists only when the Rust World reports that a node is heard.

Dragging a node is therefore a model edit, not a canvas-only transform:

```text
pointer drag → moveNode → Chip.x/y → World::radio → RSSI / Scan / Gone → snapshot
```

## Source layout

```text
src/
  lib.rs                     shared composition root
  main.rs                    native CLI composition
  web_runtime.rs             browser lab + raw WASM ABI
  soc.rs                     SoC + CPU-backend registry
  soc/
    phy6252/
      mod.rs                 PHY6252 namespace boundary
      pins.rs                package pad → GPIO/ADC metadata
      chip.rs                CPU + SoC runtime integration
      bus.rs                 current MMIO/bus surface
      discovery.rs           current strict discovery/ROM bridge
      *_rom.rs               vendor ROM/HLE groups
      hci_*.rs / ll_*.rs     controller/Link Layer model
      osal*.rs               vendor runtime model
      ...                    remaining PHY6252 implementation
  board.rs                   board profiles / connector wiring
  world.rs                   environment model
  sim.rs                     native multi-node scheduler
  emu.rs                     native single-node runtime
  tui.rs                     terminal presentation
  ble_host.rs                host BLE bridge
web/
  src/
    engine-worker.js         WASM/platform adapter
    elements.js              metadata-driven Board + World elements
    app.js                   editor/controller wiring
  smoke.mjs                  executable WASM regression
```

### Transitional namespace shim

The PHY6252 files have been moved physically under `src/soc/phy6252/`. Existing code still imports many of them as `crate::chip`, `crate::bus`, etc. `src/lib.rs` temporarily mounts those files with `#[path = "soc/phy6252/..."]`.

This is deliberate. It separates two risky operations:

1. **physical ownership** — completed by moving the implementation under the SoC directory without changing the blobs;
2. **Rust namespace migration** — can happen incrementally, module by module, with small reviewable diffs.

New PHY6252 contracts should be added under `crate::soc::phy6252::*`; `pins` already follows that rule. The compatibility shim should shrink rather than gain new modules.

## Registered hardware

| ID | CPU backend | SoC state | Board state |
|---|---|---|---|
| `phy6252` | `zmu/cortex-m0` | executable native + WASM | PB-03F + headless executable |
| `ch592f` | `riscv/qingke-v4c` | planned | WeAct profile registered, execution blocked |

CH592F is a sibling SoC. It must never enter the codebase as PHY6252 MMIO plus a different Board profile.

The intended direction is:

```text
soc/
  phy6252/
  ch592f/
board profiles
world runtime
native + browser frontends
```

The CH592F board becomes executable only after a CH592F CPU/memory/MMIO backend can execute a minimal real image.

## Strictness is an architectural feature

`--strict` and browser `strict: true` mean an unmodeled firmware-visible behavior is an error instead of an implicit successful no-op. This keeps Firmverse useful as an executable specification: missing behavior becomes an observable requirement.

Refactors must keep the regression paths green, including:

- Rust unit tests / Clippy / rustfmt;
- PHY6252 flasher tests;
- demo firmware;
- pinned PHY62XX SDK 3.1.2 vendor BLE image build;
- board smoke;
- RSSI ranking;
- multi-node mesh;
- capability stress;
- persistent NOR restart;
- public GitHub Action smoke;
- WASM instantiation + real zmu instruction execution + two-node World smoke.

## Frontend rule

A frontend may render data differently, but hardware knowledge must come from the owning layer.

Examples:

- TUI asks `BoardProfile` for connector rows and indicator meanings;
- browser `<firmverse-board>` asks the exported registry for the same data;
- TUI and browser inspector use `soc::phy6252::pins` for GPIO/ADC mapping;
- raw output asks the selected Board profile how to name active indicators;
- World never asks any of those presentation questions;
- browser RF lines are based on World observations, not UI distance heuristics.

This is the test for future UI work: if deleting the PB-03F board profile would require editing a PB-03F pin list in TUI or JavaScript, board knowledge has leaked again.

## Next steps

1. Shrink the `#[path]` compatibility shim by moving PHY6252 modules into the real Rust namespace in small groups.
2. Split the current bus/discovery responsibilities into peripheral modules plus explicit ROM/HLE dispatch.
3. Extend declarative World configuration/editor with walls, RF loss, noise, movement and virtual advertisers.
4. Add browser persistence/import/export adapters without putting storage into World itself.
5. Add differential traces against real PHY6252 hardware.
6. Introduce `soc/ch592f/` with loader, memory map and QingKe/RISC-V execution before enabling the WeAct board.

The invariant is more important than the directory names: **CPU executes, SoC behaves, Board wires, World surrounds. Frontends only adapt and visualize.**
