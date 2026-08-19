# Firmverse

**Virtual embedded systems lab for real firmware.**

Firmverse runs firmware against explicit CPU, SoC and board models, then places one or more virtual devices into a shared World. It can be used as a CLI locally, as a reusable GitHub Action, or as a WebAssembly Browser Lab.

**[Open the live Browser Lab →](https://pom4h.github.io/firmverse/)**

The Browser Lab runs the same Rust/`jjkt/zmu` core locally in WebAssembly. Firmware stays in your browser; add multiple boards, move them through a shared World, inspect GPIO/ADC/UART state and observe RF/RSSI links produced by the Rust model.

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

The important rule is ownership: the CPU executes instructions, the SoC owns firmware-visible hardware, the Board owns physical wiring, and the World owns the environment between devices.

## GitHub Action

The shortest useful integration is one step after your firmware build:

```yaml
- uses: actions/checkout@v6

# Build your firmware here and produce build/app.hex.

- name: Test firmware in Firmverse
  uses: Pom4H/firmverse@v1
  with:
    firmware: build/app.hex
    board: pb03f-kit
    strict: 'true'
    expect: |
      UART application-ready
```

The Action takes care of the Firmverse Rust toolchain, build cache and the pinned `jjkt/zmu` backend. The caller only supplies the firmware image and its behavioral contract.

For a deterministic two-node mesh regression:

```yaml
- name: Test two devices in one virtual world
  uses: Pom4H/firmverse@v1
  with:
    firmware: build/app.hex
    board: pb03f-kit
    mode: mesh
    world: mesh
    nodes: '2'
    ticks: '200'
    strict: 'true'
    expect: |
      WORLD mesh
      NODE n0
      NODE n1
```

Supported Action inputs:

| Input | Default | Meaning |
|---|---|---|
| `firmware` | required | Intel HEX path in the caller workspace |
| `board` | `pb03f-kit` | Firmverse board profile |
| `mode` | `single` | `single` or `mesh` |
| `world` | `mesh` | World used by mesh mode |
| `nodes` | `2` | Number of identical firmware nodes in mesh mode |
| `ticks` | `200` | Deterministic mesh duration |
| `strict` | `true` | Fail on unknown SoC MMIO / vendor ROM behavior |
| `max-insns` | empty | Optional instruction budget override |
| `expect` | empty | Newline-separated fixed strings that must occur in output |
| `log` | `firmverse.log` | Output log path |

Outputs are `log` and `binary`. See [`docs/GITHUB_ACTION.md`](docs/GITHUB_ACTION.md) for the complete contract.

## Status

| Layer | Model | Status |
|---|---|---|
| CPU | Cortex-M0 via `jjkt/zmu` | working native + WebAssembly |
| SoC | PHY6252 | working, strict regression coverage |
| Board | AI-Thinker PB-03F-Kit | working |
| Board | headless PHY6252 | working |
| SoC | WCH CH592F / QingKe V4C | registered, execution backend not implemented yet |
| Board | WeAct CH592F Core Board | registered, waits for CH592F SoC |
| World | mesh / still / crowd | working native + Browser Lab |
| Browser | WASM Worker + Board/World custom elements | working |

`firmverse` fails closed for a registered board whose SoC backend does not exist. CH592F is intentionally **not** treated as another PHY6252 board.

## Browser Lab

The browser runs the **same Rust PHY6252/zmu core**, compiled to `wasm32-unknown-unknown`. CPU execution stays inside a Web Worker; the main thread only visualizes and edits the model.

```text
Rust core → firmverse.wasm → Web Worker → Board + World custom elements
```

Build the static lab:

```sh
bash tools/build_web.sh
python3 -m http.server 8080 -d web/dist
```

Then open `http://localhost:8080` and drop an Intel HEX file into the page.

The first Browser Lab slice supports:

- loading real PHY6252 Intel HEX without uploading it to a server;
- several firmware nodes in one World;
- draggable board positions backed by real World `x/y` coordinates;
- live RF/RSSI links produced by the Rust World model;
- PB-03F visualization generated from `BoardProfile` metadata;
- live indicator state;
- external GPIO switches and ADC sliders generated from SoC pin metadata;
- UART inspection;
- `mesh`, `still` and `crowd` Worlds;
- Run / Stop / Step execution in a Worker.

The important detail is that the browser does not keep its own PB-03F pinout or RF formula. Registry metadata comes from Rust and dragging a node changes the coordinates used by `World::radio()`.

See [`docs/WEB.md`](docs/WEB.md) for the browser ABI, metadata model and editor architecture.

## Local build

```sh
git clone --recurse-submodules https://github.com/Pom4H/firmverse.git
cd firmverse
cargo build --release --locked
```

Inspect the registered layers:

```sh
./target/release/firmverse socs
./target/release/firmverse boards
./target/release/firmverse worlds
```

## Run one firmware image

PHY6252 currently accepts Intel HEX images.

```sh
./target/release/firmverse firmware/build/kit-demo.hex
./target/release/firmverse --raw firmware/build/kit-demo.hex
./target/release/firmverse --strict --once firmware/build/kit-demo.hex
./target/release/firmverse --board pb03f-kit --tui firmware/build/kit-demo.hex
```

`--strict-mmio` remains an alias for `--strict`.

Board selection is independent from the SoC implementation:

```sh
./target/release/firmverse --board pb03f-kit firmware/build/kit-demo.hex
./target/release/firmverse --board headless firmware/build/kit-demo.hex
```

The `headless` profile uses the same PHY6252 SoC without PB-03F connector/LED semantics.

## Run several devices in one World

```sh
./target/release/firmverse sim \
  --world mesh \
  --node a=firmware/build/rssi-rank.hex \
  --node b@3,0=firmware/build/rssi-rank.hex
```

Deterministic CLI form:

```sh
./target/release/firmverse sim \
  --strict \
  --once \
  --ticks 200 \
  --raw \
  --world mesh \
  --node a=firmware/build/rssi-rank.hex \
  --node b=firmware/build/rssi-rank.hex
```

A World works with node identity, position, radio observations and environment inputs. It does not know that a node happens to be a PB-03F-Kit.

## Repository layout

```text
action.yml             public GitHub Action contract
action/run.sh          Action runtime wrapper
src/
  lib.rs               shared native/WASM composition root
  main.rs              native CLI composition
  web_runtime.rs       Browser Lab runtime + raw WASM ABI
  soc.rs               SoC + CPU backend registry
  soc/phy6252/         PHY6252 implementation and package metadata
  board.rs             board profiles and physical wiring metadata
  world.rs             environment / RF model
  sim.rs               native multi-node runtime
  tui.rs               terminal frontend consuming board + SoC metadata
  emu.rs               native single-node runtime frontend
  ble_host.rs          host BLE bridge
web/
  src/                 static Browser Lab + custom elements + Worker
  smoke.mjs            executable WASM regression
firmware/               regression/demo firmware
host/                   host-side helpers
examples/github-actions/ full workflow examples
docs/                   focused documentation
```

PHY6252 implementation files are being namespaced incrementally. They already live physically under `src/soc/phy6252/`; a small `#[path]` compatibility shim in `src/lib.rs` keeps existing internal module names stable while imports are migrated without a giant mechanical diff.

## Documentation

- [`docs/GITHUB_ACTION.md`](docs/GITHUB_ACTION.md) — public Action inputs, outputs and examples.
- [`docs/WEB.md`](docs/WEB.md) — WebAssembly runtime, Worker ABI, Board elements and World editor.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) — ownership boundaries, runtime composition and migration rules.
- [`docs/PHY6252.md`](docs/PHY6252.md) — implemented PHY6252 surface, package pins and regression strategy.
- [`docs/CI.md`](docs/CI.md) — deeper CI strategy and direct CLI integration.
- [`HARDWARE_FLASH.md`](HARDWARE_FLASH.md) — flashing real PHY6252 hardware and SDK 3.1.2 flow.
- [`PROTOCOL.md`](PROTOCOL.md) — raw line protocol used by frontends and tests.

## Design principle

> **SoC defines how the chip works. Board defines how it is physically assembled. World defines the reality around it.**

That boundary is what allows Firmverse to grow from PHY6252 into mixed Cortex-M and RISC-V systems while native, CI and browser frontends continue to execute and visualize the same model.