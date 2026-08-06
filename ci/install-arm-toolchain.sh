#!/usr/bin/env bash
#
# Install the Arm GNU toolchain (arm-none-eabi), which builds the One ROM
# firmware and plugins.
#
# One toolchain, one version, everywhere: CI, the build container, and a
# developer's own machine all install through this script, so a firmware binary
# built in one place is built by the same compiler as one built in another.
# The version is pinned in ci/arm-toolchain-version - see that file before
# changing it.
#
# Usage: ci/install-arm-toolchain.sh [install-dir] [version]
#   install-dir  where to unpack (default: $HOME/arm-gnu-toolchain)
#   version      toolchain version (default: the pinned ci/arm-toolchain-version)
#
# Progress goes to stderr and the toolchain's bin directory to stdout, so the
# caller can do:
#
#   export TOOLCHAIN="$(ci/install-arm-toolchain.sh)"
#
# Re-running with a version already present is a no-op.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

INSTALL_DIR="${1:-$HOME/arm-gnu-toolchain}"
VERSION="${2:-$(cat "${SCRIPT_DIR}/arm-toolchain-version")}"

# Arm publishes a build per host platform, named by this suffix.  Note there is
# no Intel macOS build from 15.3.Rel1 onwards.
case "$(uname -s)/$(uname -m)" in
    Linux/x86_64)         ARCH="x86_64" ;;
    Linux/aarch64)        ARCH="aarch64" ;;
    Linux/arm64)          ARCH="aarch64" ;;
    Darwin/arm64)         ARCH="darwin-arm64" ;;
    Darwin/x86_64)        ARCH="darwin-x86_64" ;;
    *)
        echo "Unsupported host $(uname -s)/$(uname -m) - no Arm GNU toolchain build for it" >&2
        exit 1
        ;;
esac

NAME="arm-gnu-toolchain-${VERSION}-${ARCH}-arm-none-eabi"
TARBALL="${NAME}.tar.xz"

# Arm moved toolchain hosting to gitlab.arm.com from 15.3.Rel1 onwards;
# developer.arm.com carries 15.2.rel1 and earlier only, so a version bump across
# that boundary is a URL change as well as a version change.
BASE_URL="https://gitlab.arm.com/api/v4/projects/tooling%2Fgnu-toolchains-for-arm/packages/generic/gnu-toolchain/${VERSION}"

TARGET="${INSTALL_DIR}/${NAME}"
LINK="${INSTALL_DIR}/arm-toolchain"

if [ ! -x "${TARGET}/bin/arm-none-eabi-gcc" ]; then
    echo "Installing Arm GNU toolchain ${VERSION} (${ARCH}) into ${INSTALL_DIR}..." >&2
    mkdir -p "${INSTALL_DIR}"

    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp}"' EXIT

    curl -fsSL -o "${tmp}/${TARBALL}" "${BASE_URL}/${TARBALL}"

    # Arm ships a plain "<sha256>  <filename>" line, so verify before unpacking
    # rather than trusting a 150MB download over the network.
    curl -fsSL -o "${tmp}/${TARBALL}.sha256asc" "${BASE_URL}/${TARBALL}.sha256asc"
    echo "Verifying checksum..." >&2
    if command -v sha256sum >/dev/null 2>&1; then
        ( cd "${tmp}" && sha256sum -c "${TARBALL}.sha256asc" >&2 )
    else
        ( cd "${tmp}" && shasum -a 256 -c "${TARBALL}.sha256asc" >&2 )
    fi

    # Unpack to a temporary name and move into place, so an interrupted install
    # cannot leave a half-extracted tree that the next run mistakes for good.
    tar -xf "${tmp}/${TARBALL}" -C "${tmp}"
    rm -rf "${TARGET}.partial"
    mv "${tmp}/${NAME}" "${TARGET}.partial"
    rm -rf "${TARGET}"
    mv "${TARGET}.partial" "${TARGET}"
else
    echo "Arm GNU toolchain ${VERSION} (${ARCH}) already present in ${INSTALL_DIR}" >&2
fi

# A stable path that does not name the version, for callers that want to point
# at "the" toolchain - firmware/Makefile's default TOOLCHAIN is one.
ln -sfn "${TARGET}" "${LINK}"

"${LINK}/bin/arm-none-eabi-gcc" --version | head -1 >&2

echo "${LINK}/bin"
