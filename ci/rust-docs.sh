#!/usr/bin/env bash
set -e

cd rust
echo "Generating documentation for Rust crates..."

# A representative config/board for the crates that embed the firmware emulator
# (its build.rs requires CONFIG/BOARD), mirroring ci/rust-lint.sh.  The choice is
# arbitrary - rustdoc checks the documentation, not this configuration.
EMU_CONFIG=onerom-config/test-0.json
EMU_BOARD=fire-24-a

echo "Generating documentation for onerom-app..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-app --no-deps

echo "Generating documentation for onerom-cli..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-cli --no-deps

echo "Generating documentation for onerom-config..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-config --no-deps

echo "Generating documentation for onerom-database..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-database --no-deps

echo "Generating documentation for onerom-fw..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-fw --no-deps

echo "Generating documentation for fw-config-gen..."
RUSTDOCFLAGS="-D warnings" cargo doc -p fw-config-gen --no-deps

echo "Generating documentation for onerom-fw-driver..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-fw-driver --no-deps

echo "Generating documentation for onerom-fw-geometry..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-fw-geometry --no-deps

echo "Generating documentation for onerom-fw-parser..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-fw-parser --no-deps

echo "Generating documentation for onerom-gen..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-gen --no-deps

echo "Generating documentation for onerom-metadata..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-metadata --no-deps

echo "Generating documentation for onerom-protocol..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-protocol --no-deps

echo "Generating documentation for schema-gen..."
RUSTDOCFLAGS="-D warnings" cargo doc -p schema-gen --no-deps

# These embed the firmware emulator, so they need CONFIG/BOARD and build the
# firmware C.  They are documented last: they are the slow ones, and until now
# they were not documented at all, which is how broken doc links accumulated in
# them unnoticed.
echo "Generating documentation for onerom-fw-emulator..."
CONFIG="$EMU_CONFIG" BOARD="$EMU_BOARD" \
    RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-fw-emulator --no-deps

echo "Generating documentation for onerom-fw-tester..."
CONFIG="$EMU_CONFIG" BOARD="$EMU_BOARD" \
    RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-fw-tester --no-deps

echo "Generating documentation for onerom-plugin-tester..."
CONFIG="$EMU_CONFIG" BOARD="$EMU_BOARD" \
    RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-plugin-tester --no-deps
