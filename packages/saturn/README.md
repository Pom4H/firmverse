# @firmverse/saturn

Portable source package for bundlers, Node 22.16+ with `--experimental-strip-types`,
and browser Workers. No npm publication or native addon is required. `src/index.ts`
exports `compileControlIR`, `inspectProgram`, `SaturnRuntime`, and runtime/compiler
hashes. Both generated WASM binaries are embedded for offline use.

There is one binary compiler: the same Rust `saturn_compiler.rs` used by the CLI.
There is one FBD execution implementation: pinned upstream C, compiled with the
same bridge for native and independent WASM instances. The package has no CPU/SoC
emulator dependency. It does not enable the separate BrowserLab controller RPC;
that registry's `browserExecution` still describes that API, not this package.

```ts
import { compileControlIR, SaturnRuntime } from './src/index.ts';
const compiled = compileControlIR({
  schema: 'firmverse/saturn-control-ir@1',
  project: {name:'Example',version:'1',buildTime:'reproducible'},
  elements: [
    {id:'input',type:'INP_PIN',params:[1]},
    {id:'output',type:'OUT_PIN',inputs:['input'],params:[1]},
  ],
});
const plc = SaturnRuntime.createSync();
const result = plc.load(compiled.fbdbin);
if (!result.ok) throw new Error(result.message);
plc.setInput(1,1);
plc.step(20);
const snapshot = plc.snapshot();
const commands = plc.renderScreen(); // observation only, never another step
plc.restore(snapshot);
```

## State and boundaries

RAM values, block storage, edge flags, host I/O, hardware properties, NVRAM and
runtime counters are serialized, not native pointers or lookup caches. The outer
snapshot pins the exact program bytes and runtime hash; the C payload validates
size, program fingerprint and integrity before mutation. C snapshots use the
pinned little-endian ABI and are for trusted local persistence, not a signed
exchange format. `reset()` is a cold reset; retained-state migration across
program replacement is deliberately not implicit. Model clock is supplied by the
caller, one step per scheduled scan. Reading HMI does not advance execution.

Compiler format coverage is all 41 FBD v11 types. Portable execution is bounded
to 256 local blocks with a guarded graph depth and no network variables, Modbus,
MFUN/RTC/random or event-log environment. Unsupported capabilities reject at load
rather than returning convincing fake data. Native snapshot likewise rejects
programs requiring those external capabilities. Native process-global runtime
still uses its existing lock; use WASM instances for concurrent controllers.

HMI capture is bounded to 256 commands. Text is validated against the upstream
32-byte formatting buffer and one floating-point argument; date/time formatting,
other printf conversions and oversized records are rejected. The shared SCADA
presentation layer handles semantic widgets and compiles only supported LCD nodes.

A valid `.fbdbin` is application data, not complete board firmware. Hardware
compatibility, actual board timing, physical module maps, flash deployment and
safety qualification have not been validated here. Numerical corner cases in
upstream blocks still follow that implementation; this is not a claim of
IEC-wide conformance or arbitrary native firmware execution.

## Rebuild and test

Use the pinned recursive submodules, Rust with `wasm32-unknown-unknown` and WASI
SDK 24. Install Cargo dependencies first (or provide an offline vendor directory).

```sh
cargo fetch --locked --manifest-path packages/saturn-compiler/Cargo.toml
WASI_SDK_PATH=/opt/wasi-sdk python3 tools/build_saturn_package.py
node --experimental-strip-types packages/saturn/test.ts
cargo test --locked --lib
```

`manifest.json` records upstream identity and generated artifact hashes. Generated
source is committed so consumers do not need Rust or a C compiler merely to run.
The original upstream MIT license is retained in `RUNTIME_LICENSE`.
