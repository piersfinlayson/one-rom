#!/usr/bin/env bash
#
# Test the CLI and Studio Linux packages for one architecture on the oldest
# glibc they support, in an Ubuntu 22.04 container.  The test fails if:
# - the container's glibc differs from ci/linux-min-glibc-version
# - either package depends on a newer libc6 than that
# - either package fails to install
# - `onerom --version` fails
# - Studio, started under a virtual X display, exits before a timeout
#
# Requires Docker, on Linux or macOS, able to run the architecture natively or
# by emulation.
#
# Usage: ci/test-linux-debs.sh <amd64|arm64> <dir>...
#   dir  a directory holding packages, for example rust/cli/dist.  Across all
#        of them there must be exactly one onerom-cli and one onerom-studio
#        package for the architecture.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# The Ubuntu release whose glibc is the minimum.  The test fails if the two
# disagree, so a change to ci/linux-min-glibc-version requires one here too.
IMAGE="ubuntu:22.04"
MIN_GLIBC="$(tr -d '[:space:]' < "${SCRIPT_DIR}/linux-min-glibc-version")"
STUDIO_SECONDS=15

usage() {
    echo "Usage: $0 <amd64|arm64> <dir>..." >&2
    exit 1
}

[ $# -ge 2 ] || usage
ARCH="$1"
shift
case "${ARCH}" in
    amd64|arm64) ;;
    *) usage ;;
esac

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
echo "${PRETTY_NAME}, $(uname -m), glibc ${glibc}"
if [ "${glibc}" != "${MIN_GLIBC}" ]; then
    echo "The container has glibc ${glibc}, not the minimum ${MIN_GLIBC}" >&2
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

# A desktop provides these.  Studio loads them at run time, so its package
# doesn't depend on them.
echo "Installing a virtual X display..."
apt-get -qq update
DEBIAN_FRONTEND=noninteractive apt-get -qq install -y xvfb libgl1 libegl1 \
    libxkbcommon-x11-0 libxcursor1 libxrandr2 libxi6 libx11-xcb1 libvulkan1 \
    mesa-vulkan-drivers > /dev/null

export XDG_RUNTIME_DIR=/tmp/xdg
mkdir -m 700 -p "${XDG_RUNTIME_DIR}"
Xvfb :99 -screen 0 1280x800x24 > /dev/null 2>&1 &
for _ in $(seq 50); do
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
    docker run -i --rm --platform "linux/${ARCH}" \
        -e MIN_GLIBC="${MIN_GLIBC}" -e STUDIO_SECONDS="${STUDIO_SECONDS}" \
        "${IMAGE}" bash -c "${INNER}"

echo "Linux ${ARCH} packages passed"
