// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Where each option's values come from, in the two tables that decide it.
//
// The file is pasted in rather than compiled on its own, by `build/source.rs`
// to fill `Opt::source` and by `tests/sources.rs` to check the tables against
// the CLI.  That is why it opens with a plain comment and names a `Source` it
// does not declare - the two readers each bring their own, and a build script
// cannot depend on the crate it is building.
//
// Neither table holds a value.  The board names live in the crate that knows
// the boards, and what is written here is only which crate to ask.
//
// `cargo fmt` does not follow an `include!`, so this file is the one part of
// the crate the fmt gate never sees.  Keep it in rustfmt style by hand.

/// The placeholder an option shows, and what that name says about its values.
///
/// The CLI writes a `value_name` on nearly every option that takes one, and
/// most of those names already answer the question.  This is what makes the
/// mapping cost almost nothing to keep: a new `--board` on a new command
/// arrives with its source already attached, because the CLI author wrote the
/// placeholder for a user rather than for us.
///
/// `DEVICE` rather than `SERIAL` reaches [`Source::Serial`], and the two are
/// opposite ends of the same word.  `--serial DEVICE` picks one of the One
/// ROMs on the bus, so a pane can list them.  `--serial-override SERIAL` is
/// the serial a user is about to *give* a One ROM, which nothing can offer a
/// list for.
pub static DERIVED: &[(&str, Source)] = &[
    ("BOARD", Source::Board),
    ("CHIP", Source::ChipType),
    ("COLOUR", Source::Colour),
    ("DEVICE", Source::Serial),
    ("DIR", Source::Directory),
    ("TYPE", Source::PluginType),
    ("VERSION", Source::Version),
];

/// Options one at a time, where [`DERIVED`] cannot reach them or reads them
/// wrong.
///
/// An entry beats a placeholder, so this does both jobs.  Almost all of it is
/// the first because two placeholders each cover two different things.  Which
/// one an option means is in its help text and in nothing a machine can read.
///
/// - `FILE` is a file to read that has to exist or a file to write that need
///   not.
/// - `PIN` is a pin on the board or the PIN of a key on a signing server.
///
/// The second job has one entry.  `self download --version` shows `VERSION`
/// like the firmware options do and takes a CLI release rather than a
/// firmware one, so it is the single place a placeholder means something
/// different on one command from what it means everywhere else.
///
/// The first field is the words after `onerom`, and the second is the option's
/// long name without its dashes.  An empty path is a global option.
///
/// An entry naming a command or an option the CLI does not have fails the
/// build.  The CLI is the master, and a table still pointing at something it
/// dropped is a table nobody re-read.
pub static ANNOTATIONS: &[(&str, &str, Source)] = &[
    ("program", "config", Source::OpenFile),
    ("program", "save-config", Source::SaveFile),
    ("program", "firmware", Source::OpenFile),
    ("program", "base-firmware", Source::OpenFile),
    ("program", "output", Source::SaveFile),
    ("program", "reset-host", Source::Pin),
    ("image convert", "input", Source::OpenFile),
    ("image convert", "output", Source::SaveFile),
    ("image swap-bytes", "input", Source::OpenFile),
    ("image swap-bytes", "output", Source::SaveFile),
    ("image deinterleave", "input", Source::OpenFile),
    ("image deinterleave", "output", Source::SaveFile),
    ("inspect image", "output", Source::SaveFile),
    ("inspect peek live", "output", Source::SaveFile),
    ("inspect peek memory", "output", Source::SaveFile),
    ("inspect gpio", "pin", Source::Pin),
    ("control poke live", "input", Source::OpenFile),
    ("control poke memory", "input", Source::OpenFile),
    ("control reset", "pin", Source::Pin),
    ("control pin", "pin", Source::Pin),
    ("monitor log", "output", Source::SaveFile),
    ("console", "output", Source::SaveFile),
    ("update slot", "image", Source::OpenFile),
    ("hardware commission", "key", Source::OpenFile),
    ("hardware sign", "key", Source::OpenFile),
    ("firmware build", "config", Source::OpenFile),
    ("firmware build", "save-config", Source::SaveFile),
    ("firmware build", "base-firmware", Source::OpenFile),
    ("firmware build", "output", Source::SaveFile),
    ("firmware inspect", "firmware", Source::OpenFile),
    ("firmware download", "output", Source::SaveFile),
    ("self download", "output", Source::SaveFile),
    ("self download", "version", Source::CliVersion),
];
