#!/bin/bash

CONT_NAME="onerom-build"
REPO="ghcr.io/piersfinlayson"

get_git_hash() {
  git rev-parse --short HEAD 2>/dev/null || echo "unknown"
}

# The Arm GNU toolchain version the container must build the firmware with.
# ci/arm-toolchain-version is the single source of truth, shared with CI and
# ci/install-arm-toolchain.sh; the Dockerfile deliberately has no default.
get_arm_gcc_version() {
  local version_file
  version_file="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/arm-toolchain-version"

  if [[ ! -f "${version_file}" ]]; then
    echo "Error: ${version_file} not found" >&2
    exit 1
  fi

  tr -d '[:space:]' < "${version_file}"
}

# The Emscripten SDK version the container must build One ROM Lens with.
# ci/emscripten-version is the single source of truth, shared with CI and
# ci/install-emscripten.sh; the Dockerfile deliberately has no default.
get_emscripten_version() {
  local version_file
  version_file="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/emscripten-version"

  if [[ ! -f "${version_file}" ]]; then
    echo "Error: ${version_file} not found" >&2
    exit 1
  fi

  tr -d '[:space:]' < "${version_file}"
}

get_build_date() {
  date -u +%Y-%m-%d
}

validate_version() {
  local version=$1
  local allow_dev=$2
  
  if [[ "$version" == "dev" && "$allow_dev" != "true" ]]; then
    echo "Error: 'dev' version not allowed for release builds"
    exit 1
  fi
  
  if [[ -z "$version" ]]; then
    echo "Error: Version cannot be empty"
    exit 1
  fi
}