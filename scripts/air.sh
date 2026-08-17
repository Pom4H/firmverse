#!/usr/bin/env bash
# Advertise the kit demo over Bluetooth LE (macOS CoreBluetooth = RF PHY).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --release --manifest-path "$ROOT/Cargo.toml"
bash "$ROOT/host/ble/build.sh"
exec python3 "$ROOT/scripts/air.py" "$@"
