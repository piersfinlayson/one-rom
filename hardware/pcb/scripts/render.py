#!/usr/bin/env python3
"""Render every Fire PCB, top and bottom, for recolouring in a browser.

Each side of each PCB produces three files:

- <name>-<side>.webp - the board with a grey solder mask and HASL pads.
- <name>-<side>-mask.webp - the solder mask's coverage, in the alpha channel.
- <name>-<side>-pads.webp - the exposed pads' coverage, in the alpha channel.

A browser recolours the solder mask and pads using the coverage images as CSS
masks, so one render serves every colour.  Coverage is the difference between
renders that change only the mask colour or only the copper finish.

Images are scaled to PX_PER_MM so every board appears at the same scale.  The
board files are rendered from temporary copies and left unchanged.

The script fails on a Fire board in rust/config/json that is neither in PCBS
nor in EXCLUDED.

    python3 render.py --out <dir>
"""

import argparse
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parents[3]
VERIFIED = ROOT / "hardware/pcb/verified"
CONFIG = ROOT / "rust/config/json"

# (PCB directory, firmware board, image name)
PCBS = [
    ("rp2350-24-pin-rev-a", "fire-24-a", "fire-24-a"),
    ("fire-24-usb-b", "fire-24-usb-b", "fire-24-usb-b"),
    ("fire-24-c", "fire-24-c", "fire-24-c"),
    ("fire-24-d", "fire-24-d", "fire-24-d"),
    ("fire-24-e", "fire-24-e", "fire-24-e"),
    ("fire-24-f", "fire-24-f", "fire-24-f"),
    ("fire-24-g", "fire-24-g", "fire-24-g"),
    ("fire-28-a", "fire-28-a", "fire-28-a"),
    ("fire-28-a2", "fire-28-a", "fire-28-a2"),
    ("fire-28-a3", "fire-28-a", "fire-28-a3"),
    ("fire-28-a4", "fire-28-a", "fire-28-a4"),
    ("fire-28-b", "fire-28-b", "fire-28-b"),
    ("fire-28-c", "fire-28-c", "fire-28-c"),
    ("fire-28-d", "fire-28-d", "fire-28-d"),
    ("fire-32-a", "fire-32-a", "fire-32-a"),
    ("fire-32-b2", "fire-32-b", "fire-32-b2"),
    ("fire-32-c", "fire-32-c", "fire-32-c"),
    ("fire-40-a", "fire-40-a", "fire-40-a"),
    ("fire-40-b", "fire-40-b", "fire-40-b"),
    ("fire-40-c", "fire-40-c", "fire-40-c"),
]

# Fire boards in the firmware config without a page entry.
EXCLUDED = {"fire-24-eadb01"}

PX_PER_MM = 17
RENDER_PX = 2000
MASKS = {"grey": "#808080D4", "white": "#FFFFFFD4", "black": "#000000D4"}
MAC_KICAD_CLI = "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"


def check_coverage():
    boards = {p.stem for p in CONFIG.glob("fire-*.json")}
    mapped = {board for _, board, _ in PCBS}
    missing = sorted(boards - mapped - EXCLUDED)
    if missing:
        sys.exit(f"No PCB mapped for {', '.join(missing)}: add it to PCBS or EXCLUDED")


def board_width_mm(text):
    xs = []
    for item in re.finditer(r"\(gr_(?:line|arc)\b(.*?)\(layer \"Edge\.Cuts\"\)", text, re.S):
        xs += [float(x) for x in re.findall(r"\((?:start|mid|end) (-?[\d.]+) ", item.group(1))]
    return max(xs) - min(xs)


def variant(text, project, mask, finish):
    text = text.replace("${KIPRJMOD}", str(project))
    text = re.sub(r'(\(type "(?:Top|Bottom) Solder Mask"\)\s*\(color )"[^"]*"',
                  rf'\1"{mask}"', text)
    return re.sub(r'\(copper_finish "[^"]*"\)', f'(copper_finish "{finish}")', text)


def render(cli, pcb, side, out):
    subprocess.run([cli, "pcb", "render", "--side", side, "--quality", "high",
                    "--background", "transparent", "--width", str(RENDER_PX),
                    "--height", str(RENDER_PX), "-o", str(out), str(pcb)],
                   check=True, capture_output=True)
    img = np.asarray(Image.open(out).convert("RGBA")).astype(float) / 255
    # KiCad shades the background around the board with a partly transparent
    # shadow. Only the board itself is opaque.
    img[img[..., 3] < 0.98] = 0
    img[..., 3] = np.ceil(img[..., 3])
    return img


def coverage(a, b, full):
    return np.clip(np.abs(a[..., :3] - b[..., :3]).sum(-1) / full, 0, 1)


def save_coverage(cov, size, path):
    half = Image.fromarray((cov * 255).astype(np.uint8)).resize(
        (size[0] // 2, size[1] // 2), Image.LANCZOS)
    alpha = (np.round(np.asarray(half) / 255 * 3) / 3 * 255).astype(np.uint8)
    rgba = np.zeros(alpha.shape + (4,), np.uint8)
    rgba[..., 3] = alpha
    Image.fromarray(rgba, "RGBA").save(path, "WEBP", lossless=True)


def render_pcb(cli, src, name, out, tmp):
    text = src.read_text()
    width = board_width_mm(text)
    for side in ("top", "bottom"):
        imgs = {}
        for key, mask, finish in (("grey", MASKS["grey"], "HAL SnPb"),
                                  ("white", MASKS["white"], "HAL SnPb"),
                                  ("black", MASKS["black"], "HAL SnPb"),
                                  ("enig", MASKS["grey"], "ENIG")):
            pcb = tmp / f"{key}.kicad_pcb"
            pcb.write_text(variant(text, src.parent, mask, finish))
            imgs[key] = render(cli, pcb, side, tmp / f"{key}.png")
        box = Image.fromarray((imgs["grey"][..., 3] * 255).astype(np.uint8)).getbbox()
        crop = {k: v[box[1]:box[3], box[0]:box[2]] for k, v in imgs.items()}
        scale = PX_PER_MM * width / crop["grey"].shape[1]
        size = (round(crop["grey"].shape[1] * scale), round(crop["grey"].shape[0] * scale))

        diff = np.abs(crop["white"][..., :3] - crop["black"][..., :3]).sum(-1)
        mask = coverage(crop["white"], crop["black"], np.median(diff[diff > 0.6]))
        pads = coverage(crop["enig"], crop["grey"], 0.15)

        Image.fromarray((crop["grey"] * 255).astype(np.uint8), "RGBA").resize(size, Image.LANCZOS) \
            .save(out / f"{name}-{side}.webp", "WEBP", quality=70)
        save_coverage(mask, size, out / f"{name}-{side}-mask.webp")
        save_coverage(pads, size, out / f"{name}-{side}-pads.webp")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True, type=pathlib.Path, help="directory to write images to")
    parser.add_argument("--kicad-cli", help="kicad-cli to use, found on PATH or in KiCad.app by default")
    args = parser.parse_args()

    check_coverage()
    cli = args.kicad_cli or shutil.which("kicad-cli") or MAC_KICAD_CLI
    args.out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for directory, _, name in PCBS:
            [src] = (VERIFIED / directory).rglob("*.kicad_pcb")
            print(name, flush=True)
            render_pcb(cli, src, name, args.out, pathlib.Path(tmp))


if __name__ == "__main__":
    main()
