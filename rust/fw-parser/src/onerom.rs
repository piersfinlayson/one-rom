// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! sdrr-fw-parser
//!
//! Types and parsing support for schema-format (v0.7.0+) OneROM firmware.

use core::fmt;

use onerom_metadata::{
    CURRENT_METADATA_VERSION, DeviceMemoryView, Generations, ONEROM_INFO_VERSION, OneromInfo,
    RUNTIME_INFO_VERSION,
};

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, format, vec::Vec};

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
// RuntimeAbsence
// ---------------------------------------------------------------------------

/// Why a parse came back with no runtime info.
///
/// The firmware writes `onerom_runtime_info_t` in RAM once it is up, so a
/// device that is not up has none, and a firmware file holds no RAM to hold
/// one.  Without a reason a user cannot tell those apart from a fault.
///
/// The variants are the four points it can go missing, in the order the parse
/// reaches them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAbsence {
    /// The info header's runtime pointer is null.
    NoPointer,

    /// The memory the runtime pointer names could not be read.
    Unreadable,

    /// The memory was read and does not hold the runtime magic, so nothing
    /// has written runtime info there.
    NotRunning,

    /// The magic was there and this build could not read the structure
    /// behind it.
    Unparsed,
}

impl fmt::Display for RuntimeAbsence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoPointer => "The image's pointer to runtime info is not set.",
            Self::Unreadable => "Runtime info could not be read from RAM.",
            Self::NotRunning => "The device is stopped and has no runtime info.",
            Self::Unparsed => "Runtime info is present but could not be parsed.",
        })
    }
}

// ---------------------------------------------------------------------------
// NewerGeneration
// ---------------------------------------------------------------------------

/// A structure carrying a generation newer than this build knows.
///
/// Every field this build knows is parsed and present, since a newer
/// generation keeps existing offsets where they are.  Naming the generation
/// is all this build can say about what lies beyond them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct NewerGeneration {
    /// The structure, named as the schema names it.
    pub structure: &'static str,

    /// The generation the device carries.
    pub device_generation: u32,

    /// The newest generation of that structure this build knows.
    pub known_generation: u32,
}

impl fmt::Display for NewerGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is generation {}, and this build understands generation {}, so it carries \
             fields this build cannot parse.",
            self.structure, self.device_generation, self.known_generation
        )
    }
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
/// [`runtime_absence`](Self::runtime_absence) says which of those it was.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OneRom {
    info: Option<OneromInfo>,
    parse_errors: Vec<ParseError>,
    runtime_absence: Option<RuntimeAbsence>,
}

impl OneRom {
    /// Construct a new `OneRom` from parsed components.
    ///
    /// Called internally by [`Parser::parse_format_schema`]; not intended for
    /// direct use by callers.
    pub(crate) fn new(
        info: Option<OneromInfo>,
        parse_errors: Vec<ParseError>,
        runtime_absence: Option<RuntimeAbsence>,
    ) -> Self {
        Self {
            info,
            parse_errors,
            runtime_absence,
        }
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

    /// Why [`runtime`](Self::runtime) is `None`, or `None` where it is not.
    ///
    /// Kept apart from [`parse_errors`](Self::parse_errors) because most of
    /// the reasons are not errors, and a caller treats a parse error as a
    /// fault worth refusing over.
    pub fn runtime_absence(&self) -> Option<RuntimeAbsence> {
        self.runtime_absence
    }

    /// Returns the structures the device carries a newer generation of than
    /// this build knows, empty where there are none.
    ///
    /// A structure this parse did not read contributes nothing - there are no
    /// bytes to have a generation.
    pub fn newer_generations(&self) -> Vec<NewerGeneration> {
        let mut found = Vec::new();
        let Some(info) = &self.info else {
            return found;
        };

        let mut check = |structure, device, known| {
            if device > known {
                found.push(NewerGeneration {
                    structure,
                    device_generation: device,
                    known_generation: known,
                });
            }
        };

        check("onerom_info_t", info.version, ONEROM_INFO_VERSION);
        if let Some(metadata) = &info.metadata {
            check(
                "onerom_metadata_header_t",
                metadata.version,
                CURRENT_METADATA_VERSION,
            );
        }
        if let Some(runtime) = &info.runtime {
            check(
                "onerom_runtime_info_t",
                runtime.version,
                RUNTIME_INFO_VERSION,
            );
        }

        found
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
///
/// `runtime_absence` is what the caller found while loading the regions, and
/// is `None` where it registered a runtime region.  The remaining reason - a
/// registered region the parser would not take - is only visible here.
pub(crate) fn parse_onerom_from_view(
    view: &DeviceMemoryView<'_>,
    info_addr: u32,
    mut pre_errors: Vec<ParseError>,
    runtime_absence: Option<RuntimeAbsence>,
) -> OneRom {
    // Nothing has been read yet, so no generation is known.  Each structure
    // fills its own in as it parses, and a field gated on one not yet read
    // yields the default the schema declares.
    match OneromInfo::parse(view, info_addr, Generations::UNKNOWN) {
        Ok(info) => {
            let absence = match (&info.runtime, runtime_absence) {
                (Some(_), _) => None,
                (None, Some(reason)) => Some(reason),
                (None, None) => Some(RuntimeAbsence::Unparsed),
            };
            OneRom::new(Some(info), pre_errors, absence)
        }
        Err(e) => {
            pre_errors.push(ParseError::new("OneromInfo", format!("{e:?}")));
            OneRom::new(None, pre_errors, runtime_absence)
        }
    }
}
