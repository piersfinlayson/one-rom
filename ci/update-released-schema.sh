#!/usr/bin/env bash
#
# Refresh rust/metadata/metadata_schema_released.toml from the working schema.
#
# The copy is what the metadata build script compares today's layout against,
# so that a structure whose bytes moved without its generation number rising
# fails the build.  Refreshing it on every build would absorb the very change
# it exists to catch, so it is done by hand, once per development cycle - when
# the release has gone out and the branch version is about to rise.
#
# Three files change in that one commit: the copy this script writes, [schema]
# firmware_release in rust/metadata/metadata_schema.toml, and the repo-root
# Makefile's VERSION_MAJOR/MINOR/PATCH, which `cargo run -p doc-gen` checks.
#
# Usage: ci/update-released-schema.sh
#
# Run from anywhere in the tree.

set -e

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
WORKING="${REPO_ROOT}/rust/metadata/metadata_schema.toml"
RELEASED="${REPO_ROOT}/rust/metadata/metadata_schema_released.toml"

if [ ! -f "${WORKING}" ]; then
    echo "ERROR: ${WORKING} is not there" >&2
    exit 1
fi

if cmp -s "${WORKING}" "${RELEASED}"; then
    echo "Already identical - nothing to refresh."
    exit 0
fi

cp "${WORKING}" "${RELEASED}"

echo "Refreshed ${RELEASED#"${REPO_ROOT}"/}"
echo
echo "Now raise [schema] firmware_release in rust/metadata/metadata_schema.toml"
echo "and VERSION_MAJOR/MINOR/PATCH in the Makefile, and commit the three"
echo "together."
