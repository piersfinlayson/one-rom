#!/usr/bin/env bash
#
# Install zig and cargo-zigbuild, which link the CLI and Studio Linux packages
# against the glibc version in ci/linux-min-glibc-version rather than the build
# machine's own.
#
# CI and the release build machine both install through this script, so a
# package built in one place is linked by the same zig as one built in the
# other.  The versions are pinned in ci/zig-version and
# ci/cargo-zigbuild-version.
#
# Usage: ci/install-zig.sh [install-dir]
#   install-dir  where to install (default: $HOME/zig-toolchain)
#
# Progress goes to stderr and the bin directory holding zig and cargo-zigbuild
# to stdout, so the caller can do:
#
#   export PATH="$(ci/install-zig.sh):$PATH"
#
# Re-running with both versions already present is a no-op.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

INSTALL_DIR="${1:-$HOME/zig-toolchain}"
ZIG_VERSION="$(tr -d '[:space:]' < "${SCRIPT_DIR}/zig-version")"
ZIGBUILD_VERSION="$(tr -d '[:space:]' < "${SCRIPT_DIR}/cargo-zigbuild-version")"

case "$(uname -s)/$(uname -m)" in
    Linux/x86_64)   HOST="x86_64-linux" ;;
    Linux/aarch64)  HOST="aarch64-linux" ;;
    *)
        echo "Unsupported host $(uname -s)/$(uname -m) - the Linux packages build on Linux only" >&2
        exit 1
        ;;
esac

NAME="zig-${HOST}-${ZIG_VERSION}"
TARGET="${INSTALL_DIR}/${NAME}"
BIN="${INSTALL_DIR}/bin"

if [ ! -x "${TARGET}/zig" ]; then
    echo "Installing zig ${ZIG_VERSION} (${HOST}) into ${INSTALL_DIR}..." >&2
    mkdir -p "${INSTALL_DIR}"

    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp}"' EXIT

    # ziglang.org publishes each release's tarball URL and sha256 in one index,
    # so verify against that before unpacking.
    curl -fsSL -o "${tmp}/index.json" https://ziglang.org/download/index.json
    read -r URL SHA256 < <(python3 - "${tmp}/index.json" "${ZIG_VERSION}" "${HOST}" <<'EOF'
import json, sys
entry = json.load(open(sys.argv[1]))[sys.argv[2]][sys.argv[3]]
print(entry["tarball"], entry["shasum"])
EOF
)
    curl -fsSL -o "${tmp}/${NAME}.tar.xz" "${URL}"
    echo "Verifying checksum..." >&2
    echo "${SHA256}  ${tmp}/${NAME}.tar.xz" | sha256sum -c - >&2

    # Unpack to a temporary name and move into place, so an interrupted install
    # cannot leave a half-extracted tree that the next run mistakes for good.
    tar -xf "${tmp}/${NAME}.tar.xz" -C "${tmp}"
    rm -rf "${TARGET}.partial"
    mv "${tmp}/${NAME}" "${TARGET}.partial"
    rm -rf "${TARGET}"
    mv "${TARGET}.partial" "${TARGET}"
else
    echo "zig ${ZIG_VERSION} (${HOST}) already present in ${INSTALL_DIR}" >&2
fi

mkdir -p "${BIN}"
ln -sfn "${TARGET}/zig" "${BIN}/zig"

if [ "$("${BIN}/cargo-zigbuild" --version 2>/dev/null)" != "cargo-zigbuild ${ZIGBUILD_VERSION}" ]; then
    echo "Installing cargo-zigbuild ${ZIGBUILD_VERSION} into ${INSTALL_DIR}..." >&2
    cargo install cargo-zigbuild --version "${ZIGBUILD_VERSION}" --locked --force \
        --root "${INSTALL_DIR}" >&2
else
    echo "cargo-zigbuild ${ZIGBUILD_VERSION} already present in ${INSTALL_DIR}" >&2
fi

echo "zig $("${BIN}/zig" version), $("${BIN}/cargo-zigbuild" --version)" >&2

echo "${BIN}"
