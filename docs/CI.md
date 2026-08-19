# CI and GitHub Actions

Firmverse is designed to be used as a firmware regression tool from another repository, not only from its own CI.

Copyable workflow examples live under [`../examples/github-actions/`](../examples/github-actions/).

## What a firmware CI job should prove

A useful Firmverse job usually answers four separate questions:

1. **Can Firmverse itself build from a locked dependency graph?**
2. **Can the selected board/SoC combination be resolved?**
3. **Can the firmware execute without reaching unknown strict MMIO/ROM behavior?**
4. **For multi-node firmware, does the expected World-level interaction occur?**

Do not replace all four with a single `cargo build` check.

## Build Firmverse in a firmware repository

Recommended pattern:

```yaml
- uses: actions/checkout@v6

- uses: actions/checkout@v6
  with:
    repository: Pom4H/firmverse
    path: .firmverse
    submodules: recursive

- uses: dtolnay/rust-toolchain@stable

- uses: actions/cache@v5
  with:
    path: |
      ~/.cargo/registry/index
      ~/.cargo/registry/cache
      ~/.cargo/git/db
      .firmverse/target
    key: firmverse-${{ runner.os }}-${{ hashFiles('.firmverse/Cargo.lock') }}

- run: cargo build --release --locked --manifest-path .firmverse/Cargo.toml
```

`actions/cache@v5` uses the Node.js 24 action runtime. Self-hosted runners must be new enough for Node 24 actions; GitHub documents runner `2.327.1` or newer as the minimum for cache v5.

## Single-image strict smoke

```yaml
- name: Run firmware in Firmverse
  run: |
    .firmverse/target/release/firmverse \
      --board pb03f-kit \
      --strict \
      --once \
      --raw \
      firmware/build/app.hex \
      | tee firmverse.log
```

Strict mode is the important part. A job that only proves the process starts can hide missing firmware-visible hardware behavior.

If the application is expected to emit a known UART/raw marker, assert it explicitly:

```yaml
- run: grep -q 'UART application-ready' firmverse.log
```

The assertion belongs to the firmware repository because Firmverse cannot know the application contract.

## Multi-node regression

```yaml
- name: Run two nodes in one mesh World
  run: |
    .firmverse/target/release/firmverse sim \
      --board pb03f-kit \
      --world mesh \
      --strict \
      --once \
      --ticks 200 \
      --raw \
      --node a=firmware/build/app.hex \
      --node b=firmware/build/app.hex \
      | tee mesh.log

- run: |
    grep -q '^READY$' mesh.log
    grep -q '^WORLD mesh ' mesh.log
    grep -q '^NODE a ' mesh.log
    grep -q '^NODE b ' mesh.log
```

Add application-level assertions for neighbor discovery, packets, state transitions or GPIO/UART output as appropriate.

## Pin Firmverse for reproducibility

For production firmware CI, do not silently follow every Firmverse `main` commit forever. Pin the checkout to a reviewed tag or commit:

```yaml
- uses: actions/checkout@v6
  with:
    repository: Pom4H/firmverse
    ref: <reviewed-tag-or-commit>
    path: .firmverse
    submodules: recursive
```

Then update the pin deliberately after reviewing Firmverse changes.

## Cache policy

Cache build products; do not make cache contents part of correctness.

The workflow must still work on a cache miss. Use `Cargo.lock` in the key and build with `--locked` so a dependency resolution change cannot be introduced only because a cache was cold.

## Artifacts

Raw Firmverse logs are useful CI artifacts because they are deterministic evidence of what the virtual device observed.

```yaml
- uses: actions/upload-artifact@v7
  if: always()
  with:
    name: firmverse-log
    path: firmverse.log
```

For multi-node tests, keep the entire raw output instead of only the final grep result.

## Firmverse repository CI

Firmverse's own CI has two layers:

### `test`

- Python flasher tests;
- rustfmt;
- Clippy;
- Rust unit tests;
- locked binary build;
- public CLI registry smoke (`socs`, `boards`, `worlds`).

### `demo-smoke`

- builds regression firmware;
- validates the pinned PHY62XX SDK 3.1.2 vendor BLE image;
- runs board smoke;
- runs RSSI ranking;
- runs a two-node mesh;
- runs capability stress;
- restarts against persistent NOR state.

The second job consumes the binary produced by the first job. This prevents a different rebuild from being tested than the one published by the build job.

## Examples

- [`firmware-smoke.yml`](../examples/github-actions/firmware-smoke.yml)
- [`mesh-regression.yml`](../examples/github-actions/mesh-regression.yml)

They are examples, not magic application tests: replace the firmware path and add assertions that represent your firmware's actual contract.
