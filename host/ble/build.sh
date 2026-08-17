#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
swiftc -O -framework CoreBluetooth -framework Foundation \
  -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist -Xlinker "$ROOT/Info.plist" \
  -o "$ROOT/phy6252-ble" "$ROOT/Ble.swift"
echo "$ROOT/phy6252-ble"
