# CI and GitHub Actions

Firmverse is designed to be consumed from a firmware repository. The recommended integration is the public reusable Action:

```yaml
- uses: Pom4H/firmverse@v1
  with:
    firmware: build/app.hex
```

The Action hides Firmverse's own Rust build, cache layout and pinned `jjkt/zmu` dependency. Direct CLI integration remains available when a repository needs lower-level control.

## Recommended firmware CI

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

      - name: Firmverse regression
        id: firmverse
        uses: Pom4H/firmverse@v1
        with:
          firmware: build/app.hex
          board: pb03f-kit
          strict: 'true'
          expect: |
            UART application-ready

      - uses: actions/upload-artifact@v7
        if: always()
        with:
          name: firmverse-log
          path: ${{ steps.firmverse.outputs.log }}
```

A useful Firmverse job proves three things: the selected board/SoC combination can execute, strict mode does not reach unknown firmware-visible behavior, and application-specific output matches the firmware contract.

## Multi-node regression

```yaml
- name: Firmverse mesh regression
  uses: Pom4H/firmverse@v1
  with:
    firmware: build/app.hex
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

The Action's mesh mode intentionally instantiates the same firmware image several times. Use the CLI directly for heterogeneous firmware images, explicit coordinates, or more advanced World control.

## Why strict mode is the default

A CI job that only proves the process starts can hide missing MMIO or vendor-ROM behavior. The Action therefore defaults `strict` to `true`. If emulation reaches an unsupported firmware-visible surface, the regression should fail rather than silently turn that operation into a no-op.

## Assertions

The `expect` input is a newline-separated list of fixed strings. Every line must occur in the raw Firmverse output.

```yaml
with:
  expect: |
    READY
    UART boot complete
    UART selftest pass
```

Application assertions belong to the firmware repository because Firmverse cannot know what a particular product should emit.

## Version pinning

For normal use, follow the v1 compatibility line:

```yaml
uses: Pom4H/firmverse@v1
```

For maximum supply-chain reproducibility, replace `v1` with an exact reviewed Firmverse commit SHA. Major refs are compatibility lines; breaking Action input/output changes require a new major version.

## What the Action does internally

Remote GitHub Actions do not automatically include git submodules. Firmverse depends on the vendored `jjkt/zmu` repository, so the Action explicitly fetches the exact zmu revision pinned by Firmverse when it is absent. It then builds `firmverse` with `cargo build --release --locked` and caches the resulting target directory with `actions/cache@v5`.

This implementation detail is deliberately invisible to firmware repositories.

## Direct CLI integration

Use direct checkout/build only when the reusable Action is too restrictive:

```yaml
- uses: actions/checkout@v6
  with:
    repository: Pom4H/firmverse
    ref: <reviewed-commit>
    path: .firmverse
    submodules: recursive

- uses: dtolnay/rust-toolchain@stable

- run: cargo build --release --locked --manifest-path .firmverse/Cargo.toml

- run: |
    .firmverse/target/release/firmverse sim \
      --board pb03f-kit \
      --strict \
      --once \
      --ticks 200 \
      --raw \
      --world mesh \
      --node a=build/a.hex \
      --node b@3,0=build/b.hex
```

This path exposes the complete CLI surface, including heterogeneous nodes and explicit positions.

## Firmverse's own CI

The repository validates both the engine and the public Action contract.

The main `ci` workflow covers rustfmt, Clippy, Rust tests, flasher tests, the locked binary build, PHY62XX SDK 3.1.2 vendor BLE, board smoke, RSSI ranking, multi-node mesh, capability stress and persistent NOR.

The `public-action-smoke` workflow builds real Cortex-M0 fixture firmware and then invokes Firmverse through `uses: ./` in both single-node and mesh modes. That is the release gate for the `Pom4H/firmverse@v1` interface.

## Full examples

- [`firmware-smoke.yml`](../examples/github-actions/firmware-smoke.yml)
- [`mesh-regression.yml`](../examples/github-actions/mesh-regression.yml)
- [`GITHUB_ACTION.md`](GITHUB_ACTION.md) — complete Action contract.
