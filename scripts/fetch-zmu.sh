#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${1:-$ROOT/third_party/zmu}"
REPO_URL="${ZMU_REPO_URL:-https://github.com/jjkt/zmu.git}"

if [[ ! -d "$DEST/.git" ]]; then
  mkdir -p "$(dirname "$DEST")"
  git clone --depth 1 "$REPO_URL" "$DEST"
else
  git -C "$DEST" fetch --depth 1 origin
  git -C "$DEST" reset --hard FETCH_HEAD
fi
echo "$DEST"
