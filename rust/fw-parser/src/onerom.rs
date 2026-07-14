// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! sdrr-fw-parser
//!
//! Types and parsing support for schema-format (v0.7.0+) OneROM firmware.

use onerom_metadata::{DeviceMemoryView, OneromInfo};

#[cfg(not(feature = "std"))]
use alloc::{format, vec::Vec};

use crate::ParseError;

// ---------------------------------------------------------------------------
// FirmwareFormat
// ---------------------------------------------------------------------------

/// The format of a detected OneROM firmware image.
///
/// Returned by [`crate::Parser::detect_format`] to indicate which parsing path
/// should be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareFormat {
    /// Pre-v0.7.0 hand-crafted format.  Use [`crate::Parser::parse_format_original`].
    Original,

    /// v0.7.0+ schema-driven metadata format.  Use [`crate::Parser::parse_format_schema`].
    Schema,
}

// ---------------------------------------------------------------------------
// OneRom
// ---------------------------------------------------------------------------

/// Parsed representation of a schema-format (v0.7.0+) OneROM firmware image.
///
/// Constructed by [`crate::Parser::parse_format_schema`].  Fields are accessed via
/// methods rather than directly to allow the internal representation to evolve
/// without breaking callers.
///
/// `metadata` and `runtime` may be `None` if the corresponding region could
/// not be read or parsed — for example, if the device is not running (no
/// runtime info in RAM) or if the metadata pointer was invalid.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OneRom {
    info: Option<OneromInfo>,
    parse_errors: Vec<ParseError>,
}

impl OneRom {
    /// Construct a new `OneRom` from parsed components.
    ///
    /// Called internally by [`Parser::parse_format_schema`]; not intended for
    /// direct use by callers.
    pub(crate) fn new(info: Option<OneromInfo>, parse_errors: Vec<ParseError>) -> Self {
        Self { info, parse_errors }
    }

    /// Returns the parsed `OneromInfo` header, or `None` if parsing failed.
    pub fn info(&self) -> Option<&OneromInfo> {
        self.info.as_ref()
    }

    /// Returns the parsed metadata header, or `None` if unavailable.
    ///
    /// Convenience accessor that drills through [`info`](Self::info).
    pub fn metadata(&self) -> Option<&onerom_metadata::OneromMetadataHeader> {
        self.info.as_ref()?.metadata.as_ref()
    }

    /// Returns the parsed runtime info, or `None` if unavailable.
    ///
    /// Runtime info is only present when the OneROM device is actively
    /// running; it will typically be `None` when parsing a firmware file.
    ///
    /// Convenience accessor that drills through [`info`](Self::info).
    pub fn runtime(&self) -> Option<&onerom_metadata::OneromRuntimeInfo> {
        self.info.as_ref()?.runtime.as_ref()
    }

    /// Returns a mutable reference to the metadata header, or `None` if
    /// unavailable.
    ///
    /// Intended for callers that wish to modify metadata in place and then
    /// re-serialise it via [`onerom_metadata::serialize`].
    pub fn metadata_mut(&mut self) -> Option<&mut onerom_metadata::OneromMetadataHeader> {
        self.info.as_mut()?.metadata.as_mut()
    }

    /// Returns non-fatal parse errors encountered while building this object.
    pub fn parse_errors(&self) -> &[ParseError] {
        &self.parse_errors
    }
}

// ---------------------------------------------------------------------------
// parse_format_schema implementation helper
// ---------------------------------------------------------------------------

/// Parse schema-format firmware using a pre-assembled [`DeviceMemoryView`].
///
/// Called from [`Parser::parse_format_schema`] after all memory regions have
/// been loaded and registered.  Separated to keep the async loading logic
/// in `Parser` and the synchronous parse logic here.
pub(crate) fn parse_onerom_from_view(
    view: &DeviceMemoryView<'_>,
    info_addr: u32,
    mut pre_errors: Vec<ParseError>,
) -> OneRom {
    match OneromInfo::parse(view, info_addr) {
        Ok(info) => OneRom::new(Some(info), pre_errors),
        Err(e) => {
            pre_errors.push(ParseError::new("OneromInfo", format!("{e:?}")));
            OneRom::new(None, pre_errors)
        }
    }
}
