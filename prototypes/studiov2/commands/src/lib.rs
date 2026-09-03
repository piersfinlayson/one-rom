// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Every One ROM CLI command and option, as plain data.
//!
//! The CLI's argument definitions are the master.  `build/` reads
//! `rust/cli/src/args/` at build time and writes this crate's contents, so a
//! command added to the CLI appears here on the next build with nobody typing
//! anything.
//!
//! Nothing here knows what a widget is.  A consumer — Studio's generated
//! screen today, a browser through WASM later — reads the same description and
//! draws it however it draws things.  That is why there is no `iced`, no
//! `clap` and no `std` in this crate's dependencies.
//!
//! Two things the description deliberately does not carry, because the source
//! it is read from does not know them: where an option's values come from, and
//! anything about how a pane should be laid out.

#![no_std]

extern crate alloc;

pub mod name;

include!(concat!(env!("OUT_DIR"), "/description.rs"));

/// One command a user can run, named by the words that reach it.
///
/// A command is always a leaf.  `onerom control rgb` takes no arguments of its
/// own and is not here — `onerom control rgb on` is.  The tree a menu needs is
/// recovered from [`Command::path`], which is what lets one flat list serve
/// both a list and a hierarchy.
pub struct Command {
    /// The words after `onerom`, e.g. `["control", "rgb", "on"]`.
    pub path: &'static [&'static str],

    /// The one-line description, from the command's doc comment.
    pub about: &'static str,

    /// The rest of the doc comment, where there is one.  Carries the worked
    /// examples the CLI prints under `--help`.
    pub long_about: Option<&'static str>,

    /// The command's own options, in the order they are declared.
    ///
    /// The options every command accepts are not repeated here — see
    /// [`GLOBALS`].
    pub opts: &'static [Opt],

    /// Sets of options the CLI treats as one choice.
    ///
    /// Four commands declare one of these, and 16 say the same thing one way
    /// or another once [`Opt::conflicts`] is counted.  It is the only grouping
    /// written down anywhere, and it is a stronger statement than a heading —
    /// it says a user may pick one of these and not two.
    pub groups: &'static [Group],
}

/// Options the CLI treats as one choice, from a clap `ArgGroup`.
pub struct Group {
    /// The group's name, which [`Opt::conflicts`] and [`Opt::requires`] may
    /// name in place of an option.
    pub name: &'static str,

    /// The long names of the options in it.
    pub opts: &'static [&'static str],

    /// Whether one of them has to be given.  With `!multiple`, that is a
    /// radio group with no way to pick none.
    pub required: bool,

    /// Whether more than one may be given at once.
    pub multiple: bool,
}

/// One option of one command.
///
/// Every One ROM CLI argument is `--name value` or a bare `--flag`, so there
/// is nothing here for a positional.
pub struct Opt {
    /// The long name, without the dashes.
    pub long: &'static str,

    /// Other spellings that reach the same option, such as `color` for
    /// `colour`.  Not used to build a command line — they are here so a
    /// search for what a user calls the thing finds it.
    pub aliases: &'static [&'static str],

    /// The option's help text.
    pub help: &'static str,

    /// The placeholder the CLI shows for the value, e.g. `FILE`, `MS`,
    /// `PERCENT`.
    pub value_name: Option<&'static str>,

    /// What sort of value it takes.
    pub kind: Kind,

    /// Whether the option can be left out.  Read from the field being
    /// `Option<T>` rather than `T`.
    pub optional: bool,

    /// Whether the option can be given more than once.  Read from the field
    /// being `Vec<T>`.
    pub multiple: bool,

    /// The value used when the option is left out, where the CLI states one.
    pub default: Option<&'static str>,

    /// Options and [`Group`]s that cannot be given alongside this one.
    ///
    /// A name here is resolved against the command's options first and its
    /// groups second — the CLI writes both, e.g. `conflicts_with =
    /// "reboot_mode"` names a group.
    pub conflicts: &'static [&'static str],

    /// Options and [`Group`]s that have to be given when this one is.
    pub requires: &'static [&'static str],

    /// Where the option's values come from, where anything knows.
    ///
    /// A name rather than a list, and an enum rather than a name, so a
    /// consumer that does not recognise one cannot compile.  The values
    /// themselves stay in the crate that owns them.
    pub source: Option<Source>,
}

/// Where an option's values come from.
///
/// This is the half clap cannot express.  `--board` takes one of the boards
/// this build knows, and clap sees a string, because the check is a
/// hand-written parser rather than a value set.
///
/// Naming the source rather than carrying the values is what lets one
/// description serve a desktop app and a browser: neither can be handed a Rust
/// function, and both can look a name up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// A file that has to exist.
    OpenFile,
    /// A path to write, which need not exist yet.
    SaveFile,
    /// A directory.
    Directory,
    /// A board this build supports.
    Board,
    /// A chip type.
    ChipType,
    /// A colour, by name or as hex.
    Colour,
    /// A pin on the board in front of the user.
    ///
    /// The only source whose values depend on something outside the
    /// description — which board is connected.
    Pin,
    /// A released firmware version.
    Version,
    /// A released version of the CLI itself.
    ///
    /// Separate from [`Source::Version`] because the two are different lists.
    /// `self download --version` takes a CLI release, and offering it firmware
    /// versions would be confidently wrong rather than merely unhelpful.
    CliVersion,
    /// A One ROM on the bus, by serial.
    ///
    /// The serial of a device to act on, which can be offered as a list.  Not
    /// the serial a user is about to *give* a device — `--serial-override`
    /// takes one of those and nothing can offer a list of it.
    Serial,
    /// A plugin type.
    PluginType,
}

impl Opt {
    /// Whether a user has to supply a value before the command can run.
    ///
    /// A repeatable option never has to be: clap accepts none of it, whatever
    /// the field's type says, so `Vec<T>` reads as optional even though it is
    /// not written `Option<Vec<T>>`.  Without this, `--vid-pid` blocks the Run
    /// button on every command in the CLI.
    pub fn must_supply(&self) -> bool {
        !self.optional
            && !self.multiple
            && self.default.is_none()
            && !matches!(self.kind, Kind::Flag)
    }
}

/// What sort of value an option takes.
///
/// This is read from the Rust type of the field, which is the only place it is
/// written down — clap keeps a name, an action and a count, and throws the
/// type away.
pub enum Kind {
    /// A bare `--flag`, from a `bool` field.
    Flag,

    /// Free text, from a `String` field.
    Text,

    /// A whole number, from a `u8`, `u16`, `u32` or `usize` field.
    ///
    /// The bounds are not here.  Where a limit exists it is a firmware
    /// constant with a name, and naming it is a separate job from reading the
    /// argument definitions.
    Number,

    /// One of a fixed set the CLI itself advertises.
    ///
    /// Five options out of 173 reach here — two formats, two GPIO states and
    /// the log level.  Every other value set in this tree sits behind a
    /// hand-written parser that says nothing about what it accepts, and lands
    /// in [`Kind::Domain`] instead.
    Choice(&'static [&'static str]),

    /// A value of a type this crate does not model, named by that type —
    /// `RgbColour`, `Pin`, `LoadAddress`.
    ///
    /// A consumer with no opinion about the name shows a text box, which is
    /// what the CLI accepts anyway.  A consumer that recognises it can offer
    /// the real values.
    Domain(&'static str),
}
