# One ROM Studio v2 — decision and plan

Investigation of the CLI, Studio, Web and the desktop GUI toolkits, plus two
working prototypes. August 2026.

**Decision: build on Iced.** Native desktop, Mac/Windows/Linux with Linux first
class, offline, Rust throughout, signed and notarised as today. Day one replaces
what Studio does now over Fire USB — no Ice, no SWD — and adds a multi-slot
builder UI as the main programming path.

---

## H. What the investigation found

**H1. Studio does less than it appears, so day one is a small target.** Its whole
device vocabulary is three verbs: read flash words, flash an image, reboot.
Nothing in `rust/studio/src/` mentions picobootx, LEDs, GPIO, memory writes,
erase or CDC. The in-app config builder is unreachable — the entry that opens it
is commented out at `src/config.rs:150` — and its emitter writes
`"cs0" = "active_low"`, an `=` where JSON needs a `:` (`config.rs:401-418`).
Three screens work: analyse a device or file, build from someone else's config
JSON, view the log.

**H2. Studio's trouble was its own code, not Iced.** It calls `Task::none` 68
times and `Task::then`/`and_then` zero times — Iced's async composition goes
unused, and the app hand-builds state machines through the message loop. 31
functions, including the lowest-level USB reads, return the GUI's `AppMessage`,
so no device code is callable outside Iced. The whole model, up to a `Vec<u8>`
flash image, is deep-cloned on every message (`app/mod.rs:156`). The prototypes
avoided all three without fighting the framework.

**H3. The CLI's library already covers the device surface. The orchestration is
missing.** `onerom_cli`'s library is 7.3 kLOC across 18 modules and gives a GUI
`enumerate_devices`, `read_device_info`, `read_memory`, `write_memory`,
`flash_program`, `flash_program_read`, `flash_erase`, `reboot`, `set_led`,
`set_rgb`, `gpio_set`, `gpio_query`, `pulse`, CDC log streaming, the `--slot`
grammar with config-JSON generation, and plugin logic via `onerom-app`. Welded
into the binary: the compose-and-program pipeline (`src/firmware.rs`,
`src/program.rs`) and every interactive confirmation, which reads stdin inside
functions a GUI would call. Studio has this library and uses three items from it.

**H4. Sharing a frontend with Web buys little.** Of the site programmer's 4,033
lines of JS, about 10% survives as-is, 35% is deleted because Rust already does
it, and 40% is rewritten because it is `alert()`-and-DOM orchestration with no
state model. What survives is the CSS and markup. WebUSB does not port to any
webview except Electron's.

---

## K. Why the other toolkits are out

- **K2 Tauri.** WebKitGTK composites the whole webview through DMABUF/GBM/EGL, so
  when that path fails against a driver it renders nothing. Failures cluster by
  NVIDIA driver and Wayland, not by what the app draws. Tauri's own maintainer
  says he cannot recommend it for Linux. Closed on Linux risk.
- **K3 Electron.** Bundled Chromium removes the Linux problem and costs a Node
  tree, 150-200 MB idle, a packaging rewrite and a rebuild obligation driven by
  other people's OS releases. Closed on prior experience.
- **K4 Slint.** Own renderer, real accessibility, declarative markup. Closed on
  its licence lanes — GPLv3 binaries or royalty-free with attribution — and on
  adding a third language.
- **K5 cef-rs.** The only Rust-owns-the-process option with one engine
  everywhere. A binding, not a framework: no IPC, no updater, no installer
  generation, and macOS signing rewritten for CEF's helper-process bundles.
  Nobody ships a conventional desktop app on it.
- **Ruled out earlier.** egui, on a documented full re-layout of large scroll
  areas every frame. Dioxus, mid-migration to a beta renderer with no
  notarisation story. GTK4, no Windows accessibility. CXX-Qt, pre-1.0 with QML as
  a fourth language. Verso and Blitz, not ready. Sciter and Ultralight, closed
  source.

**Iced's own costs, accepted.** No accessibility at all, issue #552 open since
2020. No stylesheet, so every widget carries its style explicitly. No
letter-spacing. Upstream is effectively one maintainer, though the sub-crates do
get patches — `iced_widget` is at 0.14.2 against the facade's 0.14.0.

---

## Q. The two prototypes, and how to run them

