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
# firmware emulator.  These are linted together in one pass.  onerom-studio is
# among them, and needs libudev/libusb present; its own workflow
# (.github/workflows/build-studio.yml) only fires on rust/studio/** changes, so
# linting it here is what catches a workspace-wide change that breaks it.
echo "Running clippy (host crates)..."
cargo clippy \
    -p onerom-app \
    -p onerom-cli \
    -p onerom-config \
    -p onerom-database \
    -p onerom-fw \
    -p fw-config-gen \
    -p onerom-fw-driver \
    -p onerom-fw-geometry \
    -p onerom-fw-parser \
    -p onerom-gen \
    -p onerom-metadata \
    -p onerom-protocol \
    -p onerom-studio \
    -p schema-gen \
    --all-targets -- -D warnings

# onerom-fw-tester embeds the firmware emulator, so it needs CONFIG/BOARD, and
# onerom-plugin-tester links against onerom-fw-tester's library.
echo "Running clippy (onerom-fw-tester, onerom-plugin-tester)..."
CONFIG="$EMU_CONFIG" BOARD="$EMU_BOARD" \
    cargo clippy -p onerom-fw-tester -p onerom-plugin-tester --all-targets -- -D warnings

# onerom-lab pins its own nightly toolchain (rust-toolchain.toml) and is a
# binary-only crate that builds for the RP2350 (thumbv8m via its
# .cargo/config.toml).  Lint it from its own directory so that toolchain and
# target apply; add the target first (for the nightly toolchain), mirroring
# lab's build-all.sh.
#
# --no-deps confines this pass to lab's own code.  Without it, clippy also lints
# the workspace crates lab depends on - onerom-config, onerom-protocol and
# onerom-database - under nightly's lint set rather than stable's, so a lint
# promoted to warn-by-default in nightly fails the build over a file the host
# pass above already covers on stable.  That is not hypothetical: it happened to
# `matches![...]` in onerom-config's build script, which is nursery (allow) on
# stable and warn-by-default on nightly.  All three crates are linted in the host
# pass, so nothing loses coverage here.
echo "Running clippy (onerom-lab)..."
( cd lab \
    && rustup target add thumbv8m.main-none-eabihf \
    && cargo clippy --no-deps --bins -- -D warnings )

# onerom-fw-emulator and onerom-lens build for wasm (they compile the firmware
# C to wasm via Emscripten), so they are linted against the wasm target.
echo "Running clippy (wasm: onerom-fw-emulator, onerom-lens)..."
CONFIG="$EMU_CONFIG" BOARD="$EMU_BOARD" \
    cargo clippy -p onerom-fw-emulator -p onerom-lens \
    --target wasm32-unknown-emscripten -- -D warnings

echo "Rust lint passed."
