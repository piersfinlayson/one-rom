// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Unified parsed device representation.
//!
//! [`ParsedDevice`] wraps either an original-format ([`Sdrr`]) or
//! schema-format ([`OneRom`]) parse result and provides a common interface
//! for callers that do not need to distinguish between firmware generations.

use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;

use crate::ParseError;
use crate::info::Sdrr;
use crate::onerom::OneRom;

/// The complete parsed state of a One ROM device, regardless of firmware
/// generation.
///
/// Constructed by [`Parser::parse_device`], which detects the firmware format
/// and delegates to the appropriate parser.  Callers that need
/// format-specific fields can downcast via [`as_original`] or [`as_schema`].
///
/// [`as_original`]: ParsedDevice::as_original
/// [`as_schema`]: ParsedDevice::as_schema
/// [`Parser::parse_device`]: crate::Parser::parse_device
#[derive(Debug, Clone)]
pub enum ParsedDevice {
    /// Pre-v0.7.0 hand-crafted format.
    Original(Sdrr),

    /// v0.7.0+ schema-driven metadata format.
    Schema(OneRom),
}

impl ParsedDevice {
    /// Returns `true` if the device was actively running when parsed.
    ///
    /// For original-format devices this means RAM info was successfully read.
    /// For schema-format devices this means runtime info was present.
    pub fn is_running(&self) -> bool {
        match self {
            Self::Original(sdrr) => sdrr.is_running(),
            Self::Schema(onerom) => onerom.runtime().is_some(),
        }
    }

    /// Returns the firmware version, if it could be determined.
    pub fn version(&self) -> Option<FirmwareVersion> {
        match self {
            Self::Original(sdrr) => sdrr.flash.as_ref().map(|f| f.version),
            Self::Schema(onerom) => {
                let info = onerom.info()?;
                Some(FirmwareVersion::new(
                    info.major_version,
                    info.minor_version,
                    info.patch_version,
                    info.build_number,
                ))
            }
        }
    }

    /// Returns non-fatal parse errors encountered during parsing.
    pub fn parse_errors(&self) -> &[ParseError] {
        match self {
            Self::Original(sdrr) => sdrr
                .flash
                .as_ref()
                .map(|f| f.parse_errors.as_slice())
                .unwrap_or(&[]),
            Self::Schema(onerom) => onerom.parse_errors(),
        }
    }

    /// Returns the original-format data, or `None` if this is schema-format.
    pub fn as_original(&self) -> Option<&Sdrr> {
        if let Self::Original(sdrr) = self {
            Some(sdrr)
        } else {
            None
        }
    }

    /// Returns the schema-format data, or `None` if this is original-format.
    pub fn as_schema(&self) -> Option<&OneRom> {
        if let Self::Schema(onerom) = self {
            Some(onerom)
        } else {
            None
        }
    }

    /// Returns the board type, if it could be determined.
    pub fn get_board(&self) -> Option<Board> {
        match self {
            Self::Original(sdrr) => sdrr.flash.as_ref().and_then(|f| f.board),
            Self::Schema(onerom) => {
                let hw_rev = &onerom.info()?.metadata.as_ref()?.hw.hw_rev;
                Board::try_from_str(hw_rev)
            }
        }
    }

    /// Returns whether the device is USB capable
    pub fn is_usb_run_capable(&self) -> bool {
        match self {
            Self::Original(sdrr) => sdrr
                .flash
                .as_ref()
                .map(|f| f.is_usb_run_capable())
                .unwrap_or(false),
            Self::Schema(onerom) => {
                // Get the first ROM set if it exists
                let slot = match onerom.metadata() {
                    Some(metadata) => match metadata.rom_slots.first() {
                        Some(slot) => slot,
                        None => return false,
                    },
                    None => return false,
                };

                let rom_info = match slot.roms.first() {
                    Some(info) => info,
                    None => return false,
                };

                rom_info.rbcp_rom_type
                    == onerom_config::chip::ChipType::SystemPlugin.rbcp_chip_type()
            }
        }
    }
}
