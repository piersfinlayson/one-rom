// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The commands this build really runs, and how one is found.
//!
//! No code here names a command, which is the rule the rest of the crate rests
//! on — the commands these docs name are named to say what cannot be reached.
//! A [`Runner`] declares the **options it reads**, and the framework hands it
//! whichever command takes exactly those options.  A contract is a statement
//! about an interface, and an interface is something [`studiov2_commands`]
//! carries.
//!
//! Two properties make that safe to run real code behind:
//!
//! - **Exact.** A command matches only when its option set is the runner's set,
//!   with no extras and none missing, and each option still takes the sort of
//!   value the runner reads out of it.  An option added to the CLI unmatches
//!   the runner, so the pane falls back to the stub rather than running with an
//!   option it silently ignored.
//! - **Unique.** A runner whose contract fits more than one command runs for
//!   none of them.  `board socket` and `inspect socket` take the same three
//!   options, and no contract can tell them apart, so no contract gets either.
//!
//! ## What a contract cannot reach
//!
//! A command with no options has an empty contract, and six commands here have
//! one.  `board list` is one of the six, and nothing in the description
//! separates it from `inspect info`, `inspect slots`, `inspect led`,
//! `inspect rgb` or `self check` — the description says what a command
//! *consumes* and says nothing about what it produces or what it is for.  So
//! `board list` stays stubbed.  Reaching it would mean writing its name down,
//! which is the thing this prototype exists to avoid.

pub mod chips;
pub mod image;

use onerom_cli::Error;
use studiov2_commands::{COMMANDS, Command, Kind, Opt, Source};

use crate::form::{Field, Form};
use crate::stub::Output;

/// The sort of value a runner reads out of one option.
///
/// Checked against what the description says the option takes, so a contract
/// cannot be met by an option that kept its name and changed its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takes {
    /// A path on this machine, opened or written.
    Path,
    /// A whole number.
    Count,
    /// One of a set the CLI advertises.
    OneOf,
    /// A value the runner parses itself.
    Words,
    /// A bare flag.
    Switch,
}

impl Takes {
    /// Whether an option offers what this expects of it.
    fn fits(self, opt: &Opt) -> bool {
        let path = matches!(
            opt.source,
            Some(Source::OpenFile | Source::SaveFile | Source::Directory)
        );

        match self {
            Self::Path => matches!(opt.kind, Kind::Text) && path,
            Self::Count => matches!(opt.kind, Kind::Number),
            Self::OneOf => matches!(opt.kind, Kind::Choice(_)),
            Self::Words => matches!(opt.kind, Kind::Text | Kind::Domain(_)) && !path,
            Self::Switch => matches!(opt.kind, Kind::Flag),
        }
    }
}

/// One option a runner reads, and what it reads out of it.
pub struct Need {
    /// The option's long name, as the CLI spells it.
    pub long: &'static str,
    /// What the runner expects the option to hold.
    pub takes: Takes,
}

/// A real implementation, and the option contract that finds it a command.
pub struct Runner {
    /// The options it reads.  See the module docs for what matching them means.
    pub needs: &'static [Need],

    /// What it does, as the pane says it.
    ///
    /// The pane has to tell a user whether Run is about to do something, and
    /// naming the crate that does the work is the honest version of that.
    pub does: &'static str,

    /// The work itself.
    ///
    /// Answers with `onerom-cli`'s own error type, so a failure reaches the
    /// pane in the words the CLI would have used for the same failure.
    pub work: fn(&Inputs) -> Result<Output, Error>,
}

impl Runner {
    /// Whether a command takes exactly the options this runner reads.
    fn fits(&self, command: &Command) -> bool {
        command.opts.len() == self.needs.len()
            && self.needs.iter().all(|need| {
                command
                    .opts
                    .iter()
                    .any(|opt| opt.long == need.long && need.takes.fits(opt))
            })
    }

    /// Runs it, with the error flattened the way the screen holds a result.
    pub fn run(&self, inputs: &Inputs) -> Result<Output, String> {
        (self.work)(inputs).map_err(|error| error.to_string())
    }
}

/// Every real implementation this build carries.
const RUNNERS: &[Runner] = &[image::CONVERT, image::SWAP, image::DEINTERLEAVE, chips::FIT];

/// The runner for a command, where exactly one fits it and nothing else.
///
/// The second half of the condition is the uniqueness rule.  It costs a walk of
/// every command per lookup, which is nothing at this size and is the only
/// thing standing between a contract and the wrong command.
pub fn runner(command: &Command) -> Option<&'static Runner> {
    RUNNERS.iter().find(|runner| {
        runner.fits(command) && COMMANDS.iter().filter(|other| runner.fits(other)).count() == 1
    })
}

/// What a user filled in, read by the names a contract declares.
///
/// A runner never sees a [`Form`]'s indices, so it cannot read an option its
/// contract did not ask for by accident — a name nothing answers to reads as
/// unset.
pub struct Inputs<'a> {
    /// The options the form was built from.
    opts: &'a [Opt],
    /// What is filled in, one per option.
    fields: &'a [Field],
}

impl<'a> Inputs<'a> {
    /// The filled-in values of one form.
    pub fn new(form: &'a Form) -> Self {
        Self {
            opts: form.opts,
            fields: &form.fields,
        }
    }

    /// The field for an option, by name.
    fn field(&self, long: &str) -> Option<&'a Field> {
        let index = self.opts.iter().position(|opt| opt.long == long)?;
        self.fields.get(index)
    }

    /// The text in an option, trimmed.  Empty means nothing was given.
    pub fn text(&self, long: &str) -> &'a str {
        self.field(long).map_or("", |field| field.value(0)).trim()
    }

    /// Whether a flag is ticked.
    pub fn switch(&self, long: &str) -> bool {
        self.field(long).is_some_and(Field::is_on)
    }

    /// The number in an option, where it holds one.
    pub fn count(&self, long: &str) -> Option<usize> {
        self.text(long).parse().ok()
    }
}
