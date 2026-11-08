#!/usr/bin/env python3
"""Build the 3D model of the HanBo MC-118LD-H65 upright USB-C receptacle.

Dimensions are from HanBo's MC-118LD-H65 drawing (LCSC C6332304).  The model is
a simplified one, accurate in outline, for renders and the KiCad 3D viewer.

The origin is the centre of the 16 signal pins, at the board's top surface, so
a footprint whose origin is that centre uses the model with no offset.

    python3 mc_118ld_h65.py --out ../3dmodels/MC-118LD-H65.step
"""

import argparse

import cadquery as cq

SHELL_W = 8.34
SHELL_D = 2.56
SHELL_T = 0.30
HEIGHT = 6.50
CAVITY = 4.45
TONGUE_W = 6.60
TONGUE_T = 0.70
TONGUE_TOP = 5.60
PIN_PITCH = 1.10
ROW_PITCH = 1.35
PIN_SIZE = 0.30
PIN_LEN = 2.10
TAB_X = 2.50
TAB_Y = 2.15
TAB_W = 1.00

STEEL = cq.Color(0.75, 0.75, 0.77)
PLASTIC = cq.Color(0.08, 0.08, 0.08)
GOLD = cq.Color(0.85, 0.65, 0.13)
TIN = cq.Color(0.80, 0.80, 0.82)


def obround(w, d, h):
    return cq.Workplane("XY").slot2D(w, d).extrude(h)


def shell():
    body = obround(SHELL_W, SHELL_D, HEIGHT).cut(
        obround(SHELL_W - 2 * SHELL_T, SHELL_D - 2 * SHELL_T, HEIGHT))
    for x in (-TAB_X, TAB_X):
        for y in (-1, 1):
            flange = (cq.Workplane("XY")
                      .center(x, y * (SHELL_D / 2 + (TAB_Y - SHELL_D / 2) / 2))
                      .rect(TAB_W, TAB_Y - SHELL_D / 2 + SHELL_T).extrude(SHELL_T))
            leg = (cq.Workplane("XY").workplane(offset=-PIN_LEN)
                   .center(x, y * TAB_Y).rect(TAB_W, SHELL_T).extrude(PIN_LEN + SHELL_T))
            body = body.union(flange).union(leg)
    return body


def molding():
    inner = obround(SHELL_W - 2 * SHELL_T, SHELL_D - 2 * SHELL_T, HEIGHT - CAVITY)
    tongue = (cq.Workplane("XY").workplane(offset=HEIGHT - CAVITY)
              .rect(TONGUE_W, TONGUE_T).extrude(TONGUE_TOP - (HEIGHT - CAVITY)))
    return inner.union(tongue)


def contacts():
    pads = cq.Workplane("XY")
    length = TONGUE_TOP - (HEIGHT - CAVITY) - 0.6
    for i in range(12):
        x = (i - 5.5) * 0.5
        for y in (-1, 1):
            pads = pads.union(
                cq.Workplane("XY").workplane(offset=HEIGHT - CAVITY + 0.3)
                .center(x, y * (TONGUE_T / 2 + 0.01)).rect(0.25, 0.02).extrude(length))
    return pads


def pins():
    out = cq.Workplane("XY")
    for i in range(8):
        for y in (-ROW_PITCH / 2, ROW_PITCH / 2):
            out = out.union(
                cq.Workplane("XY").workplane(offset=-PIN_LEN)
                .center((i - 3.5) * PIN_PITCH, y).rect(PIN_SIZE, PIN_SIZE)
                .extrude(PIN_LEN + HEIGHT - CAVITY))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True, help="STEP file to write")
    args = parser.parse_args()

    assy = cq.Assembly(name="MC-118LD-H65")
    assy.add(shell(), name="shell", color=STEEL)
    assy.add(molding(), name="molding", color=PLASTIC)
    assy.add(contacts(), name="contacts", color=GOLD)
    assy.add(pins(), name="pins", color=TIN)
    assy.export(args.out)


if __name__ == "__main__":
    main()
