#!/usr/bin/env bash
#
# build-docs.sh - Render the One ROM documentation set to PDF, and stage it for
# one-rom-images (images.onerom.org).
#
# The set itself, and every string that appears in a PDF but not in its source
# markdown, is defined in docs/pdf/docs.toml.  Nothing document-specific is
# stated in this script - it resolves the toolchain, runs the renderer and
# stages what comes out.
#
# Each document is versioned as the thing it documents, not as the repository:
# the CLI manual tracks the onerom-cli crate version, the chip compatibility and
# chip type references track the firmware version.  Each is read from the
# authoritative file at build time, so there is nothing to keep in step by hand.
#
# The two generated documents, COMPATIBILITY.md and CHIP-TYPES.md, are rendered
# from the committed markdown and are NOT regenerated here.  ci/rust-tests.sh
# already fails if the committed copy differs from a fresh regeneration, so once
# that gate is green the committed file is current by definition - regenerating
# would only dirty the tree against the check that just passed.  Run the gate
# before this script, not inside it.
#
# Usage: build-docs.sh [dest-prefix] [--source NAME] [--config PATH]
#
# --source names a version source from docs.toml, and so selects the documents
# that ship on that release cycle.  This matters because the documents do not
# all release together: the CLI manual moves with the onerom-cli crate, the chip
# references with the firmware, and each is published by its own release.  Build
# the whole set from one of them and you republish documents whose version has
# not moved, overwriting bytes readers already have.
#
#   ci/build-docs.sh ../one-rom-images --source firmware   # firmware release
#   ci/build-docs.sh ../one-rom-images --source cli        # CLI release
#
# With no --source every document is built, which is what CI wants - it renders
# the whole set to prove none of them breaks the renderer, and publishes nothing.
#
# --config selects a different set - docs/pdf/archive.toml holds past editions,
# rendered from the git ref they shipped at, built on demand rather than every
# release.
#
# Every document is built in each paper size docs.toml lists, currently A4 and
# US Letter - the readership is split across both, and a set built for one
# prints with shifted margins on the other.
#
# With a destination prefix the PDFs are staged into images.onerom.org, laid out
# as every other product there is:
#
#   docs/docs.json                        an index, as plugins/plugins.json is
#   docs/<slug>/releases.json             as plugins/user/blink/releases.json is
#   docs/<slug>/v<version>/<files>
#
# Each document's releases.json accumulates its editions, so a reader on an
# older CLI or firmware can still fetch the one matching their build.  It
# carries no hand-curated fields, so the merge is done here rather than by
# pasting a fragment - inserting a release into a nested array by hand is a poor
# use of a release evening.  Like build-images.sh, this script deliberately does
# not touch `latest`: publishing a document and making it the current one stay
# separate steps.
#
set -e

DEST_PREFIX=""
CONFIG=""
SOURCE=""
while [ $# -gt 0 ]; do
    case "$1" in
        --config) CONFIG=$2; shift 2 ;;
        --source) SOURCE=$2; shift 2 ;;
        -*)       echo "error: unknown option $1"; exit 1 ;;
        *)        DEST_PREFIX=$1; shift ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PDF_DIR="${PROJECT_ROOT}/docs/pdf"
OUT_DIR="${PDF_DIR}/build"
MANIFEST="${OUT_DIR}/build-manifest.json"

cd "${PROJECT_ROOT}"

command -v pandoc >/dev/null || {
    echo "error: pandoc not found - run ci/install-doc-tools.sh"; exit 1; }
command -v weasyprint >/dev/null || {
    echo "error: weasyprint not found - run ci/install-doc-tools.sh"; exit 1; }

python3 "${PDF_DIR}/render.py" --out-dir "${OUT_DIR}" \
    ${CONFIG:+--config "${PROJECT_ROOT}/${CONFIG}"} \
    ${SOURCE:+--source "${SOURCE}"}

ls -la "${OUT_DIR}"

[ -n "${DEST_PREFIX}" ] || exit 0

# Staging and the manifests both read what the renderer actually produced,
# rather than reconstructing filenames from the config.
#
# The layout follows every other product on images.onerom.org - files under
# <slug>/v<version>/, a releases.json beside the version directories, and a thin
# index above them, exactly as plugins/ does with plugins.json plus a
# releases.json per plugin.
python3 - "${MANIFEST}" "${DEST_PREFIX}" "${OUT_DIR}" <<'EOF'
import hashlib, json, shutil, sys
from pathlib import Path

manifest, dest_prefix, out_dir = (Path(a) for a in sys.argv[1:4])
build = json.loads(manifest.read_text())

docs_root = dest_prefix / "docs"
docs_root.mkdir(parents=True, exist_ok=True)


def load(path, default):
    return json.loads(path.read_text()) if path.exists() else default


def save(path, data):
    path.write_text(json.dumps(data, indent=2) + "\n")


index = load(docs_root / "docs.json", {"version": 1, "documents": []})
by_slug = {d["slug"]: d for d in index["documents"]}

for document in build["documents"]:
    slug = document["slug"]
    version = document["version"]
    version_dir = f"v{version}"
    dest = docs_root / slug / version_dir
    dest.mkdir(parents=True, exist_ok=True)

    files = []
    for entry in document["files"]:
        source = out_dir / entry["filename"]
        shutil.copy2(source, dest / entry["filename"])
        print(f"  staged at docs/{slug}/{version_dir}/{entry['filename']}")
        files.append({
            "paper": entry["paper"],
            "filename": entry["filename"],
            "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        })

    # The per-document catalogue, alongside its version directories.
    releases_path = docs_root / slug / "releases.json"
    catalogue = load(releases_path, {"version": 1, "latest": None, "releases": []})
    catalogue["display_name"] = document["title"]
    catalogue["description"] = document["description"]
    catalogue["tracks"] = document["tracks"]

    # Re-running a release replaces its entry rather than duplicating it, so a
    # rebuild after a correction does not leave two entries for one version.
    release = {"version": version, "path": version_dir, "files": files}
    catalogue["releases"] = [
        r for r in catalogue["releases"] if r["version"] != version
    ]
    catalogue["releases"].append(release)
    catalogue["releases"].sort(key=lambda r: [int(n) for n in r["version"].split(".")])

    # Ordered so a reader meets the document before its release history.
    save(releases_path, {
        "version": catalogue["version"],
        "display_name": catalogue["display_name"],
        "description": catalogue["description"],
        "tracks": catalogue["tracks"],
        "latest": catalogue["latest"],
        "releases": catalogue["releases"],
    })

    if catalogue["latest"] != version:
        print(f"  note: {slug} latest is {catalogue['latest'] or 'unset'},"
              f" this release is {version}")

    # The index carries nothing versioned, as plugins.json does not.
    if slug not in by_slug:
        entry = {"slug": slug, "title": None, "path": slug}
        by_slug[slug] = entry
        index["documents"].append(entry)
    by_slug[slug]["title"] = document["title"]
    by_slug[slug]["path"] = slug

save(docs_root / "docs.json", index)
EOF

echo
echo "merged into ${DEST_PREFIX}/docs/ - docs.json and each document's releases.json"
echo "- review the diff, set 'latest' when ready, then commit and push"
