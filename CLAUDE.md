# One ROM — Claude Code project guide

One ROM (formerly SDRR, "Software Defined Retro ROM") is an open-source ROM
replacement for retro systems. It emulates 24/28/32/40-pin mask ROMs, EPROMs,
some flash and 2K SRAM, on an RP2350 (**Fire**) or, on legacy hardware, an
STM32F4 (**Ice**). It has shipped in the thousands, and other vendors sell it
too. Treat it as a long-lived, production project.

## Working style (read first)

- Do it **right**, for the long term. Where the clean solution is more work,
  that is the one we want.
- Explain the **why** before the change, in discussion. Reasoning first, not a
  diff dropped on the wall.
- Discuss the design with me before you code. For anything beyond a trivial
  change, propose it and wait for my go-ahead.
- Raise it with me before switching technical approach. I usually have a
  better read on feasibility than you do.
- Persistence on a hard task is my call. Keep going until I say otherwise.
- Read the code before claiming how it behaves. Verify against the tree.
- Answer the question actually asked, at the scope asked.
- Ask in prose — the options and your recommendation — and let me reply in my
  own words.
- Hold the existing bar for code style, accurate comments and API docs.

## Editing this guide

This file loads on every turn of every session, so it stays short. Three kinds
of content, and the first belongs here:

| Content | Home |
| --- | --- |
| A rule — what to do, what to ask about, what I decide | Here |
| A fact about the tree — crates, directories, commands | [README.md](/README.md), `docs/`, the script's own `--help` |
| Why a thing is the way it is | A doc comment or a comment beside the code |

Where a rule needs its reasoning, put the rule here in a sentence and link the
code that carries the argument. A second copy is a copy nothing compares, and
it goes stale in silence.

Tell me what is wrong here and propose a specific correction, then wait for my
go-ahead before editing this file.

## Git and commits

- Every commit is GPG-signed with my key, and the passphrase is mine. Make the
  change, say what you did, and stop — staging, the commit and the push are
  mine, and a commit message comes when I ask for one.
- The history carries my authorship alone: leave out `Co-Authored-By` and any
  other AI attribution.
- **A commit body is bullets, one line each, one per main thing the commit
  does.** Three or fewer, and often none where the subject says it. A bullet
  earns its place by saying what the diff does not show: *what* changed, and
  *why* where the why is unobvious.
  - Reasoning a future reader genuinely needs goes in a code comment beside
    the thing it explains, or in the issue. Verification fits on one line —
    `Verified on fire-28-a/b/c/d, single and banked`.

## CHANGELOGs

- **An entry is one or two sentences — 40 words is already long.** The
  headline list at the top of a release carries the story. A detail bullet
  says what changed and, where it is unobvious, what it means for a user. Keep
  the `- This required a firmware update.` sub-bullet convention.
- **One entry per user-visible change, not per commit.** A feature built over
  several commits — device side, plugin, CLI — is one entry. A correction made
  before release folds into the entry for the thing it corrects.
- An entry exists where a user or a downstream crate can see the change.
  Refactors, test-harness fixes, CI and this file are invisible to both.
- A user-facing change goes under the current in-development heading in the
  repo-root [CHANGELOG.md](/CHANGELOG.md) **and** in the affected component's
  own: `rust/cli/CHANGELOG.md`, `rust/studio/CHANGELOG.md`, or the plugin's
  (e.g. `plugins/system/usb/CHANGELOG.md`). Vendored changelogs (tinyusb,
  `firmware/apio`, `firmware/epio`) belong to their upstreams.

## Versioning

- **Read the repo-root [CHANGELOG.md](/CHANGELOG.md) before bumping anything.**
  Its "To publish" list under the current in-development heading owns in-flight
  bumps. A crate already listed there is bumped-but-unpublished: add the
  CHANGELOG entry and leave the version as it stands. Crates without their own
  `CHANGELOG.md` (`onerom-metadata`, `onerom-gen`, …) are tracked there alone.
- These crates are on crates.io with external consumers, so **SemVer governs**.
  A non-backwards-compatible public-API change takes a **minor** bump (pre-1.0:
  `0.6.x → 0.7.0`). Breaking means altering or removing a public item's
  signature or a public field's type, or re-exposing a dependency's breaking
  bump through your own public API. Where a listed entry is a patch bump and
  your change breaks the API, escalate that entry to minor.
