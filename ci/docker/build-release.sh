#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

usage() {
  echo "Usage: $0 <version>"
  echo ""
  echo "Build multi-arch One ROM build container and push to ghcr.io."
  echo ""
  echo "Arguments:"
  echo "  version   Version tag for the build container (required, cannot be 'dev') e.g. 0.6.2"
  exit 1
}

if [[ "$1" == "-h" || "$1" == "--help" || -z "$1" ]]; then
  usage
fi

VERSION="$1"
GIT_HASH=$(get_git_hash)
BUILD_DATE=$(get_build_date)

validate_version "$VERSION" "false"

# Ensure buildx builder exists
if ! docker buildx inspect onerom-multiarch &>/dev/null; then
  echo "Creating buildx builder 'onerom-multiarch'..."
  docker buildx create --name onerom-multiarch --use
else
  docker buildx use onerom-multiarch
fi

# On macOS, unlock keychain to allow access to stored credentials, to allow
# pushing to ghcr
if [[ "$OSTYPE" == "darwin"* ]]; then
    echo "Unlocking keychain for credential access..."
    security unlock-keychain ~/Library/Keychains/login.keychain-db

    echo "Verifying ghcr.io credentials..."
    echo "ghcr.io" | docker-credential-osxkeychain get > /dev/null || {
        echo "Error: Cannot retrieve ghcr.io credentials from keychain"
        exit 1
    }
fi

docker buildx build \
  --platform linux/amd64,linux/arm64 \
  --build-arg BUILD_DATE="${BUILD_DATE}" \
  --build-arg VERSION="${VERSION}" \
  --build-arg VCS_REF="${GIT_HASH}" \
  --build-arg ARM_GCC_VERSION="$(get_arm_gcc_version)" \
  --build-arg EMSCRIPTEN_VERSION="$(get_emscripten_version)" \
  -t ${REPO}/${CONT_NAME}:${VERSION} \
  -t ${REPO}/${CONT_NAME}:latest \
  --push \
  "${SCRIPT_DIR}"

echo "Built and pushed ${REPO}/${CONT_NAME}:${VERSION} and ${REPO}/${CONT_NAME}:latest"