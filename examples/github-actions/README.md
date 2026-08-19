# GitHub Actions examples

These workflows are intended to be copied into a firmware repository under `.github/workflows/` and adapted to the application's build command, firmware path and expected behavior.

Both examples use the public reusable Action:

```yaml
uses: Pom4H/firmverse@v1
```

- [`firmware-smoke.yml`](firmware-smoke.yml) — execute one PHY6252 image in strict mode and assert application output.
- [`mesh-regression.yml`](mesh-regression.yml) — execute multiple copies of one firmware image in a deterministic mesh World and preserve the raw log.

A firmware repository does **not** need to clone Firmverse, initialize `jjkt/zmu`, install Firmverse dependencies or run Cargo itself. The Action owns those details.

Before using the examples in production CI:

1. add your firmware build step;
2. replace `build/app.hex` with the produced HEX path;
3. add `expect` lines that represent your firmware contract;
4. keep strict mode enabled unless the test intentionally explores incomplete hardware modeling;
5. use an exact Firmverse commit SHA instead of `v1` when your supply-chain policy requires immutable pins.

See [`../../docs/GITHUB_ACTION.md`](../../docs/GITHUB_ACTION.md) for the Action contract and [`../../docs/CI.md`](../../docs/CI.md) for deeper CI guidance.
