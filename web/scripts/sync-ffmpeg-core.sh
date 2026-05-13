#!/usr/bin/env bash
set -euo pipefail

SRC_DIR="./node_modules/@ffmpeg/core/dist/umd"
DST_DIR="./public/ffmpeg"

if [[ ! -d "$SRC_DIR" ]]; then
  echo "Missing $SRC_DIR. Run bun install first." >&2
  exit 1
fi

mkdir -p "$DST_DIR"
cp "$SRC_DIR/ffmpeg-core.js" "$DST_DIR/ffmpeg-core.js"
cp "$SRC_DIR/ffmpeg-core.wasm" "$DST_DIR/ffmpeg-core.wasm"
