#!/usr/bin/env bash
#
# Build One ROM Lens (wasm) for representative configs, so the wasm build cannot
# silently break.  Requires emcc/emar on PATH (see ci/install-emscripten.sh) and
# the wasm32-unknown-emscripten Rust target.
set -euo pipefail

cd "$(dirname "$0")/.."

rustup target add wasm32-unknown-emscripten >/dev/null 2>&1 || true

build_lens() {
    local config="$1" board="$2"
    echo "== One ROM Lens: CONFIG=$config BOARD=$board =="
    (cd rust && CONFIG="$config" BOARD="$board" \
        cargo build -p onerom-lens --target wasm32-unknown-emscripten)
}

# 2364 (8-bit) and 27C400 (16-bit): exercises both data widths and the 16-bit
# BYTE#/word-size geometry path.
build_lens onerom-config/test-0.json         fire-24-a
build_lens onerom-config/test/40-random.json fire-40-a

echo "One ROM Lens builds OK."
