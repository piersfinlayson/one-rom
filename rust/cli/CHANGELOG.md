# CLI Changelog

## v0.3.0 - 2026-??-??

- **Breaking: `onerom boards` is now `onerom board`**, and the bare listing it
  used to print is `onerom board list`. The `board header` and `board socket`
  views added earlier in this release are documented under their shipping names
  and never existed as `boards` subcommands in a release. There is **no alias**
  — a plural noun taking a singular argument (`onerom board header fire-24-f`)
  read wrongly, and with
  three subcommands under it the listing deserved a name of its own rather than
  being what you got by omitting one. Scripts calling `onerom boards` must be
  updated; the CLI suggests `board` rather than simply failing.
- `onerom inspect gpio` now shows one `Function` column instead of separate
  `Pad` and `Function` columns, listing everything the GPIO is: its ROM socket
  signal under the image being served, then the board peripheral, then the
  header pad. The old split was about where a name came from rather than about
  anything a reader needs — on a 32-pin board the `a17` header pad and the
  socket's `A17` line are one net, and everywhere else exactly one of the two
  columns was populated. Repeated names are shown once; an X or image-select pad
  is a function of the pin like any other.
  - It also **fixes a real omission**: the lookup stopped at the first system
    function it matched, so on a `fire-24-f` — where the status LED and the
    NeoPixel are both GPIO 29 — the table named only the status LED. Both are
    now listed, which matters because that is the pin most likely to be driven
    by accident.
  - `Function` no longer claims a GPIO is `SWCLK` or `SWDIO`. Those are
    dedicated RP2350 pins with no GPIO of their own; the `SEL_C`/`SEL_D` pads
    merely share their nets, which is a fact about the pad, not about the GPIO.
    `onerom inspect header` remains the pad-indexed view and still shows every
    role a pad carries.
- `onerom inspect gpio` now lists only the GPIOs connected to something, with a
  line saying how many were omitted; `--all` lists every GPIO. On a `fire-28-c`
  thirteen of forty-eight GPIOs are connected to nothing at all, and they buried
  the rows worth reading. The filter is on what a GPIO *is*, not on what the
  device reports using it for — the `X1`/`X2` and image-select pads report
  `free` and are exactly what the table is read to find, so they always appear.
  With no recognised board nothing is hidden, since nothing can be ruled out.
- The explanatory legend under the `inspect gpio` table is now shown only with
  `--verbose`. It is worth reading once and noise on every run after that, and
  nothing is lost by default: the cost of taking a serving pin over is stated at
  the point of action by `control pin` itself.

- Add GPIO control: `onerom control pin` drives a One ROM GPIO high, low or
  high-impedance, `onerom control reset` pulses one low to reset the host system
  One ROM is installed in, and `onerom inspect gpio` shows what every GPIO is and
  what One ROM is doing with it. All three need a *running* device with the USB
  system plugin — a stopped One ROM is in the RP2350 bootloader, where One ROM's
  own command handler does not exist, and they say so rather than failing
  obscurely.
  - `control pin --hold <MS>` holds the state for a bounded period and then
    applies `--then` (`z` unless you say otherwise); without `--hold` the state
    latches. The **device** times the hold, so an interrupted CLI cannot leave a
    pin latched. `control reset` is that with `--state low --then z` and `--hold`
    defaulting to 100ms, and it never drives the line high: a reset net has its
    own pull-up and may have other drivers on it. A `--hold` of 0 is rejected for
    `reset`, since a pulse with no end is not a reset.
  - A GPIO One ROM is itself using is refused, naming what it is doing, unless
    `--force` is given — and forcing prints what it costs: a pin serving *reads*
    (address, chip-select, `/BYTE`) goes back with `--state z`, while a pin
    serving *drives* (a data pin) leaves serving broken until the device is
    rebooted. A GPIO that is not 5V-tolerant (an RP2350 ADC pin, from static
    board metadata rather than any measurement) warns and asks, which `--yes` or
    `--force` answers. Nothing else about the pad is checked: what is wired to it
    is the user's to know.
  - `--pin` takes an MCU GPIO written `gpio<N>` only. A bare number is rejected
    rather than guessed at — `23` is a plausible GPIO, image-select pad, X pad
    and ROM socket pin at once — and the header pad names (`sel_a`, `x1`, …) are
    recognised and reported as not yet supported rather than as meaningless.
  - `inspect gpio` names each GPIO's role itself, from the board pin map and the
    chip type being served, because the device deliberately reports only what
    taking a pin over would cost (free / read by serving / driven by serving /
    system) and never what the pin is. The table's `Function` column reuses the
    same board lookups `board header` and `board socket --gpio` draw
    with, so the diagram and the table cannot drift apart. A board revision or
    ROM type this build does not recognise costs those names, not the listing,
    and a board whose physical header layout is not yet characterised falls back
    to naming pads from its pin assignments and says so.
  - The GPIO count listed and accepted is the device's own — 30 on an RP2350A, 48
    on an RP2350B — read from it rather than assumed.

  Requires One ROM firmware v0.7.1 or later and the v0.7.1 or later USB system
  plugin; against anything older the CLI says which of the two is too old rather
  than surfacing a USB stall.

