//! The prototype's error type.
//!
//! Everything fallible in this crate reports through [`Error`].  Nothing
//! returns `Result<_, String>`, and no error carries a UI message type — the
//! UI layer wraps these values in its own `Message` at the point it receives
//! them.

use std::io;

/// Anything that can go wrong in this screen.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The fake device was asked for a command it does not implement.
    #[error("unknown command: `{0}` — type `help` for the command list")]
    UnknownCommand(String),

    /// A command was given the wrong number or shape of arguments.
    #[error("bad arguments for `{command}`: {detail}")]
    BadArguments {
        /// The command that was invoked.
        command: String,
        /// What was wrong with its arguments.
        detail: String,
    },

    /// The session was closed, so no command can run against it.
    #[error("device is disconnected — run `connect` first")]
    Disconnected,

    /// A command-line option could not be understood.
    #[error("bad option `{option}`: {detail}")]
    BadOption {
        /// The option as given on the command line.
        option: String,
        /// What was wrong with it.
        detail: String,
    },

    /// Reading the process's resident set size failed.
    #[error("could not read resident set size: {0}")]
    Rss(#[source] RssError),

    /// The log store could not be reached.
    #[error(transparent)]
    Store(#[from] crate::store::StoreError),
}

/// Why a resident-set-size reading failed.
#[derive(Debug, thiserror::Error)]
pub enum RssError {
    /// Running `ps` failed outright.
    #[error("could not run `ps`: {0}")]
    Spawn(#[from] io::Error),

    /// `ps` ran but its output was not a number of kilobytes.
    #[error("unexpected `ps` output: {0:?}")]
    Parse(String),
}
