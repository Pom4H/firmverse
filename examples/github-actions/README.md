# GitHub Actions examples

These workflows are intended to be copied into a firmware repository under `.github/workflows/` and adapted to the application's firmware path and expected behavior.

- [`firmware-smoke.yml`](firmware-smoke.yml) — build a pinned Firmverse checkout and execute one PHY6252 image in strict mode.
- [`mesh-regression.yml`](mesh-regression.yml) — execute two copies of a firmware image in a deterministic mesh World and preserve the raw log.

Before using them in production CI:

1. replace the example firmware path;
2. pin `Pom4H/firmverse` to a reviewed tag or commit;
3. add assertions for your firmware's UART/GPIO/protocol behavior;
4. keep `--strict` unless the test is intentionally exploring incomplete hardware modeling.

See [`../../docs/CI.md`](../../docs/CI.md) for the reasoning behind the workflow structure.