- `onerom chips --board <board>` now reports how much of One ROM's flash each
  chip type uses. Each chip is listed with its ROM size and its image size — the
  flash used to emulate it, which is often larger than the chip (a 2364 costs 8KB
  on a 24-pin board but 256KB overhanging a 28-pin one) — grouped by how it fits
  the socket (native / overhang / fly-lead), matching `docs/COMPATIBILITY.md`.
  `--chip-type <chip>` (`-c`) answers for a single chip type. The listing now
  covers every chip the board can emulate, including the overhang and fly-lead
  combinations the previous name-only list omitted; conversely a recognised chip
  type the board cannot serve (the SRAM types) is named in a trailing line
  instead of being listed as supported. Ice (STM32) boards, for which no image
  size can be derived, keep the plain name list. `--all` is unchanged.
- `onerom board socket <board> --chip-type <chip>` (and `onerom inspect socket
  --chip-type <chip>`) now reports the chip's image size below the pinout.
- Reword the fit shown for a chip with more pins than the board whose extra pins
  carry no address lines (the 32-pin 28C512 on a 28-pin board). It read `no
  fly-leads required` directly under a `(with fly-leads)` heading, which reads as
  a contradiction; it now reads `larger socket (no fly-leads)`, saying where One
  ROM sits as well as what it does not need. `docs/COMPATIBILITY.md` is
  regenerated to match.

- Add ASCII views of a board's physical pin layouts. `onerom board header
  [<board>]` draws the pin (jumper / programming) header, annotating each
  image-select and X pad with the MCU GPIO behind it and — on RP2350 (Fire)
  boards — whether that GPIO is 5V-tolerant (`5V`) or 3.3V-only (`!!3V3!!`, an
  ADC pin). `onerom board socket [<board>] [--chip-type <chip>] [--gpio]` draws the
  ROM socket as a DIP pinout: GPIOs by default, ROM pin functions (address /
  data / chip-select / `BYTE` / …) with `--chip-type <chip>`, and both with
  `--gpio`. A chip whose pin count differs from the board's is drawn at the
  larger of the two, bottom-justified, matching `docs/COMPATIBILITY.md`: a
  smaller ROM on a larger One ROM marks the hanging-out pins `overhang`; a larger
  ROM on a smaller One ROM marks the pins One ROM cannot reach `(empty)` and
  shows the `X1`/`X2` fly-lead each overhanging address line needs (e.g.
  `A12 → X1`). The board is inferred from a connected One ROM when omitted, and
  the same views are available for the connected device as `onerom inspect
  header` and `onerom inspect socket`.
- Fix `onerom image swap-bytes` panicking at startup (even for `--help`) with a
  clap short-option collision: `-i` was claimed by both the global `--vid-pid`
  and swap-bytes' `--input`. `--input`/`--output` are now long-only (aliases
  `--in`/`--out` unchanged). Added a `verify_cli` test that runs clap's
  `debug_assert()` over the whole command tree to catch such collisions in CI.
- Add `onerom image convert --from <fmt> --to <fmt> --input <file> --output <file>
  [--load-address <addr>]`, converting ROM images between `binary` and `ihex`
  (Intel HEX). The format set is designed to accept further formats later.
- Add `format` and `load_address` keys to `--slot` for Intel HEX ROM images:
  `--slot file=rom.hex,type=2364,cs1=active_low,format=ihex[,load_address=$E000]`.
  `format` accepts `binary` (default) or `ihex`; `load_address` (only valid with
  `format=ihex`) accepts a decimal or `0x`/`$`-prefixed hex address that maps to
  byte 0 of the ROM.  The same keys are available in config files.
- Support devices with overridden serials
- Allow `--plugin` to be combined with `--config-file` on `program` and
  `firmware build`; the plugins are inserted ahead of the config's ROM slots.
  Errors if the config already defines a plugin of its own.
- The ROM type is now stored in metadata using the exact spelling the user
  entered, on both the `--config-file` (`"type"`) and `--slot type=...` paths,
  instead of a canonicalised name (e.g. `27SF512` is retained rather than
  normalised to `27512`).  The resolved type still drives all behaviour.

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
