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
11% cannot.** clap does not carry what a good pane needs — of 176 options, four have a value set clap can see, and `--board` is a
bare string on fourteen commands. Colour, pin, chip type and firmware version
all sit behind opaque parsers, and the legal pin set depends on the connected
board.

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

**U3. Generated panes across the whole CLI.** The value of generation is that a
new CLI capability reaches the GUI with nobody writing a pane. A handful of
commands cannot test that. Point the generator at the entire clap tree, produce
a pane for all 44 commands, and look at what comes out.

That gives what a paper classification cannot. The real split rather than the
judged one, what the generator costs to write, what the annotation file has to
carry measured against the whole surface, and whether generated panes sit beside
hand-built ones without looking out of place. It also puts the twelve LED and
RGB commands on screen as twelve entries (S3), which is easier to judge in the
flesh than in the abstract.

Three are worth looking at hardest. `image convert` has the only real value sets
in the CLI and should come out well. `control rgb on` wants a colour swatch and
a brightness slider and should come out badly. `inspect gpio` is the case that
decides it, with real fields, a real table, and a pin picker that depends on the
connected board. If that one is usable, generation is worth more than the 27% on
paper.

U3 doubles as the test T9 asks for, since a generated pane would be the third
screen in the shell.

**Order.** U2 and U3 are independent and either can start now. Neither needs U1.
The device surface U2 wants is already in the CLI library, and the commands U3
generates well are already library-backed. U1 is the first piece of product work
rather than a dependency for the other two.

---

## Design questions found, not answered

Recorded so they are not lost. None needs deciding to start.

- **P2c** — whether to generate GUI panes from the CLI's clap definitions plus a
  small annotation file. It pays for about a dozen commands out of 44.
- **P3** — whether the CI gate is satisfied per command or per option. Per
  command lets a pane expose a command while missing half its flags.
- **S1** — the ROM-type sort order belongs in Rust, reachable from the apps and
  from WASM, rather than in `js/site/utils.js`. It is the visible end of a wider
  question: what else in that JS is logic that should sit in Rust.
- **S2** — the log pane's window rebuild costs about 7 ms of work Iced then
  repeats. Appending rather than rebuilding removes most of it, and so would a
  small addition to Iced itself.
- **S3** — whether the twelve LED and RGB commands become two panes with a mode
  selector. They are 27% of the CLI's leaves, so the answer generalises.
