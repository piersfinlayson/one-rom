# CLI Changelog

## v0.3.0 - 2026-??-??

- **Breaking: `onerom boards` is now `onerom board`**, and the bare listing it
  printed is `onerom board list`. There is no alias, so scripts calling
  `onerom boards` must be updated; the CLI suggests `board` rather than simply
  failing.
- **Ice (STM32) boards are listed separately and rejected where the CLI cannot
  use them.** The CLI has never had an STM32 path — every firmware path
  composes an RP2350 image and every device path speaks picoboot — but the
  merged list implied otherwise, and `--board ice-24-d` failed several layers
  down as a missing release. `scan`, `program`, `firmware build`,
  `firmware download`, `firmware inspect --board`, `control pin`,
  `control reset` and `inspect gpio` now reject an Ice `--board` up front. The
  commands that only *describe* hardware still take them: `board header`,
  `board socket`, `chips` and `firmware releases`.
- **Add GPIO control**: `onerom control pin` drives a One ROM GPIO high, low or
  high-impedance, `onerom control reset` pulses one low to reset the host
  system One ROM is installed in, and `onerom inspect gpio` shows what every
  GPIO is and what One ROM is doing with it. All three need a *running* device
  with the USB system plugin, and say so.
  - `control pin --hold <MS>` holds the state for a bounded period and then
    applies `--then` (`z` by default); without `--hold` the state latches. The
    **device** times the hold, so an interrupted CLI cannot leave a pin
    latched. `control reset` is that with `--state low --then z` and `--hold`
    defaulting to 100ms; it never drives the line high, and rejects `--hold 0`.
  - `--pin` takes `gpio<N>` or a header pad name — `sel_a` to `sel_e` (`sel-a`
    and `sela` accepted) and `x1`/`x2`. A bare number is rejected as ambiguous,
    as are the broken-out address pads (`a<N>`), with a reason. `--state` and
    `--then` accept `1`/`0` alongside `high`/`low`.
  - A GPIO One ROM is itself using is refused, naming what it is doing, unless
    `--force`, which prints what forcing costs: a pin serving *reads* goes back
    with `--state z`, a pin serving *drives* leaves serving broken until
    reboot. A GPIO that is not 5V-tolerant warns, which `--yes` or `--force`
    answers.
  - `control pin`, `control reset` and `inspect gpio` take `--board`, needed
    only to resolve a `--pin` pad name on a board this build does not
    recognise.

  Requires One ROM firmware v0.7.1 or later and the v0.7.1 or later USB system
  plugin; against anything older the CLI says which of the two is too old
  rather than surfacing a USB stall.

