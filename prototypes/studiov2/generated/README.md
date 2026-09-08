# Generated command panes prototype

A pane for every One ROM CLI command, in Iced 0.14, generated from the CLI's
own argument definitions.

**Question.** If GUI panes are generated from the CLI rather than written, what
comes out?

**Answer. 49 panes, no line of GUI code naming a command, and 45 of them with
nothing left to type blind.** The four that are not are `program`,
`firmware build`, `image convert` and `self download`.

**Four of the 49 run for real**, against the crates the CLI itself calls. The
rest are stubbed, and every pane says which it is.

## Run

```bash
cargo run -p studiov2-generated       # the panes alone
cargo run -p studiov2-shell           # beside the builder and the log panes
cargo run -p studiov2-generated -- --list
```

`cargo test --workspace` runs 103 tests, 39 of them here.

## Where the panes come from

[`studiov2-commands`](../commands) reads `rust/cli/src/args/` at build time and
emits plain data — 49 commands, 167 options, 6 globals. This crate reads that
and nothing else. **No code here names a command.** A command added to the CLI
reaches the screen on the next build with nobody typing anything, or it does
not reach it at all.

The widget for an option is decided in one place, `ui::value_widget`. The
description says two things about an option: what sort of value it takes, and
where its values come from. The second is a name — `Board`, `Colour`, `Pin` —
and [`resolve`](src/resolve.rs) is the one place that turns a name into values,
reading them out of `onerom-config` and `onerom-cli`. A board added to the tree
reaches every board picker with nobody typing anything.

| what the description says | widget |
| --- | --- |
| values from a set the CLI takes nothing outside of | pick list |
| values from a set the CLI takes others besides | text box, and a pick list |
| a file or a directory | text box, and a Browse button |
| a flag | checkbox |
| a fixed set of values | pick list |
| a number | text box that refuses non-digits |
| text, or a type it does not model | text box |

## What runs

Four commands do their real work: the three under `image` and `firmware chips`.
`onerom-gen` does the conversions and the transforms, `onerom-gen` and
`onerom-config` between them do the chip fit figures, and a failure comes back
as `onerom-cli`'s own error in the words the CLI would have used.

**They are not calls into the CLI.** They are re-implementations here, because
every `cmd_*` lives in the CLI's binary and nothing outside it can call one.
`rust/cli/src/lib/` holds device primitives, not commands.

That is a limit of this prototype rather than of generating panes. It cannot
edit the CLI, so it re-implemented four commands and found them by matching on
the options a command takes — a way to reach per-command code without writing a
command's name. A production app would instead move the CLI's dispatch and its
`cmd_*` functions into `onerom-cli`'s library behind one entry point, and a pane
would hand over the command it already holds.

## What came out

**45 panes have no bare text box on them.** Every option is a control that
knows what it is for: a checkbox, a number, a pick list of the real values, or
a path with a Browse button.

**62 of the 173 options carry a value set**, and every one of them was a bare
box before: 28 files and directories, 14 boards, 5 firmware versions, 5
colours, 4 pins, 3 chip types, a plugin type, a serial and a CLI release. `--colour` gets its
ten names and a swatch of whatever is chosen. `--pin` gets the pads of the
board in front of the user, and falls back to a box with no device connected,
because which pads exist is a fact about the board.

**Two of them draw their options as groups, because clap carries argument
relationships**, which was not expected. `control erase` declares that exactly one of `--all`,
`--offset` and `--address` is required, that `--offset` needs `--length`, and
that `--stopped` and `--running` exclude each other. The pane draws that as two
labelled groups with the constraints enforced. `control reboot` is the other.
16 of the 49 commands say something of the kind, and it is the only grouping
written down anywhere.

**15 options are still a bare box**, and six of them are genuinely free text —
names and descriptions. The rest are `--slot`, `--plugin`, `--load-address`,
`--serial-override`, `--vid-pid` and `self download --target`, which take
values no list can hold.