- The **firmware carries the in-development branch version** (e.g. `0.7.1`) and
  nothing else. That version signals newly available plugin APIs and is what a
  plugin's `min_fw_version` targets. `firmware/ora/api.h` bumps only for a
  non-backwards-compatible change, which stays off the table.

## Documents that go stale

Nothing mechanical catches a hand-written document describing behaviour, so
update it in the same commit as the behaviour.

- **[docs/CLI-MANUAL.md](/docs/CLI-MANUAL.md), whenever the CLI prints
  something different.** That is the trigger, wider than "the CLI gained an
  option": a reworded label, an added or dropped line, a column that can now
  hold something new. The manual's own conventions — verbatim example output,
  the doc-gen markers, its two breaking-changes sections, the `---` before
  every top-level section — are in a comment at the top of that file.
  - Where the change is also described in `docs/COMPATIBILITY.md`,
    `docs/CHIP-TYPES.md`, another `docs/` file, or a **generator** that emits
    one, update that too, in the same commit.
- **`rust/lab/README.md`** — One ROM Lab's interface. Touch `rust/lab/src/cli/`,
  its `scripts/`, or the command set, and update it.
- **`docs/ADDING-CHIP-TYPES.md`** — the chip-type contribution path. It names
  `chip-types.json` fields, `SUPPORTED_CHIP_TYPES` in `rust/gen/src/v2/`, the
  generator commands and `ci/test-emu.sh`. It also states that
  `rbcp_chip_type` requires a matching PR against the `rom-bus-control-protocol`
  repo, which stays true.
- **[README.md](/README.md)** — its "Ways in" table, crate table and
  regression-testing section describe the tree's shape, so adding, retiring or
  renaming a crate reaches it, as does changing what CI covers. Keep **counts**
  (of tests, boards, chip types) out of it, so growth leaves it right.
- A crate that becomes deprecated or unmaintained says so in **its own README**
  (`onerom-protocol`, `onerom-database`).

## Firmware

`firmware/` is the C and hand-optimised Thumb core firmware, fully bare metal —
registers directly, with no pico-sdk or vendor HAL. Serving is cycle- and
timing-critical, and on RP2350 it runs on PIO and DMA. Build output is
`firmware/build/onerom-rp235x.bin`.

- `firmware/ora/` is the plugin API: `api.h`, `plugin.h`, `system.h`,
  `plugin.ld`, `plugin.mk`, `examples/`, `tests/`.
  - `examples/` teaches the API — small, single-idea plugins a plugin author
    reads. `tests/` holds plugins that **test** the API on a device, and CI
    neither builds nor runs them: each needs a One ROM, a debug probe, and the
    USB connector moved by hand between programming and running. A test there
    is dormant by design, and its README says what it validated and what change
    would make it worth running again. A test belongs in `tests/` — the two
    directories pull apart fast, and an instrument makes a poor example.
- `firmware/test` + `test.mk`. `test.mk`'s `WASM=1` mode cross-compiles the
  firmware to WebAssembly for One ROM Lens, driven by the root
  `libonerom-test-wasm` target.
- On invalid config or build the firmware enters **limp mode** (LED blink
  patterns, `utils.c` / `globals.c`).
- The firmware **enforces plugin `min_fw_version`**: at plugin launch
  (`firmware/src/plugin.c`) it compares the plugin header's
  `min_fw_{major,minor,patch}_version` against the running firmware and refuses
  a plugin needing newer firmware.

## Rust workspace (`rust/`)

[README.md](/README.md)'s crate table says what each crate is for. Three things
the table leaves out:

- **`onerom-fw-driver` and `onerom-fw-geometry` are separate crates on purpose,
  and four constraints keep them that way.** Nothing reachable from
  `onerom-lens`'s `[build-dependencies]` may depend on `onerom-fw-emulator`,
  neither crate may gain a **build script**, `onerom-fw-driver` keeps **zero
  dependencies**, and `onerom-fw-geometry` stays host-only. The reasoning is in
  each crate's own `src/lib.rs` docs — read those before touching either
  manifest.
- `onerom-fw-emulator` re-exports `driver`, and `onerom-fw-tester` re-exports
  `driver` and `pin_cache`, so `onerom_fw_emulator::driver::…` and
  `onerom_fw_tester::pin_cache::…` keep working.
- **Shared device logic belongs in `onerom-cli`.** `onerom-studio` depends on
  it, and chip-ID identity, GET_INFO reads and reboot/reconnect handling are
  written there and consumed from there rather than reimplemented in Studio or
  split into a crate of their own.

