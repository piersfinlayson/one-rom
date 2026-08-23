#!/usr/bin/env bash
#
# Install lcov, which captures the coverage ci/coverage-run.sh measures.
#
# One version, everywhere: CI, the build container, and a developer's own
# machine all install through this script, so a marker in the source means the
# same thing wherever coverage is measured.  The version is pinned in
# ci/lcov-version - see that file before changing it.
#
# Built from source rather than taken from the distribution because Ubuntu
# packages lcov 2.0, which has no notion of LCOV_UNREACHABLE_START and reads it
# as an ordinary comment.  The lines the marker covers then count as unreached,
# and the floors fail with nothing saying why.
#
# Usage: ci/install-lcov.sh [install-dir] [version]
#   install-dir  where to install (default: $HOME/lcov)
#   version      git tag to build (default: the pinned ci/lcov-version)
#
# Progress goes to stderr and lcov's bin directory to stdout, so the caller can
# do:
#
#   export PATH="$(ci/install-lcov.sh):$PATH"
#
# Re-running with the version already present is a no-op.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

INSTALL_DIR="${1:-$HOME/lcov}"
VERSION="${2:-$(tr -d '[:space:]' < "${SCRIPT_DIR}/lcov-version")}"

# A directory per version, so a version change installs beside the old one
# rather than into it, and nothing has to be cleaned out by hand first.
TARGET="${INSTALL_DIR}/lcov-${VERSION}"
LINK="${INSTALL_DIR}/current"

# lcov writes its install prefix into the scripts it installs, so this has to
# be the final path rather than a temporary one moved into place afterwards.
# The stamp is what says an install finished: it is written last, and an
# install that stopped part-way leaves the directory without one and is redone
# rather than trusted.
STAMP="${TARGET}/.installed"

if [ "$(cat "${STAMP}" 2>/dev/null)" != "${VERSION}" ]; then
    echo "Installing lcov ${VERSION} into ${TARGET}..." >&2

    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp}"' EXIT

    git clone --depth 1 --branch "${VERSION}" \
        https://github.com/linux-test-project/lcov.git "${tmp}/lcov" >&2

    rm -rf "${TARGET}"
    mkdir -p "${TARGET}"
    make -C "${tmp}/lcov" install PREFIX="${TARGET}" >&2
else
    echo "lcov ${VERSION} already present in ${TARGET}" >&2
fi

# lcov is perl, and reaches for modules the distribution packages separately.
# A missing one shows up the first time it runs, so run it here rather than
# leaving it to the middle of a coverage campaign.
if ! "${TARGET}/bin/lcov" --version >&2; then
    echo >&2
    echo "lcov ${VERSION} installed but does not run.  It needs perl and" >&2
    echo "modules the distribution packages separately - on Ubuntu:" >&2
    echo "  perl libcapture-tiny-perl libdatetime-perl libjson-xs-perl" >&2
    exit 1
fi

echo "${VERSION}" > "${STAMP}"

# A stable path that does not name the version, for a caller that wants to
# point at "the" lcov.
ln -sfn "${TARGET}" "${LINK}"

echo "${LINK}/bin"