Both live under `prototypes/studiov2/`, each kept out of the `rust/` workspace by
an empty `[workspace]` table in its own `Cargo.toml`. No tracked file was
changed to add them. Each carries a short README.

**Q1. Slot builder — `prototypes/studiov2/slot-builder/`**

```
cd prototypes/studiov2/slot-builder && cargo run
```

Reproduces the site's One ROM Builder tab: board picker, firmware version, the
board wireframe and jumper legend drawn on a canvas, slot cards with per-chip
control lines, the live flash usage bar, plugin pickers, build and save.

It is not a mock. Board list, jumper header geometry, chip types, chip sizes,
how many chip-select dropdowns each chip type gets, file formats and the flash
total all come from `onerom-config` and `onerom-gen`. Build runs through
`Builder::from_json` and produces metadata and ROM images with the real USB
plugin binary embedded.

Two gaps worth carrying forward. **Build output is metadata and ROM images — a
flashable image also needs the base firmware downloaded and padded to 48KB on
the front.** **ROM types come out in the crates' order, not the website's**,
because the website sorts them in `js/site/utils.js` with a hand-written size
table. That sort belongs in Rust, reachable from the apps and from WASM. It is
one visible case of a wider question — how much of that JS is logic that should
sit in Rust.

Also faked: firmware versions are a hardcoded list rather than a manifest fetch,
the plugin catalogue is read off disk so no `min_fw_version` check runs, and
there is no licence-acceptance step. No device I/O at all.

About 1,200 lines of UI and drawing, 300 of them the wireframe. The JS
equivalent is roughly 1,100 lines of JS plus 90 of HTML and 480 of CSS. Clean
build 58s, incremental 2.6s.

Against the HTML: Iced has no letter-spacing, so the small-caps group titles are
untracked, and there are no tooltips or transitions. Dropdowns and scrollbars are
Iced's rather than the platform's. Fonts are close but not exact.

For review without a screen-recording grant: `ONEROM_SPIKE_SHOT` is a path to
write a screenshot to, `ONEROM_SPIKE_SIZE` is `WIDTHxHEIGHT`, and
`ONEROM_SPIKE_SETUP` is a comma-separated script (`board:fire-28-d`, `add`,
`chip:0:23128`, `file:0:/path`, `nohelp`).

**Q2. Log and console panes — `prototypes/studiov2/log-viewer/`**

```
cd prototypes/studiov2/log-viewer && cargo run --release
```

`--help` lists the switches. `--selftest --quit-when-done` proves selection and
copy against the real system clipboard, and **overwrites the clipboard**.
`--console-demo` runs a scripted device session. `--bench 1000,9000,90000
--chunk 500` reproduces the append timings. `cargo test --release` runs seven
tests that drive the widget with real mouse and keyboard events and a recording
clipboard.

A drag selects exactly the right lines, Cmd+C puts them on the clipboard
byte-for-byte, and edits are refused without corrupting the buffer. The console
pane is a read-only editor for scrollback plus a text box for input, with
command history.

Two things it found. `iced_term` cannot serve a serial console — it hardcodes
spawning a PTY and its backend type is private, so feeding it device bytes means
forking 576 lines. And async tasks returned from separate updates have no
ordering between them, so several commands in flight reply out of order. One
command in flight fixes it, which is what a device shell wants anyway.

---

## R. Log output

**R1. The log keeps everything the device sends**, for the whole session, no cap.
The widget shows a window of about 120 lines over it. Its cost does not grow
with the log.

**R2. The log is not bounded by what the widget can hold.** How many lines fit
inside Iced's text widget before it stutters is a question about the widget, and
has no bearing on how much log a session keeps.

**R3. The windowed design works, on stock widgets, with no custom text widget.**
Built and measured in the prototype. The scrollbar represents the whole log and
its position is exact. Selection and copy work across ranges far larger than the
window — select-all on 120,000 lines while the widget holds 120, answered from
storage. Live tail slides correctly, and scrolling up holds position while the
log grows behind you. Search runs over the whole log off the update thread and
scrolls to the hit.

**R4. What a million lines costs.** On a 32GB M5 MacBook Air: about 20 MB of
memory over an empty app, jump anywhere in 15-18 ms, search the whole log in
51 ms. The same million lines held in the widget instead reaches 699 MB with
121 ms stalls, and can neither search nor jump.

