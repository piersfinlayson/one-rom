# One ROM — Claude Code project guide

One ROM (formerly SDRR, "Software Defined Retro ROM") is an open-source ROM
replacement for retro systems. It emulates 24/28/32/40-pin mask ROMs, EPROMs,
some flash, and 2K SRAM, using an RP2350 (**Fire**) or, on legacy hardware, an
STM32F4 (**Ice**). Shipped in the thousands; other vendors sell it too. Treat
it as a long-lived, production project.

## Working style (read first)

- Do it **right**, for the long term. No hacks, no throwaway "make it pass"
  fixes. If the clean solution is more work, that is the one we want.
- Explain the **why** before the change — in discussion, not in the commit
  message or the CHANGELOG. Reasoning first, not a diff dumped on the wall.
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
  passphrase, so you **cannot** create commits — never run `git commit`, and
  never `git push`.
- **Never offer a stage-and-commit command, or a commit message, unbidden.**
  Make the change, say what you did, and stop.
- **No `Co-Authored-By` trailer**, and no other Claude/AI attribution, in
  commit messages — keep it out of the history entirely.
- **Commit bodies are bullets, one line each, one per main thing the commit
  does.** Three or fewer; often none at all, where the subject says it. A
  bullet states *what*, and *why* only where the why is not obvious from the
  what. Prose paragraphs in a commit body are the failure mode to avoid.
  - Leave out: mechanism the diff already shows, alternatives considered and
    rejected, notes that tests were added, and verification run-logs (one
    line — `Verified on fire-28-a/b/c/d, single and banked` — not a
    transcript). Reasoning a future reader genuinely needs goes in a code
    comment next to the thing it explains, or in the issue.
- **CHANGELOG entries are one or two sentences — 40 words is already long.**
  The headline list at the top of a release carries the story; a detail
  bullet says what changed and, where it is not obvious, what it means for a
  user. Same for the component CHANGELOGs. Keep the existing
  `- This required a firmware update.` sub-bullet convention.
  - **One entry per user-visible change, not per commit.** A feature built
    over several commits — device side, plugin, CLI — is one entry, not one
    per layer. Corrections made before release fold into the entry for the
    thing they correct.
  - Internal-only work (refactors, test-harness fixes, CI, CLAUDE.md) gets no
    entry unless a user or a downstream crate can see it.
- Keep **CHANGELOGs** current. When a change is user-facing, add an entry —
  under the current in-development version heading — to the repo-root
  [CHANGELOG.md](/CHANGELOG.md) **and** the affected component's own:
  `rust/cli/CHANGELOG.md`, `rust/studio/CHANGELOG.md`, or the relevant plugin's
  `CHANGELOG.md` (e.g. `plugins/system/usb/CHANGELOG.md`). Leave vendored
  changelogs (tinyusb, `firmware/apio`, `firmware/epio`) alone.
- **When you touch the CLI, keep [docs/CLI-MANUAL.md](/docs/CLI-MANUAL.md) in
  sync, in the same commit.** Any user-facing CLI change — new/changed
  subcommands or options, altered conflicts, changed output — must be reflected
  there. The manual is the user-facing reference; do not let it drift behind the
  CLI CHANGELOG.
  - The trigger is **"the CLI prints something different"**, not "the CLI gained
    an option". Rewording a label, adding or dropping a line, changing what a
    column can contain — each is a manual change, and each is easy to miss
    precisely because no option changed. Where the manual quotes example output,
    paste a **verbatim run**; hand-written examples drift and quietly become
    wrong.
  - **A value the manual states that something else owns is written inside a
    marker naming that source, never bare** — a schema constant, or the CLI's
    own version banner. See "Quoting a constant in a document" below. A number
    inside a quoted run is the exception: it is a record of what the command
    printed, so it stays a plain literal, and a marker inside a fenced code
    block is refused.
  - The manual carries **two** breaking-changes sections, and a CLI change that
    breaks an existing command line updates both. `# New Breaking Changes`, near
    the top, lists **only the release in development**. `## Appendix: Breaking
    Change History` at the end keeps every release, newest first. At release
    time the top section's entries move down under a new version heading in the
    appendix, and the top section is emptied for the next cycle.
  - **Neither section is ever deleted.** A release with no breaking changes
    keeps `# New Breaking Changes` and says there are none. A reader who has
    learned to look there must not find it missing.
  - Every top-level section in the manual is preceded by `---`, which
    `docs/pdf/docs.css` renders as a page break. A new section needs one.
  - The manual is not the only user-facing document a CLI change can reach. If
    what you changed is also described in `docs/COMPATIBILITY.md`,
    `docs/CHIP-TYPES.md` or another `docs/` file — or in a **generator** that
    emits one — update that too, in the same commit.