## Building

    make                            # base (empty) firmware, DEBUG_LOGGING=1 for logs
    ci/build.sh ci                  # clean + build base -> builds/ci/
    ci/build.sh release <version>   # package a prior ci build -> builds/<version>/

Flashable images come from the CLI — see [Testing firmware on a device](#testing-firmware-on-a-device-cli)
and README.md's build section. Every script under `ci/` documents itself in its
header comment. The ones with rules attached:

- **`ci/test-emu.sh <24|28|32|40>`**, or no argument for all four. CI runs them
  as parallel jobs. Run one at a time in a given working tree, since each
  regenerates the same `firmware/generated/gen-config.c` and rebuilds the same
  `firmware/build-test/`.
- **`ci/coverage-*.sh`** measure line coverage of the C the testers drive, and
  CI gates every push against the per-file floors in
  `ci/coverage-baseline.txt`. `--raise` moves a floor up. Lowering one is a
  hand edit, and the commit says why.
  - Floors come from the run set in `ci/coverage-campaign.txt` **and from CI's
    own figures**, since a smaller set or a local machine measures lower. The
    campaign is expensive to run locally, so ask me first.
    `ci/coverage-run.sh <board> <config>` against one pair is the development
    loop, on Linux alone — `ci/coverage-docker.sh` gets you there from macOS.
  - Genuinely unreachable code is marked `LCOV_UNREACHABLE_START` /
    `LCOV_UNREACHABLE_STOP` with a comment saying why, and lcov checks the
    claim against the counters. Writing one:
    - **A region covers the arm's body, from inside the opening brace.** The
      line carrying the condition runs on every call, so a marker above the
      `if` claims a line that executes and the capture fails naming it.
    - Where the reason is arithmetic rather than a runtime check, a
      `STATIC_ASSERT` holds the thing that makes it unreachable — see the flame
      table in `firmware/src/piodma/pioled.c`.
    - A marker added to make a number work turns the figure into a lie. Where a
      branch is hard to reach, write the test.

**Toolchain versions are pinned, one file each:** `ci/arm-toolchain-version`,
`ci/emscripten-version`, `ci/lcov-version` and `ci/c-compiler-version`. The
matching `ci/install-*.sh` scripts install the pinned version, and CI, the
container and a developer's machine all use them, so one compiler builds a
given binary wherever it is built. `ci/docker/` takes all four as build args
from `ci/docker/build.sh`, and the Dockerfile deliberately carries no default,
since a stale default there is how the container once ended up on a different
compiler.

Some checked-in files are generated, and `ci/rust-tests.sh` fails where the
committed copy differs from a fresh regeneration:

- `cargo run -p onerom-gen --bin compat` → `docs/COMPATIBILITY.md`
- `cargo run -p schema-gen --bin schema-gen` → `onerom-config/schema.json`
- `cargo run -p onerom-gen --bin layout -- --write-baseline` →
  `ci/layout-baseline.txt`, the flash each chip type costs on each board. A
  diff says the numbers moved, and `… --check` says whether that is an
  improvement or a regression.
- `docs/CHIP-TYPES.md` — the `onerom-config` build script rewrites it on any
  build, and it is checked so a `chip-types.json` change lands with its
  regenerated doc.

**Run the end-of-work gates as work wraps up, and check with me first.** There
are three: `ci/rust-lint.sh` (fmt and clippy at `-D warnings`),
`ci/rust-tests.sh` and `ci/rust-docs.sh`, the last two slow. During iteration,
per-crate `cargo check`, `clippy` and targeted tests are enough. `cargo run -p
doc-gen` sits in the test gate and generates nothing — it checks the values
documents state against the sources that own them.

`onerom-config`'s generated modules (`src/{chip,hw}/generated.rs` and their
`mod.rs`, all git-ignored) are rustfmt-formatted at generation time by
`config/build/fmt.rs`, which is what keeps the fmt gate green. Keep that path
intact when touching the code generators.

## Testing firmware on a device (CLI)

