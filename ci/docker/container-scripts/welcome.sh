#!/bin/bash
CONTAINER_VERSION="${VERSION:-unknown}"
. /etc/os-release
echo "======================="
echo "One ROM Build Container"
echo "======================="
echo ""
echo "Built:    ${BUILD_DATE:-unknown}"
echo "Version:  ${CONTAINER_VERSION}"
echo "Git Hash: ${VCS_REF:-unknown}"
echo ""
echo "Dist:     ${PRETTY_NAME}"
echo "ARM GCC:  $(/opt/arm-toolchain/bin/arm-none-eabi-gcc --version 2>/dev/null | head -n1 || echo 'not found')"
echo "Rust:     $(rustc --version 2>/dev/null || echo 'not found')"
echo "Cargo:    $(cargo --version 2>/dev/null || echo 'not found')"
echo "probe-rs: $(probe-rs --version 2>/dev/null || echo 'not found')"
echo "picotool: $(picotool version 2>/dev/null || echo 'not found')"
echo ""

cat << 'EOF'
A One ROM build environment - build the firmware, build the tooling, and run
the tooling.

To get started:
  ./clone.sh && cd one-rom

Build the base firmware (a single image for all Fire boards):
  scripts/build-empty-fw.sh

Copy the built firmware to the output directory:
  ../copy-fw.sh

Build the tooling (e.g. the CLI):
  cd rust/cli && cargo build --release

Run the tooling - build a configured firmware for a board and ROM config:
  onerom firmware build --config-file onerom-config/vic20-pal.json \
      --board fire-24-e --out firmware.bin
EOF

echo ""
echo "======================="