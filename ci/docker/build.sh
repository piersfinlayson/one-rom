#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

usage() {
  echo "Usage: $0 [version]"
  echo ""
  echo "Build the One ROM build container locally for host architecture."
  echo ""
  echo "Arguments:"
  echo "  version   Version tag for the build container (default: dev)"
  exit 1
}

if [[ "$1" == "-h" || "$1" == "--help" ]]; then
  usage
fi

VERSION="${1:-dev}"
GIT_HASH=$(get_git_hash)
BUILD_DATE=$(get_build_date)

validate_version "$VERSION" "true"

docker build \
  --build-arg BUILD_DATE="${BUILD_DATE}" \
  --build-arg VERSION="${VERSION}" \
  --build-arg VCS_REF="${GIT_HASH}" \
  --build-arg ARM_GCC_VERSION="$(get_arm_gcc_version)" \
  --build-arg EMSCRIPTEN_VERSION="$(get_emscripten_version)" \
  -t ${CONT_NAME} \
  "${SCRIPT_DIR}"

echo "Built ${CONT_NAME} version ${VERSION}: ${CONT_NAME}:latest"