#!/bin/bash
#
# Build the Raspberry Pi OS test image and push it to ghcr.io.
#
# `ci/test-linux-debs.sh armv6` tests the armhf packages in this image on an
# emulated ARMv6 CPU.  The image is a minimal Raspberry Pi OS Bookworm with the
# packages in ci/linux-test-packages.txt, built from the Raspbian repository.
# That repository keeps only the latest version of each package, so the image
# is built once, pushed, and pinned by tag in ci/raspios-test-image-version.
#
# Requires Docker able to run armhf programs, natively or by emulation.
#
# Usage: ci/docker/build-raspios-test.sh <version> [nopush]
#   version  the image tag, which ci/raspios-test-image-version then pins
#   nopush   build and import the image locally without pushing it
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

IMAGE_NAME="onerom-raspios-test"
SUITE="bookworm"
MIRROR="http://raspbian.raspberrypi.com/raspbian/"
KEY_URL="https://archive.raspbian.org/raspbian.public.key"

usage() {
  echo "Usage: $0 <version> [nopush]" >&2
  exit 1
}

[ $# -ge 1 ] && [ $# -le 2 ] || usage
VERSION="$1"
PUSH=true
if [ $# -eq 2 ]; then
  [ "$2" = "nopush" ] || usage
  PUSH=false
fi
validate_version "$VERSION" "false"

IMAGE="${REPO}/${IMAGE_NAME}:${VERSION}"
PACKAGES="$(grep -v '^#' "${SCRIPT_DIR}/../linux-test-packages.txt" | paste -sd, -)"

echo "Building ${IMAGE} from Raspberry Pi OS ${SUITE}..."

# qemu-user-static is installed only because mmdebstrap fails without it, even
# where the host runs armhf programs already.
docker run --rm --privileged \
  -e SUITE="${SUITE}" -e MIRROR="${MIRROR}" -e KEY_URL="${KEY_URL}" \
  -e PACKAGES="${PACKAGES}" \
  debian:bookworm bash -c '
set -euo pipefail
apt-get -qq update >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get -qq install -y \
  mmdebstrap arch-test qemu-user-static curl gpg ca-certificates >/dev/null 2>&1
curl -fsSL "${KEY_URL}" | gpg --dearmor > /raspbian.gpg
mmdebstrap --quiet --mode=root --variant=minbase --architectures=armhf \
  --keyring=/raspbian.gpg --include="${PACKAGES}" \
  "${SUITE}" - "${MIRROR}"
' | docker import --platform linux/arm/v6 - "${IMAGE}"

docker run --rm --platform linux/arm/v6 "${IMAGE}" \
  sh -c '. /etc/os-release; echo "${PRETTY_NAME}, $(getconf GNU_LIBC_VERSION)"'

if [ "${PUSH}" = false ]; then
  echo "Built ${IMAGE} locally"
  exit 0
fi

# On macOS, unlock the keychain so docker can read the ghcr.io credentials
if [[ "$OSTYPE" == "darwin"* ]]; then
  security unlock-keychain ~/Library/Keychains/login.keychain-db
fi

docker push "${IMAGE}"
echo "Pushed ${IMAGE}.  Set ci/raspios-test-image-version to ${VERSION}."
