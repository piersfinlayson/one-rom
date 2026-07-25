#!/usr/bin/env bash
set -e

cd rust
echo "Linting Rust crates (rustfmt + clippy)..."

# A representative config/board so the crates that embed the firmware emulator
# (its build.rs requires CONFIG/BOARD) can be built for linting.  The choice is
# arbitrary - clippy checks the code, not this specific configuration.
EMU_CONFIG=onerom-config/test-0.json
EMU_BOARD=fire-24-a

# Build onerom-config first so its generated modules (src/chip/generated.rs,
# src/hw/generated.rs, and the matching mod.rs files - all git-ignored) exist
# and are rustfmt-formatted at generation time, keeping the fmt check below
# honest.
echo "Building onerom-config (generates formatted source)..."
cargo build -p onerom-config

echo "Checking formatting (cargo fmt)..."
cargo fmt --all -- --check

# Host crates: everything that builds for the host toolchain without the
# firmware emulator.  These are linted together in one pass.
echo "Running clippy (host crates)..."
cargo clippy \
    -p onerom-app \
    -p onerom-cli \
    -p onerom-config \
    -p onerom-database \
    -p onerom-fw \
    -p fw-config-gen \
    -p onerom-fw-parser \
    -p onerom-gen \
    -p onerom-metadata \
    -p onerom-protocol \
    -p schema-gen \
    --all-targets -- -D warnings

# onerom-fw-tester embeds the firmware emulator, so it needs CONFIG/BOARD.
echo "Running clippy (onerom-fw-tester)..."
CONFIG="$EMU_CONFIG" BOARD="$EMU_BOARD" \
    cargo clippy -p onerom-fw-tester --all-targets -- -D warnings

# onerom-lab pins its own nightly toolchain (rust-toolchain.toml) and is a
# binary-only crate; lint it from its own directory so that toolchain applies.
echo "Running clippy (onerom-lab)..."
( cd lab && cargo clippy --bins -- -D warnings )

# onerom-fw-emulator and onerom-lens build for wasm (they compile the firmware
# C to wasm via Emscripten), so they are linted against the wasm target.
echo "Running clippy (wasm: onerom-fw-emulator, onerom-lens)..."
CONFIG="$EMU_CONFIG" BOARD="$EMU_BOARD" \
    cargo clippy -p onerom-fw-emulator -p onerom-lens \
    --target wasm32-unknown-emscripten -- -D warnings

echo "Rust lint passed."
