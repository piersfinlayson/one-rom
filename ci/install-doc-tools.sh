#!/usr/bin/env bash
#
# Install the documentation toolchain - pandoc and WeasyPrint - used by
# ci/build-docs.sh to render docs/ to PDF.
#
# Usage: ci/install-doc-tools.sh [install-dir]
#   install-dir  where to install (default: $HOME/onerom-doc-tools)
#
# Prints the bin directory on stdout, so CI can put it on PATH the way it does
# for the Arm toolchain:
#
#   echo "$(ci/install-doc-tools.sh)" >> "$GITHUB_PATH"
#
# Both versions are pinned - ci/pandoc-version and ci/weasyprint-version - so a
# PDF is rendered by the same tools wherever it is built.  This matters more
# here than for a compiler: a WeasyPrint release can move page breaks, so an
# unpinned upgrade silently repaginates every document.
#
# pandoc is taken from its own release archives rather than a system package,
# because a distribution's pandoc is whatever version that distribution froze.
# WeasyPrint goes into a virtualenv, so nothing is installed into the system
# Python.
#
# The one thing this script cannot install is WeasyPrint's native dependency,
# Pango, which needs a package manager.  It checks for it and says what to run.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

INSTALL_DIR="${1:-$HOME/onerom-doc-tools}"
PANDOC_VERSION="$(tr -d '[:space:]' < "${SCRIPT_DIR}/pandoc-version")"
WEASYPRINT_VERSION="$(tr -d '[:space:]' < "${SCRIPT_DIR}/weasyprint-version")"

BIN_DIR="${INSTALL_DIR}/bin"
mkdir -p "${BIN_DIR}"

case "$(uname -s)" in
    Linux)  OS=linux ;;
    Darwin) OS=macOS ;;
    *)      echo "error: unsupported OS $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
    x86_64|amd64)  [ "${OS}" = linux ] && ARCH=amd64 || ARCH=x86_64 ;;
    aarch64|arm64) ARCH=arm64 ;;
    *)             echo "error: unsupported arch $(uname -m)" >&2; exit 1 ;;
esac

# ---------------------------------------------------------------- pandoc --

PANDOC_DIR="${INSTALL_DIR}/pandoc-${PANDOC_VERSION}"
if [ ! -x "${PANDOC_DIR}/bin/pandoc" ]; then
    echo "installing pandoc ${PANDOC_VERSION} (${OS}/${ARCH})" >&2
    rm -rf "${PANDOC_DIR}"
    mkdir -p "${PANDOC_DIR}"
    BASE="https://github.com/jgm/pandoc/releases/download/${PANDOC_VERSION}"
    TMP="$(mktemp -d)"
    trap 'rm -rf "${TMP}"' EXIT
    if [ "${OS}" = linux ]; then
        curl -fsSL "${BASE}/pandoc-${PANDOC_VERSION}-linux-${ARCH}.tar.gz" \
            | tar -xz -C "${PANDOC_DIR}" --strip-components=1
    else
        curl -fsSL -o "${TMP}/pandoc.zip" \
            "${BASE}/pandoc-${PANDOC_VERSION}-${ARCH}-macOS.zip"
        unzip -q "${TMP}/pandoc.zip" -d "${TMP}"
        mv "${TMP}"/pandoc-*/* "${PANDOC_DIR}/"
    fi
fi
ln -sf "${PANDOC_DIR}/bin/pandoc" "${BIN_DIR}/pandoc"

# ------------------------------------------------------------ weasyprint --

VENV_DIR="${INSTALL_DIR}/venv"
if [ ! -x "${VENV_DIR}/bin/weasyprint" ] \
   || ! "${VENV_DIR}/bin/weasyprint" --version 2>/dev/null \
        | grep -q "${WEASYPRINT_VERSION}"; then
    echo "installing WeasyPrint ${WEASYPRINT_VERSION}" >&2
    rm -rf "${VENV_DIR}"
    python3 -m venv "${VENV_DIR}"
    "${VENV_DIR}/bin/pip" install --quiet --upgrade pip
    "${VENV_DIR}/bin/pip" install --quiet "weasyprint==${WEASYPRINT_VERSION}"
fi
ln -sf "${VENV_DIR}/bin/weasyprint" "${BIN_DIR}/weasyprint"

# WeasyPrint imports its native dependencies at run time, so a missing Pango
# shows up as a failed render rather than a failed install.  Fail here instead.
if ! "${VENV_DIR}/bin/weasyprint" --version >/dev/null 2>&1; then
    echo "error: WeasyPrint cannot load its native dependencies." >&2
    if [ "${OS}" = linux ]; then
        echo "  run: sudo apt-get install -y libpango-1.0-0 libpangoft2-1.0-0" >&2
    else
        echo "  run: brew install pango libffi" >&2
    fi
    exit 1
fi

echo "${BIN_DIR}"
