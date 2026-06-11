// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Generates firmware artifacts for One ROM.

//#![no_std]

extern crate alloc;

pub mod builder;
pub mod firmware;
pub mod image;
pub mod meta;

pub use builder::{Builder, ChipConfig, ChipSetConfig, Config, FileData, FileSpec, License};
pub use firmware::{
    DebugConfig, FireConfig, FireCpuFreq, FireServeMode, FireVreg, FirmwareConfig, IceConfig,
    IceCpuFreq, LedConfig, ServeAlgParams,
};
pub use image::{Chip, ChipSet, ChipSetType, CsConfig, CsLogic, SizeHandling};
pub use image::{PAD_BLANK_BYTE, PAD_NO_CHIP_BYTE};
pub use meta::{MAX_METADATA_LEN, Metadata, PAD_METADATA_BYTE};

use alloc::string::String;
use onerom_config::chip::ChipType;
use onerom_config::fw::{FirmwareVersion, ServeAlg};

pub use builder::MAX_SUPPORTED_FIRMWARE_VERSION;
use onerom_config::hw::Board;

/// Version of metadata produced by this version of the crate
pub const METADATA_VERSION: u32 = 1;
const METADATA_VERSION_STR: &str = "1";

/// Firmware size reserved at the start of flash, before metadata
pub const FIRMWARE_SIZE: usize = 48 * 1024; // 48KB

pub const MIN_FIRMWARE_OVERRIDES_VERSION: FirmwareVersion = FirmwareVersion::new(0, 6, 0, 0);