- **Add ASCII views of a board's physical pin layouts.** `onerom board header
  [<board>]` draws the pin (jumper / programming) header, annotating each
  image-select and X pad with the GPIO behind it and — on Fire boards —
  whether that GPIO is 5V-tolerant (`5V`) or 3.3V-only (`!!3V3!!`, an ADC pin).
  `onerom board socket [<board>] [--chip-type <chip>] [--gpio]` draws the ROM
  socket as a DIP pinout: GPIOs by default, ROM pin functions with
  `--chip-type`, and both with `--gpio`. A chip whose pin count differs from
  the board's is drawn at the larger of the two, bottom-justified, marking
  `overhang` and `(empty)` pins and the `X1`/`X2` fly-lead each overhanging
  address line needs. The board is inferred from a connected One ROM when
  omitted; `onerom inspect header` and `onerom inspect socket` are the
  device-side forms.
- **`onerom chips --board <board>` now reports how much flash each chip type
  uses.** Each chip is listed with its ROM size and its image size — often
  larger than the chip (a 2364 costs 8KB on a 24-pin board but 256KB
  overhanging a 28-pin one) — grouped by socket fit, matching
  `docs/COMPATIBILITY.md`. `--chip-type <chip>` (`-c`) answers for one type,
  and `board socket --chip-type` / `inspect socket --chip-type` report the same
  figure below the pinout. The listing now covers every chip the board can
  emulate, including the overhang and fly-lead combinations the name-only list
  omitted; a recognised type the board cannot serve is named in a trailing line
  instead. Ice boards keep the plain name list.
  - The Fit column is now legended, and the fit for a chip in a larger socket
    needing no signal wiring reads `larger socket (no fly-leads)` rather than
    `no fly-leads required`, which sat under a `(with fly-leads)` heading and
    read as a contradiction.
- **ROM images can be rearranged during a build**, via a new `transform=` key
  on `--slot`: `swap_bytes` reverses the byte order within each 16-bit word,
  and `deinterleave:<offset>/<stride>[/<bytes>]` extracts one lane from an
  interleaved image. Several may be combined with `+` and are applied in the
  order given, which matters. The same is expressible as a `"transform"` array
  in a config file. Previously the only option was `onerom image swap-bytes`,
  which rewrites the source file.
- **Add `onerom image deinterleave`** — the standalone counterpart to
  `onerom image swap-bytes` (`--offset`, `--stride`, `--bytes`). Both share
  their implementation with the `transform=` slot key. Neither reads Intel HEX;
  run `onerom image convert` first.
- Add `onerom image convert --from <fmt> --to <fmt> --input <file> --output
  <file> [--load-address <addr>]`, converting ROM images between `binary` and
  `ihex` (Intel HEX). The format set is designed to accept further formats
  later.
- Add `format` and `load_address` keys to `--slot` for Intel HEX ROM images:
  `--slot file=rom.hex,type=2364,cs1=active_low,format=ihex[,load_address=$E000]`.
  `format` accepts `binary` (default) or `ihex`; `load_address` (only valid
  with `format=ihex`) is a decimal or `0x`/`$`-prefixed address mapping to byte
  0 of the ROM. The same keys are available in config files.
- Add `--serial-override`, setting a custom USB serial number used while One
  ROM is running. A stopped One ROM continues to use its chip ID.
- Allow `--plugin` to be combined with `--config-file` on `program` and
  `firmware build`; the plugins are inserted ahead of the config's ROM slots.
  Errors if the config already defines a plugin of its own.
- The ROM type is now stored in metadata using the exact spelling the user
  entered, on both the `--config-file` (`"type"`) and `--slot type=...` paths,
  instead of a canonicalised name (`27SF512` is retained rather than normalised
  to `27512`). The resolved type still drives all behaviour.
- `onerom scan --list-boards` now prints the same listing as `onerom board
  list` rather than its own one-liner.
- `onerom inspect gpio` now shows one `Function` column instead of separate
  `Pad` and `Function` columns, listing everything the GPIO is: its ROM socket
  signal under the image being served, then the board peripheral, then the
  header pad, deduplicated.
  - This **fixes a real omission**: the lookup stopped at the first system
    function it matched, so on a `fire-24-f` — where the Status LED and the RGB
    LED are both GPIO 29 — the table named only the Status LED.
  - `Function` no longer claims a GPIO is `SWCLK` or `SWDIO`. Those are
    dedicated RP2350 pins with no GPIO of their own; the `SEL_C`/`SEL_D` pads
    merely share their nets. `onerom inspect header` remains the pad-indexed
    view.
- `onerom inspect gpio` now lists only the GPIOs connected to something, saying
  how many were omitted; `--all` lists every GPIO. The `X1`/`X2` and
  image-select pads always appear, being what the table is read to find.
- The explanatory legend under the `inspect gpio` table is now shown only with
  `--verbose`.
- `board socket` and `inspect socket` now say when a board has no GPIO map,
  instead of drawing the diagram with the GPIO column blank all the way down.
  The `--chip-type` function view is unaffected, being drawn from the chip's
  pinout rather than the socket map.
- `board header` and `inspect header` now say `command unsupported` for a board
  with no pin-header descriptor, where they said `nothing to draw` and called
  the descriptor missing "yet".
- `onerom inspect header` and `onerom inspect socket` no longer tell you to
  pass `--board` when they cannot identify the board. Neither has a `--board`;
  they now point at `onerom board header <board>` / `onerom board socket
  <board>`, which draw a board by name.
- Stop hard-wrapping prose in console output. A handful of messages broke a
  sentence at a fixed width, which the terminal then wrapped again at its own.
  Multi-line messages that put a *separate* sentence on its own indented line
  are unchanged; that is structure, not wrapping.
- Fix `onerom image swap-bytes` panicking at startup (even for `--help`) with a
  clap short-option collision: `-i` was claimed by both the global `--vid-pid`
  and swap-bytes' `--input`. `--input`/`--output` are now long-only (aliases
  `--in`/`--out` unchanged).
- New `Error` variants: `ImageTransform`, `IceBoardUnsupported`, and the GPIO
  control errors (device not running, pin in use, no free hold slot).

## v0.2.0 - 2026-07-20

- Support firmware v0.7.x
- Move plugin handling to onerom-app crate.

## v0.1.11 - 2026-07-12

- Prevent program command from allowing --plugin and --firmware simultanesouly (as ---plugins is ignored anyway).

## v0.1.10 - 2026-07-02

- Added more help on size handling when programming or building firmware.

## v0.1.9 - 2026-06-02

- New 28 and 32 pin boards - fire-28-c and fire-32-b.

## v0.1.8 - 2026-05-26

- Re-added 23QL384.
- Add `onerom chips` to show supported chip types and their aliases.
- Fixed `onerom program --scan-slots`, to correctly re-query One ROM after programming.
- More ROM type aliases.

## v0.1.7 - 2026-05-18

- Added 27C100 type as synonym of 27C301/27C1000
- Added `onerom image swap-bytes` to swap bytes in a 16-bit ROM image file.

## v0.1.6 - 2026-05-14

- Moved 23QL384 support to a 23QL512 type.
- Fixed bug introduced in 0.1.5 where a (benign) error is reported requerying One ROM after programming.

## [v0.1.5] - 2025-05-12

- Added _prototype_ support for the new 23QL384 ROM type.  May be deprecated or modified in a future release.
- Add support for `onerom program .. --scan-slots` to auto-run `onerom scan --slots` after programming. 

## [0.1.4] - 2026-05-08

- Add 2364, 2732, 2716, 2708 and 2704 support on One ROM 28 boards.  See the main [CHANGELOG](/CHANGELOG.md) for important notes on this support, including warnings about potential damage if not used correctly.

## [0.1.3] - 2026-04-25

- Add support for labels/names for slots.

# [0.1.2] - 2026-04-02

- Add batch programming mode for programming multiple devices with the same firmware more quickly.
- Add 27C080, 28C16, 28C64, 28C256 and 28C512 ROM support.

## [0.1.1] - 2026-03-26

- Allow program --fw as well as --firmware.
