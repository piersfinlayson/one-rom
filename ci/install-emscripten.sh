#!/usr/bin/env bash
#
# Install the Emscripten SDK (emsdk), used to build One ROM Lens for wasm.
#
# Usage: ci/install-emscripten.sh [install-dir] [version]
#   install-dir  where to clone emsdk (default: $HOME/emsdk)
#   version      emsdk version to install/activate (default: the pinned
#                ci/emscripten-version)
#
# After running, source "<install-dir>/emsdk_env.sh" to put emcc/emar on PATH.
#
# The version is pinned rather than tracking "latest" because an emscripten
# release can break the build with no change on our side: 6.0.6 flipped
# DEFAULT_TO_CXX to false, which stopped libc++abi/libunwind being linked and
# broke every Rust wasm32-unknown-emscripten link (see rust/lens/build.rs).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

INSTALL_DIR="${1:-$HOME/emsdk}"
VERSION="${2:-$(tr -d '[:space:]' < "${SCRIPT_DIR}/emscripten-version")}"

if [ ! -d "$INSTALL_DIR/.git" ]; then
    git clone https://github.com/emscripten-core/emsdk.git "$INSTALL_DIR"
fi

cd "$INSTALL_DIR"
git pull --ff-only || true
./emsdk install "$VERSION"
./emsdk activate "$VERSION"

echo "Emscripten ($VERSION) installed in $INSTALL_DIR"
echo "Run: source \"$INSTALL_DIR/emsdk_env.sh\""
