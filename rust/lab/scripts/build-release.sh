#!/usr/bin/env bash
#
# Build One ROM Lab's release images and check each one.
#
# Usage: rust/lab/scripts/build-release.sh --version X.Y.Z
#
# The version must be Lab's own, from rust/lab/Cargo.toml.  The release workflow
# passes the version from its lab-vX.Y.Z tag, so a tag that doesn't match Lab
# fails here before anything is built.
#
# Writes these to rust/lab/dist, which it empties first:
#   onerom-lab-<board>-X.Y.Z.bin    one per Fire board, with that board built in
#   onerom-lab-no-board-X.Y.Z.bin   without a board built in
#   onerom-lab-elf-X.Y.Z.zip        every image's ELF
#
# Lab builds with the nightly pinned in rust/lab/rust-toolchain.toml.  Each .bin
# is a flat image for the start of flash, made from its ELF by objcopy from the
# Arm toolchain pinned in ci/arm-toolchain-version.  TOOLCHAIN is that
# toolchain's bin directory, as for firmware/Makefile.
#
# Each .bin is checked with this tree's `onerom firmware inspect` for Lab's
# version and the board built in.  The CLI looks for One ROMs at startup, so it
# is passed a VID:PID nothing uses and opens none of this machine's devices.
set -euo pipefail

usage() {
    echo "Usage: $0 --version X.Y.Z" >&2
    exit 1
}

error() {
    echo "ERROR: $1" >&2
    exit 1
}

VERSION=""
while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            [ $# -ge 2 ] || usage
            VERSION="$2"
            shift 2
            ;;
        *)
            usage
            ;;
    esac
done
[ -n "$VERSION" ] || usage

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LAB_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
ROOT="$(cd "${LAB_DIR}/../.." && pwd)"
DIST="${LAB_DIR}/dist"

TOOLCHAIN="${TOOLCHAIN:-/opt/arm-toolchain/bin}"
OBJCOPY="${TOOLCHAIN}/arm-none-eabi-objcopy"
ARM_VERSION="$(tr -d '[:space:]' < "${ROOT}/ci/arm-toolchain-version")"

# Lab reads CS1 to CS3 at build time and a release image leaves them unset.
# BOARD is set per image below.
unset CS1 CS2 CS3

# RUSTUP_TOOLCHAIN would override rust-toolchain.toml.
unset RUSTUP_TOOLCHAIN

command -v python3 >/dev/null || error "python3 is required"
command -v zip >/dev/null || error "zip is required"

[ -x "${OBJCOPY}" ] || error "${OBJCOPY} not found.  Set TOOLCHAIN to the bin directory ci/install-arm-toolchain.sh prints."
grep -qiF "Arm GNU Toolchain ${ARM_VERSION} " <<< "$("${OBJCOPY}" --version)" ||
    error "${OBJCOPY} isn't from the pinned Arm GNU toolchain ${ARM_VERSION}"

cd "${LAB_DIR}"
rustup target add thumbv8m.main-none-eabihf
echo "Lab's Rust: $(rustc --version)"
echo "objcopy:    $("${OBJCOPY}" --version | sed -n 1p)"

metadata="$(cargo metadata --no-deps --format-version 1)"
lab_version="$(python3 -c '
import json, sys
m = json.load(sys.stdin)
print(next(p["version"] for p in m["packages"] if p["name"] == "onerom-lab"))
' <<< "${metadata}")"
target_dir="$(python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])' <<< "${metadata}")"

[ "${VERSION}" = "${lab_version}" ] ||
    error "--version ${VERSION} isn't Lab's version, ${lab_version}, from rust/lab/Cargo.toml"

ELF="${target_dir}/thumbv8m.main-none-eabihf/release/onerom-lab-fire"
ONEROM="${target_dir}/debug/onerom"

# From rust/, so the workspace's toolchain and host target build the CLI rather
# than Lab's.
echo "Building this tree's onerom CLI..."
( cd "${ROOT}/rust" && cargo build -p onerom-cli )

# The value of one `Name: value` line of `onerom firmware inspect` output.
field() {
    awk -v name="$1" -F': +' '$1 == name { print $2 }' <<< "$2"
}

# check_image <bin> <board, or "(not set)">
check_image() {
    local bin="$1" board="$2" out err
    err="$(mktemp)"
    out="$("${ONEROM}" firmware inspect --firmware "${bin}" --vid-pid 0000:0000 2> "${err}")"
    if [ -s "${err}" ]; then
        cat "${err}" >&2
        rm -f "${err}"
        error "onerom firmware inspect warned about $(basename "${bin}")"
    fi
    rm -f "${err}"

    if [ "$(field Firmware "${out}")" != "One ROM Lab" ] ||
        [ "$(field Version "${out}")" != "${VERSION}" ] ||
        [ "$(field Board "${out}")" != "${board}" ]; then
        echo "${out}" >&2
        error "$(basename "${bin}") isn't One ROM Lab ${VERSION} for ${board}"
    fi
    echo "Checked $(basename "${bin}"): One ROM Lab ${VERSION}, board ${board}"
}

# build_image <image name> <BOARD value, empty for none>
#
# Every image builds to the same ELF, so each is copied away before the next
# build.
build_image() {
    local name="$1" board="$2"
    local stem="onerom-lab-${name}-${VERSION}"
    local expected="${board:-"(not set)"}"

    echo "Building ${stem}..."
    BOARD="${board}" cargo build --release
    cp "${ELF}" "${DIST}/elf/${stem}.elf"
    "${OBJCOPY}" -O binary --gap-fill=0xff "${DIST}/elf/${stem}.elf" "${DIST}/${stem}.bin"
    check_image "${DIST}/${stem}.bin" "${expected}"
}

rm -rf "${DIST}"
mkdir -p "${DIST}/elf"

build_image no-board ""
for cfg in "${ROOT}"/rust/config/json/fire-*.json; do
    board="$(basename "${cfg}" .json)"
    build_image "${board}" "${board}"
done

zip -q -X -9 -j "${DIST}/onerom-lab-elf-${VERSION}.zip" "${DIST}"/elf/*.elf
rm -rf "${DIST}/elf"

echo "Built in ${DIST}:"
ls -l "${DIST}"
