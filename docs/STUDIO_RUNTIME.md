# Firmverse Studio runtime contract

Firmverse Studio is a frontend over target capabilities. It must not assume that every target is a microcontroller or that every executable artifact is machine code.

## Target families

```text
MCU target
  source / firmware artifact
  -> CPU backend
  -> SoC
  -> Board
  -> World

Managed controller target
  domain source
  -> target IR
  -> target compiler
  -> controller artifact
  -> exact controller runtime
  -> I/O + HMI
  -> World
```

For Saturn-PLC today:

```text
engineering model / future SPL
  -> firmverse/saturn-control-ir@1
  -> Saturn FBD v11 compiler
  -> .fbdbin
  -> crossrw/fbd-runtime v11
```

The frontend can share Run Destination, inspector, trace and scenario concepts while each target keeps its native execution model.

## Browser registry

The Browser runtime `registry` RPC is the source of truth for Studio target discovery. It now includes:

- `boards` and `socs` for machine-code targets;
- `controllers` for managed targets;
- `terminals["saturn-plc"]` for symbolic Saturn I/O;
- `compilerSchemas["saturn-plc"] = "firmverse/saturn-control-ir@1"`.

Studio must build target pickers and I/O inspectors from this registry instead of maintaining a second JavaScript device map.

## Browser compiler RPC

The Saturn compiler is pure Rust and is safe to expose in the existing WASM runtime even though the exact FBD execution runtime is not yet packaged for browser isolation.

Request:

```json
{
  "op": "compileSaturnControlIr",
  "controlIr": {
    "schema": "firmverse/saturn-control-ir@1",
    "project": {
      "name": "Pump station",
      "version": "1",
      "buildTime": "2026-08-29"
    },
    "elements": [
      { "id": "di", "type": "INP_PIN", "params": [1] },
      { "id": "do", "type": "OUT_PIN", "inputs": ["di"], "params": [1] }
    ]
  }
}
```

Response:

```json
{
  "ok": true,
  "artifact": {
    "format": "fbdbin",
    "encoding": "hex",
    "data": "...",
    "bytes": 0,
    "elements": 2,
    "screens": 0,
    "rtl": 7
  },
  "listing": [
    {
      "index": 0,
      "id": "di",
      "type": "INP_PIN",
      "inputs": [],
      "params": [1],
      "comment": ""
    }
  ]
}
```

The artifact bytes come from the same Rust compiler used by native CI and command-line tools. Studio does not contain another `.fbdbin` serializer.

## Execution capability is separate from compilation

`browserExecution=false` for Saturn-PLC is intentional. Browser compilation does not imply browser execution.

The upstream FBD runtime currently owns process-global state. Browser execution should be enabled only after the exact runtime can run inside isolated WASM/process contexts. Firmverse must not introduce a second FBD interpreter merely to make a UI demo work.

## Run Destination model

Studio should ultimately present destinations such as:

```text
PB-03F Simulator
PB-03F connected device
Saturn-PLC Simulator
Saturn-PLC connected device
```

A destination declares capabilities rather than forcing all targets through one implementation:

```text
compile
run
stop
reset
setInput
setSetpoint
inspect
trace
deploy
```

A simulator can expose `compile/run/inspect/trace`; a physical controller can expose `inspect/deploy`; a target may support both through different adapters.

## Safety

Compilation and simulation are non-destructive. Deploying a controller program to physical equipment is a different operation and must remain explicit. Studio should use a plan/diff/apply flow and require human approval for operations that can change physical-control behaviour.
