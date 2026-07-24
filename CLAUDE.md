# One ROM — Claude Code project guide

One ROM (formerly SDRR, "Software Defined Retro ROM") is an open-source ROM
replacement for retro systems. It emulates 24/28/32/40-pin mask ROMs, EPROMs,
some flash, and 2K SRAM, using an RP2350 (**Fire**) or, on legacy hardware, an
STM32F4 (**Ice**). Shipped in the thousands; other vendors sell it too. Treat
it as a long-lived, production project.

## Working style (read first)

- Do it **right**, for the long term. No hacks, no throwaway "make it pass"
  fixes. If the clean solution is more work, that is the one we want.
- Explain the **why** before the change. Reasoning first, not a diff dumped on
  the wall.
- **Do not switch technical approach without asking first.** If you think a
  chosen path is wrong, stop and raise it — do not quietly re-architect. I
  usually have a better read on feasibility than you do.
- Do not give up on a hard task. Persistence is my call, not yours.
- Do not assert from memory. Read the code before making claims about how it
  behaves; verify against the tree.
- If this guide looks wrong or incomplete, verify against the code, then tell
  me and propose a specific correction. Do not edit `CLAUDE.md` without my
  go-ahead.
- Hold the existing bar for code style, accurate comments, and API docs.
- Answer the question actually asked; do not infer extra scope.
- **Ask questions as free-form text, not multiple-choice.** I dislike the
  predefined-answer question UI — it is too limiting. When you need a decision,
  put the options and your recommendation in prose and let me reply in my own
  words.
- Discuss and review designs with me before you start coding. For anything
  beyond a trivial change, propose the design, and wait for my go-ahead.

## Git & commits

- All commits are **GPG-signed** with my key. You do **not** have the
  passphrase, so you **cannot** create commits — never run `git commit`.
  Prepare and stage the change, then hand me the exact command plus the commit
  message to run myself.
- **No `Co-Authored-By` trailer**, and no other Claude/AI attribution, in
  commit messages — keep it out of the history entirely.
- Only commit when I ask; only push when I ask.
- Keep **CHANGELOGs** current. When a change is user-facing, add an entry —
  under the current in-development version heading — to the repo-root
  [CHANGELOG.md](/CHANGELOG.md) **and** the affected component's own:
  `rust/cli/CHANGELOG.md`, `rust/studio/CHANGELOG.md`, or the relevant plugin's
  `CHANGELOG.md` (e.g. `plugins/system/usb/CHANGELOG.md`). Leave vendored
  changelogs (tinyusb, `firmware/apio`, `firmware/epio`) alone.
- **When you touch the CLI, keep [docs/CLI-MANUAL.md](/docs/CLI-MANUAL.md) in
  sync.** Any user-facing CLI change — new/changed subcommands or options,
  altered conflicts, changed output — must be reflected there, including the
  version banner near the top ("as of release vX.Y.Z"). The manual is the
  user-facing reference; do not let it drift behind the CLI CHANGELOG.
- **Before bumping any crate/component version, read the repo-root
  [CHANGELOG.md](/CHANGELOG.md) first.** Its "To publish" list under the current
  in-development heading is the source of truth for in-flight version bumps. A
  crate or plugin already listed there is bumped-but-unpublished — do **not**
  bump it again for a further change in the same cycle; just add the CHANGELOG
  entry. Crates without their own `CHANGELOG.md` (`onerom-metadata`,
  `onerom-gen`, …) are tracked **only** via that list.
- These crates are published to crates.io with external consumers, so **SemVer
  governs**. A non-backwards-compatible public-API change requires a **minor**
  bump (pre-1.0: `0.MINOR.PATCH`, so `0.6.x → 0.7.0`), never a patch. The
  "already listed → don't bump again" rule assumes the further change is
  backwards-compatible *with the planned bump*: if a crate is listed at a
  **patch** bump and your new change **breaks its public API**, escalate that
  listed entry from patch to minor. Breaking = altering/removing a public item's
  signature or a public field's type, or transitively re-exposing a
  dependency's breaking bump through your own public API.
- The **firmware is not separately versioned** beyond the in-development branch
  version (e.g. `0.7.1`); that version is what signals newly available plugin
  APIs and is what a plugin's `min_fw_version` targets. `firmware/ora/api.h` is
  **not** version-bumped for backwards-compatible additions — only a
  non-backwards-compatible change would bump it, and those are off the table.