**R5. Two things to know before building on it.** Every window move costs about
7 ms, nearly all of it Iced rebuilding text it is about to rebuild — comfortable
today with no headroom, and removable either by appending rather than rebuilding
or by a small addition to Iced itself. And Iced publishes no drag event once the
pointer leaves the widget, so dragging a selection past the edge needs its own
auto-scroll. That is 30 lines, and the prototype has it.

---

## T. How the app is structured

A shared crate at the bottom holds the shared state objects and the styling.
Each screen is a crate above it, independently buildable, with a small binary
that runs it standalone. The app orchestrates the screens and holds the shared
state. Built, not designed on paper: four crates (`shared`, `slot-builder`,
`log-viewer`, `shell`), three binaries, 24 passing tests, under
`prototypes/studiov2/`.

**T1. The shared crate exists for the palette.** One window has one theme, so a
palette owned by one screen is one the other cannot have. Screens-only means
duplicating it, or a sideways dependency.

**T2. The crate split buys compiler enforcement and costs almost nothing.** A
reach from the log screen into the builder is `E0433`, a hard error. In one
crate with several binaries it compiles with an unused-import warning. The split
cost 73 changed lines, every one a `use` statement. Clean build 41.1s against
34.2s, incremental 1.1s against 1.7s.

**T3. What turned out to be shared.** The log. The device selection, but not the
attached-device list. The built image, cut to a name, a description and bytes.
And, on nobody's list beforehand, 280 lines of styling. Nothing else wanted
sharing.

**T4. One `&Shared` per call, not a borrow per shared thing.** Studio's
`Create::view` takes four borrows and worsens as more is shared, because it
passes pieces. One `Shared` does not grow with what is in it.

**T5. Some work appears only at composition time.** Under one window the log
screen's stock buttons rendered gold against the One ROM palette. 11 buttons,
4 text inputs and a pick list needed explicit styles. Compose early.

**T6. Sharing the log forces a refactor, in any structure.** The log view owned
its store and can no longer, so 16 methods grew a `store: &Store` parameter,
across about 50 call sites.

**T7. Plumbing is per screen, not per message.** Two screens cost about 63 lines
of it, against 297 lines of shell and ~5,700 of screens. A standalone runner is
a miniature app owning the shared state, about 50 lines a screen.

**T8. Iced specifics.** Iced's `Component` trait is deprecated and was not
needed. Subscriptions map cleanly, but the app must keep a hidden screen's
subscription running or the log stops with no sign. Fonts and the window title
are application-level. A screen can call `iced::exit()`, so a screen can kill
the app.

**T9. Where these results hold.** They were measured with two screens. At twelve
the orchestrating `update` carries twelve arms and the shared state perhaps
fifteen fields, which is worth measuring again when it gets there. Every screen
sees the whole shared state, and the compiler guarantees one thing about it —
that a screen cannot reach another screen's code (T2). Which parts of the shared
state a screen reads is left to whoever writes it. Giving each screen a smaller
object holding only what that screen needs would let the compiler police that
too, at the price of the long parameter lists T4 avoids.

**T10. The shared crate stays dependency-light** — `iced` and `thiserror`. That
is what forced the image down to bytes rather than `onerom_gen::Built`, and the
board down to a string.

---

## J. Do this first

**Lift the compose-and-program pipeline out of the CLI binary into the library**,
with decisions separated from the asking, and a progress channel in place of
`println!`. Size: **M, ~2-3 kLOC**, mostly from `firmware.rs` and `program.rs`.
Its log-streaming interface should not assume USB, since SWD may return for
logging.

Its absence is why Studio and Web each grew their own half-built pipeline, and
why neither supports firmware overrides or licence acceptance. It makes CLI/GUI
parity structural rather than a discipline — both drive one library, so a new
capability lands once.

**Size of the rest: L, ~6-8 kLOC** on top of J.

---

## P. Generating the GUI from the CLI

**P1. Across all 44 shipping commands: 27% generate well, 61% generate badly,
11% cannot.** Judged, not measured. Section V built them and counted 49 leaf
commands and 173 options, and V1 has the real split. clap does not carry what a
good pane needs — of the options, five have a value set clap can see, and
`--board` is a bare string on fourteen commands. Colour, pin, chip type and
firmware version all sit behind opaque parsers, and the legal pin set depends on
the connected board.

