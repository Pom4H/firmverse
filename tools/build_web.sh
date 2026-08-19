#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

rustup target add wasm32-unknown-unknown >/dev/null
cargo build --release --locked --lib --target wasm32-unknown-unknown

rm -rf web/dist
mkdir -p web/dist
cp target/wasm32-unknown-unknown/release/firmverse.wasm web/dist/firmverse.wasm
cp web/src/index.html web/src/styles.css web/src/app.js web/src/elements.js web/src/engine-worker.js web/dist/

printf 'Firmverse browser lab: %s bytes wasm\n' "$(wc -c < web/dist/firmverse.wasm)"
