#!/usr/bin/env bash
set -euo pipefail

workspace="${GITHUB_WORKSPACE:-$PWD}"
binary="${FIRMVERSE_BIN:?FIRMVERSE_BIN is required}"
firmware="${FIRMVERSE_FIRMWARE:?FIRMVERSE_FIRMWARE is required}"
board="${FIRMVERSE_BOARD:-pb03f-kit}"
mode="${FIRMVERSE_MODE:-single}"
world="${FIRMVERSE_WORLD:-mesh}"
nodes="${FIRMVERSE_NODES:-2}"
ticks="${FIRMVERSE_TICKS:-200}"
strict="${FIRMVERSE_STRICT:-true}"
require_advertising="${FIRMVERSE_REQUIRE_ADVERTISING:-false}"
max_insns="${FIRMVERSE_MAX_INSNS:-}"
expect="${FIRMVERSE_EXPECT:-}"
log="${FIRMVERSE_LOG:-firmverse.log}"

if [[ "$firmware" != /* ]]; then
  firmware="$workspace/$firmware"
fi
if [[ "$log" != /* ]]; then
  log="$workspace/$log"
fi

if [[ ! -f "$firmware" ]]; then
  echo "::error::Firmverse firmware not found: $firmware" >&2
  exit 2
fi
if [[ ! -x "$binary" ]]; then
  echo "::error::Firmverse binary not executable: $binary" >&2
  exit 2
fi

for pair in "strict:$strict" "require-advertising:$require_advertising"; do
  name="${pair%%:*}"
  value="${pair#*:}"
  case "$value" in
    true|false) ;;
    *)
      echo "::error::$name must be true or false, got: $value" >&2
      exit 2
      ;;
  esac
done

if [[ -n "$max_insns" && ! "$max_insns" =~ ^[0-9]+$ ]]; then
  echo "::error::max-insns must be an integer, got: $max_insns" >&2
  exit 2
fi
if [[ "$require_advertising" == true && "$mode" != single ]]; then
  echo "::error::require-advertising currently requires mode=single" >&2
  exit 2
fi

mkdir -p "$(dirname "$log")"

common=()
if [[ "$strict" == true ]]; then
  common+=(--strict)
fi
if [[ -n "$max_insns" ]]; then
  common+=(--max-insns "$max_insns")
fi

case "$mode" in
  single)
    cmd=(
      "$binary"
      --board "$board"
      --once
      --raw
      "${common[@]}"
      "$firmware"
    )
    ;;
  mesh)
    if [[ ! "$nodes" =~ ^[0-9]+$ || "$nodes" -lt 2 ]]; then
      echo "::error::mesh mode requires nodes >= 2, got: $nodes" >&2
      exit 2
    fi
    if [[ ! "$ticks" =~ ^[0-9]+$ || "$ticks" -lt 1 ]]; then
      echo "::error::ticks must be >= 1, got: $ticks" >&2
      exit 2
    fi
    cmd=(
      "$binary"
      sim
      --board "$board"
      --once
      --raw
      --world "$world"
      --ticks "$ticks"
      "${common[@]}"
    )
    for ((i = 0; i < nodes; i++)); do
      cmd+=(--node "n${i}=$firmware")
    done
    ;;
  *)
    echo "::error::mode must be single or mesh, got: $mode" >&2
    exit 2
    ;;
esac

printf 'Firmverse:'
printf ' %q' "${cmd[@]}"
printf '\n'

"${cmd[@]}" 2>&1 | tee "$log"

if [[ "$require_advertising" == true ]]; then
  if ! grep -Fq -- 'BLE HCI LE_SetAdvEnable enabled=1' "$log"; then
    echo "::error::PHY6252 firmware did not enable BLE advertising before the execution budget ended" >&2
    exit 3
  fi
  echo '✓ BLE advertising enabled'
fi

if [[ -n "$expect" ]]; then
  while IFS= read -r needle || [[ -n "$needle" ]]; do
    [[ -z "$needle" ]] && continue
    if ! grep -Fq -- "$needle" "$log"; then
      echo "::error::Expected Firmverse output not found: $needle" >&2
      exit 3
    fi
    echo "✓ $needle"
  done <<< "$expect"
fi

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  echo "log=$log" >> "$GITHUB_OUTPUT"
fi