The finding that decides this: **generation is strongest where a GUI adds least,
and weakest where it adds most.** It handles file-in, file-out image conversion
well — a command anyone would keep doing from the CLI. It handles pin control,
the socket and header drawings, LED colour and anything reading device state
badly, and those are the panes that justify a GUI existing.

**P2c. Generating from clap plus an annotation file pays for about a dozen
commands, and not for the rest.** clap can walk its own command tree, which is
how `clap_complete` and `clap_mangen` work, so a hidden `--dump-schema` could
feed the GUI's build script. For two-thirds of the commands the annotation
needed to rescue a generated pane would be longer than the clap definition it
annotates, which makes it a hand-written schema in disguise. Worse, the ranges
and defaults it would carry already live in `metadata_schema.toml`, so it would
be the second copy that the project's cross-project-constants rule exists to
prevent.

**P3. The gate is worth more than the generation, costs less, and does not
depend on it.** A build
failure when the CLI grows a capability the GUI does not expose delivers most of
the value on its own. `cli_assert` in `rust/cli/src/main.rs` already walks the
whole clap tree and already fails the build on a rule breach, so this is another
assertion in a mechanism that exists. Hand-written screens declare which commands
they cover, so the gate covers everything. One fork inside it stays open: whether
the gate is satisfied per command or per option. Per command lets a pane expose a
command while missing half its flags. Per option is real pressure and noisy at
first.

**P4. Wherever generation is used, hold the line deliberately.** `onerom-metadata` works because a struct
and a parser are right or wrong, never ugly. GUI panes have aesthetics, and the
failure mode is the first pane that must look different: either an escape hatch,
and the generator stops being the source of truth, or a schema contorted into a
worse GUI toolkit. Every individual exception will look reasonable.

---

## U. Possible next steps

Each is a prototype in the same style as the two that exist — small, runnable,
and built to answer one question that would otherwise be argued about.

**U1. Lift the compose-and-program pipeline into the CLI library (J).** Not a
prototype. It is the first piece of product work, and it is what lets the slot
builder produce a flashable image and put it on a device, the gap named in its
own README. It improves the CLI on its own.

**U2. A device screen, against real hardware.** Both prototypes are offline. The
builder does no I/O and the log viewer runs off a synthetic generator. Talking
to hardware is the app's reason for existing, and it is the part the existing
Studio got worst. Its device calls returned the GUI's message type, its
multi-step operations were hand-built state machines, and it waited for a
rebooting device with a fixed one-second sleep.

The screen does what `onerom scan --slots` does — every attached device with its
board, MCU, firmware version, state and serial, and each slot's chip type, size,
chip-select configuration, source and plugin name. It adds the full parse from
`onerom-fw-parser`, the whole metadata and runtime tree that `inspect info`
dumps as JSON.

That last part is where a GUI beats the CLI rather than matching it. Every byte
of device state is reachable by design, and today it arrives as a JSON dump few
people read. The same data as a tree a user can expand, search and copy out of
is usable.

What it tests beyond how it looks: enumeration while devices are plugged and
unplugged, the reboot where a device returns at a different USB address, a real
log stream rather than a generated one, and what a device in the wrong state
does to the screen. It fails if the async plumbing pushes the design back
towards Studio's shape.

**U3. Generated panes across the whole CLI. Built — section V has the answer.**
It asked for the real split rather than the judged one, what the generator costs
to write, and what an annotation file has to carry measured against the whole
surface. All three came back, along with a fourth thing nobody asked for: what it
would take to make a generated pane actually run its command.

**Order.** U2 does not need U1 — the device surface it wants is already in the
CLI library. U1 is the first piece of product work rather than a dependency.

---

## V. Generated panes across the whole CLI

The third prototype, answering U3. Two crates and a third tab in the shell.
`commands/`'s build script reads `rust/cli/src/args/` with `syn` and emits plain
data — 49 leaf commands, 167 command options, 6 globals — as `no_std` with
`alloc`, no clap and no iced, so a browser through WASM could read the same
description. `generated/` is an Iced screen that draws a pane from that data and
nothing else. **No line of GUI code names a command.**

