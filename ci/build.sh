#!/usr/bin/env bash

#
# build.sh - Build and release script for One ROM project
#
# Usage:
#   ci/build.sh ci              - Build firmware
#   ci/build.sh release v1.2.3  - Package CI build for release
#   ci/build.sh clean           - Delete builds/ directory
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BUILD_DIR="${PROJECT_ROOT}/firmware/build"
FIRMWARE_BIN="onerom-rp235x.bin"

#
# Display usage information and exit
#
usage() {
    echo "Usage: $0 <command> [args]"
    echo "Commands:"
    echo "  ci                - Build firmware"
    echo "  release <version> - Package CI build for release (e.g. v1.2.3)"
    echo "  clean             - Delete builds/ directory"
    exit 1
}

#
# Remove the entire builds/ directory
#
clean_builds() {
    echo "Cleaning builds directory..."
    rm -rf "${PROJECT_ROOT}/builds"
    echo "Done."
}

#
# Build firmware with retry
# Returns: 0 on success, 1 on failure
#
build_firmware() {
    make clean-firmware-build > /dev/null 2>&1 || true

    local attempt=1
    local max_attempts=2

    while [[ $attempt -le $max_attempts ]]; do
        echo "  - Attempt ${attempt}: make firmware"
        if make firmware > /dev/null; then
            break
        fi
        attempt=$((attempt + 1))
        if [[ $attempt -gt $max_attempts ]]; then
            echo "ERROR: Build failed after ${max_attempts} attempts"
            return 1
        fi
    done

    if [[ ! -f "${BUILD_DIR}/${FIRMWARE_BIN}" ]]; then
        echo "ERROR: Expected output ${FIRMWARE_BIN} not found in ${BUILD_DIR}"
        return 1
    fi

    return 0
}

#
# Main
#
main() {
    [[ $# -lt 1 ]] && usage

    case "$1" in
        clean)
            clean_builds
            ;;

        ci)
            cd "${PROJECT_ROOT}"
            echo "Performing initial clean..."
            make clean > /dev/null 2>&1 || true

            local ci_dir="${PROJECT_ROOT}/builds/ci"
            mkdir -p "$ci_dir"

            echo "Building firmware..."
            build_firmware

            cp "${BUILD_DIR}/${FIRMWARE_BIN}" "$ci_dir/"
            echo "CI build complete: ${ci_dir}/${FIRMWARE_BIN}"
            ;;

        release)
            [[ $# -ne 2 ]] && usage
            local version="$2"
            local ci_dir="${PROJECT_ROOT}/builds/ci"
            local release_dir="${PROJECT_ROOT}/builds/${version}"
            local firmware_dir="${release_dir}/firmware"

            if [[ ! -f "${ci_dir}/${FIRMWARE_BIN}" ]]; then
                echo "ERROR: No CI build found at ${ci_dir}. Run 'ci/build.sh ci' first."
                exit 1
            fi

            rm -rf "$release_dir"
            mkdir -p "$firmware_dir"

            local bin_name="onerom-${version}.bin"
            cp "${ci_dir}/${FIRMWARE_BIN}" "${firmware_dir}/${bin_name}"

            cd "$firmware_dir"
            zip "onerom-${version}.zip" "${bin_name}" > /dev/null
            cd "${PROJECT_ROOT}"

            echo "Release ${version} complete: ${firmware_dir}"
            ;;

        *)
            usage
            ;;
    esac
}

main "$@"