## Firmware

`firmware/` is the C + hand-optimised ARM (thumb) core firmware. It is **fully
bare metal — no SDK, no HAL** (no pico-sdk, no vendor HAL). Serving is cycle-
and timing-critical; on RP2350 it runs on PIO/DMA.

- `firmware/src`, `firmware/include`, `firmware/link` — sources, headers,
  linker scripts.
- `firmware/ora/` — the plugin API: `api.h`, `plugin.h`, `system.h`,
  `plugin.ld`, `plugin.mk`, `examples/`.
- `firmware/test` + `test.mk`. `test.mk`'s `WASM=1` mode cross-compiles the
  firmware to WebAssembly (via Emscripten) for One ROM Lens; the root
  `libonerom-test-wasm` target drives it.
- Build output: `firmware/build/onerom-rp235x.bin` (`BIN_PREFIX ?= onerom-rp235x`).

One ROM Lens — a cycle-exact PIO emulator / browser tool — now lives in the Rust
workspace as [rust/lens](/rust/lens) (`onerom-lens`), built on `onerom-fw-emulator`
and compiled to wasm; see its [README](/rust/lens/README.md). (The old C shim
`firmware/lens/` + `firmware/lens.mk` are gone.)
- Build output: `firmware/build/onerom-rp235x.bin` (`BIN_PREFIX ?= onerom-rp235x`).

On invalid config/build the firmware enters **limp mode** (LED blink patterns;
`utils.c` / `globals.c`). The firmware **enforces plugin `min_fw_version`**: at
plugin launch (`firmware/src/plugin.c`) it compares the plugin header's
`min_fw_{major,minor,patch}_version` against the running firmware and refuses to
run a plugin that requires newer firmware.

## Rust workspace (`rust/`)

Directory → package name:

- `fw` → `onerom-fw` — firmware-image composition; resolves/downloads base
  firmware via `releases.json` (`src/net.rs`).
- `cli` → `onerom-cli` — the One ROM CLI (binary `onerom`). Full device tool,
  not a front-end: subcommands `scan`, `program`, `inspect`, `control`,
  `update`, `image`, `peek`, `poke`, `reboot`, `firmware`. Talks to devices
  over USB via `picoboot` / the `picobootx` extension (`nusb`), and uses
  `onerom-fw`, `onerom-gen`, `onerom-fw-parser`, `onerom-metadata`,
  `onerom-app`, `onerom-config` among others.
- `gen` → `onerom-gen` — firmware/metadata generator; also driven by the
  `one-rom-wasm` repo.
- `config` → `onerom-config` — ROM/RAM config model.
- `metadata` → `onerom-metadata` — embedded firmware metadata.
- `protocol` → `onerom-protocol` — implements RBCP (see related repos).
- `fw-parser` → `onerom-fw-parser`; `fw-emulator` → `onerom-fw-emulator`;
  `fw-tester` → `onerom-fw-tester`; `fw-config-gen` → `fw-config-gen`.
- `app` → `onerom-app`; `studio` → `onerom-studio` (desktop GUI, released
  independently via `studio-*` tags); `lab` → `onerom-lab` (hardware tester);
  `database` → `onerom-database`.
- `lens` → `onerom-lens` — One ROM Lens, the browser PIO/DMA waveform viewer;
  a wasm (`wasm32-unknown-emscripten`) binary built on `onerom-fw-emulator`.
- `schema-gen` → `schema-gen` — emits `onerom-config/schema.json` from the
  `onerom-gen` config type.

Legacy `sdrr-*` crate names are gone; everything is `onerom-*` now.

**Direction — Studio onto `onerom-cli` lib.** The long-term plan is to rewrite
`onerom-studio` so it relies mostly on the `onerom-cli` library rather than
carrying its own duplicate device logic. So `onerom-studio` depends on
`onerom-cli`, and new shared device logic (chip-ID identity, GET_INFO reads,
reboot/reconnect handling) belongs in `onerom-cli` and should be consumed from
there — not reimplemented in Studio, and not split into a separate crate.

## Building

Base (empty) firmware, from the repo root:

    scripts/build-empty-fw.sh [-d] [-l]     # -d debug logging, -l logging