```
cargo run -p studiov2-generated   # the panes alone, --list names them
cargo run -p studiov2-shell       # beside the builder and the log panes
```

fmt, clippy, tests and docs are all green. 103 tests across the workspace, 78 of
them here.

**V1. All 49 panes generate, and 45 of them have no field a user must type into
blind.** A command added to the CLI gets a pane on the next build with nobody
writing GUI code. 62 of the 173 options carry a value set, and every one of them
was a bare box before: 28 files and directories, 14 boards, 5 firmware versions,
5 colours, 4 pins, 3 chip types, a plugin type, a serial and a CLI release. 15
options stay a bare box, 6 of those genuinely free text — names and
descriptions. The four panes with a blind field are `program`, `firmware build`,
`image convert` and `self download`.

**V2. The annotation costs 38 lines, against about 330 lines of clap
definition.** P2c expected the opposite. Two tables, neither holding a value —
both name the crate to ask. 10 lines derive 36 options from the CLI's own
placeholders, and 28 lines name the other 26 one at a time.

`BOARD`, `COLOUR`, `PIN` and `CHIP` already say what the values are, because the
CLI author wrote the placeholder for a user. 25 of the 26 hand-named options are
`FILE`, the one placeholder covering two different things — a file to read has
to exist and a file to write must not. Only 5 of the 173 options have a value
set clap itself advertises, which is P1's point, and the placeholder recovers
most of what P1 thought was lost.

**V3. clap carries argument relationships, which was not expected.** 4 commands
declare an `ArgGroup`, and 16 say something of the kind once `conflicts_with`
and `requires` are counted, across 92 conflict names and 9 requirements. It is
the only grouping written down anywhere. `control erase` is the case to look at:
exactly one of `--all`, `--offset` and `--address` is required, `--offset` needs
`--length`, and `--stopped` and `--running` exclude each other. The pane draws
that as two labelled groups, and enforces it.

**V4. The three commands U3 named.**

| command | expected | came out |
| --- | --- | --- |
| `image convert` | well | two real value sets, two file pickers, runs for real — `--load-address` is the bare box |
| `control rgb on` | badly | swatch yes, brightness slider no, because no bound is written down |
| `inspect gpio` | decides it | pin, board and flag all real. The table it prints is guessed |

The pattern across all three: the **input** half of a pane generates well, and
the **output** half is where the description runs out.

**V5. Nothing outside the CLI binary can run a CLI command.** Four commands run
for real — the three under `image`, plus `firmware chips` — and they are **454
lines of re-implementation inside the GUI crate** calling `onerom-gen` and
`onerom-config`, not calls into the CLI. Every `cmd_*` lives in the binary
(`rust/cli/src/image.rs` and its siblings) and the dispatch is one match from
`rust/cli/src/main.rs:59` onwards. `rust/cli/src/lib/` holds device primitives,
not commands.

That is a limit of the prototype rather than of the approach. It cannot edit the
CLI, so it re-implemented four commands and found them by matching on the
options a command takes, to avoid writing a command's name down. Studio v2 moves
that dispatch and the `cmd_*` functions into `onerom-cli`'s library behind one
entry point, and a pane hands over the command it already holds — no per-command
GUI code, no name, all 49. It is the same lift as J and belongs with it.

**V6. What the description does not carry**, found by building the panes:

- **A bound on a number.** 22 of the 46 numeric options are milliseconds and 6 a
  percentage, so none of them can be a slider. The bounds exist as named
  firmware constants.
- **Which option belongs beside which.** V3's relationships cover the
  constraint. Nothing covers the layout.
- **Whether a command needs a device.** `requires_device()` is a trait
  implementation on the parsed arguments, invisible to anything walking the
  definitions.
- **What a command prints, and the prose it prints around it.** `firmware chips`
  prints a trailing note and a plugin-type list, and the pane drops both without
  a word. `stub::shape` guesses table, tree or drawing from the first and last
  word of a command's path, which is the one placeholder that reads command
  words at all.
- **That one option scopes another.** A chip-type list could narrow to the board
  chosen in `--board`.

**V7. Costs and limits.**

- **`program` (28 options) and `firmware build` (18) are the worst panes.**
  `program --slot` takes `file=…,type=23128,cs1=active-low` — a language inside
  one option — and will not generate however much the description carries. The
  slot builder is the hand-written answer to the same command, and the shell
  shows both.
