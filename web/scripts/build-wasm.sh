#!/usr/bin/env bash
set -euo pipefail

if ! command -v wasm-pack >/dev/null 2>&1; then
  echo "wasm-pack is required. Install it first (preferably via mise)." >&2
  exit 1
fi

wasm-pack build ../crates/synthris-wasm --target web --out-dir ../../web/pkg
