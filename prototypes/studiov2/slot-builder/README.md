# Slot builder prototype

The website's One ROM Builder tab, rebuilt in Iced 0.14.

**Question.** Does the web builder's design survive the move to a Rust GUI
toolkit, and how much work is it?

**Answer. Yes, at about the same size as the web version.** Roughly 1,200 lines
of UI and drawing, 300 of it the board wireframe on a canvas, against the
website's 1,100 of JS plus 90 of HTML and 480 of CSS. Clean build 58s,
incremental 2.6s.

## Run

```bash
cargo run
```

## Driven by the real crates

Board list, jumper header geometry, chip types per board, chip sizes, the number
of chip-select dropdowns each chip type needs, file formats and the flash total
all come from `onerom-config` and `onerom-gen`. Build runs the real
`onerom_gen::Builder` pipeline with the tree's own plugin binaries from
`plugins/dist/`.

## Not done

- **Build output is metadata and ROM images.** A flashable image also needs the
  base firmware downloaded and padded to 48KB on the front.
- **ROM types come out in the crates' order, not the website's.** The website
  sorts them in `js/site/utils.js`, which carries a hand-written size table.
  That sort belongs in Rust, reachable from the apps and from WASM.
- Firmware versions are a hardcoded list rather than a manifest fetch.
- The plugin catalogue is read off disk, so no `min_fw_version` check runs.
- No licence acceptance, no tooltips, no device I/O.

## Against the web original

Iced has no letter-spacing, so the small-caps group titles are untracked, and
there are no transitions. Dropdowns and scrollbars are Iced's rather than the
platform's. Fonts are the same Inter and Michroma the website uses, embedded
from copies already in this repo.

## Screenshots without a screen-recording grant

`ONEROM_PROTO_SHOT` is a path to write a PNG to, `ONEROM_PROTO_SIZE` is
`WIDTHxHEIGHT`, and `ONEROM_PROTO_SETUP` is a comma-separated script —
`nohelp`, `board:fire-28-d`, `add`, `chip:0:23128`, `file:0:/path/to.bin`.

```bash
ONEROM_PROTO_SHOT=/tmp/shot.png ONEROM_PROTO_SETUP=nohelp,board:fire-28-d cargo run
```
