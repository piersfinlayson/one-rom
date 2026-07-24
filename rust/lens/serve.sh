#!/usr/bin/env bash
#
# Build One ROM Lens for a CONFIG/BOARD and serve it locally in a browser.
#
# Usage: rust/lens/serve.sh [CONFIG] [BOARD] [PORT]
#   CONFIG  firmware config JSON (default: onerom-config/test/24-random-23xx.json)
#   BOARD   board name           (default: fire-24-a)
#   PORT    http port            (default: 8000)
#
# Requires emcc/emar on PATH (see ci/install-emscripten.sh, or `brew install
# emscripten`) and the wasm32-unknown-emscripten Rust target
# (`rustup target add wasm32-unknown-emscripten`).
set -euo pipefail

# This script lives in rust/lens/; the repo root is two levels up.
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

CONFIG="${1:-onerom-config/test/24-random-23xx.json}"
BOARD="${2:-fire-24-a}"
PORT="${3:-8000}"

OUT="rust/lens/build/web"
WASM_DIR="rust/target/wasm32-unknown-emscripten/debug"

echo "Building One ROM Lens (CONFIG=$CONFIG BOARD=$BOARD)..."
(cd rust && CONFIG="$CONFIG" BOARD="$BOARD" \
    cargo build -p onerom-lens --target wasm32-unknown-emscripten)

# Assemble the web bundle: static assets + the emscripten loader and wasm.
mkdir -p "$OUT"
cp rust/lens/web/index.html rust/lens/web/logic-analyzer.js rust/lens/web/style.css "$OUT/"
cp "$WASM_DIR/onerom-lens.js" "$WASM_DIR/onerom_lens.wasm" "$OUT/"

echo "Serving One ROM Lens at http://localhost:$PORT/  (Ctrl-C to stop)"
python3 -m http.server -d "$OUT" "$PORT"
