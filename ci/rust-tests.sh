#!/usr/bin/env bash
set -e

cd rust
echo "Running tests for Rust crates..."

echo "Testing onerom-app..."
cargo test -p onerom-app
cargo test -p onerom-app -- --ignored

echo "Testing onerom-cli..."
cargo test -p onerom-cli

echo "Testing onerom-config..."
cargo test -p onerom-config

echo "Testing onerom-database..."
cargo test -p onerom-database

echo "Testing onerom-fw..."
cargo test -p onerom-fw

echo "Testing fw-config-gen..."
cargo test -p fw-config-gen

echo "Testing onerom-fw-parser..."
cargo test -p onerom-fw-parser
cargo test -p onerom-fw-parser --no-default-features

echo "Testing onerom-gen..."
cargo test -p onerom-gen

echo "Testing onerom-metadata..."
cargo test -p onerom-protocol

echo "Testing onerom-protocol..."
cargo test -p onerom-protocol

echo "Testing schema-gen..."
cargo test -p schema-gen

# Generated files that are checked in must match a fresh regeneration.  Both
# generators resolve their output path from CARGO_MANIFEST_DIR, so it does not
# matter that we are in rust/ rather than the repo root.
echo "Checking generated files are up to date..."
cargo run -p onerom-gen --bin compat
cargo run -p schema-gen --bin schema-gen

GENERATED_FILES=(
    "docs/COMPATIBILITY.md"
    "onerom-config/schema.json"
)

cd ..
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "Not a git work tree - skipping the generated file check"
elif ! git diff --quiet -- "${GENERATED_FILES[@]}"; then
    echo
    echo "ERROR: checked-in generated files differ from a fresh regeneration:"
    git diff --stat -- "${GENERATED_FILES[@]}"
    echo
    echo "They have been regenerated in your working tree.  Review the diff and"
    echo "commit it - do not hand-edit these files."
    exit 1
fi

