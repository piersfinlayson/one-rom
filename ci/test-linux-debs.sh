#!/usr/bin/env bash
#
# Test the CLI and Studio Linux packages for one platform in a container.  The
# test fails if:
# - the container's glibc is older than ci/linux-min-glibc-version, or differs
#   from it on Ubuntu 22.04
# - either package depends on a newer libc6 than that
# - either package fails to install
# - `onerom --version` fails
# - Studio, started under a virtual X display, exits before a timeout
#
# Platforms:
#   amd64, arm64, armhf  Ubuntu 22.04, whose glibc is the minimum
#   armv6                the armhf packages in Raspberry Pi OS, the image pinned
#                        in ci/raspios-test-image-version, on an emulated ARMv6
#                        CPU (Raspberry Pi 1 and Zero)
#
# Requires Docker, on Linux or macOS, able to run the platform natively or by
# emulation.
#
# Usage: ci/test-linux-debs.sh <amd64|arm64|armhf|armv6> <dir>...
#   dir  a directory holding packages, for example rust/cli/dist.  Across all
#        of them there must be exactly one onerom-cli and one onerom-studio
#        package for the platform's architecture.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

MIN_GLIBC="$(tr -d '[:space:]' < "${SCRIPT_DIR}/linux-min-glibc-version")"

usage() {
    echo "Usage: $0 <amd64|arm64|armhf|armv6> <dir>..." >&2
    exit 1
}

[ $# -ge 2 ] || usage
PLATFORM="$1"
shift

# The Ubuntu release whose glibc is the minimum.  The test fails if the two
# disagree, so a change to ci/linux-min-glibc-version requires one here too.
UBUNTU_IMAGE="ubuntu:22.04"

QEMU_CPU=""
case "${PLATFORM}" in
    amd64|arm64)
        ARCH="${PLATFORM}"
        DOCKER_PLATFORM="linux/${PLATFORM}"
        IMAGE="${UBUNTU_IMAGE}"
        EXACT_GLIBC=1
        INSTALL_PACKAGES=1
        STUDIO_SECONDS=15
        ;;
    armhf)
        # Studio starts more slowly under emulation, and the 32-bit ARM
        # platforms run emulated on GitHub's runners.
        ARCH=armhf
        DOCKER_PLATFORM="linux/arm/v7"
        IMAGE="${UBUNTU_IMAGE}"
        EXACT_GLIBC=1
        INSTALL_PACKAGES=1
        STUDIO_SECONDS=30
        ;;
    armv6)
        # Raspberry Pi OS is built for ARMv6, unlike Ubuntu's ARMv7 armhf, so
        # the whole container runs on the emulated ARMv6 CPU.  Its glibc is
        # newer than the minimum and the test packages are already installed.
        ARCH=armhf
        DOCKER_PLATFORM="linux/arm/v6"
        IMAGE="ghcr.io/piersfinlayson/onerom-raspios-test:$(tr -d '[:space:]' < "${SCRIPT_DIR}/raspios-test-image-version")"
        EXACT_GLIBC=0
        INSTALL_PACKAGES=0
        STUDIO_SECONDS=30
        QEMU_CPU=arm1176
        ;;
    *)
        usage
        ;;
esac

TEST_PACKAGES="$(grep -v '^#' "${SCRIPT_DIR}/linux-test-packages.txt" | xargs)"

# Find exactly one package of each, so a stale package left in a directory
# fails rather than being tested in place of the new one.
find_one() {
    local found=()
    for dir in "$@"; do
        for f in "${dir}"/"${PACKAGE}"_*_"${ARCH}".deb; do
            # Absolute, since tar resolves each -C below from the one before
            [ -e "$f" ] && found+=("$(cd "$(dirname "$f")" && pwd)/$(basename "$f")")
        done
    done
    if [ "${#found[@]}" -ne 1 ]; then
        echo "Expected one ${PACKAGE} ${ARCH} package, found ${#found[@]}: ${found[*]:-}" >&2
        exit 1
    fi
    echo "${found[0]}"
}
CLI_DEB="$(PACKAGE=onerom-cli find_one "$@")"
STUDIO_DEB="$(PACKAGE=onerom-studio find_one "$@")"