**A long table keeps its sections and scrolls inside its own bounds.**
`firmware chips --board fire-28-d` is 49 rows under three headings — native,
overhang, fly-lead — and `Output::Table` carries them, so the pane draws the
same three. The rows scroll in a region 420 pixels tall with the column titles
fixed above it, which keeps the options, the command line and the Run button on
screen beside the answer. A short table keeps its own height and shows no
scrollbar. Neither half names a command: the renderer draws whatever sections it
is handed, and the sections come from how `supported_chips` already orders its
entries.

**One command will not generate however much the description carries.**
`program --slot` takes `file=…,type=23128,cs1=active-low,cs2=active-low` — a
language of its own inside one option. `program` has 28 options and
`firmware build` has 18, and they are the two worst panes here. The builder
prototype next door is the hand-written answer to the same command, and the
shell shows both.

## What the description does not carry

Found by building the panes, not by reading the argument definitions:

- **A bound on a number.** 22 of the 46 numeric options are milliseconds and 6
  are a percentage, so `--brightness` and `--hold` draw the same box and
  neither can be a slider. The bounds exist as named firmware constants.
- **Which option belongs beside which.** Groups cover the constraint. Nothing
  says `--colour` and `--brightness` belong side by side.
- **Whether a command needs a device.** It is a trait implementation on the
  parsed arguments, so nothing walking the definitions can see it.
- **What a command prints.** A table, ASCII art, a tree, a line, or nothing.
  `stub::shape` guesses from the words in a command's path, and the guess is
  marked as one.
- **That one option scopes another.** A chip-type list could be narrowed to the
  chips the board in `--board` supports, and a source names a set without
  saying what it depends on. Only `Pin` has a dependency, and it is on the
  connected device rather than on another option.

## Limits

- **45 of the 49 are stubbed**, and a checkbox forces the error path so error
  rendering can be seen. It has nothing to force on the four that run, and its
  label says so. No device, no network and no USB bus is touched by any of it.
- **A stubbed result is invented.** `stub::run` makes up content that fits the
  shape it guessed.
- **A pane per leaf command is not obviously right.** Six commands are a title
  and a Run button, and `control rgb` is seven panes, four of them with the same
  four options.
- **Three answers are faked, and the page says so.** Browse fills in a sample
  path rather than opening a dialog, the version lists are written out rather
  than fetched from `images.onerom.org`, and the device list is invented. The
  sentence on screen is `resolve::FAKED`. On a pane that runs, the sample path
  is one the library really opens and does not find, so Browse followed by Run
  is a real failure in the library's own words.

## Screenshots

`ONEROM_PROTO_SHOT` is a path to write a PNG to, `ONEROM_PROTO_SIZE` is
`WIDTHxHEIGHT`, and `ONEROM_PROTO_SETUP` is a comma-separated script —
`cmd:<path>` selects a command, `fill` answers everything it insists on,
`set:<option>:<value>` answers one, `run` presses Run, `bottom` scrolls the page
to the end so a result is in shot, and `table:<pixels>` scrolls a table result
that far down its own region. `src/dev.rs` lists the lot.

```bash
ONEROM_PROTO_SHOT=/tmp/erase.png ONEROM_PROTO_SETUP="cmd:control erase" \
  cargo run -p studiov2-generated
```

A real run wants real files, and `set:` is how they get in — its value is
everything after the option name, so a path goes in whole:

```bash
ONEROM_PROTO_SHOT=/tmp/convert.png ONEROM_PROTO_SIZE=1180x900 \
ONEROM_PROTO_SETUP="cmd:image convert,set:from:binary,set:to:ihex,\
set:input:/tmp/kernal.bin,set:output:/tmp/kernal.hex,run,bottom" \
  cargo run -p studiov2-generated
```

A `cmd:` naming no command exits non-zero rather than photographing the wrong
pane.
