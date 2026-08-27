#!/usr/bin/env bash
set -e

# The generated-file check below compares each file against a snapshot taken
# here, before any generator has run, rather than against git.
#
# Against git it asked the wrong question.  A file like docs/CLI-MANUAL.md is
# only partly generated - the assembler rewrites the text between its fragment
# markers and leaves the rest of the document alone - so diffing the whole file
# against the index reported a developer's own hand-written prose as a stale
# generated file, and passed only once that prose was staged.  What matters is
# whether the generator produced something different from what is committed,
# which a snapshot answers directly, on a dirty working tree or a clean one, and
# without needing a git work tree at all.
#
# docs/CHIP-TYPES.md is why the snapshot is taken this early: the onerom-config
# build script rewrites it on any build, so by the first cargo command below it
# has already been regenerated and a later snapshot would compare it to itself.
SNAPSHOT_DIR=$(mktemp -d)
trap 'rm -rf "${SNAPSHOT_DIR}"' EXIT

# Every file the check may look at.  The fixed generated files, plus every
# candidate fragment host - the assembler is handed these paths to search and
# names the ones it filled in, which is not known until it has run, so all of
# them are snapshotted and only the named ones are compared.
SNAPSHOT_CANDIDATES=(
    "docs/COMPATIBILITY.md"
    "docs/CHIP-TYPES.md"
    "onerom-config/schema.json"
    "ci/layout-baseline.txt"
    "INSTALL.md"
    "README.md"
)
while IFS= read -r doc; do
    SNAPSHOT_CANDIDATES+=("${doc}")
done < <(find docs -name '*.md')

for rel in "${SNAPSHOT_CANDIDATES[@]}"; do
    if [ -f "${rel}" ]; then
        mkdir -p "${SNAPSHOT_DIR}/$(dirname "${rel}")"
        cp "${rel}" "${SNAPSHOT_DIR}/${rel}"
    fi
done

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
# check as any other generated file.  Only the region is rewritten, which is why
# the check compares against a snapshot: the rest of the host is hand-written
# prose that a developer is entitled to have uncommitted.  The paths are
# repository-relative, matching the snapshot keys, and the run is from rust/
# like everything above it.  Assigned first rather than piped, so a failure in
# the assembler - an unclosed region, a fragment that is not there - stops the
# script under set -e instead of feeding an empty list to a loop that then
# reports nothing wrong.
echo "Filling in documentation fragment regions..."
FRAGMENT_HOSTS=$(cargo run -q -p doc-gen --bin doc-assemble -- --fragments docs INSTALL.md README.md)
while IFS= read -r host; do
    if [ -n "${host}" ]; then
        GENERATED_FILES+=("${host}")
    fi
done <<< "${FRAGMENT_HOSTS}"

cd ..
STALE=()
for rel in "${GENERATED_FILES[@]}"; do
    if [ ! -f "${SNAPSHOT_DIR}/${rel}" ] || ! cmp -s "${SNAPSHOT_DIR}/${rel}" "${rel}"; then
        STALE+=("${rel}")
    fi
done

if [ ${#STALE[@]} -ne 0 ]; then
    echo
    echo "ERROR: checked-in generated files differ from a fresh regeneration:"
    for rel in "${STALE[@]}"; do
        if [ -f "${SNAPSHOT_DIR}/${rel}" ]; then
            changed=$(diff "${SNAPSHOT_DIR}/${rel}" "${rel}" | grep -c '^[<>]' || true)
            echo " ${rel} | ${changed} line(s) differ"
        else
            echo " ${rel} | newly generated, was not checked in"
        fi
    done
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