- **Hand-written docs describing behaviour go stale silently — keep them in the
  same commit as the behaviour.** Nothing mechanical catches these, so they are
  the ones that rot. The pairs that have bitten so far:
  - `rust/lab/README.md` — One ROM Lab's interface. It drifted a whole rewrite
    behind the code, still describing a build-time-configured RTT tool after Lab
    became an interactive USB CDC shell. Touch `rust/lab/src/cli/`, its
    `scripts/`, or the command set, and update it.
  - `docs/ADDING-CHIP-TYPES.md` — the chip-type contribution path. It names
    `chip-types.json` fields, `SUPPORTED_CHIP_TYPES` in `rust/gen/src/v2/`, the
    generator commands and `ci/test-emu.sh`. Change any of those and the doc is
    wrong. It also states that `rbcp_chip_type` requires a matching PR against
    the `rom-bus-control-protocol` repo — that stays true.
  - The root [README.md](/README.md) — its "Ways in" table, crate table and
    regression-testing section describe the tree's shape. Adding, retiring or
    renaming a crate, or changing what CI covers, reaches it. Deliberately keep
    **counts** (of tests, boards, chip types) out of it, so growth does not make
    it wrong.
  - A crate that becomes deprecated or unmaintained says so in **its own
    README** (`onerom-protocol`, `onerom-database`), rather than being quietly
    dropped from mention.
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
  `plugin.ld`, `plugin.mk`, `examples/`, `tests/`.
  - `examples/` teaches the API — small, single-idea plugins a plugin author
    reads. `tests/` holds plugins that **test** the API on a device, and are
    neither built nor run by CI: each needs a One ROM, a debug probe, and the
    USB connector moved by hand between programming and running. A test there
    is dormant by design, and its README says what it validated and what change
    would make it worth running again. Do not put a test in `examples/` — the
    two pull apart fast, and an instrument makes a terrible example.
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
- `protocol` → `onerom-protocol` — the One ROM Lab wire protocol, spoken
  between an airfrog (over SWD) and a One ROM Lab device; carried over
  `airfrog-rpc`. Not RBCP.
- `fw-parser` → `onerom-fw-parser`; `fw-emulator` → `onerom-fw-emulator`;
  `fw-tester` → `onerom-fw-tester`; `fw-config-gen` → `fw-config-gen`.
- `fw-driver` → `onerom-fw-driver` — GPIO bitmask builders and `ControlLine`.
  **Zero dependencies, no build script**, by design — see below.
- `fw-geometry` → `onerom-fw-geometry` — the pure, config-derived half of the
  host-side test/visualisation stack: `pin_cache` (socket pin → MCU GPIO
  resolution) and `substitution` (`chip_substitution`). **No build script.**
- `app` → `onerom-app`; `studio` → `onerom-studio` (desktop GUI, released
  independently via `studio-*` tags); `lab` → `onerom-lab` (hardware tester);
  `database` → `onerom-database`.
- `lens` → `onerom-lens` — One ROM Lens, the browser PIO/DMA waveform viewer;
  a wasm (`wasm32-unknown-emscripten`) binary built on `onerom-fw-emulator`.
