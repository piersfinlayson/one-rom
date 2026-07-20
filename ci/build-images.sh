#!/usr/bin/env bash
#
# build-images.sh - Build and stage the One ROM base firmware for one-rom-images
# (images.onerom.org).
#
# From v0.7.0 there is a single base firmware for every Fire board: it is the
# same across RP2350A/RP2350B and across all Fire hardware variants. Ice is no
# longer supported. Because the base firmware is board- and MCU-variant
# agnostic, it is built once and staged in a single location; every Fire board
# in the manifest fragment points at that one firmware.bin via a shared `path`.
# (The board/MCU still matter when generating per-variant metadata and images -
# just not for the base firmware staged here.)
#
# The fragment is written to /tmp/releases.json for manual pasting into
# one-rom-images/releases.json. This script deliberately does not touch the
# `latest` field - update that by hand once the release is ready.
#
set -e

VERSION=$1
DEST_PREFIX=$2
DEST_VERSION=v$VERSION

if [ -z "$VERSION" ] || [ -z "$DEST_PREFIX" ]; then
    echo "Usage: $0 <version> <destination_prefix>"
    echo "  - example: $0 0.7.0 ../one-rom-images"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
FIRMWARE_BIN="${PROJECT_ROOT}/firmware/build/onerom-rp235x.bin"

# Shared staging location. Every Fire board resolves here via its `path`
# override in the manifest fragment below.
SHARED_MODEL="fire"
SHARED_MCU="rp2350"

cd "${PROJECT_ROOT}"

# Build the base firmware once. This is the same `make firmware` output that
# ci/build.sh publishes, so the staged image is identical to the release image.
echo "Building One ROM base firmware v${VERSION}..."
make clean-firmware-build > /dev/null 2>&1 || true
make firmware

if [ ! -f "${FIRMWARE_BIN}" ]; then
    echo "ERROR: expected firmware ${FIRMWARE_BIN} not found"
    exit 1
fi

# Stage it in the single shared destination.
dest_dir="${DEST_PREFIX}/${DEST_VERSION}/${SHARED_MODEL}/${SHARED_MCU}"
mkdir -p "${dest_dir}"
cp "${FIRMWARE_BIN}" "${dest_dir}/firmware.bin"
echo "Staged base firmware at ${dest_dir}/firmware.bin"

# Build the manifest fragment. Every Fire board is listed by name (so clients
# that look up a specific board still find it), but each points at the shared
# ${SHARED_MODEL}/${SHARED_MCU} path via its `path` override. Board list is
# derived from rust/config/json/fire-*.json so it never goes stale.
json_boards=""
for cfg in "${PROJECT_ROOT}/rust/config/json"/fire-*.json; do
    [ -f "$cfg" ] || continue
    board=$(basename "$cfg" .json)
    json_boards+="
                {
                    \"name\": \"$board\",
                    \"path\": \"${SHARED_MODEL}\",
                    \"mcus\": [
                        {\"name\": \"${SHARED_MCU}\"}
                    ]
                },"
done
json_boards="${json_boards%,}"

cat > "/tmp/releases.json" <<EOF
        {
            "version": "$VERSION",
            "path": "$DEST_VERSION",
            "boards": [$json_boards
            ]
        }
EOF

echo "JSON manifest fragment written to /tmp/releases.json"
echo "- paste it into one-rom-images/releases.json (and update 'latest' by hand when ready)"
