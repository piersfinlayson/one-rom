#!/usr/bin/env python3
"""Panelise PCBs for JLCPCB, joined by V-cuts, with rails on the left and right.

Each --row is one board design, repeated --cols times.  Rows are listed from
the top of the panel.  Every board must be the same width.  Boards abut with no
gap so each V-cut runs straight across the panel.  Board corners are square in
the panel.

Each row's directory holds, for a single board:

- <name>.kicad_pcb, with its zones filled
- <name>-all-pos.csv or <name>-top-pos.csv
- <name>-bom.csv

The CSV files are read from the directory itself or its fab/ subdirectory.

The output directory contains:

- <panel>.kicad_pcb, and <panel>.kicad_pro copied from the first board's
- gerbers/ - the gerbers, the drill file and a zip of both
- fab/<panel>-all-pos.csv and fab/<panel>-bom.csv for the whole panel

Each designator in the panel's fab files has _<n> added for board n.  Boards
are numbered from 1, left to right along each row, starting at the top row.

The panel's gerbers use the first board's plot settings.  The board files are
left unchanged.  Requires kicad-cli.

    python3 panelise.py --cols 8 --row <dir> [--row <dir> ...] --out <dir>
"""

import argparse
import csv
import pathlib
import re
import shutil
import subprocess
import sys
import uuid
import zipfile

MAC_KICAD_CLI = "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"

# JLCPCB's SMT assembly figures, in mm.
RAIL = 5.0
TOOLING_HOLE = 2.0
FIDUCIAL_COPPER = 1.0
FIDUCIAL_MASK = 2.0
FIDUCIAL_INSET = 3.85

# Top left corner of the panel on the KiCad page, in mm.
ORIGIN = (50.0, 50.0)

# Tooling holes are centred in a rail, this far from the top or bottom of the
# panel.  The right rail has no bottom hole so the panel can't be loaded
# rotated.
HOLE_FROM_END = 5.0
# Fiducials are this far from the top or bottom of the panel, on both sides.
# The right rail has no top fiducial.
FIDUCIAL_FROM_END = 10.0

# Height of each "V-CUT" label, in mm.  Each horizontal V-cut is labelled on
# the left and right rails, above the line.  Each vertical V-cut is labelled
# above the panel's top edge.
LABEL = 0.8

# JLCPCB accepts at most 200 designators on a BOM line.
BOM_LINE_MAX = 200

HEADER_BLOCKS = {
    "version", "generator", "generator_version", "general", "paper",
    "title_block", "layers", "setup", "property",
}

COORD = re.compile(r"\((xy|start|end|mid|center|at) (-?[\d.]+) (-?[\d.]+)")
NET_REF = re.compile(r"\(net (\d+)(\)| \")")
UUID = re.compile(r'\(uuid "([0-9a-fA-F-]+)"\)')


def fmt(v):
    s = f"{v:.6f}".rstrip("0").rstrip(".")
    return "0" if s == "-0" else s


class Board:
    def __init__(self, directory):
        self.dir = pathlib.Path(directory)
        self.name = self.dir.name
        self.pcb = self.dir / f"{self.name}.kicad_pcb"
        if not self.pcb.is_file():
            sys.exit(f"missing {self.pcb}")
        self.pos = self.find("-all-pos.csv", "-top-pos.csv")
        self.bom = self.find("-bom.csv")
        self.head, self.blocks, self.tail = split(self.pcb.read_text())
        self.nets = {}
        for b in self.blocks:
            m = re.match(r'\t\(net (\d+) "(.*)"\)$', b)
            if m:
                self.nets[m.group(1)] = m.group(2)
        xs, ys = [], []
        for b in self.blocks:
            if b.startswith("\t(gr_") and '(layer "Edge.Cuts")' in b:
                for _, x, y in COORD.findall(b):
                    xs.append(float(x))
                    ys.append(float(y))
        if not xs:
            sys.exit(f"{self.pcb} has no board outline")
        self.left, self.top = min(xs), min(ys)
        self.width, self.height = max(xs) - self.left, max(ys) - self.top

    def find(self, *suffixes):
        tried = []
        for d in (self.dir, self.dir / "fab"):
            for s in suffixes:
                f = d / f"{self.name}{s}"
                if f.is_file():
                    return f
                tried.append(str(f))
        sys.exit("missing all of:\n  " + "\n  ".join(tried))


