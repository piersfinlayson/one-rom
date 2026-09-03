// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Filling in where each option's values come from.
//!
//! clap knows an option takes a `String` and nothing more, so the description
//! would show a text box for a board name, a colour and a chip type alike.
//! What clap does carry is the placeholder the CLI shows a user - `BOARD`,
//! `COLOUR`, `CHIP` - and that placeholder is already the answer for most of
//! them.
//!
//! So this reads `sources.rs`'s two tables and applies them in one pass over
//! the finished walk: the placeholder first, and a hand-written entry where a
//! placeholder cannot say enough.  An option neither table reaches keeps
//! `None` and stays a text box, which is what the CLI gives a user anyway.

use crate::walk::{Description, Kind, Opt};

include!("../sources.rs");

/// Where an option's values come from.
///
/// A mirror of `Source` in `src/lib.rs`, the same way [`Opt`] and [`Kind`]
/// mirror their counterparts.  `sources.rs` names these variants and is read
/// both here and by the crate's own tests, so one spelling has to work in a
/// build script that cannot depend on the crate it is building.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    OpenFile,
    SaveFile,
    Directory,
    Board,
    ChipType,
    Colour,
    Pin,
    Version,
    CliVersion,
    Serial,
    PluginType,
}

impl Source {
    /// The variant's name, for writing the description out.
    pub fn variant(self) -> &'static str {
        match self {
            Self::OpenFile => "OpenFile",
            Self::SaveFile => "SaveFile",
            Self::Directory => "Directory",
            Self::Board => "Board",
            Self::ChipType => "ChipType",
            Self::Colour => "Colour",
            Self::Pin => "Pin",
            Self::Version => "Version",
            Self::CliVersion => "CliVersion",
            Self::Serial => "Serial",
            Self::PluginType => "PluginType",
        }
    }
}

/// Give every option its source, and say which were left without one.
pub fn apply(description: &mut Description) {
    check(description);

    for opt in &mut description.globals {
        opt.source = resolve("", opt);
    }
    for command in &mut description.commands {
        let path = command.path.join(" ");
        for opt in &mut command.opts {
            opt.source = resolve(&path, opt);
        }
    }

    note(description);
}

/// One option's source, from the annotation table first and the option's own
/// placeholder second.
///
/// An entry wins over a placeholder, so a placeholder that reads the wrong way
/// on one command can be corrected without dropping the rule that reads it
/// right everywhere else.
fn resolve(path: &str, opt: &Opt) -> Option<Source> {
    ANNOTATIONS
        .iter()
        .find(|(command, long, _)| *command == path && *long == opt.long)
        .map(|(_, _, source)| *source)
        .or_else(|| {
            let value_name = opt.value_name.as_deref()?;
            DERIVED
                .iter()
                .find(|(name, _)| *name == value_name)
                .map(|(_, source)| *source)
        })
}

/// Fail the build on an annotation entry that names nothing.
///
/// The two failures are told apart because the fix differs: a command that
/// moved wants the path corrected, and an option that went wants the line
/// deleted.
fn check(description: &Description) {
    for (path, long, _) in ANNOTATIONS {
        let opts = if path.is_empty() {
            &description.globals
        } else {
            &description
                .commands
                .iter()
                .find(|command| command.path.join(" ") == *path)
                .unwrap_or_else(|| {
                    panic!("sources.rs annotates onerom {path}, which is not a command")
                })
                .opts
        };

        assert!(
            opts.iter().any(|opt| opt.long == *long),
            "sources.rs annotates onerom {path} --{long}, which is not an option of it"
        );
    }
}

/// Say which options reach a user as a bare text box.
///
/// A missing entry is not an error - most options really do take free text,
/// and inventing a source for one would be worse than showing a box.  It is
/// still worth saying out loud, because the alternative is reading all 173 to
/// find the ones nothing has been decided about.
///
/// A number and a fixed set of values are left out: neither is a text box, and
/// nothing in `Source` would improve either.
fn note(description: &Description) {
    let mut left: Vec<String> = Vec::new();
    let mut total = 0;

    let commands = description
        .commands
        .iter()
        .map(|command| (format!("onerom {}", command.path.join(" ")), &command.opts));
    let globals = std::iter::once(("global options".to_string(), &description.globals));

    for (where_from, opts) in globals.chain(commands) {
        let unsourced: Vec<&str> = opts
            .iter()
            .filter(|opt| opt.source.is_none())
            .filter(|opt| matches!(opt.kind, Kind::Text | Kind::Domain(_)))
            .map(|opt| opt.long.as_str())
            .collect();
        if unsourced.is_empty() {
            continue;
        }
        total += unsourced.len();
        left.push(format!("{where_from}: --{}", unsourced.join(", --")));
    }

    if left.is_empty() {
        return;
    }

    println!("cargo:warning={total} options take text with no source for it:");
    for line in left {
        println!("cargo:warning=  {line}");
    }
}
