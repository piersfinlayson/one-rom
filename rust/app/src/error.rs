// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Error types for `onerom-app`.
//!
//! There are two, reflecting the crate's pure/transport split:
//!
//! - [`PluginError`] is the pure, structured error returned by every
//!   synchronous decision function (manifest parsing, release selection,
//!   binary verification, config generation). Its variants carry structured
//!   data - names, versions, sizes - and its [`Display`](core::fmt::Display)
//!   output states *facts only*. It deliberately contains no application
//!   specific guidance (for example "run `onerom plugin` to list plugins"):
//!   that phrasing belongs to whichever application is reporting the error, and
//!   is added there.
//!
//! - [`Error`] is returned by the asynchronous entry points that delegate
//!   fetching to a [`PluginFetch`](crate::PluginFetch) implementation. It is
//!   generic over the host's transport error type `E` and carries it back out
//!   untouched, so the host retains full typed access to its own fetch failures
//!   (for example distinguishing a connection failure from an HTTP status
//!   code). The crate never inspects or stringifies `E`.

use alloc::string::String;
use core::fmt;

use onerom_config::fw::FirmwareVersion;

use crate::plugin::{PluginType, PluginVersion};

/// A pure, structured error from `onerom-app`'s decision logic.
///
/// Every variant carries the data needed for a caller to render its own
/// message; the built-in [`Display`](core::fmt::Display) implementation gives a
/// neutral, fact-only rendering. Applications that want richer, context
/// specific wording match on the variant and format their own text - see the
/// crate-level documentation on the pure/transport split.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PluginError {
    /// More than one plugin of the given type was specified. At most one system
    /// plugin and one user plugin are supported per firmware image.
    #[error("a {0} plugin has already been specified")]
    DuplicatePlugin(PluginType),

    /// A user plugin was specified without an accompanying system plugin. A
    /// user plugin requires a system plugin to be present.
    #[error("a user plugin requires a system plugin, but none was specified")]
    UserPluginWithoutSystem,

    /// A plugin binary is larger than a single plugin slot. The two values are
    /// the actual size and the maximum, both in bytes.
    #[error("plugin binary is {0} bytes, which exceeds the maximum of {1} bytes")]
    TooLarge(usize, usize),

    /// The named plugin was not present in the top-level plugins manifest.
    #[error("plugin '{0}' was not found in the plugins manifest")]
    NotFound(String),

    /// The named plugin has no release matching the requested version.
    #[error("plugin '{0}' has no release with version {1}")]
    VersionNotFound(String, PluginVersion),

    /// The selected release requires a newer firmware than the one in use. The
    /// fields are the plugin name, its version, the minimum firmware it
    /// requires, and the firmware actually selected.
    #[error(
        "plugin '{name}' version {version} requires firmware {min_fw} or later (selected firmware is {fw})"
    )]
    Incompatible {
        /// Plugin name.
        name: String,
        /// The plugin release version under consideration.
        version: PluginVersion,
        /// Minimum firmware version this release supports.
        min_fw: FirmwareVersion,
        /// The firmware version actually selected.
        fw: FirmwareVersion,
    },

    /// The selected release is known to be incompatible with the firmware in
    /// use because that firmware is at or beyond the release's upper bound. The
    /// fields are the plugin name, its version, the first firmware version the
    /// release is incompatible with, and the firmware actually selected.
    #[error(
        "plugin '{name}' version {version} is not compatible with firmware {from} or later (selected firmware is {fw})"
    )]
    IncompatibleNewer {
        /// Plugin name.
        name: String,
        /// The plugin release version under consideration.
        version: PluginVersion,
        /// First firmware version this release is known to be incompatible with.
        from: FirmwareVersion,
        /// The firmware version actually selected.
        fw: FirmwareVersion,
    },

    /// A plugin binary was too small to contain a valid header. The fields are
    /// the binary's source (path or URL), its actual length, and the minimum
    /// required, both in bytes.
    #[error(
        "plugin binary from '{0}' is {1} bytes, too small for a valid header (minimum {2} bytes)"
    )]
    BinaryTooSmall(String, usize, usize),

    /// A plugin binary's header magic did not match. The fields are the source,
    /// the magic found, and the magic expected.
    #[error("plugin binary from '{0}' has invalid header magic {1:#010x} (expected {2:#010x})")]
    InvalidMagic(String, u32, u32),

    /// A plugin's type in the manifest did not match the type declared in its
    /// binary header. The fields are the plugin name, the manifest type, and
    /// the header type.
    #[error("plugin '{0}' type mismatch: manifest says {1}, binary header says {2}")]
    TypeMismatch(String, PluginType, PluginType),

    /// A plugin's version in the manifest did not match the version declared in
    /// its binary header. The fields are the plugin name, the manifest version,
    /// and the header version.
    #[error("plugin '{0}' version mismatch: manifest says {1}, binary header says {2}")]
    VersionMismatch(String, PluginVersion, PluginVersion),

    /// A plugin binary's SHA-256 digest did not match the value recorded in the
    /// manifest.
    #[error("SHA-256 mismatch for plugin binary '{binary}' (expected {expected}, got {got})")]
    Sha256Mismatch {
        /// The binary's source (path or URL).
        binary: String,
        /// Expected digest, lower-case hex.
        expected: String,
        /// Actual digest, lower-case hex.
        got: String,
    },

    /// A plugin binary declared itself to be a PIO plugin, which is not
    /// currently supported.
    #[error("plugin binary from '{0}' is a PIO plugin, which is not currently supported")]
    PioNotSupported(String),

    /// A plugin binary's header declared an unrecognised plugin type value.
    #[error("plugin binary from '{0}' has an unrecognised type value: {1}")]
    UnknownBinaryType(String, u8),

    /// A plugin entry in the manifest declared an unrecognised type string. The
    /// fields are the plugin name and the offending type string.
    #[error("plugin '{0}' has an unrecognised type '{1}' in the manifest")]
    UnknownManifestType(String, String),

    /// A `--plugin`-style specification string was malformed. The contained
    /// string describes the syntax problem.
    #[error("invalid plugin specification: {0}")]
    SpecSyntax(String),

    /// A manifest could not be parsed as JSON. The fields are the manifest's
    /// source (path or URL) and the underlying parse error.
    #[error("failed to parse manifest JSON from '{0}': {1}")]
    ManifestJson(String, String),
}

