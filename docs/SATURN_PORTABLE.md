# Portable Saturn integration and state

The optional source package `packages/saturn` complements the existing native
CLI and BrowserLab metadata. SCADA can compile its engineering DSL into the
versioned ControlIR and use the package without adopting the CPU/SoC World or a
second editor. It receives exact program bytes and source-element listings.

The native Rust `SaturnPlc` now exposes `snapshot`, `restore` and observation-only
`render`. The isolated WASM package exposes the same underlying C bridge, one
instance per controller. `render` cannot advance timers. Snapshot support covers
the local deterministic profile; transport/clock/random-dependent programs remain
explicitly unsupported for portable execution and saved-state continuation.

The pure Rust inspector validates graph-reference bounds, terminated metadata,
HMI record boundaries and safe text formatting before native FFI. A portable
loader prepares a candidate instance before replacing a live program. Corrupt
snapshots do not partially modify the instance. Program and runtime identity are
mandatory in the portable snapshot envelope.

The physical controller deployment path is unchanged: compilation or simulation
never flashes a device. Base pin mappings and the supported language/runtime
version are not proof of compatibility with an unspecified physical board.

SCADA retains ownership of equipment, wiring, model time, SQL/archive, alarms and
reports. A common presentation tree in SCADA drives live HTML, frozen report HTML
and the bounded controller-screen backend. Firmverse owns executable artifact and
runtime semantics, not process physics or application-level authorization.