- `schema-gen` → `schema-gen` — emits `onerom-config/schema.json` from the
  `onerom-gen` config type.

Legacy `sdrr-*` crate names are gone; everything is `onerom-*` now.

**Why `onerom-fw-driver` and `onerom-fw-geometry` are separate crates.**
`onerom-fw-emulator`'s build script compiles the whole firmware C for a
`CONFIG`/`BOARD`, so *anything* depending on that crate pays for a firmware
build. `onerom-lens` needs the emulator for wasm **and** needs pure pin geometry
in its **build script**. While the build-script side reached
`onerom-fw-emulator` (through `onerom-fw-tester`), a single `cargo build -p
onerom-lens --target wasm32-unknown-emscripten` built the emulator twice
concurrently — once for the host, once for wasm — and the two `make` runs raced
over the shared `firmware/generated/gen-config.c`, `firmware/apio` and
`firmware/epio`, failing differently every run. Four things must stay true:

- Nothing reachable from `onerom-lens`'s `[build-dependencies]` may depend on
  `onerom-fw-emulator`.
- Neither crate may gain a **build script**.
- `onerom-fw-driver` has **no dependencies at all**, and that is the point of
  it: the emulator re-exports `driver`, so anything this crate depends on is
  compiled into Lens's wasm build. Here the rule is compiler-enforced — a
  dependency that cannot build for `wasm32-unknown-emscripten` breaks Lens.
