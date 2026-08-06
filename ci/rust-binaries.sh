#!/usr/bin/env bash
#
# Build the user-facing binaries - the One ROM CLI and One ROM Studio - for the
# host.
#
# Regular CI compiled both only incidentally, through clippy, tests and docs,
# and never linked a binary: Studio's installers are built by
# .github/workflows/build-studio.yml (on a Studio change, and for a release),
# and nothing built the CLI at all.  A change that breaks either binary should
# fail CI when it lands, not when a release is cut.
#
# Host only - this is a build check, not the cross-platform packaging that
# build-studio.yml does.  Release profile, because that is what ships.
set -e

cd rust

echo "Building onerom-cli..."
cargo build -p onerom-cli --release

echo "Building onerom-studio..."
cargo build -p onerom-studio --release

echo "Binaries built."
