// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Errors raised by the prototype's non-UI code.
//!
//! Every fallible path outside the view returns one of these, so a failure
//! carries what went wrong rather than a formatted string the caller has to
//! parse back apart.

use std::path::PathBuf;

/// Anything the builder can refuse or fail at.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file on disk could not be read.
    #[error("could not read {path}")]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// The built image could not be written.
    #[error("could not write {path}")]
    Write {
        /// The file that could not be written.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// Build was asked for with no One ROM type chosen.
    #[error("select a One ROM type first")]
    NoBoard,

    /// A slot is missing its image file.
    #[error("slot {0} has no image file")]
    NoFile(usize),

    /// A slot is missing its emulated chip type.
    #[error("slot {0} has no ROM type")]
    NoChipType(usize),

    /// An Intel HEX load address could not be parsed.
    #[error("slot {slot}: load address '{value}' is not a number (try 0xE000)")]
    LoadAddress {
        /// The slot carrying the bad value.
        slot: usize,
        /// What the user typed.
        value: String,
    },

    /// The config JSON could not be serialised.
    #[error("could not serialise the config")]
    Config(#[from] serde_json::Error),

    /// `onerom-gen` refused the config or the build.
    ///
    /// `onerom_gen::Error` is `no_std` and implements neither `core::error::Error`
    /// nor `Display` for a source chain, so its own text is the whole message.
    #[error("{0:?}")]
    Gen(onerom_gen::Error),

    /// The board and MCU variant disagree.
    #[error("{0:?}")]
    Properties(onerom_config::Error),
}

/// The prototype's result type.
pub type Result<T> = std::result::Result<T, Error>;

/// An error and everything under it, on one line.
///
/// `thiserror` prints only the outermost message, and the causes carry the
/// detail a user needs — which file, which chip type.
pub fn chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        out.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    out
}

impl From<onerom_gen::Error> for Error {
    fn from(error: onerom_gen::Error) -> Self {
        Error::Gen(error)
    }
}

impl From<onerom_config::Error> for Error {
    fn from(error: onerom_config::Error) -> Self {
        Error::Properties(error)
    }
}