/// The error type returned by `onerom-app`'s asynchronous entry points.
///
/// Generic over the host transport error `E` supplied by the
/// [`PluginFetch`](crate::PluginFetch) implementation. The crate never bounds,
/// inspects, or stringifies `E`; it simply carries it back out so the host can
/// match on its own error type. A [`Display`](core::fmt::Display) rendering is
/// available only when `E` itself is `Display`, purely as a convenience for
/// hosts that want it - it is not required in order to use `Error`.
#[derive(Debug)]
pub enum Error<E> {
    /// A pure decision failed. Carries the structured [`PluginError`].
    Plugin(PluginError),

    /// A host fetch failed. Carries the source that was being fetched and the
    /// host's own transport error, untouched.
    Fetch {
        /// The path or URL the host was asked to fetch.
        source: String,
        /// The host's transport error.
        error: E,
    },
}

impl<E> From<PluginError> for Error<E> {
    fn from(e: PluginError) -> Self {
        Error::Plugin(e)
    }
}

impl<E> Error<E> {
    /// Construct a [`Error::Fetch`] from a source and the host's transport
    /// error.
    pub fn fetch(source: impl Into<String>, error: E) -> Self {
        Error::Fetch {
            source: source.into(),
            error,
        }
    }
}

// A `Display` rendering is offered only when the host's transport error is
// itself `Display`. This keeps `Error<E>` usable (via matching) for hosts whose
// `E` is not `Display`, while remaining convenient for those whose is.
impl<E: fmt::Display> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Plugin(e) => write!(f, "{e}"),
            Error::Fetch { source, error } => {
                write!(f, "failed to fetch '{source}': {error}")
            }
        }
    }
}

impl<E: fmt::Display + fmt::Debug> core::error::Error for Error<E> {}
