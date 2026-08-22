# Firmverse GitHub Action

Firmverse can be consumed directly from another repository as a reusable GitHub Action:

```yaml
- uses: Pom4H/firmverse@v1
  with:
    firmware: build/app.hex
```

The Action is intentionally the primary CI interface. A firmware repository should not need to clone Firmverse, initialize its `jjkt/zmu` submodule, know the Cargo layout, or reconstruct the correct CLI flags.

## Minimal workflow

```yaml
name: firmware

on:
  pull_request:
  push:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6

      - name: Build firmware
        run: ./build-firmware.sh

      - name: Run Firmverse
        uses: Pom4H/firmverse@v1
        with:
          firmware: build/app.hex
          board: pb03f-kit
          strict: 'true'
```

## Behavioral assertions

`expect` accepts newline-separated fixed strings. Every non-empty line must occur in the Firmverse log or the Action fails.

```yaml
- uses: Pom4H/firmverse@v1
  with:
    firmware: build/app.hex
    expect: |
      UART application-ready
      UART selftest pass
```

Single-node Action mode is deterministic `--once --raw`; assert firmware/application markers rather than the interactive raw-mode `READY` marker. Mesh mode does emit `READY` as part of its multi-node protocol.

This keeps application behavior in the firmware repository while Firmverse owns emulation behavior.

## BLE advertising boot assertion

For a single PHY6252 node, `require-advertising` turns successful BLE startup
into a first-class CI assertion. The Action fails if the real guest firmware
does not reach `HCI_LE_SetAdvEnable(1)` before its execution budget ends:

```yaml
- uses: Pom4H/firmverse@v1
  with:
    firmware: build/app.hex
    board: pb03f-kit
    strict: 'true'
    require-advertising: 'true'
```

The option intentionally requires `mode: single`. It proves that CPU and OSAL
startup continued far enough to enable advertising; it does not claim that an
external radio received a packet. Because the pinned SDK runs a startup LED
sequence before BLE initialization, this assertion uses a deterministic 50
million instruction budget unless `max-insns` is set explicitly.

## Mesh regression

The same firmware image can be instantiated several times inside one deterministic World:

```yaml
- uses: Pom4H/firmverse@v1
  with:
    firmware: build/app.hex
    mode: mesh
    world: mesh
    nodes: '3'
    ticks: '500'
    strict: 'true'
    expect: |
      READY
      WORLD mesh
      NODE n0
      NODE n1
      NODE n2
```

Each node gets an independent firmware instance and deterministic identity. The current `mesh` Action mode intentionally uses the same firmware image for all nodes; heterogeneous node specifications remain available through the Firmverse CLI.

## Inputs

| Input | Required | Default | Description |
|---|---:|---|---|
| `firmware` | yes | — | Firmware Intel HEX path, relative to the caller workspace unless absolute |
| `board` | no | `pb03f-kit` | Registered Firmverse board profile |
| `mode` | no | `single` | `single` or `mesh` |
| `world` | no | `mesh` | World used by mesh mode |
| `nodes` | no | `2` | Number of identical firmware nodes in mesh mode; must be at least 2 |
| `ticks` | no | `200` | Deterministic world ticks in mesh mode |
| `strict` | no | `true` | Enable fail-closed unknown MMIO/vendor-ROM behavior |
| `require-advertising` | no | `false` | In single-node PHY6252 mode, fail unless guest firmware enables BLE advertising; defaults that assertion to 50M instructions |
| `max-insns` | no | empty | Optional instruction budget override |
| `expect` | no | empty | Newline-separated fixed strings required in the log |
| `log` | no | `firmverse.log` | Log path, relative to the caller workspace unless absolute |

## Outputs

| Output | Description |
|---|---|
| `log` | Absolute path to the generated Firmverse log |
| `binary` | Absolute path to the Firmverse binary built by the Action |

Example artifact upload:

```yaml
- name: Firmverse
  id: firmverse
  uses: Pom4H/firmverse@v1
  with:
    firmware: build/app.hex

- uses: actions/upload-artifact@v7
  if: always()
  with:
    name: firmverse-log
    path: ${{ steps.firmverse.outputs.log }}
```

## What the Action manages

The caller does not need to prepare Firmverse dependencies. The Action:

1. installs the Rust toolchain;
2. restores Cargo sources and compiled Firmverse dependencies from a cache keyed by the Rust compiler and Firmverse source identity;
3. ensures the pinned `jjkt/zmu` revision is present, even though GitHub does not download repository submodules for remote Actions;
4. builds the locked `firmverse` release binary;
5. runs either single-node or mesh mode;
6. captures a log and evaluates `expect` assertions.

The pinned zmu revision is part of the Action implementation and changes only with a reviewed Firmverse update.

## Versioning

Use the major Action ref for normal CI:

```yaml
uses: Pom4H/firmverse@v1
```

For maximum supply-chain reproducibility, pin an exact reviewed Firmverse commit SHA instead. The `v1` ref is the compatibility line: its input/output contract should remain backward-compatible while v1 evolves.

## Runner support

The v1 Action is tested on `ubuntu-latest`. It uses Bash and the standard GitHub-hosted runner toolchain. Other runners may work, but Linux is the supported CI target until they are covered by the Action smoke matrix.