/// Error type
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Error {
    RightSize {
        chip_type: ChipType,
        size: usize,
        size_handling: SizeHandling,
    },
    ImageTooSmall {
        chip_type: ChipType,
        index: usize,
        expected: usize,
        actual: usize,
    },
    ImageTooLarge {
        chip_type: ChipType,
        image_size: usize,
        expected_size: usize,
    },
    DuplicationNotExactDivisor {
        chip_type: ChipType,
        image_size: usize,
        expected_size: usize,
    },
    BufferTooSmall {
        location: &'static str,
        expected: usize,
        actual: usize,
    },
    NoChips {
        id: usize,
    },
    TooManyChips {
        id: usize,
        expected: usize,
        actual: usize,
    },
    TooFewChips {
        id: usize,
        expected: usize,
        actual: usize,
    },
    MissingCsConfig {
        chip_type: ChipType,
        line: &'static str,
    },
    MissingPointer {
        id: usize,
    },
    InvalidServeAlg {
        serve_alg: ServeAlg,
    },
    InconsistentCsLogic {
        first: CsLogic,
        other: CsLogic,
    },
    InvalidConfig {
        error: String,
    },
    UnsupportedConfigVersion {
        version: u32,
    },
    DuplicateFile {
        id: usize,
    },
    InvalidFile {
        id: usize,
        total: usize,
    },
    MissingFile {
        id: usize,
    },
    UnsupportedToolChipType {
        chip_type: ChipType,
    },
    UnsupportedBoardChipType {
        board: Board,
        chip_type: ChipType,
    },
    InvalidLicense {
        id: usize,
    },
    UnvalidatedLicense {
        id: usize,
    },
    BadLocation {
        id: usize,
        reason: String,
    },
    UnsupportedFrequency {
        frequency_mhz: u32,
    },
    FirmwareTooOld {
        feat: &'static str,
        version: FirmwareVersion,
        minimum: FirmwareVersion,
    },
    UnsupportedFeature {
        feat: &'static str,
    },
    FirmwareTooNew {
        version: FirmwareVersion,
        maximum: FirmwareVersion,
    },
    /// Some firmware versions are explicitly unsupported, due to known issues
    /// with them.  For example 0.6.3.
    FirmwareUnsupported {
        version: FirmwareVersion,
    },
    Base64,
    Base16,
    InvalidPluginImage {
        plugin_type: ChipType,
        image_file: String,
        error: String,
    },
}
type Result<T> = core::result::Result<T, Error>;

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::RightSize {
                chip_type,
                size,
                size_handling,
            } => write!(
                f,
                "The provided image is already the correct size ({size} bytes) for a {chip_type}.  The {size_handling} option should not be used.  Remove it."
            ),
            Error::ImageTooSmall {
                chip_type,
                index: _,
                expected,
                actual,
            } => write!(
                f,
                "The provided image is too small for a {chip_type}.\n  Expected at least {expected} bytes, got {actual} bytes.\n  Consider using the duplicate or padding options to make the image larger."
            ),
            Error::ImageTooLarge {
                chip_type,
                image_size,
                expected_size,
            } => write!(
                f,
                "The provided chip image is larger than the size supported by a {chip_type}: expected at most {expected_size} bytes, got {image_size} bytes"
            ),
            Error::DuplicationNotExactDivisor {
                chip_type,
                image_size,
                expected_size,
            } => write!(
                f,
                "Image duplication requires that the size of the provided image is an exact divisor of the size required by a {chip_type}.\n  {image_size} is not an exact divisor of {expected_size}.\n  Consider using the padding option instead."
            ),
            Error::BufferTooSmall {
                location,
                expected,
                actual,
            } => write!(
                f,
                "Internal error: Buffer for {location} is too small: expected at least {expected} bytes, got {actual} bytes"
            ),
            Error::NoChips { id } => write!(f, "No chips were specified for set {id}"),
            Error::TooManyChips {
                id,
                expected,
                actual,
            } => write!(
                f,
                "Too many chips specified for set {id}.\n  Expected at most {expected}, got {actual}"
            ),
            Error::TooFewChips {
                id,
                expected,
                actual,
            } => write!(
                f,
                "Too few chips specified for set {id}.\n  Expected at least {expected}, got {actual}"
            ),
            Error::MissingCsConfig { chip_type, line } => write!(
                f,
                "The configuration is missing required chip select line {line} configuration for {chip_type}"
            ),
            Error::MissingPointer { id } => {
                write!(f, "Internal error: Missing pointer with internal id: {id}")
            }
            Error::InvalidServeAlg { serve_alg } => {
                write!(
                    f,
                    "The configured serving algorithm is not valid for the type of chip, ROM or set: {serve_alg}"
                )
            }
            Error::InconsistentCsLogic { first, other } => write!(
                f,
                "The configured chip select logic is self-inconsistent:\n  The first is {first}, the other is {other}"
            ),
            Error::InvalidConfig { error } => write!(
                f,
                "There is a problem with the supplied configuration:\n  {error}"
            ),
            Error::UnsupportedConfigVersion { version } => {
                write!(
                    f,
                    "The configuration version {version} is unsupported by this tool"
                )
            }
            Error::DuplicateFile { id } => write!(
                f,
                "Internal error: Duplicate file supplied with internal id: {id}"
            ),
            Error::InvalidFile { id, total } => {
                write!(
                    f,
                    "Internal error: Invalid file with internal id: {id}, total files: {total}"
                )
            }
            Error::MissingFile { id } => {
                write!(f, "Internal error: Missing file with internal id: {id}")
            }
            Error::UnsupportedToolChipType { chip_type } => {
                write!(f, "This tool does not support chip type {chip_type}")
            }
            Error::UnsupportedBoardChipType { board, chip_type } => {
                write!(
                    f,
                    "The board {board} does not support chip type {chip_type}"
                )
            }
            Error::InvalidLicense { id } => {
                write!(f, "Internal error: No license exists with internal id {id}")
            }
            Error::UnvalidatedLicense { id } => write!(
                f,
                "Internal error: A license with internal id {id} has not been validated"
            ),
            Error::BadLocation { id, reason } => {
                write!(
                    f,
                    "An invalid location was specified for the file with internal id {id}\n  {reason}"
                )
            }
            Error::UnsupportedFrequency { frequency_mhz } => {
                write!(
                    f,
                    "Unsupported MCU frequency for this One ROM: {frequency_mhz}MHz"
                )
            }
            Error::FirmwareTooOld {
                feat,
                version,
                minimum,
            } => write!(
                f,
                "Selected firmware version {version} does not support {feat}\n  The minimum supported version for {feat} is {minimum}"
            ),
            Error::UnsupportedFeature { feat } => {
                write!(f, "The {feat} feature is currently unsupported")
            }
            Error::FirmwareTooNew { version, maximum } => write!(
                f,
                "Selected firmware version {version} is too new\n  The maximum firmware version supported by this tool is {maximum}"
            ),
            Error::FirmwareUnsupported { version } => write!(
                f,
                "Selected firmware version {version} is unsupported by this tool due to known issues"
            ),
            Error::Base64 => write!(f, "Base64 encoding/decoding error"),
            Error::Base16 => write!(f, "Base16 encoding/decoding error"),
            Error::InvalidPluginImage {
                plugin_type,
                image_file,
                error,
            } => write!(
                f,
                "The provided {plugin_type} image {image_file} is invalid:\n  {error}"
            ),
        }
    }
}

pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn metadata_version() -> &'static str {
    METADATA_VERSION_STR
}

pub trait MetadataWriter {
    fn metadata_len(&self) -> usize;
    fn total_set_count(&self) -> usize;
    fn rom_images_size(&self) -> usize;
    fn write_all(&self, buf: &mut [u8], rtn_chip_data_ptrs: &mut [u32]) -> Result<usize>;
    fn write_roms(&self, buf: &mut [u8]) -> Result<()>;
}