Use the installed `onerom` CLI, or build this tree's with `cargo build -p
onerom-cli`. Ask me before flashing, since it changes device state.

- `onerom scan` — read-only discovery, showing board, firmware version, state
  (`Running`/`Stopped`) and serial. Run it first, and again after flashing to
  confirm the device came back up `Running`.
- **Test the local build.** Pass `--base-firmware
  firmware/build/onerom-rp235x.bin` to `program` or `firmware build`. Left off,
  the CLI downloads the *released* base firmware and your changes sit outside
  the test.
- A stopped One ROM sits in the RP2350 bootloader, so `scan` finds it and
  `scan --slots` reads its slot metadata with no plugins flashed. Add
  `--plugin usb` where the test needs the device to **serve while staying on
  the USB bus**. One system plus one user plugin is the maximum: `usb` is
  system, `host-control` and `blink` are user.
- `--plugin` combines with `--config` as well as `--slot` — the plugins go
  ahead of the config's ROM slots, and a config defining its own plugin is an
  error. To pass plugins alongside command-line ROMs, use `--slot`:

      onerom program \
        --slot file=<path|url>,type=23128,cs1=active-low,cs2=active-low,cs3=active-high \
        --base-firmware firmware/build/onerom-rp235x.bin \
        --plugin usb \
        --out /tmp/fw.bin

  Per-slot firmware overrides go in the spec — `,led=off`, `,cpu-freq=200MHz`,
  `,force-16-bit=true` — the same set expressible as `firmware_overrides` in a
  config file.
- The RGB LED needs no plugin from firmware v0.7.2: the firmware drives it and
  `onerom control rgb` reaches it, with `onerom inspect rgb` reading back.
- `--out <file>` saves the composed image *and* flashes. `onerom firmware build
  --out <file>` composes alone, and `onerom firmware inspect --firmware <bin>`
  dumps contents. The output path and the firmware to inspect are options,
  never positional. Board is inferred from the connected device, and `--board`
  overrides. `program` composes before writing, so a bad build aborts with the
  device untouched.

## CS-to-data timing (`onerom-fw-tester`)

`pio-tester` asserts on every run how many cycles after CS assertion the device
serves the byte for that cycle — `rust/fw-tester/src/cs_timing.rs`, whose module
docs explain why the bulk read pass cannot see a serving slowdown on its own.

- The expected latency comes from **config**, via
  `onerom_gen::compat::serving_alg_info`, and the firmware's own report serves
  only to cross-check that it programmed the window the config called for. An
  expectation taken from the device would move along with a device bug.
- A new chip type needs no change here. A new *algorithm* does, and will fail
  to compile until it gets one.
- `timing.rs`'s `CYCLES_*` are **correctness margins** for the bulk pass rather
  than sensitivity knobs. Keep them where they are and fix the serving.

For the measured number rather than a pass or fail:

    BASE_DIR=$(pwd) CONFIG=onerom-config/test/24-random-23xx.json \
      BOARD=fire-24-a cargo run -p onerom-fw-tester --example cs_sweep

## CLI arguments (`rust/cli/src/args/`)

`rust/cli/src/main.rs`'s `cli_assert` module walks the whole clap tree and fails
the build on a breach of the first four, so a new option is checked
mechanically.

- **Every argument is `--name value`.** The CLI has no positionals anywhere.
- **A short flag means one thing across the whole CLI**: `-b` board, `-o`
  output, `-i` input, `-c` chip-type, `-l` length, `-f` force, `-n` no-reboot,
  `-m` msd, `-p`/`-r` stopped/running. The global options claim `-s -i -u -y -v
  -h`, and a subcommand reusing one panics at startup, which is what
  `verify_cli` catches. `-a` is the sole grandfathered exception (`--address`
  vs `--all`), pinned by its own test.
- **Long names are kebab-case.** A snake_case alias exists where the option
  names a JSON config key, and then it matches that key verbatim (`turbo_boot`,
  `instance_name`, `boot_logging`, `serial_override`) so a config key pastes
  straight onto the command line.
- **Every option carries a `///` doc comment** — clap reads help text from
  there, and a `//` comment leaves it silently empty — plus a `value_name` of
  one uppercase word.
