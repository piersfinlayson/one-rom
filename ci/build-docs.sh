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
# Every document is built in each paper size docs.toml lists, currently A4 and
# US Letter - the readership is split across both, and a set built for one
# prints with shifted margins on the other.
#
# With a destination prefix the PDFs are staged per document, mirroring how
# cli/ and studio/ are laid out on images.onerom.org, and this release is merged
# into that repository's docs.json.
#
# docs.json accumulates history, so that a reader on an older CLI or firmware can
# still fetch the edition matching their build.  Unlike releases.json it carries
# no hand-curated fields, so the merge is done here rather than by pasting a
# fragment - inserting a release into a nested array by hand is a poor use of a
# release evening.  Like build-images.sh, this script deliberately does not touch
# `latest`: publishing a document and making it the current one stay separate
# steps.
#
set -e

DEST_PREFIX=$1

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

python3 "${PDF_DIR}/render.py" --out-dir "${OUT_DIR}"

ls -la "${OUT_DIR}"

[ -n "${DEST_PREFIX}" ] || exit 0

# Staging and the manifest merge both read what the renderer actually produced,
# rather than reconstructing the filenames from the config.
python3 - "${MANIFEST}" "${DEST_PREFIX}" "${OUT_DIR}" <<'EOF'
import hashlib, json, shutil, sys
from pathlib import Path

manifest, dest_prefix, out_dir = (Path(a) for a in sys.argv[1:4])
build = json.loads(manifest.read_text())

docs_json = dest_prefix / "docs.json"
if docs_json.exists():
    catalogue = json.loads(docs_json.read_text())
else:
    catalogue = {"version": 1, "latest": {}, "documents": []}

by_slug = {d["slug"]: d for d in catalogue["documents"]}

for document in build["documents"]:
    slug = document["basename"].removeprefix("one-rom-")
    path = f"docs/{slug}"
    dest = dest_prefix / path
    dest.mkdir(parents=True, exist_ok=True)

    files = []
    for entry in document["files"]:
        source = out_dir / entry["filename"]
        shutil.copy2(source, dest / entry["filename"])
        print(f"  staged at {path}/{entry['filename']}")
        files.append({
            "paper": entry["paper"],
            "filename": entry["filename"],
            "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        })

    entry = by_slug.get(slug)
    if entry is None:
        entry = {"slug": slug, "releases": []}
        by_slug[slug] = entry
        catalogue["documents"].append(entry)
    entry["title"] = document["title"]
    entry["tracks"] = document["version_source"]
    entry["path"] = path

    # Re-running a release replaces its entry rather than duplicating it, so a
    # rebuild after a correction is safe.
    release = {"version": document["version"], "files": files}
    entry["releases"] = [
        r for r in entry["releases"] if r["version"] != document["version"]
    ]
    entry["releases"].append(release)
    entry["releases"].sort(key=lambda r: [int(n) for n in r["version"].split(".")])

    current = catalogue["latest"].get(slug)
    if current != document["version"]:
        print(f"  note: {slug} latest is {current or 'unset'},"
              f" this release is {document['version']}")

docs_json.write_text(json.dumps(catalogue, indent=2) + "\n")
EOF

echo
echo "merged into ${DEST_PREFIX}/docs.json"
echo "- review the diff, set 'latest' when ready, then commit and push"
