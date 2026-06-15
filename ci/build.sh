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
# Get display name for a hardware revision
# Args: hw_rev
#
get_hw_display() {
    local hw_rev="$1"
    case "$hw_rev" in
        fire-24-a)     echo "Fire 24 A" ;;
        fire-24-usb-b) echo "Fire 24 B" ;;
        fire-24-c)     echo "Fire 24 C" ;;
        fire-24-d)     echo "Fire 24 D" ;;
        fire-24-e)     echo "Fire 24 E" ;;
        fire-24-f)     echo "Fire 24 F" ;;
        fire-28-a)     echo "Fire 28 A" ;;
        fire-28-b)     echo "Fire 28 B" ;;
        fire-28-c)     echo "Fire 28 C" ;;
        fire-32-a)     echo "Fire 32 A" ;;
        fire-32-b)     echo "Fire 32 B" ;;
        fire-40-a)     echo "Fire 40 A" ;;
        fire-40-b)     echo "Fire 40 B" ;;
        *)             echo "$hw_rev" ;;
    esac
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
# Generate manifest JSON for release
# Args: version, firmware_dir
#
generate_manifest() {
    local version="$1"
    local firmware_dir="$2"
    local manifest_file="${firmware_dir}/manifest.json"

    # Determine GitHub repository
    local github_repo="${GITHUB_REPOSITORY:-}"
    if [[ -z "$github_repo" ]]; then
        local git_remote
        git_remote=$(git remote get-url origin 2>/dev/null || echo "")
        if [[ "$git_remote" =~ github.com[:/]([^/]+/[^/.]+) ]]; then
            github_repo="${BASH_REMATCH[1]%.git}"
        fi
    fi

    if [[ -z "$github_repo" ]]; then
        echo "ERROR: Could not determine GitHub repository. Set GITHUB_REPOSITORY or configure git remote."
        exit 1
    fi

    # Build hardware section from rust/config/json/*.json
    local hardware_json="{"
    local first=true

    for hw_config_file in "${PROJECT_ROOT}/rust/config/json"/*.json; do
        [[ -f "$hw_config_file" ]] || continue
        local hw_rev
        hw_rev=$(basename "$hw_config_file" .json)

        # Only include fire hardware entries
        [[ "$hw_rev" != fire-* ]] && continue

        local description
        description=$(jq -r '.description // ""' "$hw_config_file")
        local usb_support
        usb_support=$(jq -r '.mcu.usb.present // false' "$hw_config_file")
        local display
        display=$(get_hw_display "$hw_rev")

        [[ "$first" == true ]] && first=false || hardware_json+=","
        hardware_json+="\"${hw_rev}\":{\"display\":\"${display}\",\"description\":\"${description}\",\"usb_support\":${usb_support}}"
    done
    hardware_json+="}"

    local models_json='{"fire":{"display":"Fire"}}'

    local manifest
    manifest=$(echo "{\"version\":\"${version}\",\"hardware\":${hardware_json},\"models\":${models_json},\"artifacts\":[]}" | jq '.')
    echo "$manifest" > "$manifest_file"

    echo "Generated manifest: ${manifest_file}"
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

            generate_manifest "$version" "$firmware_dir"
            echo "Release ${version} complete: ${firmware_dir}"
            ;;

        *)
            usage
            ;;
    esac
}

main "$@"