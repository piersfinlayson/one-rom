#!/usr/bin/env python3
"""Render the One ROM documentation set to PDF.

The set, and every string that appears in a PDF but not in its source markdown,
is defined in docs.toml.  Nothing document-specific is written here.

Cover text comes from the document itself: the title is the markdown's leading
H1 and the subtitle is the paragraph that follows it.  Every One ROM document
already opens by naming itself and saying in a sentence what it covers, so a
second copy of that text kept alongside the build would only be a copy to let
drift.  The H1 is dropped from the body, since the cover now carries it.

A manifest of what was built is written to the output directory, for
ci/build-docs.sh to stage from.
"""

import argparse
import datetime
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

PDF_DIR = Path(__file__).resolve().parent

# A title may name the version of the thing it documents - COMPATIBILITY.md
# opens "One ROM Chip Compatibility - firmware v0.7.2".  The cover prints the
# version in its own right, so trim the clause rather than saying it twice.
TITLE_VERSION_CLAUSE = re.compile(r"\s*[-–—]\s*\S*\s*v\d+\.\d+\.\d+\s*$")

# Contents entries long enough to wrap read badly, and in CHIP-TYPES.md every
# pinout heading is "23QL384 - A composite ROM type, serving a combined ...".
# The body keeps the full heading, where the description earns its place - the
# contents keeps the part that identifies it.
TOC_TRUNCATE_OVER = 40
TOC_SPLIT = re.compile(r"\s+[-–—]\s+")

# A document written for GitHub may carry its own hand-written contents list,
# as CHIP-TYPES.md does.  The generated contents supersedes it, so it is
# dropped rather than printed twice.
HAND_WRITTEN_TOC = re.compile(r"^#{1,3}\s+(table of )?contents\s*$", re.I)

NOT_PROSE = re.compile(
    r"""^(
        \#             |   # heading
        >              |   # block quote
        \|             |   # table row
        (```|~~~)      |   # fenced code
        ([-*+]\s)      |   # bullet list - the space matters, since a paragraph
                           # may open with an inline code span or emphasis
        (\d+\.\s)      |   # numbered list
        ([-*_]{3,}$)       # horizontal rule
    )""",
    re.X,
)


# -------------------------------------------------------------- versions --

def cli_version(root):
    """The onerom-cli crate version."""
    text = (root / "rust/cli/Cargo.toml").read_text()
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.M)
    if not match:
        sys.exit("error: no version in rust/cli/Cargo.toml")
    return match.group(1)


def firmware_version(root):
    """The firmware version, from the root Makefile."""
    text = (root / "Makefile").read_text()
    parts = []
    for field in ("MAJOR", "MINOR", "PATCH"):
        match = re.search(rf"^VERSION_{field}\s*:?=\s*(\S+)", text, re.M)
        if not match:
            sys.exit(f"error: no VERSION_{field} in Makefile")
        parts.append(match.group(1))
    return ".".join(parts)


VERSION_READERS = {"cli": cli_version, "firmware": firmware_version}


# -------------------------------------------------------------- markdown --

def is_prose(line):
    """True if a stripped line opens an ordinary paragraph."""
    return not NOT_PROSE.match(line)


def strip_inline_markup(text):
    """Reduce inline markdown to the plain text behind it."""
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)  # links
    text = re.sub(r"[`*_]", "", text)
    return text.strip()


def drop_hand_written_toc(lines):
    """Remove a hand-written contents section, up to the next heading."""
    out = []
    skipping = False
    for line in lines:
        if HAND_WRITTEN_TOC.match(line):
            skipping = True
            continue
        if skipping:
            if line.startswith("#"):
                skipping = False
            else:
                continue
        out.append(line)
    return out


def read_document(path):
    """Return (title, subtitle, body) for a markdown document."""
    lines = path.read_text().split("\n")

    title = None
    start = 0
    for i, line in enumerate(lines):
        if line.startswith("# "):
            title = TITLE_VERSION_CLAUSE.sub("", strip_inline_markup(line[2:]))
            start = i + 1
            break
    if title is None:
        sys.exit(f"error: {path} has no leading H1 to use as a title")

    subtitle = ""
    paragraph = []
    for line in lines[start:]:
        stripped = line.strip()
        if not stripped:
            if paragraph:
                break
            continue
        if not is_prose(stripped):
            if paragraph:
                break
            continue
        paragraph.append(stripped)
    if paragraph:
        subtitle = strip_inline_markup(" ".join(paragraph))

    body = drop_hand_written_toc(lines[:start - 1] + lines[start:])
    return title, subtitle, "\n".join(body)


