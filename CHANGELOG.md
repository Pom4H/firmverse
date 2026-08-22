# Changelog

All notable changes to Firmverse are documented in this file. Firmverse follows
Semantic Versioning for the CLI, reusable GitHub Action and public Rust
interfaces.

## [1.5.0] - 2026-08-22

Firmverse 1.5.0 is the first consolidated release of the virtual embedded
systems lab. It combines deterministic firmware emulation, multi-node Worlds,
the Browser Lab, CI integration and recoverable real-hardware flashing behind
one project boundary.

### Added

- A shared SoC/Board/World runtime with explicit ownership boundaries.
- Deterministic multi-node simulation with independent firmware images,
  identities, positions and RF/RSSI observations.
- Built-in `mesh`, `still` and `crowd` Worlds.
- A WebAssembly Browser Lab using the same Rust execution and World core as the
  native CLI.
- A reusable `Pom4H/firmverse@v1` GitHub Action with behavioral log assertions,
  strict-mode execution and mesh regression support.
- `require-advertising`, a PHY6252 single-node Action assertion that fails when
  guest firmware never reaches BLE advertising enable.
- A transport-independent PHY62xx flasher core used by both real USB-UART
  hardware and a deterministic in-memory ROM/NOR harness.
- Reconstruction of a bootable Intel HEX from bytes actually programmed into
  harness NOR.
- Application-assisted ROM handoff: BREAK, project-owned token, safe application
  reset, ROM synchronization at 9600 baud and recovery from an already-open
  command monitor.
- Stateful NOR persistence, SPI flash unique-ID modeling and boot-from-programmed-
  NOR proof.
- PB-03F board metadata, live pinout TUI, ADC/GPIO controls and host BLE bridges
  for Linux and macOS.
- Host-backed HCI LE scan-parameter and scan-enable command paths used by the
  real SDK 3.1.2 observer firmware.
- Initial CH592F/WeAct registrations that fail closed until the QingKe backend is
  implemented.

### Improved

- PHY6252 SDK 3.1.2 compatibility, including ROM, OSAL, BLE, timer, cache, SPI,
  DMA, crypto and secure-boot execution paths.
- Strict MMIO and ROM diagnostics with exact fault context.
- Boot-info handling and explicit distinction between the PHY62xx boot start
  field and Intel HEX type-05 entry points.
- Deterministic selection of the SDK boot vector at `0x1fff1838`, preventing
  incidental SRAM data from being misidentified as a Cortex-M vector table.
- CI coverage across Python tooling, Rust formatting, Clippy, unit tests, demo
  firmware, Browser Lab smoke tests and the public Action contract.
- Rust 1.98 CI compatibility without raising the source API floor solely for a
  toolchain-style Clippy lint.

### Safety

- The application handoff token remains product-owned and is never embedded in
  generic Firmverse code.
- Full-chip erase remains opt-in; normal flashing plans only affected sectors.
- The deterministic harness models NOR `1 → 0` programming and sector erase
  semantics instead of returning unconditional success.
- Application-assisted entry first probes recoverable ROM states, allowing an
  interrupted flash session to resume without another physical reset.

[1.5.0]: https://github.com/Pom4H/firmverse/releases/tag/v1.5.0