- **A pane per leaf command is not obviously right**, which is S3 in the flesh.
  Six commands are a title and a Run button. `control rgb` is seven panes, four
  of them with the same four options.
- **Faked, and the screen says so.** Browse fills in a sample path rather than
  opening a dialog, the version lists are written out rather than fetched, and
  the device list is invented. 45 panes are stubbed, and every pane says which
  it is.

**V8. A pane is generated or hand-written, never both.** A generated pane is a
fair starting point for a hand-written one. Editing it moves that command onto
the list P3 already needs — the commands a hand-written screen declares it
covers — and it stops being generated.

That answers the first half of P4, the escape hatch that quietly stops the
generator being the source of truth. There is no half-generated pane to patch,
so an exception has to be a whole pane and is visible as one. P4's other half, a
description contorted to suit a GUI, is untouched by this, and V6 is where it
would show.

The cost is that a hand-written pane stops picking up a new CLI option for free,
where a generated one has it on the next build. P3's gate is what catches that.

**V9. A command should return its output rather than print it.** V6 records that
the description says nothing about what a command prints, so `stub::shape`
guesses from the words in a command's path. This is the answer to that bullet.

Today a `cmd_*` writes to stdout, so what it produced exists only as text on a
terminal. Return a value instead, and the CLI binary formats it for a terminal
while a GUI renders the same value its own way. The prototype's own model took
real output without changing:

```rust
enum Output {
    Line(String),
    Table { headers, body },   // body is flat rows, or rows under headings
    Drawing(String),           // fixed-pitch, a socket or a jumper header
    Tree(Node),                // a parsed firmware image
    Nothing,
}
```

Those five carried the four commands V5 runs for real. They do not carry the
prose a command prints around a table — `firmware chips` prints a trailing note
and a plugin-type list, and both are lost.

It is not annotation. It falls out of the signature, `cmd_chips(...) ->
Result<Output, Error>`, so the compiler holds it to what the command does. It is
the same lift V5 describes rather than work beside it, because a `cmd_*` cannot
move into the library while it prints and prompts. The CLI gains on its own too:
every command could answer in JSON, where `inspect telemetry` is the only one
that does today.

Two of the design questions below moved during this work. **S2** was answered by
running these panes in the shell, and **S4** was found here.

---

## Design questions found, not answered

Recorded so they are not lost. P2c and S2 have since been answered and say
where. The rest still stand.

- **P2c** — answered by V2. The annotation is 38 lines against about 330 lines
  of clap definition, and it covers the whole CLI rather than the dozen commands
  this expected. Most of it derives from the placeholders the CLI already
  writes, so the file names a source and never carries a value.
- **P3** — whether the CI gate is satisfied per command or per option. Per
  command lets a pane expose a command while missing half its flags.
- **S1** — the ROM-type sort order belongs in Rust, reachable from the apps and
  from WASM, rather than in `js/site/utils.js`. It is the visible end of a wider
  question: what else in that JS is logic that should sit in Rust.
- **S2** — answered, and it was worse than stated. The log pane rebuilt its
  whole 120-line window on every tick, and the slot builder its 200-line tail,
  each shaping every line twice. That cost the same at 20 lines a second as at
  5,000, and left no headroom for macOS App Nap, which demotes a background
  window to an efficiency core after about 30 seconds and runs it four times
  slower. The shell pinned a core and stopped drawing. Sliding the window
  instead of rebuilding it holds 22% where it used to pin at 99%, and the cost
  now tracks the lines arriving.
- **S3** — whether the twelve LED and RGB commands become two panes with a mode
  selector. They are twelve of the CLI's 49 leaves, so the answer generalises.
  V7 has them on screen.
- **S4** — 11 of the CLI's 173 help texts name another option in CLI spelling,
  e.g. `--length` reads "paired with --offset or --address". A generated pane
  labels its controls `Offset` and `Address`, so the pane shows two spellings
  of the same thing. Closing it means rewording those doc comments in
  `rust/cli/src/args/`, which changes what `--help` prints — and a CLI reader
  may want the dashes. Not all 11 are the same case: one quotes a whole command
  line a user would type, where the spelling is right. Production needs this
  settled. The U3 prototype carries it as a known rough edge.