- `onerom-fw-geometry` is **host-only**: its consumers are `onerom-fw-tester`
  and `onerom-lens`'s *build script*, and it is absent from Lens's wasm graph.
  So the wasm constraint does **not** bind it, and nothing will fail if it gains
  a host-only dependency. What binds it is the first rule above. (The
  metadata-reading half of `geometry` stayed in `onerom-fw-tester` because it
  calls `onerom_fw::get_rom_files`, and `onerom-fw` pulls in `smol`, which
  cannot build for wasm — a constraint that applied before `driver` was split
  out. Moving it now would only make Lens's build script slower, so leave it.)

`onerom-fw-emulator` re-exports `driver`, and `onerom-fw-tester` re-exports
`driver` and `pin_cache`, so existing `onerom_fw_emulator::driver::…` and
`onerom_fw_tester::pin_cache::…` paths keep working.

**Direction — Studio onto `onerom-cli` lib.** The long-term plan is to rewrite
`onerom-studio` so it relies mostly on the `onerom-cli` library rather than
carrying its own duplicate device logic. So `onerom-studio` depends on
`onerom-cli`, and new shared device logic (chip-ID identity, GET_INFO reads,
reboot/reconnect handling) belongs in `onerom-cli` and should be consumed from
there — not reimplemented in Studio, and not split into a separate crate.

## Building

Base (empty) firmware, from the repo root:

    make                                    # DEBUG_LOGGING=1 for debug logging

Flashable image — use the CLI (`onerom-cli`, or download from
https://onerom.org/cli). `onerom program` is the primary build-and-flash
workflow; `onerom firmware` builds a binary without programming a device:

    # build + flash a connected One ROM (board inferred from the device):
    onerom program --config onerom-config/vic20-pal.json

    # build a firmware binary without flashing (--board is required unless a
    # One ROM is connected to infer it from):
    onerom firmware build \
      --base-firmware firmware/build/onerom-rp235x.bin \
      --config onerom-config/vic20-pal.json \
      --board fire-24-e \
      --output /tmp/firmware.bin

CI / release firmware builds:

    ci/build.sh ci                  # clean + build base -> builds/ci/onerom-rp235x.bin
    ci/build.sh release <version>   # package a prior ci build -> builds/<version>/...
    ci/build.sh clean

Other `ci/` scripts: `build-images.sh` (populates the `images.onerom.org`
channel), `build-cross-fw.sh` (cross-builds the `onerom-fw` **tool** —
orthogonal to firmware variant builds, do not conflate), `rust-tests.sh`,
`rust-docs.sh`, `rust-lint.sh`, `rust-tools.sh`, `rust-binaries.sh`,
`test-emu.sh`, `coverage-run.sh`, `coverage-campaign.sh`,
`coverage-report.sh`. Reproducible builds use the container in `ci/docker/`.

`test-emu.sh` takes a socket size — `ci/test-emu.sh 24|28|32|40`, or no argument
for all of them. CI runs the four as parallel jobs; run one at a time in a given
working tree, since every test regenerates the same `firmware/generated/gen-config.c`
and rebuilds the same `firmware/build-test/`.

`ci/coverage-*.sh` measure line coverage of the C the testers drive, and CI
gates it on every push: `ci/coverage-baseline.txt` holds a floor per file, and
a file may not drop below its. `--raise` moves a floor up, never down —
lowering one is a hand edit and the commit says why.

Floors come from the run set in `ci/coverage-campaign.txt` **and from CI's own
figures** — a smaller set, or any local machine, measures lower and sets them
wrong. Running the campaign locally is expensive, so ask first;
`ci/coverage-run.sh <board> <config>` against one pair is the development
loop.

Coverage needs GNU gcc, so `coverage-run.sh` refuses to run anywhere but Linux.
From another platform use `ci/coverage-docker.sh <board> <config>`, which runs
the same script in the `onerom-build` container against a copy of the tree, and
brings the tracefiles back to `build/coverage` for `ci/coverage-report.sh` to
read on the host.

Code that genuinely cannot be reached is marked `LCOV_UNREACHABLE_START` /
`LCOV_UNREACHABLE_STOP` with a comment saying why. lcov **fails the capture**,
writing no tracefile at all, if a marked line turns out to have been reached, so
the comment's claim is checked against the counters rather than taken. That is
the whole reason for the marker over `LCOV_EXCL_*`, which drops the lines
quietly and would let a stale claim sit there for as long as it liked.

- **A region covers the arm's body, from inside the opening brace.** The line
  carrying the condition is evaluated on every call, so a marker placed above
  the `if` claims a line that runs, and the capture fails naming it.
- Where the reason is arithmetic rather than a runtime check, a `STATIC_ASSERT`
  holds the thing that makes it unreachable — see the flame table in
  `firmware/src/piodma/pioled.c`.
- A marker added to make a number work turns the figure into a lie. If a branch
  is hard to reach, that is the signal to write the test.

**Toolchain versions are pinned, in one place each:** `ci/arm-toolchain-version`
(Arm GNU, for the firmware and plugins), `ci/emscripten-version` (emsdk, for
Lens) and `ci/lcov-version` (lcov, for coverage).
`ci/install-arm-toolchain.sh`, `ci/install-emscripten.sh` and
`ci/install-lcov.sh` install the pinned version and are what CI, the container
and a developer's machine all use, so a firmware binary is built by the same
compiler wherever it is built. lcov is built from source rather than installed
from the distribution because Ubuntu packages 2.0, which reads
`LCOV_UNREACHABLE_START` as an ordinary comment — `ci/coverage-run.sh` refuses
to run below the pinned version rather than measure with a marker nothing
checks. The `ci/docker/` image takes all three as build args from
`ci/docker/build.sh` — the Dockerfile deliberately has **no** default, because a
stale default there is how the container silently ended up on a different
compiler. Note Arm moved toolchain
hosting to `gitlab.arm.com` from 15.3.Rel1; `developer.arm.com` carries 15.2.rel1
and earlier only.

Some checked-in files are generated and must stay in sync — `ci/rust-tests.sh`
**fails** if the committed copy differs from a fresh regeneration:

- `cargo run -p onerom-gen --bin compat` → `docs/COMPATIBILITY.md`
- `cargo run -p schema-gen --bin schema-gen` → `onerom-config/schema.json`
- `cargo run -p onerom-gen --bin layout -- --write-baseline` →
  `ci/layout-baseline.txt`, the flash each chip type costs on each board.
  A diff says the numbers moved; `cargo run -p onerom-gen --bin layout --
  --check` says whether that is an improvement or a regression.
- `docs/CHIP-TYPES.md` — no command of its own, the `onerom-config` build script
  rewrites it on any build. Checked because otherwise a `chip-types.json` change
  gets committed without the regenerated doc.

`cargo run -p doc-gen` is in the same gate but is **not** a generator: it writes
nothing. It checks the values documents state against the sources that own them
— see below.

(e.g. a version bump changes `COMPATIBILITY.md`.) These generators, along with
`ci/rust-tests.sh` and `ci/rust-docs.sh` (slow — `rust-docs.sh` especially), are
end-of-work validation, **not** per-change checks. **Do not run them
proactively.** As a piece of work approaches completion, check with me before
running them; during iteration, per-crate `cargo check` / `clippy` / targeted
tests are enough.

**Formatting & clippy** are a CI gate: `ci/rust-lint.sh` runs
`cargo fmt --all -- --check` and `clippy` with `-D warnings` across the whole
workspace. A single `cargo clippy --workspace` can't build the tree, so the
script groups crates by how they build: host crates in one pass; `onerom-fw-tester`
and the wasm pair (`onerom-fw-emulator`, `onerom-lens`) with `CONFIG`/`BOARD` set
because their build embeds the firmware emulator (the wasm pair targets
`wasm32-unknown-emscripten`); and `onerom-lab` from its own dir on its pinned
nightly (`--bins`). Like the other end-of-work scripts, run it as work wraps up,
not per-change. `onerom-studio` is linted with the host crates (it needs
libudev/libusb present), and `ci.yml` *builds* both Studio and the CLI via
`ci/rust-binaries.sh` (host only, release profile).
`.github/workflows/build-studio.yml` now only builds the cross-platform
installers — it fires solely on `rust/studio/**`, so it could never have caught a
workspace-wide change breaking Studio, which is why the linting lives in this
gate instead. Neither workflow releases anything: Studio and CLI releases are
built locally. Note: `onerom-config`'s generated
modules
(`src/{chip,hw}/generated.rs` and their `mod.rs` — all git-ignored) are
rustfmt-formatted **at generation time** by the build script
(`config/build/fmt.rs`) so the fmt gate stays green; keep that path intact if you
touch the code generators.

## CS-to-data timing (`onerom-fw-tester`)

`pio-tester` asserts, on every run, how many cycles after CS assertion the
device serves the byte **for that cycle** — `rust/fw-tester/src/cs_timing.rs`,
reported as `timing_checks`/`timing_failures` alongside the data and bus
counters. It exists because the bulk read pass cannot detect a serving
slowdown on its own: a single-chip image is replicated across the SRAM index
bits the chip's address lines do not drive, so a stale pre-CS fetch returns the
right byte anyway.

- The expected latency is derived from **config**, via
  `onerom_gen::compat::serving_alg_info`, and the firmware's own report is used
  only to cross-check that it programmed the window the config called for. Do
  not invert that: an expectation taken from the device moves along with a
  device bug.
- New chip types need no change here. A new *algorithm* does, and will not
  compile until it gets one — `cs_timing` mirrors the C algorithm enums and
  asserts against the firmware's `NUM_*_ALGS`.
- `timing.rs`'s `CYCLES_*` are **correctness margins** for the bulk pass, not
  sensitivity knobs. Do not lower one to make a pass go green.

To get the measured number rather than a pass/fail — after a timing assertion
fails, or when adding an algorithm:

    BASE_DIR=$(pwd) CONFIG=onerom-config/test/24-random-23xx.json \
      BOARD=fire-24-a cargo run -p onerom-fw-tester --example cs_sweep

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
        --slot file=<path|url>,type=23128,cs1=active-low,cs2=active-low,cs3=active-high \
        --base-firmware firmware/build/onerom-rp235x.bin \
        --plugin usb --plugin rgb \
        --out /tmp/fw.bin

  Per-slot firmware overrides go in the `--slot` spec, e.g. `,led=off` (status
  LED), `,cpu-freq=200MHz`, `,force-16-bit=true` — the same overrides expressible
  as `firmware_overrides` in a config file.
- `--out <file>` saves the composed image *and* flashes; `onerom firmware build
  --out <file>` composes without flashing; `onerom firmware inspect --firmware
  <bin>` dumps contents. The output path and the firmware to inspect are always
  options, never positional arguments. Board is inferred from the connected
  device (override with `--board`). `program` composes the image before writing, so a bad build aborts
  without touching the device.

## CLI arguments (`rust/cli/src/args/`)

Conventions the whole CLI holds to. `rust/cli/src/main.rs`'s `cli_assert`
module walks the entire clap tree and fails the build on a breach of the first
four, so a new option is checked mechanically rather than by review.

- **Every argument is `--name value`. No positionals, anywhere.** `board header
  <board>` was the one exception, and its own error told the user to pass
  `--board`, which did not exist.
- **A short flag means one thing across the whole CLI**, not just within a
  command: `-b` board, `-o` output, `-i` input, `-c` chip-type, `-l` length,
  `-f` force, `-n` no-reboot, `-m` msd, `-p`/`-r` stopped/running. Do not spend
  a letter on a command-local meaning. `-s -i -u -y -v -h` are claimed by the
  global options — a subcommand reusing one **panics at startup**, which is
  why `verify_cli` exists. `-a` is the sole grandfathered exception
  (`--address` vs `--all`), pinned by its own test.
- **Long names are kebab-case.** A snake_case alias exists *only* where the
  option names a JSON config key, and then it must match that key verbatim
  (`turbo_boot`, `instance_name`, `boot_logging`, `serial_override`) so a
  config key can be pasted straight onto the command line. Never alias an
  option to its own long name.
- **Every option needs a `///` doc comment** — a `//` comment silently gives
  clap no help text — and a `value_name` of one uppercase word.
- **Give values a `value_parser`** so a bad one fails at parse time rather than
  part-way through the work. Where the values are an enum owned by another
  crate, drive them from that type's `supported_values()` (see
  `args/image.rs`'s `ImageFormatParser`) so a new variant flows through with no
  CLI change — `onerom-gen` stays clap-free, so no `ValueEnum` derives there.
- Do **not** write `required = true` on a non-`Option` field; clap already
  requires it.
- **Examples in doc comments must be runnable.** `firmware inspect
  firmware.bin` sat in the help for months describing an argument form the
  command never had.
- A user-visible CLI change updates [docs/CLI-MANUAL.md](/docs/CLI-MANUAL.md)
  in the same commit — see the Git & commits section, which covers this and the
  other `docs/` files a CLI change can reach.

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
- **A new plugin API call is not done until the plugin API tester exercises
  it, in the same commit.** That tester is
  [rust/fw-tester/src/bin/plugin_api_tester/](/rust/fw-tester/src/bin/plugin_api_tester/),
  run by `make test-api` and by `ci/test-emu.sh` against every board of a
  family. A new `ORA_ID_*` needs three things: an entry in `tests/lookup.rs`'s
  active list, a wrapper on `Emulator`
  (`rust/fw-emulator/src/emulator.rs`) so a test can call it, and a test of
  what the call actually does — its refusals and its edges, not just that it
  resolves. `lookup.rs` reads the `api_id_t` enum out of `api.h` and fails on
  an ID in neither of its lists, so the classification is caught mechanically.
  Nothing catches a classified ID with no test behind it.
  - A plugin that drives the new call is **not** that test. The USB plugin
    tester (`rust/usb-tester`) reaches the LED engine through the plugin's own
    commands and says only what its wire format can carry — the API's own
    boundary, its sizes and its refusals, is what the plugin API tester owns.
- **Exposing device metadata to plugins:** tag the field in
  `rust/metadata/metadata_schema.toml` with `plugin_key = { name = "…", id = N }`.
  String fields then resolve via `ORA_ID_GET_METADATA_STR`, unsigned
  scalar/enum fields via `ORA_ID_GET_METADATA_UINT` — no hand-written firmware.
  Key ids are one permanent namespace: never renumber or reuse. `status_led_enabled`
  is the live status-LED state and the cross-plugin coordination channel (written
  by `ora_set_status_led`, read via its `STATUS_LED_STATE` key).

## Cross-project constants

**A value more than one of firmware, plugin and host has to agree on is
declared once, in `rust/metadata/metadata_schema.toml`, and never written out
by hand a second time.** Writing it twice does not merely risk drift — it makes
drift silent, because nothing compares the two copies. Every constant there is
emitted three ways from the one declaration:

| Reaches | How | Name |
| --- | --- | --- |
| Firmware C | `firmware/generated/onerom_metadata.h` | the schema name |
| Rust (CLI, tools, tests) | `pub const` on `onerom_metadata` | the schema name |
| Plugins (`ora_api = true` only) | `firmware/ora/onerom_constants_generated.h`, included by `api.h` | `ORA_` + the schema name |

Adding one:

```toml
[[constants]]
name = "LED_MAX_HOLD_MS"
type = "u32"
value = 60000
ora_api = true                     # omit unless a plugin needs it
comment = """The longest hold either LED accepts, in milliseconds."""
```

- **`ora_api = true` puts it in the plugin API.** A plugin builds against
  `firmware/ora` alone and cannot include the firmware's metadata header, so a
  value it must agree with the firmware on needs this tag. The ORA name is the
  schema name with an `ORA_` prefix, derived rather than given, so either can be
  found from the other. Leave it off otherwise — most constants are
  firmware-and-host only, and the plugin API is a published surface.
- **Two constants that happen to share a value stay two constants.**
  `ORA_LED_MAX_HOLD_MS` and `ORA_GPIO_MAX_HOLD_MS` are both 60000 for different
  reasons.
- **The `comment` becomes the doc comment in all three outputs**, so write it
  for whoever reads it last.

### Quoting a constant where Rust demands a literal

A doc comment takes a literal, and a `pub const` is not one — so clap's help
text cannot state a value by naming the constant. Hand-typing the number there
is the defect above, in prose.

`onerom-metadata` generates `ALL_CONSTANTS`, every constant as `(name, value)`
in plain text, for a build script to write one file per constant into `OUT_DIR`
(see `rust/cli/build.rs`). `include_str!` and `concat!` are accepted where a
literal is required, so:

```rust
const HELP_BEACON_PERIOD: &str = concat!(
    "Milliseconds for one blink. Defaults to ",
    const_str!("LED_BEACON_DEFAULT_PERIOD_MS"),
    "."
);

#[arg(long, value_name = "MS", value_parser = parse_beacon_period,
      help = HELP_BEACON_PERIOD)]
pub period: Option<u16>,
```

**`help = …` rather than a `///` comment, and only for this.** Clap reads a doc
comment as a literal and would see an unexpanded macro, leaving the option with
*no help at all* — which builds clean and is caught only by reading `--help`.
Every other option keeps its `///` comment, as the CLI arguments section says.
`ALL_CONSTANTS`' own doc comment carries the full recipe for a new consumer.

A name with no matching constant fails the build and says which file it looked
for, so a typo or a retired constant is caught rather than leaving a stale
number behind.

### Quoting a constant in a document

A document has the same problem and the opposite solution. `docs/CLI-MANUAL.md`
states hold limits, LED periods and the reset pulse, and those belong to the
schema. It states them inside a marker naming the source:

```markdown
The device's own limit is <!--[const:GPIO_MAX_HOLD_MS:seconds]-->60 seconds<!--[/]-->.
```

`cargo run -p doc-gen` reads the schema and fails if the text between the
markers is not what the source says today, naming file, line, expected and
found. **It never writes a document** — a tool with write access to thousands of
lines of hand-written prose, to save a hand edit on the day a constant changes,
is a bad trade. The markers are HTML comments, so they are invisible to a reader
and are stripped before the PDFs are rendered.

- `source:name[:format]`. Sources are `const` (the metadata schema) and
  `version` (a crate's version, e.g. `version:cli`). Formats are `raw`, `ms`,
  `seconds` and `code`; an unknown one is an error, as is an unknown source or
  name.
- Several constants that share a value are named together —
  `const:LED_BEACON_MIN_PERIOD_MS+LED_BLINK_MIN_PERIOD_MS` — and all must agree
  with the text, so one moving apart from the other is caught.
- **A marker inside a fenced code block is refused.** A number in a pasted run
  is a record of what the command printed, not a claim about today.
- The manual's own version banner is `version:cli`, so it is checked rather than
  remembered.
- The check runs in `ci/rust-tests.sh`, and again in `docs/pdf/render.py` for
  every document read from the working tree — a release is built by hand and the
  gate may not have been. Past editions, which name a git ref, are read as they
  shipped and are not checked.
- Marking a document up is opt-in, one value at a time: a document with no
  markers passes, so any `docs/` file can adopt them when it is worth it.

## Total parseability — non-negotiable

**A host tool of the same generation as the device parses every single byte that
device holds, and that must stay true.** Flash data, metadata, runtime info, the
RTT control block and the ROM data are all parsed, and `onerom inspect info`
dumps the lot as JSON
(`rust/cli/src/inspect.rs`). Few users reach for it. It is core to One ROM's
architecture and it is not up for trade.

A host starts at one fixed address, `onerom_info_t` at the metadata base in
flash, follows its `runtime` pointer to `onerom_runtime_info_t` in RAM and
confirms the magic. Everything that only exists while the device runs hangs off
runtime info. On a v0.7.0+ device the whole thing dumps as one tree.

It falls out of the schema: a field declared in
`rust/metadata/metadata_schema.toml` generates the C struct in
`firmware/generated/onerom_metadata.h`, the Rust type, its parser and its
`serde::Serialize` impl, and appears in the JSON with no host-side code written.

So:

- **Never leave device state nothing points to.** A `.bss` static sits at a
  build-dependent address with no path from the anchor, so no host can find it
  and the dump silently omits it. Four stranded bytes are as invisible as sixty.
- **Never hand-write parsing for anything the schema could describe.**
- **An older host ignores fields it does not know, and says that it did.** It is
  not expected to parse a newer device's new fields. It is expected not to go
  quiet about them - showing less without a word, or dropping a whole structure
  because its shape moved, is a defect and not degradation.
- **Every byte added to runtime info needs Piers's explicit approval**, one byte
  at a time. Being reachable is not optional. What it costs in RAM is his call,
  always, in advance.

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
- `rom-bus-control-protocol` (RBCP) — protocol spec; the device side is
  implemented by the `host-control` plugin. There is no host-side RBCP
  implementation in this repo (in particular, `onerom-protocol` is not it).
- `picoboot` — host-side Rust crate for the RP2040/RP2350 PICOBOOT USB
  interface (used by `onerom-cli`).
- `picobootx` — device-side PICOBOOT extension library adding custom commands;
  exposed by One ROM's system USB plugin.

## Hardware notes

- `hardware/pcb/` holds KiCad files, per revision, verified/unverified.
- RP2350 runs 5V-tolerant with no level shifters.
- GPIO-to-ROM-pin mapping is driven by 2-layer PCB routing, so data/address
  lines are not in logical GPIO order; pre-processing accounts for this.