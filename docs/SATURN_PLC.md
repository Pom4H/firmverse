# Saturn-PLC in Firmverse

Saturn-PLC is the first **managed controller target** in Firmverse.

It is deliberately not registered as a fake MCU or SoC. Firmverse now has two execution families:

```text
MCU firmware
  -> CPU backend
  -> SoC
  -> Board
  -> World

managed controller program
  -> controller runtime
  -> I/O + HMI boundary
  -> World / Studio
```

For Saturn-PLC the program artifact is `.fbdbin` and the runtime is the exact upstream
[`crossrw/fbd-runtime`](https://github.com/crossrw/fbd-runtime) v11 pinned in
`third_party/fbd-runtime`.

## Why the exact runtime

Firmverse must not maintain a second interpretation of FBD semantics. A program that passes in
the virtual target should execute the same element semantics as the real controller runtime.
The Rust layer validates the artifact boundary and owns developer tooling; the C runtime owns
execution.

Before calling `fbdInit`, Firmverse checks:

- the v11 `END_MARK` and element types;
- input/parameter layout bounds;
- declared schema size;
- CRC32 when the schema declares its size;
- required RTL, HMI screen count and Modbus use.

Then the native backend runs:

```text
fbdInit
  -> fbdSetMemory
  -> fbdDoStep
```

## CLI

List managed targets:

```bash
firmverse controllers
```

Run a real Saturn program with simulated field inputs:

```bash
firmverse plc pump.fbdbin \
  --input AI1=430 \
  --input DI1=1 \
  --input DI2=0 \
  --steps 100 \
  --period-ms 10
```

Override an HMI setpoint by index before the run:

```bash
firmverse plc pump.fbdbin --setpoint 0=450
```

Machine-readable mode is intended for CI, Studio and agents:

```bash
firmverse plc pump.fbdbin --input AI1=430 --raw
```

It emits `CONTROLLER`, `PROGRAM`, `PROJECT`, `SP`, `WP` and `OUT` records.

## Base Saturn-PLC I/O profile

Inputs:

- `DI1..DI10` -> FBD pin indexes `1..10`;
- `AI1..AI2` -> `11..12`;
- `T1..T5` -> `13..17`.

Outputs:

- `DO1..DO11` -> `1..11`;
- `AO1..AO2` -> `12..13`.

Raw numeric pin indexes are an implementation detail of the package. Studio/project languages
should refer to symbolic terminals.

## Browser / Studio registry

The existing Browser Lab registry now exposes managed controllers next to boards and SoCs. For
Saturn-PLC it also publishes the symbolic input/output terminal profile. This lets Studio build
Run Destination pickers and I/O inspectors from the Rust source of truth instead of maintaining
a second pin map in JavaScript.

`browserExecution` remains `false` until the exact upstream runtime is packaged as an isolated
WASM runtime instance. Metadata is available in the browser today; Firmverse does not pretend a
second FBD interpreter is equivalent to the device runtime.

## HMI / debugger surface

The runtime bridge already exposes:

- project name/version/build time;
- setpoints: caption, current/default/min/max/divider/step;
- watchpoints: caption/value/divider;
- I/O hints embedded in `.fbdbin`;
- physical input/output state;
- hardware properties used by FBD (`Ethernet`, `NTP`, timezone, battery voltage).

These values are the first Saturn-PLC debugger/inspector model for Firmverse Studio.

## Safety and architecture boundaries

Firmverse executes and inspects controller programs. It does **not** automatically activate a
program on a real PLC. Flash/deploy remains an explicit Run Destination operation with a plan
and human approval for destructive or physical-control changes.

The upstream FBD runtime currently owns process-global state. Native Firmverse therefore guards
it with a process-wide lock and runs one exact Saturn runtime instance at a time. Multi-controller
Worlds should use isolated runtime instances (WASM/process contexts) rather than introducing a
second Rust FBD interpreter.

## Next Studio slice

The next layer should expose the same target through the Studio runtime API:

```text
Target: Saturn-PLC
Artifact: .fbdbin
Run Destination: Simulator / real PLC
Inspectors: I/O / SP / WP / HMI / trace
Scenario: input changes + time + expected outputs
```

That API is shared conceptually with MCU targets even though their execution backends are
fundamentally different.