def shorten_toc(html):
    """Trim over-long contents entries to their identifying part."""

    def trim(match):
        opening, text = match.group(1), match.group(2)
        if len(text) > TOC_TRUNCATE_OVER:
            head = TOC_SPLIT.split(text, maxsplit=1)[0]
            if head != text and len(head) >= 3:
                text = head
        return f"{opening}{text}</a>"

    start = html.find('<nav id="TOC"')
    end = html.find("</nav>", start)
    if start < 0 or end < 0:
        return html
    toc = re.sub(r"(<a\b[^>]*>)([^<]*)</a>", trim, html[start:end])
    return html[:start] + toc + html[end:]


# ----------------------------------------------------------------- build --

def render(document, config, root, out_dir, commit, date, year):
    project = config["project"]
    source = root / document["source"]
    basename = document["basename"]

    version_source = document["version_source"]
    if version_source not in config["version_sources"]:
        sys.exit(f"error: {basename}: unknown version source '{version_source}'")
    version = VERSION_READERS[version_source](root)
    label = config["version_sources"][version_source]["label"].format(version=version)

    title, subtitle, body = read_document(source)

    html = subprocess.run(
        [
            "pandoc",
            "--from=gfm",
            "--to=html5",
            "--standalone",
            f"--template={PDF_DIR / 'template.html'}",
            "--toc",
            "--toc-depth=3",
            "--metadata", f"title={title}",
            "--metadata", f"subtitle={subtitle}",
            "--metadata", f"versionlabel={label}",
            "--metadata", f"date={date}",
            "--metadata", f"commit={commit}",
            "--metadata", f"repo={project['repo']}",
            "--metadata", f"source={document['source']}",
            "--metadata", f"wordmark={project['name']}",
            "--metadata", f"url={project['url']}",
            "--metadata", f"notice={project['cover_notice'].format(year=year)}",
        ],
        input=body,
        capture_output=True,
        text=True,
        check=True,
    ).stdout

    html_path = out_dir / f"{basename}.html"
    html_path.write_text(shorten_toc(html))

    footer = project["footer_notice"].format(year=year).replace("'", "\\27 ")

    built = []
    for paper, spec in config["papers"].items():
        # The page size, the cover height that depends on it, and the footer
        # notice are the only style values that cannot be static.
        paper_css = out_dir / f"paper-{paper}.css"
        paper_css.write_text(
            f"@page {{ size: {spec['size']} }}\n"
            f"@page cover {{ size: {spec['size']} }}\n"
            f"@page toc {{ size: {spec['size']} }}\n"
            f".cover {{ height: {spec['height']} }}\n"
            f"@page {{ @bottom-left {{ content: '{footer}' }} }}\n"
        )
        pdf = out_dir / f"{basename}-v{version}-{paper}.pdf"
        subprocess.run(
            [
                "weasyprint",
                "--base-url", f"{PDF_DIR}/",
                "--stylesheet", str(PDF_DIR / "docs.css"),
                "--stylesheet", str(paper_css),
                str(html_path),
                str(pdf),
            ],
            check=True,
        )
        paper_css.unlink()
        built.append({"paper": paper, "filename": pdf.name})

    html_path.unlink()
    return {
        "basename": basename,
        "source": document["source"],
        "title": title,
        "version": version,
        "version_source": version_source,
        "files": built,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", default=str(PDF_DIR / "docs.toml"))
    parser.add_argument("--out-dir", required=True)
    args = parser.parse_args()

    root = PDF_DIR.parent.parent
    config = tomllib.loads(Path(args.config).read_text())

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    commit = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"],
        capture_output=True, text=True, check=True, cwd=root,
    ).stdout.strip()
    today = datetime.date.today()
    date = f"{today.day} {today:%B %Y}"

    documents = []
    for document in config["documents"]:
        print(f"building {document['basename']}")
        documents.append(
            render(document, config, root, out_dir, commit, date, today.year)
        )

    manifest = {"version": 1, "commit": commit, "date": today.isoformat(),
                "documents": documents}
    (out_dir / "build-manifest.json").write_text(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
