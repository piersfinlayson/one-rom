#!/usr/bin/env bash
#
# Install the Emscripten SDK (emsdk), used to build One ROM Lens for wasm.
#
# Usage: ci/install-emscripten.sh [install-dir] [version]
#   install-dir  where to clone emsdk (default: $HOME/emsdk)
#   version      emsdk version to install/activate (default: latest)
#
# After running, source "<install-dir>/emsdk_env.sh" to put emcc/emar on PATH.
set -euo pipefail

INSTALL_DIR="${1:-$HOME/emsdk}"
VERSION="${2:-latest}"

if [ ! -d "$INSTALL_DIR/.git" ]; then
    git clone https://github.com/emscripten-core/emsdk.git "$INSTALL_DIR"
fi

cd "$INSTALL_DIR"
git pull --ff-only || true
./emsdk install "$VERSION"
./emsdk activate "$VERSION"

echo "Emscripten ($VERSION) installed in $INSTALL_DIR"
echo "Run: source \"$INSTALL_DIR/emsdk_env.sh\""