- **Give values a `value_parser`** so a bad one fails at parse time. Where the
  values are an enum owned by another crate, drive them from that type's
  `supported_values()` (see `args/image.rs`'s `ImageFormatParser`) so a new
  variant flows through with no CLI change. `onerom-gen` stays clap-free.
- clap already requires a non-`Option` field, so leave `required = true` off.
- **Examples in doc comments are runnable.** `firmware inspect firmware.bin`
  sat in the help for months describing an argument form the command never had.

## Config (`onerom-config/`)

JSON ROM and RAM config files plus `schema.json`, generated by `schema-gen`.
Config uses `chip_sets`/`chips`, and the old `rom_sets`/`roms` keys still parse
for back-compat.

## Plugins (`plugins/`)

Plugins are separate binaries run on a spare RP2350 core once serving has
started — system and user types, ~1KB stack each, no sandbox. The plugin API
(`firmware/ora/`) is **stable**: it may be extended, and changes stay
backwards compatible. The system USB plugin provides the device-side USB stack
and exposes the `picobootx` interface. `host-control` implements RBCP.

- **Extending the API:** add `ORA_ID_*` values additively, each taking the next
  unused number, with existing ones keeping their meaning for good. Give every
  new ID an `@since firmware vX.Y.Z` line in its `firmware/ora/api.h` doc block,
  naming the firmware version it first shipped in — that is what a plugin
  targets via `min_fw_version`.
- **A new plugin API call is done when the plugin API tester exercises it, in
  the same commit.** That tester is
  [rust/fw-tester/src/bin/plugin_api_tester/](/rust/fw-tester/src/bin/plugin_api_tester/),
  run by `make test-api` and by `ci/test-emu.sh` against every board of a
  family. A new `ORA_ID_*` needs three things: an entry in `tests/lookup.rs`'s
  active list, a wrapper on `Emulator` (`rust/fw-emulator/src/emulator.rs`) so a
  test can call it, and a test of what the call does — its refusals and its
  edges, beyond that it resolves. `lookup.rs` reads the `api_id_t` enum out of
  `api.h` and fails on an ID in neither of its lists, so classification is
  caught mechanically. A classified ID with no test behind it is caught by you.
  - A plugin driving the new call is a separate thing. The USB plugin tester
    (`rust/usb-tester`) reaches the LED engine through the plugin's own
    commands and says what its wire format can carry. The API's own boundary,
    its sizes and its refusals belong to the plugin API tester.
- **Exposing device metadata to plugins:** tag the field in
  `rust/metadata/metadata_schema.toml` with
  `plugin_key = { name = "…", id = N }`. String fields then resolve via
  `ORA_ID_GET_METADATA_STR` and unsigned scalar or enum fields via
  `ORA_ID_GET_METADATA_UINT`, with no hand-written firmware. Key ids are one
  permanent namespace, each number keeping its meaning for good.
  `status_led_enabled` is the live status-LED state and the cross-plugin
  coordination channel, written by `ora_set_status_led` and read via its
  `STATUS_LED_STATE` key.

## Cross-project constants

**A value more than one of firmware, plugin and host must agree on is declared
once, in `rust/metadata/metadata_schema.toml`, and every consumer reads it from
there.** A second hand-written copy makes drift silent, because nothing
compares the two. One declaration reaches all three:

| Reaches | How | Name |
| --- | --- | --- |
| Firmware C | `firmware/generated/onerom_metadata.h` | the schema name |
| Rust (CLI, tools, tests) | `pub const` on `onerom_metadata` | the schema name |
| Plugins (`ora_api = true` only) | `firmware/ora/onerom_constants_generated.h`, included by `api.h` | `ORA_` + the schema name |

```toml
[[constants]]
name = "LED_MAX_HOLD_MS"
type = "u32"
value = 60000
ora_api = true                     # where a plugin needs it
comment = """The longest hold either LED accepts, in milliseconds."""
```

- **`ora_api = true` puts it in the plugin API.** A plugin builds against
  `firmware/ora` alone and cannot include the firmware's metadata header, so a
  value it must agree with the firmware on needs this tag. The ORA name is
  derived rather than given, so either name finds the other. Most constants are
  firmware-and-host only, and the plugin API is a published surface.
- **Two constants that happen to share a value stay two constants.**
  `ORA_LED_MAX_HOLD_MS` and `ORA_GPIO_MAX_HOLD_MS` are both 60000 for different
  reasons.
- **The `comment` becomes the doc comment in all three outputs**, so write it
  for whoever reads it last.

Two places need a value where the language wants a literal, and each has a
mechanism with the full recipe at its home:

- **Rust, where a doc comment is the only place clap will read help text
  from.** `onerom-metadata` generates `ALL_CONSTANTS` for a build script to
  write one file per constant into `OUT_DIR` (see `rust/cli/build.rs`), and
  `include_str!` plus `concat!` do the rest. Its doc comment carries the
  recipe, including why the option must use `help = …` rather than `///`.
- **A document, where the text has to match what the source says today.** The
  value goes inside a marker naming its source, and `cargo run -p doc-gen`
  fails where the two disagree. It never writes a document.
  `rust/doc-gen/src/main.rs` and its `marker` and `format` modules carry the
  syntax, the sources and the formats.

Both fail the build on a name with no matching constant, so a typo or a retired
constant surfaces rather than leaving a stale number behind.

## Total parseability — non-negotiable

**A host tool of the same generation as the device parses every single byte
that device holds, and that stays true.** Flash data, metadata, runtime info,
the RTT control block and the ROM data are all parsed, and `onerom inspect info`
dumps the lot as JSON (`rust/cli/src/inspect.rs`). Few users reach for it. It is
core to One ROM's architecture, and it is not up for trade.

A host starts at one fixed address, `onerom_info_t` at the metadata base in
flash, follows its `runtime` pointer to `onerom_runtime_info_t` in RAM and
confirms the magic. Everything existing only while the device runs hangs off
runtime info. It falls out of the schema: a field declared in
`rust/metadata/metadata_schema.toml` generates the C struct, the Rust type, its
parser and its `serde::Serialize` impl, and appears in the JSON with no
host-side code written.

- **Every byte of device state is reachable from the anchor.** A `.bss` static
  sits at a build-dependent address with no path to it, so no host can find it
  and the dump omits it in silence. Four stranded bytes are as invisible as
  sixty.
- **Anything the schema can describe is described there**, and its parser falls
  out.
- **An older host ignores fields it does not know, and says that it did.** It
  is not expected to parse a newer device's new fields. It is expected to speak
  up about them — showing less without a word, or dropping a whole structure
  because its shape moved, is a defect.
- **Every byte added to runtime info needs my explicit approval**, one byte at
  a time. What it costs in RAM is my call, always, in advance.

## Metadata and manifest — two separate mechanisms

1. **Embedded firmware metadata (the v0.7.0 "v2" schema).** Defined by
   `onerom-metadata`, with `MIN_SCHEMA_VERSION = 0.7.0`
   (`rust/metadata/src/lib.rs`). `onerom-fw-parser` reads both layouts by
   branching on `version >= MIN_SCHEMA_VERSION`. This is the real versioned
   dual-schema, and it lives in the firmware metadata parser.
2. **The `releases.json` manifest at `images.onerom.org`.** Consumed in
   `rust/fw/src/net.rs` (`Releases`, `Release`, `Board`, `Mcu`), and in
   `rust/studio/src/app/manifest.rs` and `rust/app/src/plugin.rs`. There is a
   **single** consumer schema with **no** version-sniffing branch: back-compat
   across every historical release comes from `Option<String> path` overrides.
   The top-level `version: usize` is a data marker.

- `images.onerom.org` archives all historical releases (v0.5.x, per-board
  v0.6.x, v0.7.0). v0.7.0 entries still enumerate `boards`/`mcus`, so pre-0.7.0
  clients keep working, and the shared board- and MCU-agnostic base firmware is
  expressed by pointing several entries at the same `path`.
- The base firmware became board- and MCU-agnostic, and the composed **full
  image and its metadata stay per-hardware-variant**.
- Old releases keep their own entries, and the manifest consumer keeps its
  single schema. Collapsing the first or version-sniffing in the second breaks
  pre-0.7.0 clients, and both are off the table.
- The firmware enforces a plugin manifest's `min_fw_version`. Its
  `incompatible_from` upper bound is advisory, discovered post-build rather
  than carried in the binary header.

## Related repositories

- `one-rom-site` — the onerom.org website, including the browser programmer.
- `one-rom-images` — backs `images.onerom.org` (firmware images, configs,
  plugin manifests, Studio releases).
- `one-rom-wasm` — WASM build of `onerom-gen` for in-browser firmware
  generation (wasm.onerom.org).
- `rom-bus-control-protocol` (RBCP) — protocol spec, implemented device-side by
  the `host-control` plugin. The host side lives outside this repo, and
  `onerom-protocol` is a different thing: the One ROM Lab wire protocol spoken
  between an airfrog (over SWD) and a Lab device, carried over `airfrog-rpc`.
- `picoboot` — host-side Rust crate for the RP2040/RP2350 PICOBOOT USB
  interface, used by `onerom-cli`.
- `picobootx` — device-side PICOBOOT extension library adding custom commands,
  exposed by One ROM's system USB plugin.

## Hardware notes

- `hardware/pcb/` holds KiCad files, per revision, verified and unverified.
- RP2350 runs 5V-tolerant with no level shifters.
- 2-layer PCB routing drives the GPIO-to-ROM-pin mapping, so data and address
  lines sit outside logical GPIO order and pre-processing accounts for it.
