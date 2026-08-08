# Changelog

All notables changes between versions are documented in this file.

## v0.7.1 - 2026-08-09

Headline changes in this release:
- Intel HEX ROM input in the programming tools, ihex<->binary conversion in the CLI, and image transforms (byte swap, deinterleave) applied during a build.
- Two v0.7.0 regressions fixed: the status LED defaulting to off, and valid multi-ROM sets rejected depending on chip order.
- Image select jumpers B and D fixed on fire-28-c, fire-28-d and fire-32-b, which read the SWDIO select jumper inverted.
- RBCP (the host-control plugin) works again — the address monitor it builds on was completely broken by the v0.7.0 PIO rewrite — and now supports every ROM type the firmware serves, including 32-pin, 40-pin 16-bit and the 23QL384.
- GPIO control end to end: plugins, the USB plugin and the CLI can drive a One ROM GPIO, and `onerom control reset` can reset the host system One ROM is installed in.
- New CLI views of a board's pin header and ROM socket, and of the flash each chip type costs to emulate.
- One ROM Lens working again, and ported to a Rust/wasm crate.
- CLI `--serial-override` to set a custom serial number, exposed via the USB plugin.
- New plugin-API getters for device GPIOs and the exact per-ROM type string.
- **Breaking:** `onerom boards` is now `onerom board list`, the CLI now refuses Ice (STM32) boards, which it never supported, and `--allow-unsupported-chip-type` is gone.
- **Breaking:** the CLI's argument conventions are now consistent — no positional arguments, and each short flag means one thing across every command.

In detail:

- Add Intel HEX (ihex) as a ROM image input format.  A chip may set `"format": "ihex"` in a config file (the default remains raw binary), with an optional `"load_address"` — decimal, or `0x`/`$`-prefixed hex — giving the absolute ihex address that maps to byte 0 of the ROM; the CLI exposes the same via `--slot format=ihex,load_address=...`.  Unwritten bytes read as `0xFF`.  Decoding lives in `onerom-gen`, so the CLI, Studio and Web all get it.
- Add image transforms — byte-level rearrangements applied to a ROM image before it is written into the firmware.  A chip may carry an ordered `"transform"` array, exposed by the CLI as `--slot transform=deinterleave:1/2/2+swap_bytes`.  `swap_bytes` reverses the byte order within each 16-bit word; `deinterleave:<offset>/<stride>[/<bytes>]` extracts one lane from an interleaved image, for a wide ROM set distributed as a single file.  The list is applied in the order given, which matters, and is recorded in the firmware metadata alongside the filename.
- Add `onerom image deinterleave`, the standalone counterpart to `onerom image swap-bytes` (`--offset`, `--stride`, `--bytes`), for rewriting a file rather than transforming it during a build.  Both share the `onerom-gen` implementation used by `transform=`.  Neither reads Intel HEX; use `onerom image convert` first.
- Add `onerom image convert` to convert ROM image files between raw binary and Intel HEX (`--from`/`--to`, with `--load-address` for the ihex side).
- Add GPIO control, end to end.  `onerom control pin` drives a One ROM GPIO high, low or high impedance, optionally for a bounded period timed by the device (`--hold`, `--then`); `onerom control reset` pulses one low to reset the host system One ROM is installed in; `onerom inspect gpio` lists every GPIO with what One ROM is using it for.  `--pin` takes `gpio<N>` or a header pad name (`sel_a` to `sel_e`, `x1`/`x2`).  A GPIO One ROM is itself using is refused unless `--force`, and one that is not 5V-tolerant warns.  The primary use is a wire from a header pad to the machine's reset line, so a script can program an image and then reset the machine into it.  Plugins get the same primitives as `ORA_ID_GPIO_SET` and `ORA_ID_GPIO_QUERY`.  Requires firmware v0.7.1 and the v0.2.1 USB system plugin.
  - This required a firmware update and a USB plugin update.
- Add CLI ASCII views of a board's physical pin layouts.  `onerom board header [--board <board>]` draws the pin (jumper / programming) header pad by pad, with the GPIO behind each image-select and X pad and, on Fire boards, whether that GPIO is 5V-tolerant.  `onerom board socket [--board <board>] [--chip-type <chip>] [--gpio]` draws the ROM socket as a DIP pinout — GPIOs, the chip's pin functions, or both.  Where the chip's pin count differs from the board's, the socket is drawn at the larger with the smaller device bottom-justified, marking `overhang` and `(empty)` pins and the `X1`/`X2` fly-lead each overhanging address line needs.  The board is inferred from a connected One ROM when omitted; `onerom inspect header` and `onerom inspect socket` are the device-side forms, and take `--board` to override what the connected One ROM reports.
- `onerom chips --board <board>` now reports the flash each chip type costs to emulate, which is frequently larger than the chip — a 2364 costs 8KB on a 24-pin board but 256KB overhanging a 28-pin one — grouped by how the chip fits the socket, with `--chip-type <chip>` for a single type.  `board socket --chip-type` and `inspect socket --chip-type` report the same figure.  The listing now covers every chip the board can emulate, and a recognised type it cannot serve is named separately rather than listed as supported.  `docs/COMPATIBILITY.md` and `docs/CLI-MANUAL.md` now legend the Fit column.
- `onerom-gen` gains `compat::serving_alg_info`, reporting the serving algorithms and the GPIO window the address state machine samples for a chip on a board, derived from configuration alone.
- Add an optional per-board `jumper_header` descriptor to the board metadata, describing the 2xN pin header column by column with each pad's role, so host tools can draw an accurate diagram per board revision instead of assuming one fixed layout.  `onerom-config` exposes it as `Board::jumper_header()`, and it is cross-checked at build time against the board's electrical data so the two cannot drift apart.  The Fire 24, 28 and 32-pin boards are characterised; Fire 40 and Ice are additive JSON to follow.
- Add `--serial-override` to set a custom USB serial number, used by the USB plugin while One ROM is running.  A stopped One ROM continues to use the chip ID, the USB stack then being the RP2350 bootrom's; Studio copes with the serial changing across stopped/started, and Web works with either.  Neither Studio nor Web can program an override.
  - This required a firmware update and a USB plugin update.
- Turbo boot with more than one non-plugin ROM slot is now a warning under the CLI's `--force`, rather than a hard error.  Only the first slot is served at boot, which is what you want when it holds a bootloader that selects the others itself.
- The CLI now accepts `--plugin` alongside `--config-file` on `program` and `firmware build`; the plugins are inserted ahead of the config's ROM slots, so a plugin can be added to a stock config without editing it.  It is an error if the config already defines a plugin of its own.
- `--slot` now accepts every chip type the target firmware can serve on the board, so the overhang and fly-lead combinations `docs/COMPATIBILITY.md` documents — a 2764 on a Fire 24, say — no longer have to be built from a config file.  **Breaking:** `--allow-unsupported-chip-type` is removed, having nothing left to permit.
- **Breaking change: `onerom boards` is now `onerom board list`.**  There is deliberately no alias, so a script calling `onerom boards` must be updated — the CLI suggests `board` rather than simply failing.  A plural noun taking a singular argument read wrongly, and with three subcommands beneath it the listing deserved a name of its own.
- Ice (STM32) boards are now listed separately by `onerom board list`, and refused where the CLI cannot use them.  The CLI has never had an STM32 path — every firmware path composes an RP2350 image and every device path speaks picoboot — but a single merged list implied otherwise, and `--board ice-24-d` then failed several layers down as a missing release.  `scan`, `program`, `firmware build`, `firmware download`, `firmware inspect --board`, `control pin`, `control reset` and `inspect gpio` now reject an Ice `--board` up front; the commands that only *describe* hardware still accept them.
- `onerom inspect gpio` now shows a single `Function` column listing everything a GPIO is, in place of separate `Pad` and `Function` columns.  It names every function where a GPIO carries more than one (on a fire-24-f the status LED and the RGB LED are both GPIO 29, and only the first was listed), no longer claims a GPIO is `SWCLK` or `SWDIO`, and by default lists only the GPIOs connected to something, with `--all` for the rest.  The explanatory legend moves behind `--verbose`.
- `onerom board socket` and `onerom inspect socket` now say when a board has no GPIO map, instead of drawing the diagram with the GPIO column blank all the way down; `board header` and `inspect header` say `command unsupported` for a board with no pin-header descriptor.
- **Breaking: the CLI's argument conventions are now consistent across every command.**  No command takes a positional argument — `board header`/`board socket` take `--board` — and each short flag means one thing CLI-wide: `-b` is `--board` (no longer `--byte`), `-o` is `--output` (no longer `--offset`), `-i` is `--input` (no longer `--vid-pid`, which keeps `--id`), `-l` is `--length` (no longer `--slot`), and `-m` is `--msd` (no longer `--image`).  `--board`, `--chip-type`, `--all`, `--force`, `--no-reboot` and `--input`/`--output` gain their short forms on the commands that lacked them.
- **Breaking: `onerom control erase` now uses `--stopped`/`--running`** for its post-erase reboot mode, matching `reboot` and `program`.  As with `onerom boards`, there is deliberately no alias for the old `--reboot-stopped`/`--reboot-running`.
- `--config` is now the primary spelling of the ROM configuration file option on `program` and `firmware build`, matching how it is written everywhere else; `--config-file`, `--config-json` and `--json` remain aliases.
- `onerom image convert` now validates `--from`/`--to` as the command line is parsed, listing the accepted formats in `--help` rather than failing part-way through a conversion.  `--load-address` is likewise parsed up front, by the same code the config file uses.
- `--slot` keys and values are now documented kebab-case — `size-handling`, `load-address`, `force-16-bit`, `cs1=active-low` — matching the CLI's own argument naming; the snake_case config spellings are all still accepted, and `size-handling` (the one key that only took the snake form) now parses.
- `--slot` now accepts `cs<n>=ignore`, which a config file always could.  Chip-select values are parsed by the same `onerom-gen` code the config file uses, in place of a second, narrower copy in the CLI: previously `active_low` worked only on the command line and `ignore` only in a config file, and neither parser accepted the full set.
- Stop hard-wrapping prose in the CLI's console output.  A handful of messages broke a sentence at a fixed width, which the terminal then wrapped again at its own.
- Fix the status LED defaulting to off on v0.7.0.  It now defaults to on, and is only turned off by a per-slot firmware override that explicitly disables it.  Also fixes the limp-mode LED force-enable, which never fired, so limp-mode error blink patterns did not show when the LED was overridden off.
  - This required a firmware update.