def split(text):
    """Split a KiCad board file into its header lines, top-level blocks and
    trailing lines."""
    lines = text.split("\n")
    head, blocks, cur, i = [], [], None, 0
    while i < len(lines) and not lines[i].startswith("\t("):
        head.append(lines[i])
        i += 1
    while i < len(lines):
        line = lines[i]
        if cur is None:
            if not line.startswith("\t("):
                break
            cur = [line]
            if line.count("(") == line.count(")"):
                blocks.append(line)
                cur = None
        else:
            cur.append(line)
            if line == "\t)":
                blocks.append("\n".join(cur))
                cur = None
        i += 1
    return head, blocks, lines[i:]


def kind(block):
    return re.match(r"\t\((\w+)", block).group(1)


def shift(text, dx, dy):
    return COORD.sub(
        lambda m: f"({m.group(1)} {fmt(float(m.group(2)) + dx)} "
        f"{fmt(float(m.group(3)) + dy)}",
        text,
    )


def place(block, dx, dy, netmap, n):
    """Return a copy of a board item moved by dx, dy for board n."""
    if kind(block) == "footprint":
        # Everything in a footprint except its own position is relative to it.
        lines = block.split("\n")
        for i, line in enumerate(lines):
            if line.startswith("\t\t(at "):
                lines[i] = shift(line, dx, dy)
                break
        block = "\n".join(lines)
    else:
        block = shift(block, dx, dy)
    block = NET_REF.sub(lambda m: f"(net {netmap[m.group(1)]}{m.group(2)}",
                        block)
    return UUID.sub(
        lambda m: f'(uuid "{uuid.uuid5(uuid.NAMESPACE_OID, f"{n}:{m.group(1)}")}")',
        block,
    )


def new_uuid(tag):
    return str(uuid.uuid5(uuid.NAMESPACE_OID, f"panel:{tag}"))


def line(x1, y1, x2, y2, tag):
    return (
        f"\t(gr_line\n\t\t(start {fmt(x1)} {fmt(y1)})\n\t\t(end {fmt(x2)} {fmt(y2)})\n"
        f"\t\t(stroke\n\t\t\t(width 0.05)\n\t\t\t(type default)\n\t\t)\n"
        f'\t\t(layer "Edge.Cuts")\n\t\t(uuid "{new_uuid(tag)}")\n\t)'
    )


def text(s, x, y, angle, tag):
    return (
        f'\t(gr_text "{s}"\n\t\t(at {fmt(x)} {fmt(y)} {fmt(angle)})\n'
        f'\t\t(layer "Edge.Cuts")\n\t\t(uuid "{new_uuid(tag)}")\n'
        f"\t\t(effects\n\t\t\t(font\n\t\t\t\t(size {fmt(LABEL)} {fmt(LABEL)})\n"
        f"\t\t\t\t(thickness {fmt(LABEL * 0.15)})\n\t\t\t)\n\t\t)\n\t)"
    )


def footprint(ref, x, y, layer, attr, pad, tag):
    side = "F" if layer == "F.Cu" else "B"
    mirror = "" if side == "F" else "\n\t\t\t\t(justify mirror)"
    props = ""
    for name, value in (("Reference", ref), ("Value", ref)):
        props += (
            f'\t\t(property "{name}" "{value}"\n\t\t\t(at 0 0 0)\n'
            f'\t\t\t(layer "{side}.Fab")\n\t\t\t(hide yes)\n'
            f'\t\t\t(uuid "{new_uuid(tag + name)}")\n'
            f"\t\t\t(effects\n\t\t\t\t(font\n\t\t\t\t\t(size 1 1)\n"
            f"\t\t\t\t\t(thickness 0.15)\n\t\t\t\t){mirror}\n\t\t\t)\n\t\t)\n"
        )
    return (
        f'\t(footprint "panel:{ref}"\n\t\t(layer "{layer}")\n'
        f'\t\t(uuid "{new_uuid(tag)}")\n\t\t(at {fmt(x)} {fmt(y)})\n{props}'
        f"\t\t(attr {attr} exclude_from_pos_files exclude_from_bom)\n{pad}\t)"
    )


def tooling_hole(x, y, n):
    pad = (
        f'\t\t(pad "" np_thru_hole circle\n\t\t\t(at 0 0)\n'
        f"\t\t\t(size {fmt(TOOLING_HOLE)} {fmt(TOOLING_HOLE)})\n"
        f"\t\t\t(drill {fmt(TOOLING_HOLE)})\n"
        f'\t\t\t(layers "*.Cu" "*.Mask")\n'
        f'\t\t\t(uuid "{new_uuid(f"hole{n}pad")}")\n\t\t)\n'
    )
    return footprint(f"H{n}", x, y, "F.Cu", "through_hole", pad, f"hole{n}")