Flashable image — use the CLI (`onerom-cli`, or download from
https://onerom.org/cli). `onerom program` is the primary build-and-flash
workflow; `onerom firmware` builds a binary without programming a device:

    # build + flash a connected One ROM (board inferred from the device):
    onerom program --config onerom-config/vic20-pal.json

    # build a firmware binary without flashing:
    onerom firmware build \
      --base-firmware firmware/build/onerom-rp235x.bin \
      --config onerom-config/vic20-pal.json \
      /tmp/firmware.bin

CI / release firmware builds:

    ci/build.sh ci                  # clean + build base -> builds/ci/onerom-rp235x.bin
    ci/build.sh release <version>   # package a prior ci build -> builds/<version>/...
    ci/build.sh clean

Other `ci/` scripts: `build-images.sh` (populates the `images.onerom.org`
channel), `build-cross-fw.sh` (cross-builds the `onerom-fw` **tool** —
orthogonal to firmware variant builds, do not conflate), `rust-tests.sh`,
`rust-docs.sh`, `rust-tools.sh`, `test-emu.sh`. Reproducible builds use the
container in `ci/docker/`.

Some checked-in files are generated and must stay in sync — `ci/rust-tests.sh`
**fails** if the committed copy differs from a fresh regeneration:

- `cargo run -p onerom-gen --bin compat` → `docs/COMPATIBILITY.md`
- `cargo run -p schema-gen --bin schema-gen` → `onerom-config/schema.json`

(e.g. a version bump changes `COMPATIBILITY.md`.) These generators, along with
`ci/rust-tests.sh` and `ci/rust-docs.sh` (slow — `rust-docs.sh` especially), are
end-of-work validation, **not** per-change checks. **Do not run them
proactively.** As a piece of work approaches completion, check with me before
running them; during iteration, per-crate `cargo check` / `clippy` / targeted
tests are enough.

## Testing firmware on a device (CLI)

To flash and test a **locally-built** firmware on a connected One ROM, use the
installed `onerom` CLI (on `PATH`; or build the matching one from this tree with
`cargo build -p onerom-cli` → `target/debug/onerom`). Ask before flashing — it
changes device state.

- `onerom scan` — read-only device discovery; shows board, firmware version,
  state (`Running`/`Stopped`), serial. Run it first, and again after flashing to
  confirm the device came back up `Running`.
- **Test the local build, not a download.** Pass
  `--base-firmware firmware/build/onerom-rp235x.bin` to `program`/`firmware
  build`. Without it the CLI downloads the *released* base firmware and your
  firmware changes are not under test.
- **Discoverability never requires a plugin.** A stopped One ROM sits in the
  RP2350 bootloader (picoboot), so `scan` always finds it and `scan --slots`
  reads its slot metadata — with no plugins flashed. The system USB plugin does
  **not** provide discoverability; what it provides is the device's *own* USB
  stack, which a **Running** device needs to **serve while staying on the USB
  bus** (without it, a Running device drops off the bus until it is stopped back
  into the bootloader). So add `--plugin usb` only when the test needs the
  device to *serve while remaining USB-visible* — never "so it's discoverable".
  Add `--plugin rgb` too when testing RGB One ROMs (Piers's usual test
  hardware). Max one system + one user plugin; `usb` is system,
  `rgb`/`host-control`/`blink` are user.
- `--plugin` **can** be combined with `--config` (as well as with `--slot`): the
  plugins are inserted ahead of the config's ROM slots, and it is an error if the
  config already defines a plugin of its own. To pass plugins alongside
  command-line ROMs, use `--slot` mode:

      onerom program \
        --slot file=<path|url>,type=23128,cs1=active_low,cs2=active_low,cs3=active_high \
        --base-firmware firmware/build/onerom-rp235x.bin \
        --plugin usb --plugin rgb \
        --out /tmp/fw.bin

  Per-slot firmware overrides go in the `--slot` spec, e.g. `,led=off` (status
  LED), `,cpu-freq=200MHz`, `,force_16bit=true` — the same overrides expressible
  as `firmware_overrides` in a config file.
- `--out <file>` saves the composed image *and* flashes; `onerom firmware build
  … <out>` composes without flashing; `onerom firmware inspect <bin>` dumps
  contents. Board is inferred from the connected device (override with
  `--board`). `program` composes the image before writing, so a bad build aborts
  without touching the device.

## Config (`onerom-config/`)

JSON ROM/RAM config files plus `schema.json` (generated by `schema-gen`).
Config uses `chip_sets`/`chips`; the old `rom_sets`/`roms` keys still parse for
back-compat.

## Plugins (`plugins/`)

Plugins are separate binaries run on a spare RP2350 core once serving has
started (system + user types, ~1KB stack each, no sandbox). The plugin API
(`firmware/ora/`) is **stable**: it may be extended, but changes are guaranteed
backwards compatible. The system USB plugin provides the device-side USB stack
and exposes the `picobootx` interface. The `host-control` plugin implements
RBCP.

- **Extending the plugin API:** add new `ORA_ID_*` values additively (never
  renumber or repurpose an existing one), and give every new ID an `@since
  firmware vX.Y.Z` line in its `firmware/ora/api.h` doc block, naming the
  firmware version it first shipped in — that version is what a plugin targets
  via `min_fw_version`. `api.h` itself is only version-bumped for a
  non-backwards-compatible change (which shouldn't happen).
- **Exposing device metadata to plugins:** tag the field in
  `rust/metadata/metadata_schema.toml` with `plugin_key = { name = "…", id = N }`.
  String fields then resolve via `ORA_ID_GET_METADATA_STR`, unsigned
  scalar/enum fields via `ORA_ID_GET_METADATA_UINT` — no hand-written firmware.
  Key ids are one permanent namespace: never renumber or reuse. `status_led_enabled`
  is the live status-LED state and the cross-plugin coordination channel (written
  by `ora_set_status_led`, read via its `STATUS_LED_STATE` key).

## Metadata & manifest — two separate mechanisms

Do not conflate these:

1. **Embedded firmware metadata (the v0.7.0 "v2" schema).** Defined by
   `onerom-metadata`; `MIN_SCHEMA_VERSION = 0.7.0`
   (`rust/metadata/src/lib.rs`). `onerom-fw-parser` reads both old and new
   layouts by branching on `version >= MIN_SCHEMA_VERSION`. This is the real
   versioned dual-schema, living in the firmware metadata parser.

2. **The `releases.json` manifest at `images.onerom.org`** (not the deprecated
   local-repo copy). Consumed in `rust/fw/src/net.rs` (`Releases` / `Release` /
   `Board` / `Mcu`), and also in `rust/studio/src/app/manifest.rs` and
   `rust/app/src/plugin.rs`. There is a **single** consumer schema with **no**
   version-sniffing branch; back-compat across all historical releases is
   achieved via `Option<String> path` overrides, not a second schema. The
   top-level `version: usize` is a data marker only.

Consequences to respect:

- `images.onerom.org` archives **all** historical releases (v0.5.x, per-board
  v0.6.x, v0.7.0). v0.7.0 entries still enumerate `boards`/`mcus`, so pre-0.7.0
  clients keep working; the shared board/MCU-agnostic base firmware is expressed
  by pointing multiple entries at the same `path`.
- The base firmware became board/MCU-agnostic, but the composed **full image
  and its metadata are still per-hardware-variant**.
- Do **not** collapse old releases into a single model-level entry, and do not
  add a version-sniffing branch to the manifest consumer — that breaks pre-0.7.0
  clients and is off the table.
- Plugin manifest `min_fw_version` is enforced by the firmware (see Firmware);
  the manifest's `incompatible_from` upper bound is advisory only (discovered
  post-build, not in the binary header).

## Related repositories

- `one-rom-site` — the onerom.org website, including the browser programmer.
- `one-rom-images` — backs `images.onerom.org` (firmware images, configs,
  plugin manifests, Studio releases).
- `one-rom-wasm` — WASM build of `onerom-gen` for in-browser firmware
  generation (wasm.onerom.org).
- `rom-bus-control-protocol` (RBCP) — protocol spec; implemented by
  `onerom-protocol` and the `host-control` plugin.
- `picoboot` — host-side Rust crate for the RP2040/RP2350 PICOBOOT USB
  interface (used by `onerom-cli`).
- `picobootx` — device-side PICOBOOT extension library adding custom commands;
  exposed by One ROM's system USB plugin.

## Hardware notes

- `hardware/pcb/` holds KiCad files, per revision, verified/unverified.
- RP2350 runs 5V-tolerant with no level shifters.
- GPIO-to-ROM-pin mapping is driven by 2-layer PCB routing, so data/address
  lines are not in logical GPIO order; pre-processing accounts for this.