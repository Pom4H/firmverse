# Firmverse architecture

Firmverse is composed around hardware ownership instead of around one development board.

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

A frontend such as CLI/TUI, a host bridge, or CI observes and drives this stack. It does not get to redefine the stack.

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

For PHY6252 the package pad metadata now lives in `src/soc/phy6252/pins.rs`. `P15 -> AP_GPIO bit 9` is a SoC fact. `P15 -> Restore` is not.

### Board

Owns physical assembly around a SoC:

- connector layout;
- LED/button meaning;
- pin aliases visible on the PCB;
- external devices wired to SoC pins;
- board-specific defaults.

`BoardProfile` is data consumed by frontends. The PB-03F DIP-30 rows, LED colours and Restore meaning live there instead of inside the TUI.

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
    ▼
CLI / raw / TUI / host BLE
```

Multi-node:

```text
            ┌─ firmware A → SoC → Board ─┐
World  ◄────┼─ firmware B → SoC → Board ─┼────► observations
            └─ firmware C → SoC → Board ─┘
```

`sim::NodeSpec` carries its Board profile. `World` receives node identity/pose/radio state and therefore stays board-agnostic.

## Source layout

```text
src/
  main.rs                    composition root / CLI
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
  sim.rs                     multi-node scheduler
  emu.rs                     single-node frontend runtime
  tui.rs                     presentation consuming Board + SoC metadata
  ble_host.rs                host BLE bridge
```

### Transitional namespace shim

The PHY6252 files have been moved physically under `src/soc/phy6252/`. Existing code still imports many of them as `crate::chip`, `crate::bus`, etc. `main.rs` temporarily mounts those files with `#[path = "soc/phy6252/..."]`.

This is deliberate. It separates two risky operations:

1. **physical ownership** — completed by moving the implementation under the SoC directory without changing the blobs;
2. **Rust namespace migration** — can now happen incrementally, module by module, with small reviewable diffs.

New PHY6252 contracts should be added under `crate::soc::phy6252::*`; `pins` already follows that rule. The compatibility shim should shrink rather than gain new modules.

## Registered hardware

| ID | CPU backend | SoC state | Board state |
|---|---|---|---|
| `phy6252` | `zmu/cortex-m0` | executable | PB-03F + headless executable |
| `ch592f` | `riscv/qingke-v4c` | planned | WeAct profile registered, execution blocked |

CH592F is a sibling SoC. It must never enter the codebase as PHY6252 MMIO plus a different Board profile.

The intended direction is:

```text
soc/
  phy6252/
  ch592f/
board profiles
world runtime
```

The CH592F board becomes executable only after a CH592F CPU/memory/MMIO backend can execute a minimal real image.

## Strictness is an architectural feature

`--strict` means an unmodeled firmware-visible PHY6252 behavior is an error instead of an implicit successful no-op. This keeps the emulator useful as an executable specification: missing behavior becomes an observable requirement.

Refactors must keep the strict regression path green, including:

- Rust unit tests / Clippy / rustfmt;
- PHY6252 flasher tests;
- demo firmware;
- pinned PHY62XX SDK 3.1.2 vendor BLE image build;
- board smoke;
- RSSI ranking;
- multi-node mesh;
- capability stress;
- persistent NOR restart.

## Frontend rule

A frontend may render data differently, but hardware knowledge must come from the owning layer.

Examples:

- TUI asks `BoardProfile` for connector rows and indicator meanings;
- TUI asks `soc::phy6252::pins` for GPIO/ADC mapping;
- raw output asks the selected Board profile how to name active indicators;
- World never asks either of those questions.

This is the test for future UI work: if deleting the PB-03F board profile would require editing TUI rendering code, board knowledge has leaked again.

## Next steps

1. Shrink the `#[path]` compatibility shim by moving PHY6252 modules into the real Rust namespace in small groups.
2. Split the current bus/discovery responsibilities into peripheral modules plus explicit ROM/HLE dispatch.
3. Add declarative World configuration for walls/RF loss/movement/environment inputs.
4. Add differential traces against real PHY6252 hardware.
5. Introduce `soc/ch592f/` with loader, memory map and QingKe/RISC-V execution before enabling the WeAct board.

The invariant is more important than the directory names: **CPU executes, SoC behaves, Board wires, World surrounds.**