def fiducial(x, y, side, n):
    margin = (FIDUCIAL_MASK - FIDUCIAL_COPPER) / 2
    pad = (
        f'\t\t(pad "" smd circle\n\t\t\t(at 0 0)\n'
        f"\t\t\t(size {fmt(FIDUCIAL_COPPER)} {fmt(FIDUCIAL_COPPER)})\n"
        f'\t\t\t(layers "{side}.Cu" "{side}.Mask")\n'
        f"\t\t\t(solder_mask_margin {fmt(margin)})\n"
        f'\t\t\t(uuid "{new_uuid(f"fid{side}{n}pad")}")\n\t\t)\n'
    )
    return footprint(f"FID{side}{n}", x, y, f"{side}.Cu", "smd", pad,
                     f"fid{side}{n}")


def check_widths(rows):
    width = rows[0].width
    for b in rows:
        if abs(b.width - width) > 1e-6:
            sys.exit(f"{b.name} is {b.width:.4f} mm wide, not {width:.4f} mm")


def build_pcb(rows, cols, out_pcb):
    width = rows[0].width
    ox, oy = ORIGIN
    panel_w = 2 * RAIL + cols * width
    panel_h = sum(b.height for b in rows)

    netnames = [""]
    for b in rows:
        for name in b.nets.values():
            if name not in netnames:
                netnames.append(name)
    netnum = {name: i for i, name in enumerate(netnames)}

    items, placements = [], []
    y = oy
    n = 0
    for b in rows:
        netmap = {k: netnum[v] for k, v in b.nets.items()}
        for c in range(cols):
            n += 1
            dx = ox + RAIL + c * width - b.left
            dy = y - b.top
            placements.append((n, b, dx, dy))
            for blk in b.blocks:
                k = kind(blk)
                if k in ("net", "group", "embedded_fonts") or k in HEADER_BLOCKS:
                    continue
                if k.startswith("gr_") and '(layer "Edge.Cuts")' in blk:
                    continue
                items.append(place(blk, dx, dy, netmap, n))
        y += b.height

    x0, y0, x1, y1 = ox, oy, ox + panel_w, oy + panel_h
    frame = [
        line(x0, y0, x1, y0, "top"),
        line(x1, y0, x1, y1, "right"),
        line(x1, y1, x0, y1, "bottom"),
        line(x0, y1, x0, y0, "left"),
    ]
    for c in range(cols + 1):
        x = ox + RAIL + c * width
        frame.append(line(x, y0, x, y1, f"vcut-col{c}"))
        frame.append(text("V-CUT", x, y0 - LABEL, 0, f"label-col{c}"))
    y = oy
    for r, b in enumerate(rows[:-1]):
        y += b.height
        frame.append(line(x0, y, x1, y, f"vcut-row{r}"))
        for side, x in (("left", x0 + RAIL / 2), ("right", x1 - RAIL / 2)):
            frame.append(text("V-CUT", x, y - LABEL, 0, f"label-row{r}{side}"))

    left, right = ox + RAIL / 2, x1 - RAIL / 2
    frame += [
        tooling_hole(left, y0 + HOLE_FROM_END, 1),
        tooling_hole(left, y1 - HOLE_FROM_END, 2),
        tooling_hole(right, y0 + HOLE_FROM_END, 3),
    ]
    fl, fr = ox + FIDUCIAL_INSET, x1 - FIDUCIAL_INSET
    for side in ("F", "B"):
        frame += [
            fiducial(fl, y0 + FIDUCIAL_FROM_END, side, 1),
            fiducial(fl, y1 - FIDUCIAL_FROM_END, side, 2),
            fiducial(fr, y1 - FIDUCIAL_FROM_END, side, 3),
        ]

    first = rows[0]
    header = [blk for blk in first.blocks if kind(blk) in HEADER_BLOCKS]
    fonts = [blk for blk in first.blocks if kind(blk) == "embedded_fonts"]
    nets = [f'\t(net {i} "{name}")' for i, name in enumerate(netnames)]
    out_pcb.write_text("\n".join(
        first.head + header + nets + frame + items + fonts + first.tail))
    return placements, panel_w, panel_h


def build_pos(placements, out):
    rows = []
    for n, b, dx, dy in placements:
        with open(b.pos, newline="") as f:
            r = csv.reader(f)
            header = next(r)
            for ref, val, pkg, x, y, rot, side in r:
                rows.append([f"{ref}_{n}", val, pkg,
                             f"{float(x) + dx:.6f}", f"{float(y) - dy:.6f}",
                             rot, side])
    with open(out, "w") as f:
        f.write(",".join(header) + "\n")
        for row in rows:
            f.write(",".join(f'"{v}"' if i < 3 else v
                             for i, v in enumerate(row)) + "\n")


