#!/usr/bin/env bash
#
# Install the host C compiler the firmware's host builds are measured with.
#
# One version, everywhere: CI, the build container, and a developer's own
# machine all install through this script, so a coverage figure and a -Werror
# failure mean the same thing wherever they are produced.  The version is
# pinned in ci/c-compiler-version - see that file before changing it.
#
# The pin tracks the major version of the Arm toolchain in
# ci/arm-toolchain-version, because that is the compiler the firmware actually
# ships from.  firmware/Makefile and firmware/test.mk set no -std, so the C the
# device is built as is whatever that compiler defaults to, and a host build
# that wants to be a test of the firmware's C has to be the same generation.
# ci/c-tests.sh builds with -Werror, so the warnings it gates on are that
# compiler's warnings.
#
# Taken from ppa:ubuntu-toolchain-r/test rather than the distribution, because
# Ubuntu 24.04 archives nothing newer than gcc-14.  A source build is the
# alternative and costs an hour per image.
#
# Usage: ci/install-c-compiler.sh [version]
#   version   gcc major version (default: the pinned ci/c-compiler-version)
#
# Progress goes to stderr and the compiler's name to stdout, so the caller can
# do:
#
#   export CC="$(ci/install-c-compiler.sh)"
#
# Re-running with the compiler already present is a no-op.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

VERSION="${1:-$(tr -d '[:space:]' < "${SCRIPT_DIR}/c-compiler-version")}"
CC_NAME="gcc-${VERSION}"

SUDO=""
[ "$(id -u)" -eq 0 ] || SUDO="sudo"

if ! command -v "${CC_NAME}" >/dev/null; then
    echo "Installing ${CC_NAME}..." >&2

    export DEBIAN_FRONTEND=noninteractive
    ${SUDO} apt-get update >&2

    # The PPA only where the archive cannot supply it, so an image whose
    # distribution has caught up stops carrying a third party source.
    if ! apt-cache policy "${CC_NAME}" 2>/dev/null | grep -q 'Candidate: [0-9]'; then
        echo "${CC_NAME} is not in the archive - adding ppa:ubuntu-toolchain-r/test" >&2
        ${SUDO} apt-get install -y --no-install-recommends software-properties-common >&2
        ${SUDO} add-apt-repository -y ppa:ubuntu-toolchain-r/test >&2
        ${SUDO} apt-get update >&2
    fi

    ${SUDO} apt-get install -y --no-install-recommends "${CC_NAME}" >&2
else
    echo "${CC_NAME} already present" >&2
fi

# gcov ships with the compiler and has to match it: gcov reads a counter file
# only from its own version of gcc, and ci/coverage-run.sh passes this one to
# lcov.  A compiler present without its gcov would fail a coverage capture much
# later, with an error about the counter file rather than about the install.
command -v "gcov-${VERSION}" >/dev/null || {
    echo "${CC_NAME} installed but gcov-${VERSION} is missing." >&2
    exit 1
}

"${CC_NAME}" --version | head -1 >&2

echo "${CC_NAME}"
