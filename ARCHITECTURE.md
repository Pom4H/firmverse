# Emulator architecture

The emulator is moving from a PB-03F-Kit-shaped program toward a reusable virtual hardware platform.

## Layers

```text
CPU core
  |
  +-- SoC
  |    +-- CPU integration
  |    +-- memory map / MMIO
  |    +-- ROM ABI
  |    +-- GPIO / UART / ADC / timers / flash / radio controller
  |
  +-- Board
  |    +-- connector/pin wiring
  |    +-- LEDs, buttons and other external devices
  |    +-- board-specific defaults
  |
  +-- World / harness
       +-- RF peers
       +-- scripted IO
       +-- host BLE bridge
       +-- multi-node simulation
```

The rule is simple: firmware sees the SoC. A board only connects things to SoC pins/peripherals. A world connects boards to their environment.

## Current profiles

`phy6252 boards` lists the profiles known to the binary.

- `pb03f-kit` — AI-Thinker PB-03F-Kit on PHY6252. This is the compatibility/default profile.
- `headless` — bare PHY6252, with no PB-03F LED naming in human-readable GPIO output.
- `weact-ch592f` — reserved profile for the WeAct Studio CH592F Core Board. It intentionally fails in the current PHY6252 runtime because CH592F is a different SoC and its CPU/MMIO model has not been implemented yet.

Keeping `weact-ch592f` visible but unsupported is deliberate: it tests that the architecture represents `board -> required SoC` instead of silently treating every board as PHY6252 wiring.

## PHY6252 boundaries

PHY6252 package-pad to AP_GPIO-bit mapping is a SoC/package fact and stays in the PHY6252 command layer. PB-03F meanings such as `red`, `white`, or `Restore` are board facts and must live in the board profile.

The next refactoring steps are:

1. move PB-03F TUI pinout/connector rows into `BoardProfile`;
2. make `sim` carry a board profile per node;
3. split PHY6252-specific modules under `soc/phy6252/`;
4. replace board-specific defaults in BLE/TUI/frontends with data from the selected profile;
5. add a `soc/ch592/` implementation before enabling the WeAct CH592F profile.

## CH592F direction

CH592F must not be implemented as a PHY6252 board variant. Its future path is a sibling SoC backend, for example:

```text
src/
  soc/
    phy6252/
    ch592/
  board/
    pb03f_kit.rs
    weact_ch592f.rs
  world/
```

Once the CH592F CPU and memory/peripheral surface can execute a minimal real firmware image, the `weact-ch592f` board profile can be enabled and populated from the WeAct hardware definition rather than guessed from another board.
