# Hardware-wallet integration

`Pom4H/hardware-wallet` is the first complete-system consumer of Firmverse.
The integration starts with the narrowest trustworthy hardware boundary:
execute the actual linked Cortex-M image and replace the provisional stack
allowance with a measured high-water mark.

## Contract

The firmware provides a non-inlined completion function whose symbol contains
`firmverse_done`. Firmverse:

1. loads the ELF into a virtual Cortex-M board;
2. paints free RAM before reset;
3. places a hardware breakpoint on the completion function;
4. executes the real image through QEMU's Cortex-M backend;
5. dumps RAM after completion;
6. calculates the deepest stack write and fails on overflow;
7. emits JSON and Markdown evidence tied to the ELF SHA-256.

The GitHub Action is located at:

```text
actions/cortex-m-probe
```

A consumer supplies the ELF, target, virtual board memory map, completion
symbol and current stack gate. The outputs include measured peak stack and a
recommended stack allocation with safety margin.

## Why this belongs in Firmverse

The wallet domain, chain adapters and cryptography remain independent of the
emulator. Firmverse owns only execution evidence:

```text
hardware-wallet source
        ↓
linked Cortex-M ELF
        ↓
Firmverse virtual board
        ↓
stack / completion / future cycle trace
```

The same backend can later run bootloaders, BLE devices and other embedded
products without importing their domain logic.

## Current backend

The first backend uses QEMU plus GDB because it gives us a deterministic,
auditable execution boundary immediately. It already measures whole-program
stack usage, including crypto and parser frames.

Exact instruction/cycle accounting is intentionally not fabricated. The JSON
report currently marks cycle count unavailable. The next Firmverse backend
step is one of:

- a QEMU TCG instruction-count plugin with a documented Cortex-M cycle model;
- Firmverse-native instruction accounting;
- a DWT cycle counter on an evaluation board, reconciled against the emulator.

Until then, MCU frequency remains a measured-but-unresolved requirement while
Flash and stack can already gate candidate parts.

## Security limits

This integration does not claim side-channel equivalence, secure-element
behavior, analog power behavior or production fault-injection resistance.
Those require the later Firmverse peripheral models, NodeSpice co-simulation
and hardware-in-the-loop tests.