- Fix a v0.7.0 regression that rejected valid multi-ROM sets depending on the order of their chips.  A C64 set of Kernal (2364), Character (2332, whose CS2 is set to `ignore`) and Basic (2364) validated with the character ROM last, but was rejected with it in the middle.  The consistency check now anchors on the primary chip and validates each secondary independently, so it accepts every ordering the firmware can serve.  It also now rejects, with clearer messages, multi sets the firmware could not serve.
- Fix a v0.7.0 regression that left the address monitor — and therefore RBCP and the host-control plugin — completely non-functional.  The v0.7.0 PIO rewrite moved the monitor's state machines into the ROM-serving PIO blocks, but the capture DMA, the CS→address-read IRQ handshake and the state-machine enable were all left targeting the old, now-unused block, so no address capture ever reached the ring buffer and no knock was ever detected.  All three now follow the runtime serving-block assignment.
  - This required a firmware update.
- Extend the address monitor to every ROM type the firmware serves: 32-pin ROMs, whose address pins sit in a different GPIO bank from CS and data; 40-pin 16-bit ROMs in both `/BYTE` modes (#277); and the 23QL384's qualifier-based chip-select (`ALG_CS_2`, #278), the one ROM type that folds address lines into its select decision.  For the 23QL384 a host must keep its command signalling inside a range the ROM actually serves — below the top quarter of its address space.  The host-control README's list of unsupported types is now empty.
  - This required a firmware update.  No plugin update is needed: an existing host-control binary picks the support up from the firmware.
- Add an observed (bus) address space to the plugin API and route RBCP command decode through it.  On a 40-pin part the device does not observe the ROM's least-significant address line, so command signalling occupies a narrower address space than the byte-addressed image; decoding in byte space corrupted the command on a 16-bit ROM.  Two additive getters expose this — `ORA_ID_DEMANGLE_OBSERVED_ADDR` and `ORA_ID_GET_UNOBSERVED_ADDR_BITS` — and the RBCP spec (now v0.1.1) gains a matching "Address Line Presentation" clarification.
  - This required a firmware update and a host-control plugin update.
- Report as many RAM slots as the RAM holds, rather than at most seven, and let the host-control plugin keep those a host cannot name for its own use.  A slot is exactly one ROM region, so a small ROM now yields many small slots where the old cap of seven was meant to guarantee 64KB ones and could not — a slot has to be the size of the ROM being served.  The plugin advertises at most 170 of them, since every RBCP command that names a slot rejects 0xAA, and stages NV write transactions in the ones above that.  This makes NV storage genuinely writable on devices serving a ROM smaller than the 4KB staging buffer, where `GET_NV_CAPABILITY` previously claimed writable but every transaction failed.
  - **Potentially breaking.**  A host that assumed a RAM slot was large enough for a purpose of its own, or that there were at most seven, sees something different.
  - This required a firmware update and a host-control plugin update.
- Fix two further RBCP conformance defects in the host-control plugin.  Group 0x01 Read commands received in command mode were executed, writing their answer into a back-channel region the device had stopped maintaining and so modifying the served ROM image outside any session; Group 0x01 and Group 0x03 now consume the command's argument bytes and discard it.  And `ENTER_CMD_RESP` asking for a back-channel larger than the RAM slot was discarded silently where the specification requires a reported failure, which a host distinguishes by whether the token increments.
  - This required a host-control plugin update.
- Fix the host-control plugin writing to the RBCP response header for the three commands the specification requires to update nothing: `RBCP_RESET`, `EXIT_CMD_RESP_SILENT` and `SWITCH_AND_EXIT`.  All three ran the first half of the command processing sequence before being recognised as silent, so a host that polled the back-channel after one of them saw the token change and then waited for a completion that never came.
  - This required a host-control plugin update.
- Add the plugin metadata getter `ORA_ID_GET_METADATA_UINT`, and expose device GPIOs (status LED, RGB LED, VBUS, SWD, ext-flash CS), the live status-LED state and the RP235x variant to plugins over the existing metadata-key mechanism.  `ora_set_status_led` now drives the LED even when it was configured off, and records the state as the coordination channel plugins read; CPU fault handlers force it on so faults stay visible.  The RGB plugin now discovers its GPIOs at runtime instead of hard-coding them, and reflects the status-LED state on boards where the two share a GPIO.
  - This required a firmware update and an RGB plugin update.
- Implement the previously-reserved `ORA_ID_GET_FLASH_SLOT_EXT_INFO` as a per-ROM getter, returning the ROM type string exactly as the user entered it (e.g. `27LC512`, not the canonical `27512`) plus the filename, chip size and RBCP type.  Previously the ROM type was only reachable as its numeric RBCP code.  Additive; the `api.h` version is unchanged.
  - This required a firmware update.
- The ROM type stored in a firmware's metadata now preserves the exact string the user entered rather than a canonicalised name, on both the config-file `"type"` and CLI `--slot type=...` paths.  The resolved chip type continues to drive all behaviour; only the human-readable metadata string changed.
- Give the 23C1010 mask ROM its own RBCP chip type (`0x24`); it previously shared `0x0F` with the electrically-equivalent 27C010, which the v0.7.0+ generator no longer requires.  Changes the metadata emitted for 23C1010 images.  The RBCP spec is updated to match, and also gains the 23C1001, 27C200, HM7641 and 62256 types.
- Add the `62256` chip type — 32KB static RAM in a 28-pin DIP — as a recognised type, but **not yet supported for serving**: it shows as ✗ in `docs/CHIP-TYPES.md` and `onerom-gen` rejects any config using it.  This reserves its name, pinout and RBCP chip type ahead of SRAM serving support returning to the v0.7.x firmware.
- Recognise `9316A` as an alias of the `2316` mask ROM, alongside the existing `9316`, so an Apple II/II+ ROM stamped `9316A` resolves without editing the config.
- Fix the `sel_jumper_pull` bitfield in the fire-28-c, fire-28-d and fire-32-b hardware configs, which broke image select jumpers B and D.  The high bit sat on a grounded pin instead of the SWDIO image-select pin, which is jumpered to the RUN line and so pulled high, and the jumper was therefore read inverted on those boards.  This changes the generated firmware for those three boards.
- Fix `swd_enabled = false` having no effect on v0.7.0, where it was stored and reported but never acted on.  SWD now stays up for the whole of boot and is shut off just before serving, keeping debug port SRAM accesses off the serving DMAs.  Boot logging may now be combined with it, and stops when SWD does.  Not a debug lockout — BOOTSEL/PICOBOOT are unaffected.
  - This required a firmware update.
- Fix restoring an image select pin shared with SWD resetting the control register of the wrong GPIO, on boards where a select jumper sits on SWCLK or SWDIO.
  - This required a firmware update.
- Fix `onerom firmware build` accepting `--swd_disabled` where `program` accepted `--swd_disable`, so each underscore spelling worked on only one subcommand.  `--disable-swd` and `--swd-disable` work on both.
- Fix `size_handling: pad` filling the tail of an **Intel HEX** image with `0xAA` instead of the documented `0xFF`, putting two different fill values in one image.  Raw binary images are unaffected.
- Fix `onerom-gen`'s v2 (v0.7.0+) builder not checking that the composed ROM data fits the target board's flash.  The CLI caught an oversized config downstream in `onerom-fw`, but Studio and the web programmer do not call that path, so the check now lives in the builder itself where every consumer gets it.
- Fix `onerom image swap-bytes` panicking at startup, even on `--help`, from a clap short-option collision: `-i` was claimed by both the global `--vid-pid` and swap-bytes' `--input`.  `--input`/`--output` are now long-only (aliases `--in`/`--out` unchanged).
- Fix building a chip type only v0.7.0+ firmware serves — a `23C1001` or `HM7641` — against an older firmware reporting "This tool does not support chip type", when the tool supports it perfectly well and only the firmware is too old.  It now names the firmware version required, as every other version-gated feature does.
- Fix `onerom-lab` applying the 8-bit read timing to the 27C200.  The longer read delay and tristate settle belong to every 16-bit-capable part, but the test was keyed on the 27C400 specifically.  Affects hardware testing only.
- One ROM Lens works again, and has been ported from the old C-to-WebAssembly shim to a Rust crate (`onerom-lens`, in `rust/lens`) built on `onerom-fw-emulator` — it was untested and broken in v0.7.0.  It runs the real firmware PIO/DMA serving code, visualising address/data/control-line waveforms live in the browser, and handles 8-bit ROMs and the 27C400 in both byte and 16-bit word modes.  Build and serve it with `rust/lens/serve.sh [CONFIG] [BOARD]`; the WebAssembly build is now covered by CI.  The old `firmware/lens/` C shim and `firmware/lens.mk` are removed.  The shipped firmware is unchanged.
- Improve One ROM Lens waveform readability: distinct green HIGH and cyan LOW levels where both were previously drawn the same, neutral-grey High-Z and transition edges, held address and data values annotated with their duration in cycles and in nanoseconds derived from the firmware's real SYSCLK, cycle graduation ticks once zoomed in far enough, and a compact per-bit cursor readout with a stacked summary that follows the pointer.
- Fix the One ROM Lens signal-label hover tooltip showing `GPIO [object Object]` instead of the GPIO number.
- `onerom-gen`'s public config model — `Config`, `ChipConfig`, `ChipSetConfig`, `Location`, `License`, `FileSpec`, `FileData` — and its `Error` enum are now `#[non_exhaustive]`, so future field and variant additions are backwards-compatible for downstream crates.  This is itself a breaking change (external code must use the new `new()` constructors rather than struct literals, and a `match` on `Error` must carry a wildcard arm), hence the `onerom-gen` minor version bump.
- `onerom_config::chip::ChipType` is now `#[non_exhaustive]`, so future chip-type additions are backwards-compatible.  This is likewise a breaking change for external `match` expressions, hence the `onerom-config` minor version bump.
- `onerom-gen` is now built as a genuine `no_std` (+`alloc`) crate, matching its intended use in embedded and WASM contexts; the `#![no_std]` attribute was previously commented out.  Backwards-compatible for `std` consumers.
- Fix `docs/COMPATIBILITY.md` and `onerom chips` reporting compatibility for a chip-select configuration users cannot ask for.  Both checked each chip with only its primary select monitored, which needs the `allow_cs_ignore` config option and is rejected outright by `--slot`; they now check the configuration the tools actually produce, with every control line monitored.  HM7641 gains fire-24-a/b, the 28-pin and fire-32-a boards, which One ROM has always served; 2316, 9316, 9316A, 2332, 4732 and 9332 are no longer listed for fire-28-b/c/d, nor 23128 for fire-32-a/b, which One ROM has never been able to serve.
- **Breaking:** `onerom-gen`'s `check_chip_on_board` becomes `check_chip_set_on_board`, taking a chip-set type, chip count and CS configuration, so banked and multi sets can be checked as well as single chips; `supported_chips` takes the set shape too.  `CompatResult` gains the GPIOs whose ROM table bits address nothing, and it and `ChipCompat` are now `#[non_exhaustive]`.
- Removed `test-retired`, the old test mechanism, superseded by `onerom-fw-tester`.
- `onerom-protocol` is deprecated and `onerom-database` is unmaintained, and both now say so on crates.io.  `onerom-protocol` served the original STM32F4 One ROM Lab, which the current Fire-based Lab replaced; it also carries a `#![deprecated]` attribute, so a downstream consumer is warned at compile time rather than only in the README.  Neither is used by anything in the tree.
- GitHub releases no longer attach a base firmware binary.  The firmware Web and CLI build from is published to [images.onerom.org](https://images.onerom.org) and downloaded automatically; the attached copy was separately built and could differ from it.

To publish:
- Rust crates (in dependency order):
  - onerom-database 0.1.2
  - onerom-config 0.6.0
  - onerom-protocol 0.1.1
  - onerom-metadata 0.1.4
  - onerom-gen 0.7.0
  - onerom-fw-parser 0.8.0
  - onerom-fw 0.2.0
  - onerom-app 0.2.0
  - onerom-cli 0.3.0
- Config schema
- CLI bin 0.3.0
- Studio 0.2.1
- USB plugin 0.2.1
- RGB plugin 0.1.2
- host-control plugin 0.1.2
- RBCP spec 0.1.1
- WASM 0.5.0
- Site
- Release 0.7.1

## v0.7.0 - 2026-07-20

This release includes a significant rewrite of the One ROM firmware focusing on Fire boards.

The primary benefit of this firmware in the immediate term is more efficient use of flash for specific ROM types - in many cases, with the later board types, the amount of flash used to store a ROM image is the same as the ROM image size itself, unlike previous releases.

There is also a longer term benefit of reduced maintainability costs and also lower costs to add new ROM types in the future - meaning a better user experience.

Ice boards are capped at firmware v0.6.xx (and are not supported in v0.7.0+).  The programming tools continue to support Ice boards and v0.6.xx.

TODO
- Figure out why status LED is off on v0.7.0

TO TEST
- All ROM types live
- Host control plugin

Retired:
- `lab` (and replaced by `onerom-lab`)
- `sdrr-check` (superceded by `onerom-fw-tester`)
- `sdrr-info` (superceded by the CLI `onerom firmware inspect` command)
- `sdrr-tester` (superceded by `onerom-lab`)
- `test` (superceded by `onerom-fw-tester`)
- Silent replacement of SST39SF040 with 27C040 for fire-32-a.  Decided it was best to flag this isn't natively supported.

New:
- `onerom-app` crate, containing functionality shared between One ROM user facing apps, like CLI, Studio and the Web UI (via onerom-wasm).

Updated:
- All Rust crates and programming tools
- plugins/system/usb
- plugins/user/host-control
- CI to perform comprehensive testing of the firmware, for all ROM types, and dynamically banked and multi-ROM sets, and PIO focused plugin API functions

Limitations:
- SRAM support (6116) is not currently supported by the v0.7.xx firmware train.  This limitation is expected to be lifted in future.
- One ROM Lens has not been tested with this release, and is likely broken.

## 2026-07-04

Release hardware design files for new variants:
- fire-24-f
- fire-28-c
- fire-32-b2
- fire-40-b

All of these new hardware designs are licensed under the [CERN Open Hardware Licence Version 2 - Weakly Reciprocal (CERN-OHL-W-2.0)](/LICENSE.md#cern-ohl-w-20-license).

All previous hardware designs are re-licensed under the [CERN Open Hardware Licence Version 2 - Weakly Reciprocal (CERN-OHL-W-2.0)](/LICENSE.md#cern-ohl-w-20-license).

## v0.6.14 - 2026-07-02

- Added support for fire-40-b and fire-24-f.
- Improved lab-new scripts.

## v0.6.13 - 2026-06-02

Added:
- Enhance lab-new to allow reading of any supported ROM type and single build supporting all One ROM sizes.
- HN62402 (128KBx16/256x8) support for fire-40-a.  Uses 512KB on flash.
- Support for prototypes fire-28-c and fire-32-b.
- SST39SF040 support (fire-32-b only).  Uses 512KB on flash.
- Firmware decoding support from

Fixed:
- 23C1010 support - there were failures when creating firmware with 23C1010.

## v0.6.12 - 2026-05-26

Added
- 27C100 as synonym of 27C301/27C1000.
- 39SF0x0 and 29x0x0 as synonyms of SST39SF0x0.
- Re-added 23QL384 as a new composite ROM type.  This serves both a 23256 and a 23128 ROM, as used in the Sinclair QL, with a single configured CS line (use active high for the Sinclair QL) and it is de-selected when A14 & A15 are both high.  It is required, in addition to the 23QL512, where other peripherals supply the top 16KB of the ROM space.  This ROm type may be useful for other systems.
- More aliases of th 27C400 ROM type.  Also corrected HN62404/24 as aliases of the 27C400, not the 27C040.
- More aliases of 2364 ROM type.

## v0.6.11 - 2026-05-14

Changed:
- Type 23QL384 ROM type has been replaced with 23QL512 (48KB->64KB) as the Sinclair QL supports up to 64KB ROMs.  When flashing a 48KB ROM (for example a Minerva ROM), use the additional slot option `size=pad` to pad a 48KB ROM to a 64KB one. 

## v0.6.10 - 2026-05-12

Added:
- Support _protoype_ support for a new composite ROM type, the 23QL384.  This serves both a 23256 and a 23128 ROM, as used in the Sinclair QL, with a single configured CS line (use active high for the Sinclair QL) and it is de-selected when A14 & A15 are both high.  It may be useful for other systems.  This ROM type may be deprecated or modified in a future release.

Fixed:
- Added longer filtering/debounce support for knock detection, to work more reliably on different Commodore 64s.

## v0.6.9 - 2026-05-08

This release introduces support for the [ROM Bus Control Protocol](https://github.com/piersfinlayson/rom-bus-control-protocol) (RBCP), which allows retro systems to interact with and control One ROM directly.  This allows advanced functionality driven by the retro system, such as

- ROM based bootloaders (think `grub` for the C64)
- Dynamic ROM patching for games, demos and other applications
- Remote debugging of code running on real retro systems

This support is implemented using the new `user/host-control` plugin, which is an implementation of v0.1.0 of the ROM Bus Control Protocol.

This version of the plugin and core firmware focuses on enabling RBCP for 2364 ROMs, including 2364/2364/2332 multi-ROM sets, like those used in the C64 to serve all of basic, kernal and character ROMs from a single One ROM 24.  Support for other ROM types and configurations is expected in future releases.

In order to support this functionality, this release also includes a number of other changes, including a substantial number of new plugin API functions.  Noteworthy additions include functions to:
- collect addresses read by the host and detect "knocks" (a specific sequence of address reads) on behalf of the plugin.
- populate and switch to additional backup RAM "slots" allowing dynamic, atomic switching between ROM images.
- allow co-operation between plugins, to allow one plugin to suspend the other, in order to perform operations that might interfere with the other plugin's operation, such as flash erasing/writing.

It is believed that no non-backwards compatible changes to the plugin API have been introduced in this release, so plugins developed to older versions of the firmware should still work.  This includes the system/usb plugin.  However, there remain no guarantees of backwards compatibility for the plugin API in future releases, so if you are developing a plugin, please keep an eye on this changelog for any changes that might impact your plugin.

Also added:
- 2364, 2732, 2716, 2708 and 2704 support on One ROM 28 boards.  Important notes:
  - One ROM 28's pin 28 or the 5V header pin MUST be supplied 5V when using to emulate these 24 pin ROM types.  Failure to do this is likely to damage One ROM.
  - If your system expects a 24 pin ROM of one of these types and provides a 28 pin socket, you can probably install a 28 pin One ROM as is.  Double check pin 28 supplies 5V.
  - If you want to use a One ROM 28 in a 24 pin socket as one of these ROM types, you must install One ROM 28's pins 3-26 in that socket, with pin 3 of One ROM in the socket's pin 1 (i.e. install the "bottom" of the One ROM in the 24 pin socket, leaving the top two rows of pins overhanging the top of the socket).  You MUST supply pin 28 with 5V, either via pin 28, or via the 5V header pin.  Failure to do this is likely to damage your One ROM.
  - When emulating a 2704 or 2708 you MUST ensure that any -5V, -12V or +12V (or any voltages other that +5V) are NOT supplied to One ROM.  This can be achieved by cutting traces on the system's PCB, or not populating (or de-populating) the appropriate One ROM pins.
  - This support was tested as follows:
    - Both test and test-pio scenarios for all these ROM types have been added to CI and pass.
    - 2364 support was tested in a C64 breadbin as the kernal ROM.
    - 2732/2716 were tested in a T48 EPROM reader (with Pin Detect disabled).
    - 2708/2704 are untested, as the T48 doesn't support these types.  The 2704/2708 support is very similar to the 2716 support, so is expected to work, but please report any issues if you try it.

## v0.6.8 - 2026-04-02

- One ROM Fire - Add read support for 28C16 (24), 28C64 (28), 28C256 (28) and 28C512 (32) EEPROMs (#169).  These have all been live tested.

- Re-instate 27C080 support, supported by the 32 pin One ROM.  This only supports half the total 1MB image with a single One ROM, but two can be stacked and configured high/low to support the full 1MB.  This support has been live tested.

- Added support for 23C1010 mask programmed ROM (which is effectively a 27C010).

- Prevent CPU serve mode from being selected for One ROM 28/32/40 (as they only support PIO serving).  It can still be configured on One ROM Fire 24, but not any other Fire version.  This is mainly useful for fire-24-a and fire-24-b which don't support PIO serving for multi-ROM sets.

- Moved PCB files to hardware/pcb.

- Uploaded STL files for 3D printable One ROM cases.

## v0.6.7 - 2026-03-26

The three headlines in this release are **prototype** support for:

- One ROM CLI - a command line tool for interacting with new and old One ROM Fire devices over USB
- a "live" USB stack running the picoboot protocol live while the One ROM is serving ROM bytes
- One ROM plugins, used to extend One ROM's core functionality (and is how the new USB support is implemented)

Feedback is welcome on any of these features, and anything else One ROM does, or could do via GitHub discussions and issues!

### One ROM CLI

The [One ROM CLI](https://onerom.org/cli) is a command line tool for managing One ROM Fire devices over USB, and it aims to combine power and ease of use.

It supports all One ROM Fire devices over USB, including those using the new live USB stack, and those that predated this support.

Supported operations include:
- Scanning for all connected One ROM Fires over USB
- Building One ROM firmware
- Programming One ROM
- Inspecting One ROMs and firmware files
- Read/write access to One ROM's flash and RAM over USB
- Live reading and writing of the ROM One ROM is currently serving ("live" USB plugin only)

### Live USB Stack

This release adds **prototype** USB support **while One ROM is serving bytes**.  It no longer drops into BOOTSEL/programming mode when USB is connected.  The USB stack primarily exposes a vendor class interface, implementing Raspberry Pi's PICOBOOT protocol, the same protocol used by picotool to manage and control an RP2350 in BOOTSEL mode.

This allows you to use the following tools to perform RAM and flash operations on One ROM while One ROM is serving bytes:

- [One ROM CLI](https://onerom.org/cli) - Comprehensive command line tool for managing One ROM Fire devices over USB.
- `picotool` - Raspberry Pi's command line tool for managing RP2350 devices in BOOTSEL mode.  It can be used to read and write flash and RAM, and to reset the device.
- [pico⚡flash](https://picoflash.org) - a WebUSB PICOBOOT implementation offering the same primary primitives as picotool.

Some notes on this support:

- The plugin must be explicitly included in the config.  You can do this with a JSON config file One ROM Studio and the CLI, or, most easily with the `--plugin usb` flag when using the CLI to build firmware or program One ROM.

- There is currently no guarantee that the USB interface will be stable or remain the same in future releases.  In fact, the USB stack, as a plugin, has its own versioning and release train, may change VID/PID (the current value of 1209:f542 has not yet been approved by pid.codes).

- SRAM is directly available via PICOBOOT at 0x2000_0000.  The ROM image being served is located at this address.  However, both address bytes and data bits are mangled for ROM serving.  A "logical" image of the ROM being served is available for reading **and writing** live at 0x9000_0000.  For example, for an 8KB 2364 ROM, the actualy ROM image byte 0 is available at 0x9000_0000, byte 1 at 0x9000_0001, etc.  This allows reading **and changing** the ROM image almost instantly at runtime.  Bytes are updated in the ROM image being served in the order they come in over the PICOBOOT protocol.

  - `picotool` has a limitation that it does not support reading/writing addresses that it thinks are invalid, such as 0x9000_0000.  You will probably need to use picoflash for the time being if you don't want to use One ROM tools.

- Flash erase is **not** supported in this release.  While the ROM byte serving is entirely autonomous, erasing the flash that the plugins execute from would cause the MCU cores to fault, and break the USB stack.  Some thought needs to be applied before implementing live flash erase (and hence flash write).

- Reprogramming the image(s) stored on flash is currently only supported as part of a full firmware update, which causes One ROM to stop serving bytes while it is updated.  Updatig stored ROM images on flash is expected in a future release.

- The USB stack includes a CDC interface, which is currently unused, but may be used in the future for debugging, logging and other purposes.

- More USB functionality is expected in future releases - including live flash erase/writing, controling the image select pins (for example, to drive external devices).

### Plugins

This release adds **prototype** support for One ROM plugins, which are custom binary modules, separately built and then added to One ROM's configuration.  One ROM executes the plugins once ROM serving has been started.

Plugins can provide a massive variety of functionality and are hugely flexible - as they are effectively full microprocessor firmware in their own right.

While the RAM available to plugins varies between copious and extremely limited depending on the One ROM model, a plugin gets a full RP2350 core at its disposal, clocked at the current RP2350 clock speed, and full hardware access.  With great power comes great responsibility, and plugins can interfere with ROM serving if they do not avoid operations that might conflict - in particularly PIO/DMA operations and GPIO usage.

There is currently **no** guarantee of the [One ROM plugin API](sdrr/ora/api.h) remaining backwards compatible in any future One ROM release.  However, the general plugin concept is expected to be here to stay.

Limited stack space and static RAM is available to each plugin type - each core is allocated 1KB of stack, including the stack used to launch the plugin.  The system stack gets static RAM in addition to this whereas the user plugin has to allocate static RAM out of its 1KB stack.  There is no policing of the stack, nor any other sandboxes of plugins.

The only supported IRQs for plugins in this release are TIMER0_IRQ_0 and USBCTRL_IRQ, both of which are used by the USB system plugin.

A future concept of PIO plugin is under consideration, which would allow an optional third plugin type which would replace the main firmware's loading of its PIO and DMA byte serving algorithm - allowing the plugin to substitue its own.  This could be combined with a custom user (or system) plugin with the two operating in tandem to provide some very powerful capabilities.

### Other Changes

- A new version of [One ROM Lab](rust/lab-new/README.md) has been started and is used to test One ROM Fire 40 boards prior to shipping.  This is expected to replace other testers soon, and completely supercede the old version of One ROM Lab over time.

- Introduced fire-24-eadb01 as a possibly temporary workaround for a single fire-24-e board, as featured on Adrian's Digital Basement.  Do not rely on this hardware version, as it may be removed without warning in future.

### Fixes

- Disabling the status LED using firmware overrides didn't work for CPU serving mode for fire-24-a and fire-24-usb-b.  This has been fixed.

## v0.6.6 - 2026-02-25

The headline for this release is "bug fixes and other improvements".

There are also a significant number of fixes and improvements to the remainder of the One ROM fold, including some changes to existing behaviour, listed first:

- Improved regular (not 16 bit forced) /BYTE serving algorithm for 40 pin ROM, #153.

  This algorithm is marginally slower than previously but more robust.  If a faster algorithm is required, sacrifice /BYTE low handling with firmware_overrides->fire->force_16_bit = true in the ROM config.

- Resolved deficiencies in fire-24-a and fire-24-b PIO serving modes, #94.  All function is now supported on these boards, except for dynamic bank switching, which will likely remain unimplemented on these boards due to the lack of contiguity between X1/X2 and the CS pins.

  As a result fire-24-a and fire-24-b now default to PIO serving.  If CPU is desired (for example, for dynamic bank switching support), use firmware_overrides in the JSON config file.

- Changed default HW_REV/MCU in Makefile to fire-24-e/rp2350 (from ice-24-f/f401rc).  This only impacts using the old style `make` builds, not `scripts/onerom.sh` builds.

- All supported ROM types and CS configurations are now fully PIO tested as part of automated (CI) regression testing, for all Fire hardware revisions, #149.

- Fixed support for 2 ROM multi-ROM sets, for Fire boards in PIO mode, #110.  This impacted all Fire revisions in PIO mode - in particular the provided 1541 multi-ROM config did not work (and now does). 

- Forced unused address lines to appear driven low to the MCU for appropriate ROM types, to fix #154.

  This probably doesn't have any external impact, but if so it would be to 27 series ROMs and/or 2 set multi-ROM sets (24 pin only).  All supported 27 series ROM types have been tested manually with this change, as has 2 ROM multi-ROM sets on 24 pin boards.

- Implemented 2704 and 2708 ROM support on all existing Ice and Fire 24 pin boards, #156.

  2704/2708 support was tested on a T48 EPROM reader (with Pin Detect disabled), as a 2716 (as the T48 software doesn't support 2704/2708).  The 2704 image was correctly duplicaetd 4 times in the 2716 space and the 2708 twice.  Support for 2704/2708/2716 and 2732 was tested on fire-24-a and fire-24-e, both with PIO serving, and ice-24-j.

- This releases adds _prototype_ support for One ROM 32 #131 including:
  - Hardware revision fire-32-a
  - EPROM types 27C010, 27C020, 27C040 and 27C301
  - Support for 27C080 is included allowing two physically stacked One ROMs to be configured, each to serve half of the 27C080.  The one serving the lower 512KB should have cs1 set active low and the other one should have cs1 active high in their respective ROM configs.
  This support has been tested using the PIO emulator, but has not been tested with real hardware.  It is expected that bug fixes will be required when testing is done with real hardware, and One ROM 32 will be formally supported in a later release.

## v0.6.5 - 2026-02-22

The big new feature in this release is support for One ROM 40, hardware revision fire-40-a.  This emulates a 27C400 found in the Amiga A500, and other 16-bit systems.  I have successfully tested it on my Amiga, serving Kickstart ROM 1.3 and [DiagROMV2](https://www.diagrom.com/).

One ROM Lens is also included.  This is a web-based tool that runs the real One ROM serving algorithm within your web browser using a cycle exact PIO emulator.  It emulates One ROM's operation, allowing you to inspecting the GPIOs on a cycle by cycle basis using the logic analyzer style interface.  This is a great tool for understanding how One ROM (or any parallel ROM) works, for debugging firmware issues and interatively testing algorithm changes. To use, there are two steps:

1. Build the One ROM firmware, using an "old-style" config, with the emulated hardware type and image you want One ROM to serve: e.g. `HW_REV=fire-40-a MCU=rp2350 CONFIG=old-config/test/40-random.mk make`.
2. Build and run One ROM Lens: `make -C sdrr -f lens.mk serve`.

    Then point a browser at http://localhost:8000/index.html.

    It is likely that simpler commands will be added to the future to make this easier.

This release also introduces extensive automated regression testing for the Fire PIO serving algorithm (#134), for 24 and 28 pin boards.  As a result, a number of Fire PIO serving bugs have been fixed, including #110 and #94.

## v0.6.4 - 2026-02-07

- Fixed [#133](https://github.com/piersfinlayson/one-rom/issues/133), a complex issue where Studio v0.10 and earlier fail to build 28 pin v0.6.3 firmware images correctly.  This stemmed from 0.6.3 incorrectly including a non-backwards compatible change in order to support 231024 ROM types. As part of this:
  - v0.6.4 reverts that non-backwards compatible change, but retains 231024 support.  Specifically non-231204 ROM images use 64KB on flash, with only 231024 requiring 256KB.
  - v0.6.4 also introduces checking that the ROM image type is as expected and enters limp mode if not - rather than pretending to be OK and serving garbage.
  - Studio has been enhanced so it can be configured at runtime (via the manifest) with a maximum firmware version it can support - which overrides onerom-gen's maximum version, if the version Studio is allowed to build is lower.  Had this been in place earlier it could have been used to prevent pre-v0.1.11 version of Studio building bugged v0.6.3 firmwares, when #133 was found.

## v0.6.3 - 2026-02-03

- Added 231024 mask programmed ROM support.  Used by BBC Master and Apple IIgs.  (Only tested on BBC Master.)
- Add some Studio keyboard shortcuts (r for rescan, f for flash).

## v0.6.2 - 2026-01-27

This release of One ROM introduces RAM support - for 6116 and 2016 2KB 24-pin static RAM chips, using Fire 24 C and onwards hardware revisions.

For a demonstation of RAM support, and an explanation of how it works, see: https://youtu.be/o7dMY6p6OJU

One ROM's JSON [config file format](onerom-config/README.md) now uses "chip_sets" instead of "rom_sets" and "chips" instead of "roms".  However, the old strings will continue to work for backwards compatibility.

Other changes:
- The ability to execute the main loop from RAM has been removed, to allow 512KB to be used for ROM RAM image on RP2350.  It would be possible to add this back where the image size to be held in RAM is under 512KB, and for Ice boards, but as executing from RAM is currently unused, it has been removed for now to save development and test effort.
- fire-24-e revision added as **unverified**.  This adds USB-C, and is intended for production quantities of Fire 24 - as it required JLC's standard assembly service to be used, rather than economic, due to the use of 0201 passives.  The MCU has also been changed to the RP235A, to take advantage of the on-board flash, saving on cost (and space).
- Rename directories `config/` -> `old-config` and `rom-config` -> `onerom-config`.
- The new recommended by to build One ROM firmware is using [`scripts/onerom.sh`](scripts/onerom.sh).  This builds an empty firmware image, adds the chip metadata, and adds the ROM/RAM images in one command.  It even flashes the firmware if desired.  Images are output to [`builds/fw`](builds/fw).

## v0.6.1 - 2026-01-22

### Added

- Prototype RAM support for 24 pin RAM chips, which use the same pin-out as the 6116/2016 RAM.  See PR #98 and discussion #70. Requires building from command line using make and EXTRA_C_FLAGS=-DONE_RAM
- Support for Fire boards (fire-24-d specifically) to use image select pins that are shared with SWD pins.  Note that this means that having an SWD programmer connected at boot will tend to change the image select value to an unexpected one, as the pins will be pulled by the programmer.  In v0.6.0 only 2 image select jumpers were supported on fire-24-d.
- Support for Ice boards (ice-24-j specifically) to use image select pins that are shared with SWD pins.  Note that this means that having an SWD programmer connected at boot may change the image select value to an unexpected one, as the pins will be pulled by the programmer.  In v0.6.0 only 2 image select jumpers were supported on ice-24-j.  Also, unlike the Fire boards, the ice-24-j (and similar) boards will disconnect from the debug probe during boot because of this function, so reconnecting will be necessary.
- Check in Studio/onerom-gen that firmware version is not too new (next major release) when building firmware images.
- Promote fire-24-d and ice-24-j to verified hardware revisions.

### Fixed

- Fixed #90 in v0.6.0 when older (pre v0.1.8) versions of One ROM Studio are used to build firmware images with more than one ROM set, One ROM will not boot on any ROM set other than ROM set 0.
- 2732 ROM type serving was broken - the top 2K replicated the bottom 2K.  Fixed (#103).  This included fixing the testing, which had also not caught this issue.

### Fixed

- 2732 ROM type serving was broken - the top 2K replicated the bottom 2K.  Fixed (#103).  This included fixing the testing, which had also not caught this issue.

## v0.6.0 - 2026-01-14

**EXPERIMENTAL RELEASE** - may contain bugs, use with caution.  If you hit problems, report them as issues and revert to v0.5.10 or earlier.  See [#87](https://github.com/piersfinlayson/one-rom/pull/87) for details of testing done.

This major release of the One ROM firmware adds support for low-level One ROM firmware configuration from the ROM config files, allowing per-ROM image configuration of processor clock speed, status LED behaviour, and other low-level capabilities from One ROM Studio, and other tools.

As a major firmware release, there are some non-backwards compatible changes in this release:

- The ROM config JSON schema has changed - `sel_jumper_pull` is now a Vec<u8>, to allow per-pin SEL jumper pull direction.
- The default PIO configuration is now SLOW_CHAR_CHAR, to improve ROM serving compatibility.  PIO configuration can be overriden on a per ROM set basis using `serve_alg_params` firmware overrides.

### Added

- Support for per-ROM image low-level configuration, including processor clock speed, status LED behaviour, and other low-level capabilities.  See the ROM Config [README](rom-config/README.md) for details.
- Support for fire-24-d and ice-24-j hardware revisions.
- Enhanced onerom-fw CLI tool to receive a local firmware file.
- One ROM Studio v0.1.8.
- Firmware v0.6.0.

### Changed

- ROM config JSON schema change - `sel_jumper_pull` is now a Vec<u8>, to allow per-pin SEL jumper pull direction.
- PIO support is now built into all RP2350 firmwares, and is enabled/disabled by default using RP_PIO build flag.
- RP2350 no longer enters filesystem mode when USB connected - only PICOBOOT protocol supported.

### Known Limitations

- The firmware does not support image select pins shared with SWD pins.  At present image select pins shared with SWD pins are ignored.
  - This is expected to be added in a future release.
  - It is relevant for `fire-24-d` and `ice-24-j` hardware revisions.
  In additon, the configurable per-pin jumper sel behaviour has not been explicitly tested.
- `swd_enabled`: false is not yet functional.  (It is not clear that this is a good idea anyway, as it would make a non-USB board more difficult to reprogram.)  This setting may be removed in a future release.

## v0.5.10 - 2026-01-03

Promote fire-24-c to verified and recommended 24-pin Fire version.

### Added

- issue #77 - Support for serving multi-ROM sets using Fire PIO algorithm.
- ROM types for 32 and 40 pin ROMs

### Changed

- issue #78 - Move RAM ROM image to 0x2000_0000 to prepare for 40 pin versions.

### Fixes

- issue #76 - fire-24-c PIO algorithm fixes.

## v0.5.9 - 2026-01-01

This release adds new One ROM hardware revisions and ROM type support.

### Added

- 231024 (28 pin) ROM type support.
- Hardware revision ice-24-i - Combined Pro (SWD) and USB 24 pin Ice version.
- Hardware revision fire-24-c - Combined Pro (SWD) and USB 24 pin Fire version.
- Hardware revisions fire-28-a2 and fire-28-a3 - Combined Pro (SWD) and USB 28 pin Fire versions.

### Crates

- onerom-config 0.2.5, 0.2.6
- onerom-fw 0.1.5, 0.1.6
- onerom-gen 0.2.3, 0.2.4
- sdrr-fw-parser 0.5.7, 0.5.8

## v0.5.8 - 2025-12-12

This release adds 28 pin ROM support for the new One ROM Fire 28-pin version, fire-28-a.

### Added

- Support for 28 pin ROM types using fire-28-a.  23128 has been tested in a C64C. with 2764, 27128, 27256 and 27512 tested in an EEPROM reader.  It is STRONGLY recommended NOT to use an EEPROM reader to read One ROM, as it may apply 12V to pin A9.

## v0.5.7 - 2025-12-09

### Fixed

- Erroneous CE/OE pin numbers for 2716/2732 ROMs.

## v0.5.6 - 2025-12-09

### Added

- Support for 2716/2732 ROMs (2716 tested).
- PIO support for 2316/2716/2732 ROMs (untested)
- Studio 0.1.4

### Fixed

- Comment in Makefile - B is now default serving algorithm.

## v0.5.5 - 2025-11-18

This release adds support for the RP2350 One ROM serving ROMs using the RP2350's PIO/DIO peripheral, allowing fully autonomous ROM serving without any CPU involvement.  A VIC-20 kernal can be served successfully with the RP2350 clocked at just 22MHz, the C64 kernal at 41MHz and the both systems' character ROMs at 51MHz.  There are some limitations, meaning this is not yet the default Fire algorithm:
- Dynamically bank switched ROMs _are_ supported.
- Only CS lines which are contiguous are supported - hence the 2316 is _not_ supported on either rev A or USB rev B.
- Multi-ROM sets, via X1/X2, are _not_ supported, as they are not contiguous with the CS lines in current hardware.

For full support a hardware revision must order the CS lines contiguously either:
- X2/X1/CS1/A12/A11 or
- A11/A12/CS1/X1/X2

To build use `EXTRA_C_FLAGS=-DRP_PIO` when running `make`.  See `sdrr/src/piorom.c` for further details of the PIO/DMA algorithm, and configuration options.

### Fixed

- Enabled FP extensions on both Ice and Fire to avoid hard fault if floating point instructions are used by the compiler.

## v0.5.4 - 2025-11-12

This release adds support for the One ROM Fire (RP2350) USB version, via hardware revision `fire-24-usb-b`.  Current USB support is per Ice - plugging in USB disables ROM serving, and puts the RP2350 into DFU mode to allow firmware updates.  A future release is expected to add Fire USB support in parallel to ROM serving. 

### Added

- Added first RP2350 USB version, [Fire 24 USB B](sdrr-pcb/unverified/fire-24-24-b/README.md), as unverified.
- Added [Ice 24 USB H3](sdrr-pcb/unverified/one-rom-ice-usb-h3-24-pin/README.md) as unverified.
- Moved [Ice 24 USB H2](sdrr-pcb/verified/one-rom-ice-usb-h2-24-pin/README.md) to verified.

### Added

- `onerom-config` 0.2.2 - Added method to query ROM power pins.
- Studio:
    - See [rust/studio/CHANGELOG.md](rust/studio/CHANGELOG.md) for details.
    - Studio is released independently of firmware - see `studio-*` github tags.
- ROM Configs:
    - Blank ROM config (for manufacturing/testing) - /rom-configs/blank.json
    - Added pincoder 2021 version system 3, 5, 6 and 7 ROM configs (2332 ROMs)
- Allow `sdrr-fw-parser` to parse up to v0.5.999 firmware versions. 

### Fixes

- [#43](https://github.com/piersfinlayson/one-rom/issues/43) - Let sdrr-fw-parser parse later versions of fw. Supports up to v0.5.999 with no code/wasm/site changes required.
- Allow multi-ROM sets to be built in One ROM Studio.  Unsure when this was regressed

## v0.5.3 - 2025-10-24

The major addition in this release is the companion desktop application, One ROM Studio, which provides a native GUI for creating One ROM firmware images, and analysing and flashing One ROM devices.

<img src="docs/images/one-rom-studio-win-v0.1.0.png" alt="One ROM Studio on Windows" width="500">

Features include:
- Supported platforms: Windows, Mac, Linux (Ubuntu, Debian and Raspberry Pi OS)
- Supported One ROMs: all variants including Ice/Fire and Pro/USB
- Program One ROMs with a choice of ROM images
- Generate and save One ROM firmware files
- Analyse One ROM devices and firmware files
- Automatic debug probe and One ROM USB detection

One ROM Studio can be downloaded from https://onerom.org/studio/.

### Added

- Added "limp mode" support to detect when an incompatible firmware has been flashed to an Ice One ROM.  It blinks fast in this state and, if it is a USB model, allows the device to drop into DFU mode when USB is connected, to allow flashing of a compatible firmware.  Note that "slower" MCU firmwares are permitted by "faster" MCUs - but not the other way around.  See #37 for details.

### Fixes

## v0.5.2 - 2025-10-14

New in this release is a command line firmware image creation tool `rust/fw`.  This dynamically creates a custom One ROM firmware image from a specified:
- set of ROM images (specified as a JSON config file)
- model, board and MCU type (specified as command line arguments)
- optional firmware version (defaults to the latest version available)

To build:
```bash
cd rust/fw
cargo build --release --bin onerom-fw
# Binary is located at /rust/target/release/onerom-fw
```

This is a precursor to simplifying the CI builds to avoid having to create every combination of hardware/ROM config as build artifacts.  The hardware combinations can be pre-built with empty metadata and ROM image data, and this tool then composes a complete firmware image, ready for flashing.

As v0.5.1, and empty firmware image can also be flashed - One ROM then blinks its status LED slowly, and can have metadata and ROM images flashed to it later.

The complete list of available "empty" hardware configuration firmwares can be found at https://images.onerom.org/releases.json.

This command line tool may also be augmented with a native GUI, in parallel to the web-based tool at https://onerom.org/prog/.

### Changes

- Improved `onerom-gen` testing, and improved `onerom-config` and `onerom-gen` validation.

### Fixes

- Fix rust/gen/tests for boards with address lines all >=8.
- Fix `sdrr-info` processing of firmware from v0.5.1 onwards with detailed metadata content in a valid, but unexpected, order.

## v0.5.1 - 2025-10-13

For the average user, there are no particularly notable changes in this release, except the addition of the [Atari 800XL BASIC ROM config](/config/atari800xl.mk).

The firmware generator process underwent a major overhaul in this release:
- Support ROM types and hardware (PCB) revisions are now parsed at build time, which can be included in Web Assembly code.
- `sdrr-common` has been retired and mostly replaced with `onerom-config`
- Much of `sdrr-gen` has been moved out to `onerom-gen` crate, which can be used in other Rust projects, including Web Assembly.
- There is a new `onerom-wasm` crate at https://github.com/piersfinlayson/one-rom-wasm, which can be used to build Web Assembly code to generate One ROM firmware images in the browser.  Web Assembly packages, documentation and examples are located at https://wasm.onerom.org/.
- A metadata and ROM-less firmware can be built with `EXCLUDE_METADATA=1`.  When flashed with ths firmware, One ROM blinks its status LED slowly.  It can subsequently have metadata and ROM image fragments flashed, and, when reset, serves the newly flashed images.  This is expected to be useful for release artifacts and manufacturing, as a single firmware per hardware variant per release can be built, with metadata and ROM images added later, rather than the current need to build a firmware per hardware variant **per configuration** per release.

There is more work planned to the firmware upgrade process, including multi-stage USB device updates using the new capabilities, but this release has laid the groundwork and enabled some exciting new web-based features.

Also, the first, prototype 28 pin ROM support using hardware revision `ice-28-a` is in this release.  It successfully emulates a 27128 ROM (i.e. without the 3rd CS line on a 23128).  It has been tested successfully on a C64C, replacing the combined KERNAL/BASIC ROM with a One ROM Ice 28 A, clocked at just 55MHz using an STM32F446RCT6. This hardware revision is unlikely to be taken forwards - a different hardware layout is required to support 23128 with the 3rd CS line being used, as well as 32KB and 64KB ROMs.

Some default firmware configuration has been changed:
- BOOT_LOGGING is now disabled by default to improve boot times.

### New

- [Atari 800XL BASIC ROM config](/config/atari800xl.mk) included (and tested and working!)

### Changes

- Call out from sdrr-gen to wget to retrieve images located on sourceforge, as cloudfare seems to spot and block Rust TLS.
- Moved the PCB config/property files from `/hw-config` to `/rust/config/json`.

### Fixes

- Make "no ROMs installed" LED blink pattern slower, to be more visible.  Now on for 0.5s, off for 2.5s.  Previously flashed much too fast to be used properly.

## v0.5.0 - 2025-10-07

This release adds a bunch of hardware revisions, plus a modified flash and firmware format, to ease future device re-programming.

As a consequence of the new flash/firmware format, some of the STM32 variants (F401RB/RC, F411RC, F446RC) support 2 fewer ROM images than before.

### New

- New flash/firmware format, with firmware code, followed by ROM metadata, followed by ROM images. 
- Added One ROM Ice USB H2 (unverified).
- Added One Rom Ice USB H (verified) kicad files.
- Added One ROM Fire USB B (unverified)

### Changes

Moved USB programmer site to https://github.com/piersfinlayson/one-rom-site repo.

## v0.4.4 - 2025-09-30

This is a point release that changes the release artifact generation, to set up the One ROM USB programmer to be able to offer to flash pre-built firmware images from the release artifacts.

Release artifact change summary:
- Include additional hardware variants
- Only include .bin files (not .elf, .dis, .map) in the release artifacts.
- Remove some of the less common STM32 variants from the release artifacts (to save build time and space)
- Include a build artifact manifest JSON file. 

If the particular release artifact you want is no longer included, you can build it yourself from source.

### Changes

Updated:
- USB One ROM site (onerom.piers.rocks):
  - Added the ability to select a local file as the firmware source, in addition to a URL.
  - Add check to prevent non-USB firmware being flashed.
  - Add check to ensure the MCU the firmware was build for matches the MCU the user selected.
- CI build process, to create release artifacts:
  - Now only creates .bin files (not .elf, .dis, .map) - if you want the other types, build them yourself.
  - Creates images for multiple different hardware revisions.

## v0.4.3 - 2025-09-29

Added USB DFU support for firmware updates over USB, along with STM32 24-pin rev H hardware (24-h) which includes a micro-USB connector.  One ROM detects when USB is connected, disables ROM serving, and enters STM32 DFU mode to allow the firmware to be updated.

There is also a new web-based programmer for One ROM USB, at https://onerom.piers.rocks/.  This can be used on Chrome or Chromium based browsers on Windows, Linux, or MacOS to program an attached One ROM USB.

### Changes

Updated:
- sdrr
- sdrr-hw-config
- sdrr-pcb
- sdrr-common
- sdrr-fw-parser
- sdrr-gen
- sdrr-info

### Fixes

## v0.4.2 - 2025-09-06

Added [One ROM Lab](rust/lab/README.md) support, which allows a One ROM to be used as a ROM reader.

### Changes

- Added fw-parser for One ROM Lab
- Modify sdrr-info/parser to support Airfrog custom firmware changes

### Fixes

- Probably a few here and there.

## v0.4.1 - 2025-08-28

### Changes

- Move STM32F4 hardware rev G to verified.
- Add KiCad design files for RP2350 rev A and STM32F4 rev G.

### Fixes

- #8 - reduce severity of VOS not ready warning, as it appears to be benign.
- Include RP2350 images in github release.
- Add RP2350 build commands to README.md and generally update the docs to refer to One ROM/RP2350.

## v0.4.0 - 2025-08-24

**The RP2350 release.**

This version contains the first RP2350 PCB revision, and mostly complete firmware support.

Should you use the new RP2350 hardware revision A?  Only limited testing of the RP2350 One ROM has been done so far, but it has generally performed well.

There is one outstanding, known issue - when using the RP2350 One ROM a character ROM on a PAL VIC-20 occasionally the machine boots to a black screen background - that is the machine boots to BASIC and shows the expected text, but the screen is black, not white.  This may be a boot timing issue - that perhaps One ROM RP2350 is not booting fast enough for the VIC chip, which is getting corrupted somehow.  This issue does not appear on a C64 (which has a different video bus architecture).

You may want to order and test small quantities of RP2350 based boards for now, as there is some risk of a hardware design issue coming to light.  However, the hardware appears solid in early testing, so it is likely most issues can be overcome by firmware changes - and it is expected that the RP2350 revision A will continue to be supported in future releases, even should another variant be released.

Other notable changes:

- Instead of building with `STM=<mcu variant> make`, you now need to use `MCU`:

  ```bash
  MCU=<mcu_variant> make
  ```

### Changes

- Added RP2350 support.
  - Hardware rev A.
  - Includes single ROM images, dynamically bank switched, and multi-ROM sets.
  - Includes image select jumpers, status LED, overclocking.
  - Features not supported include: C main loop, MCO output.
  - For the gory details of supporting the RP2350, see [RP2350](docs/RP2350.md).
- Added STM32F4 24-pin PCB rev G hardware configuration.  This adds a different programming header and one more image select jumper (so 5 in total, plus X1/2).
- Added hardware and firmware configuration to specify whether the image select jumpers and X1/X2 pins are pulled high or low when the PCB jumper is closed, to allow for different PCB designs.
- Added firmware support for up to 7 image select jumpers.
- Change STM32 MCO (and MCO2) divider to be /5 (previous value was /4).  Makes it easier to measure the clock speed of an overclocked STM32F4.
- Substantially refactored platform specific code to break out platform agnostic code - significant work to `sdrr/src/main.c`, `utils.c` and `rom_impl.c`.
- Tested overclocking and various STM32 clones.

### Fixes

- It is likely that the 4th image select pin on revs E/F didn't work properly - this has been fixed.

## v0.3.1 - 2025-08-16

The project has been renamed One ROM (To Rule Them All).

This release is a few odds and ends including some hardware improvements:
- One ROM hardware revision [F2](/sdrr-pcb/verified/stm32f4-24-pin-rev-f2/README.md) is currently recommended.
- An **unverified** hardware revision [G](/sdrr-pcb/unverified/stm32f4-24-pin-rev-g/README.md) is in testing.  This brings mostly layout improvements and slightly reduced manufacturing costs.
- The fastest STM32F4 MCU, the STM32F446 has been verified to work.   This brings a max supported clock speed of 180MHz, and has run stably up to 300MHz in testing.
- The STM32F405 has provided slower than expected (more details below).  It is supported and a decent choice, but the GigaDevices GD32F405 appears to be more performant.

### Changes

- Speed up STM32F405 support:
  - The STM32F405 is under-performant vs the other devices at the same clock speed - needs around 30-40% faster clock speed.
  - Added CCM RAM support for the F405, bringing the uplift in clock speed down to around 15-20%.
  - To disable CCM RAM set `C_EXTRA_FLAGS=-DDISABLE_CCM=1` when building.
  - The STM32F405 is still a decent MCU choice for One ROM, as its max clock speed is 168MHz compared with the F411's 100MHz.  However, users may wish to use the GigaDevices clone GD32F405, which appears to have no performance penalty. 
- Hardware revision 24-f2 is now verified.  JLC have successfully fabbed using the hardware files both STM32F411 and STM32F405 variants.
- Allow more aggressive overclocking (up to 400MHz).
- Validated STM32F446RCT6 (STM32F446RET6 highly likely to work as well).  Successfully tested as C64 char ROM, and verified clock speed of 180MHz (via MCO1 showing 45MHz = SYSCLK/4).  Also overclocked to 300MHz, ran stably.
- Added **unverified** hardware revision 24-g.

### Fixes

- Explicitly prevent COUNT_ROM_ACCESS and C_MAIN_LOOP being configured together, as they are incompatible.
- Fixed ability to run main loop from RAM (this tends to be slower than from flash, so isn't recommended).

## v0.3.0 - 2025-08-12

The main user facing change in this release is the addition of support for remote analysis and co-processing alongside the SDRR device via plug-ins, such as [Airfrog](https://piers.rocks/u/airfrog) - **a tiny $3 probe for ARM devices**.  This allows you to inspect the firmware and runtime state of the SDRR device, and change its configuration and ROM data - **while it is serving ROMs**.

There is also new ROM access counting feature, which causes SDRR to count how times the CS line(s) go active.  This can be extracted and visualised using [Airfrog](https://piers.rocks/u/airfrog) and other SWD probes, to determine how often the ROMs are accessed based on host activity.

![ROM Access Graph](docs/images/access-rate.png)

The Rust tooling has been substantially refactored to easier to integrate SDRR support in third-party tooling, such as [Airfrog](https://piers.rocks/u/airfrog).  In particular there is [Firmware Parser](rust/sdrr-fw-parser/README.md) crate, which can be used to parse the firmware from a file or running SDRR, and extract information about the configuration, ROM images, and to extract ROM images from the firmware.

### Changes

- TI-99/4A and CoCo2 configurations have been added to the [third-party configs](config/third-party/README.md) directory.  Thanks to [@keronian](https://github.com/keronian) for contributing these.
- Added a C main loop implementation for which GCC produces the assembly/machine code.  This requires a roughly 25% faster clock speed.  Use `EXTRA_C_FLAGS=-DC_MAIN_LOOP` when running `make` to use this version.
- Stored off image files used to create the firmware in `output/images/`.  This allows post build inspection of the images used.  It also enables additional tests - `sdrr-info` can be now be used as an additional automated check (along with `test`), to ensure the images in the firmware are correct, and validate the behaviour of `sdrr-info` and `test` to be compared.
- Substantial refactoring of `sdrr-gen`, to make it more maintainable.
- Substantial refactoring of `sdrr-fw-parser` in order to make it suitable for airfrog integration.
- Added ROM access counting behind COUNT_ROM_ACCESS feature flag.  When enabled, the firmware updates a u32 counter at RAM address 0x20000008 every time the chip select line(s) go active - i.e. the ROM is selected.  This can be read by an SWD probe, such as [Airfrog](https://piers.rocks/u/sdrr). 
- Changed default Makefile configuration to HW_REV=24-f COUNT_ROM_ACCESS=1 STATUS_LED=1.
- Added a manufacturing test tool [`sdrr-check`](rust/sdrr-check/README.md).
- Changed default build config:
  - HW_REV=24-f
  - STATUS_LED=1
  - COUNT_ROM_ACCESS=1
- Added retrieval of mangled ROM images from firmware.  This can be used to compare the embedded images between different firmwares and to collect a pre-mangled ROM images in order to overwrite a running SDRR's RAM image with it.
- Added new "One ROM To Rule Them All" BASIC program for upcoming video. 

### Fixes

- Probably a few here and there.

## v0.2.1 - 2025-07-22

The main new feature in this version of SDRR is the addition of dynamic [bank switching](docs/MULTI-ROM-SETS.md#dynamic-bank-switching) of ROM images.  This allows SDRR to hold up to 4 different ROM images in RAM, and to switch between them **while the host is running** by using the X1/X2 pins (hardware revision F and later) to switch between them.  Some fun [C64](config/bank-c64-char-fun.mk) and [VIC-20](config/bank-vic20-char-fun.mk) character ROM configurations that support bank switching are included.

In other news:
- The default ROM serving algorithm has been improved, leading to better performance, and hence the ability to support more systems with lower powered STM32F4 devices than before.  Check out [STM32 Selection](docs/STM32-SELECTION.md). The current price/performance sweet spot is the F411.
- [Hardware revision E](sdrr-pcb/verified/stm32f4-24-pin-rev-e/README.md) is now fully verified, so manufacture these with confidence.
- If you'd rather use [revision F](sdrr-pcb/unverified/stm32f4-24-pin-rev-f/README.md) (required for multi-ROM and bank switching support), at least once user has reported getting these manufactured and working with his NTSC VIC-20 - although they did not testing either multi-ROM or bank switching support.

### Changes

- Added pull-up/downs to X1/X2 in multi-ROM cases, so that when a multi-set ROM is configured, but X1/X2 are not connected, the other ROMs in the set still serve properly.
- Improved serving algorithm `B` in the CS active low case.
- Moved to algorithm `B` by default.
- Measured performance of both algorithm on all targets.
- Refactor `rom_impl.c`, breaking out assembly code to `rom_asm.h` to make the main_loop easier to read, and commonalising a bunch of the code for greater maintainability.
- Added detection of hardware reported STM32F4 device and flash size at runtime, and comparison to firmware values - warning logs are produced in event of a mismatch.
- Verified [hw revision e](/sdrr-pcb/verified/stm32f4-24-pin-rev-e/) - supports STM32F4x5 variants in addition to F401/F411, all passives are now 0603, contains a status LED and a 4th image select jumper.
- Added [documentation](/docs/STM32-CLONES.md) on STM32 clones.
- Moved firmware parsing to [`rust/sdrr-fw-parser`](/rust/sdrr-fw-parser/README.md) crate, which can be used to parse the firmware and extract information about the configuration, ROM images, and to extract ROM images from the firmware.  Done in preparation for using the same code from a separate MCU.
- Moved Rust code to [`rust/`](/rust/) directory to declutter the repo a bit.
- Added experimenta; [build containers](/ci/docker/README.md) to assist with building SDRR, and doing so with the recommended build environment.
- Added dynamic [bank switchable](docs/MULTI-ROM-SETS.md#dynamic-bank-switching) ROM image support, using X1/X2 (you can use __either__ multi-ROM __or__ bank switching in a particular set).
- Added fun banked character ROM configs.
- Added VIC-20 NTSC config.
- Added retry in [ci/build.sh](ci/build.sh) to allow for intermittent network issues when downloading dependencies.
- Added [demo programs](demo/README.md) for C64 and VIC-20 to list SDRR features and other information/

### Fixes

- Fixed status LED behaviour, by placing outside of MAIN_LOOP_ONE_SHOT, and using the configured pin.
- Got `sdrr` firmware working on STM32F401RB/RC variants.  These have 64KB RAM, so can only support individual ROM images (quantity limited by flash) and do not support banked or multi-set ROMs.

## v0.2.0 - 2025-07-13

This version brings substantial improvements to the SDRR project, including:

- A single SDRR can be used to replace multiple ROM chips simultaneously.
- New [`sdrr-info`] tool to extract details and ROM images from firmware.
- Add your own hardware configurations, by adding a simple JSON file.
- New STM32F4 variants supported.
- Comprehensive testing of compiled in images to ensure veracity.

Care has been taken to avoid non-backwards compatible interface (such as CLI) changes, but some may may have been missed.  If you find any, please report them as issues.

### New Features

- Added support for ROM sets, allowing SDRR to serve multiple ROM images simultaneously, for certain combinations of ROM types.  This is done by connecting just the chip selects from other, empty sockets to be served, to pins X1/X2 (hardware revision 24-f onwards).  Currently tested only on VIC-20 (PAL) and C64 (PAL), serving kernal and BASIC ROMs simultaneously on VIC-20 and kernal/BASIC/character ROMs simultaneously on the C64.  See [Multi-ROM Sets](/docs/MULTI-ROM-SETS.md) for more details.
- Added [`sdrr-info`](/rust/sdrr-info/README.md) tool to parse the firmware and extract information about the configuration, ROM images, and to extract ROM images from the firmware.  In particular this allows
  - listing which STM32F4 device the firmware was built for
  - extraction of ROM images from the firmware, for checksumming and/or comparing with the originals.
- Moved hardware configuration to a dynamic model, where the supported hardware configurations are defined in configuration files, and the desired version is selected at build time.  Users can easily add configurations for their own PCB layouts, and either submit pull requests to include them in the main repository, or keep them locally.  For more details see [Custom Hardware](/docs/CUSTOM-HARDWARE.md).
- Added support F446 STM32F446R C/E variants - max clock speed 180 MHz (in excess of the F405's 168 MHz).  Currently untested.
- Added [`test`](/test/README.md), to verify the images source code files which are built into the firmware image, output the correct bytes, given the mangling that has taken place.

### Changes

- Updated VIC-20 PAL config to use VIC-20 dead-test v1.1.01.
- 24-pin PCB rev E/F gerbers provided (as yet unverified).
- Many previously compile time features now moved to runtime.
- Makefile produces more consistent and less verbose output.
- Added `sddr_info` struct to main firmware, containing firmware properties, for use at runtime, and later extraction.  This should also allow querying the firmware of a running system via SWD in future.

### Fixed

- Moved to fast speed outputs for data lines, instead of high speed, to ensure VOL is 0.4V, within the 6502's 0.8V requirement.  With high speed outputs, the VOL can be as high as 1.3V, which is beyond the 6502's 0.8V requirement.

## v0.1.0 - 2025-06-29

First release of SDRR.

- Supports F401, F411, and F405 STM32F4xxR variants.
- Includes configurations and pre-built firmware for C64, VIC-20 PAL, PET, 1541 disk drive, and IEEE disk drives.
- PCB rev D design included.
- Release binaries
