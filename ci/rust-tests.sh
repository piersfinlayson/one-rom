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

echo "Testing onerom-fw-driver..."
cargo test -p onerom-fw-driver

echo "Testing onerom-fw-geometry..."
cargo test -p onerom-fw-geometry

echo "Testing onerom-fw-parser..."
cargo test -p onerom-fw-parser
cargo test -p onerom-fw-parser --no-default-features

echo "Testing onerom-gen..."
cargo test -p onerom-gen

echo "Testing onerom-metadata..."
cargo test -p onerom-metadata

echo "Testing onerom-protocol..."
cargo test -p onerom-protocol

echo "Testing schema-gen..."
cargo test -p schema-gen

echo "Testing doc-gen..."
cargo test -p doc-gen

# Values a document states and something else owns - a hold limit from the
# metadata schema, the CLI's own version - are marked in the markdown and
# checked here.  Nothing is rewritten: a stale value is an edit for whoever
# moved the constant, not something a tool silently fixes in their prose.
echo "Checking documentation values against their sources..."
cargo run -q -p doc-gen

# Generated files that are checked in must match a fresh regeneration.  Every
# generator resolves its output path from CARGO_MANIFEST_DIR, so it does not
# matter that we are in rust/ rather than the repo root.
echo "Checking generated files are up to date..."
cargo run -p onerom-gen --bin compat
cargo run -p schema-gen --bin schema-gen
cargo run -q -p onerom-gen --bin layout -- --write-baseline

# docs/CHIP-TYPES.md needs no command of its own - the onerom-config build
# script rewrites it, and the runs above build that crate.  It is checked here
# because otherwise a chip-types.json change can be committed without the
# regenerated doc, and nothing notices.
GENERATED_FILES=(
    "docs/COMPATIBILITY.md"
    "docs/CHIP-TYPES.md"
    "onerom-config/schema.json"
    "ci/layout-baseline.txt"
)

# A markdown file in docs/ may carry a fragment region, whose text belongs to
# another file and is written into the committed one so a reader on GitHub sees
# a whole document.  The assembler fills every region in, and names each host on
# stdout - so a region left stale by an edit to the fragment fails the same
# check as any other generated file.  The paths are repository-relative, as the
# git command below wants, and the run is from rust/ like everything above it.
# Assigned first rather than piped, so a failure in the assembler - an unclosed
# region, a fragment that is not there - stops the script under set -e instead
# of feeding an empty list to a loop that then reports nothing wrong.
echo "Filling in documentation fragment regions..."
FRAGMENT_HOSTS=$(cargo run -q -p doc-gen --bin doc-assemble -- --fragments docs INSTALL.md README.md)
while IFS= read -r host; do
    if [ -n "${host}" ]; then
        GENERATED_FILES+=("${host}")
    fi
done <<< "${FRAGMENT_HOSTS}"

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
    echo
    echo "A docs/*.md file here is one carrying a fragment region, and only the"
    echo "text between its markers was rewritten.  Edit the file the marker"
    echo "names, not the region."
    echo
    echo "For ci/layout-baseline.txt, which records how much flash each chip type"
    echo "costs on each board, run this to see whether a change is an improvement"
    echo "or a regression before committing it:"
    echo "  cargo run -p onerom-gen --bin layout -- --check"
    exit 1
fi