def build_bom(placements, out):
    groups = {}
    for n, b, _, _ in placements:
        with open(b.bom, newline="") as f:
            r = csv.reader(f)
            header = next(r)
            ref_col, qty_col = header.index("Reference"), header.index("Qty")
            for row in r:
                key = tuple(v for i, v in enumerate(row)
                            if i not in (ref_col, qty_col))
                refs = [f"{ref}_{n}" for ref in row[ref_col].split(",")]
                groups.setdefault(key, (row, []))[1].extend(refs)
    with open(out, "w") as f:
        f.write(",".join(f'"{h}"' for h in header) + "\n")
        for row, refs in groups.values():
            for i in range(0, len(refs), BOM_LINE_MAX):
                part = refs[i:i + BOM_LINE_MAX]
                out_row = list(row)
                out_row[ref_col], out_row[qty_col] = ",".join(part), str(len(part))
                f.write(",".join(f'"{v}"' for v in out_row) + "\n")


def run(cli, *args):
    r = subprocess.run([cli, *args], capture_output=True, text=True)
    if r.returncode:
        sys.exit(r.stdout + r.stderr)


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--cols", type=int, required=True,
                   help="boards in each row")
    p.add_argument("--row", action="append", required=True,
                   help="board directory for one row, top row first")
    p.add_argument("--out", required=True, help="output directory")
    p.add_argument("--name", help="panel name (default: the output "
                   "directory's name)")
    p.add_argument("--kicad-cli", help="kicad-cli to use, found on PATH or "
                   "in KiCad.app by default")
    a = p.parse_args()

    cli = a.kicad_cli or shutil.which("kicad-cli") or MAC_KICAD_CLI
    if not shutil.which(cli):
        sys.exit(f"kicad-cli not found: {cli}")
    out = pathlib.Path(a.out)
    name = a.name or out.name
    if out.exists():
        sys.exit(f"{out} already exists")
    boards = {}
    rows = [boards.setdefault(d, Board(d)) for d in a.row]
    check_widths(rows)

    gerbers, fab = out / "gerbers", out / "fab"
    gerbers.mkdir(parents=True)
    fab.mkdir()
    pcb = out / f"{name}.kicad_pcb"
    placements, w, h = build_pcb(rows, a.cols, pcb)
    pro = rows[0].pcb.with_suffix(".kicad_pro")
    if pro.is_file():
        (out / f"{name}.kicad_pro").write_text(
            pro.read_text().replace(f'"{pro.name}"', f'"{name}.kicad_pro"'))
    build_pos(placements, fab / f"{name}-all-pos.csv")
    build_bom(placements, fab / f"{name}-bom.csv")

    run(cli, "pcb", "export", "gerbers", "--board-plot-params", "-o",
        f"{gerbers}/", str(pcb))
    run(cli, "pcb", "export", "drill", "--format", "excellon", "--excellon-units",
        "mm", "--excellon-zeros-format", "decimal", "--excellon-oval-format",
        "route", "-o", f"{gerbers}/", str(pcb))
    files = sorted(f for f in gerbers.iterdir()
                   if f.suffix in (".gbr", ".gbrjob", ".drl"))
    with zipfile.ZipFile(gerbers / f"{name}-gerbers.zip", "w",
                         zipfile.ZIP_DEFLATED) as z:
        for f in files:
            z.write(f, f.name)

    print(f"{name}: {len(placements)} boards")
    print(f"panel outline: {w:.4f} x {h:.4f} mm")
    ew, eh = gerber_extent(gerbers / f"{name}-Edge_Cuts.gbr")
    print(f"Edge.Cuts gerber, with labels and line width: {ew:.4f} x {eh:.4f} mm")


def gerber_extent(path):
    """Return the width and height covered by a gerber's draws and flashes,
    which use round apertures."""
    text = path.read_text()
    m = re.search(r"%FSLAX(\d)(\d)Y\d\d\*%", text)
    scale = 10 ** int(m.group(2))
    sizes = {d: float(s) for d, s in re.findall(r"%ADD(\d+)C,([\d.]+)", text)}
    lo, hi = [float("inf")] * 2, [float("-inf")] * 2
    x = y = r = 0.0
    for d, mx, my in re.findall(
            r"^(?:D(\d{2,})\*|(?:X(-?\d+))?(?:Y(-?\d+))?D0[123]\*)$",
            text, re.M):
        if d:
            r = sizes.get(d, 0.0) / 2
            continue
        x = int(mx) / scale if mx else x
        y = int(my) / scale if my else y
        lo = [min(lo[0], x - r), min(lo[1], y - r)]
        hi = [max(hi[0], x + r), max(hi[1], y + r)]
    return hi[0] - lo[0], hi[1] - lo[1]


if __name__ == "__main__":
    main()