INNER=$(cat <<'EOF'
set -euo pipefail
mkdir /debs
tar -xf - -C /debs

. /etc/os-release
glibc="$(getconf GNU_LIBC_VERSION | cut -d' ' -f2)"
echo "${PRETTY_NAME}, $(uname -m), glibc ${glibc}${QEMU_CPU:+, emulated CPU ${QEMU_CPU}}"
if [ "${EXACT_GLIBC}" = 1 ] && [ "${glibc}" != "${MIN_GLIBC}" ]; then
    echo "The container has glibc ${glibc}, not the minimum ${MIN_GLIBC}" >&2
    exit 1
fi
if ! dpkg --compare-versions "${glibc}" ge "${MIN_GLIBC}"; then
    echo "The container has glibc ${glibc}, older than the minimum ${MIN_GLIBC}" >&2
    exit 1
fi

for deb in /debs/*.deb; do
    depends="$(dpkg-deb -f "${deb}" Depends)"
    echo "$(basename "${deb}"): Depends: ${depends}"
    libc6="$(echo "${depends}" | sed -n 's/.*libc6 (>= \([0-9.]*\)).*/\1/p')"
    if [ -z "${libc6}" ]; then
        echo "$(basename "${deb}") has no libc6 dependency" >&2
        exit 1
    fi
    if ! dpkg --compare-versions "${libc6}" le "${MIN_GLIBC}"; then
        echo "$(basename "${deb}") requires libc6 ${libc6}, above the minimum ${MIN_GLIBC}" >&2
        exit 1
    fi
done

dpkg -i /debs/*.deb
onerom --version

if [ "${INSTALL_PACKAGES}" = 1 ]; then
    echo "Installing a virtual X display..."
    apt-get -qq update
    # shellcheck disable=SC2086
    DEBIAN_FRONTEND=noninteractive apt-get -qq install -y ${TEST_PACKAGES} > /dev/null
fi

export XDG_RUNTIME_DIR=/tmp/xdg
mkdir -m 700 -p "${XDG_RUNTIME_DIR}"
Xvfb :99 -screen 0 1280x800x24 > /dev/null 2>&1 &
for _ in $(seq 100); do
    [ -e /tmp/.X11-unix/X99 ] && break
    sleep 0.1
done

status=0
DISPLAY=:99 timeout "${STUDIO_SECONDS}" onerom-studio > /tmp/studio.log 2>&1 || status=$?
if [ "${status}" -ne 124 ]; then
    echo "Studio exited with status ${status} within ${STUDIO_SECONDS}s:" >&2
    cat /tmp/studio.log >&2
    exit 1
fi
echo "Studio was still running after ${STUDIO_SECONDS}s"
EOF
)

echo "Testing $(basename "${CLI_DEB}") and $(basename "${STUDIO_DEB}") in ${IMAGE}..."

# The packages go in through stdin rather than a bind mount, which Docker on
# macOS only provides for shared directories.
COPYFILE_DISABLE=1 tar --no-xattrs -cf - \
    -C "$(dirname "${CLI_DEB}")" "$(basename "${CLI_DEB}")" \
    -C "$(dirname "${STUDIO_DEB}")" "$(basename "${STUDIO_DEB}")" |
    docker run -i --rm --platform "${DOCKER_PLATFORM}" \
        -e MIN_GLIBC="${MIN_GLIBC}" -e EXACT_GLIBC="${EXACT_GLIBC}" \
        -e INSTALL_PACKAGES="${INSTALL_PACKAGES}" \
        -e TEST_PACKAGES="${TEST_PACKAGES}" \
        -e STUDIO_SECONDS="${STUDIO_SECONDS}" \
        ${QEMU_CPU:+-e QEMU_CPU="${QEMU_CPU}"} \
        "${IMAGE}" bash -c "${INNER}"

echo "Linux ${PLATFORM} packages passed"
