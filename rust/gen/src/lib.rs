// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Generates firmware artifacts for One ROM.

//#![no_std]

extern crate alloc;

pub mod builder;
pub mod compat;
pub mod firmware;
pub mod image;
pub mod meta;
pub mod v1;
pub mod v2;

pub use builder::Builder;
pub use firmware::{
    DebugConfig, FireConfig, FireCpuFreq, FireServeMode, FireVreg, FirmwareConfig, IceConfig,
    IceCpuFreq, LedConfig, ServeAlgParams,
};
pub use image::{Chip, ChipSet, ChipSetType, CsConfig, CsLogic, SizeHandling};
pub use image::{num_excess_addr_lines, requires_half_select_cs1};
pub use image::{MAX_IMAGE_SIZE, PAD_BLANK_BYTE, PAD_NO_CHIP_BYTE};
pub use meta::{MAX_METADATA_LEN, Metadata, PAD_METADATA_BYTE};
use onerom_config::mcu::Family;

use alloc::string::String;
use onerom_config::chip::ChipType;
use onerom_config::fw::{FirmwareVersion, ServeAlg};

use onerom_config::hw::Board;
pub use v1::MAX_SUPPORTED_FIRMWARE_VERSION as MAX_SUPPORTED_FIRMWARE_VERSION_V1;
pub use v1::MIN_SUPPORTED_FIRMWARE_VERSION as MIN_SUPPORTED_FIRMWARE_VERSION_V1;
pub use v1::SUPPORTED_CHIP_TYPES as SUPPORTED_CHIP_TYPES_V1;
pub use v1::UNSUPPORTED_FIRMWARE_VERSIONS as UNSUPPORTED_FIRMWARE_VERSIONS_V1;
pub use v2::MAX_FW_VERSION as MAX_SUPPORTED_FIRMWARE_VERSION_V2;
pub use v2::MIN_FW_VERSION as MIN_SUPPORTED_FIRMWARE_VERSION_V2;
pub use v2::SUPPORTED_CHIP_TYPES as SUPPORTED_CHIP_TYPES_V2;
pub use v2::UNSUPPORTED_FIRMWARE_VERSIONS as UNSUPPORTED_FIRMWARE_VERSIONS_V2;

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
    UnsupportedMcuFamily {
        family: Family,
        version: FirmwareVersion,
    },
    UnsupportedBoardConfig {
        board: Board,
        reason: String,
    },
    MetadataOverflow {
        size: usize,
    },
    MissingImageData {
        chip_type: ChipType,
        index: usize,
    },
    RomTableTooLarge {
        size: usize,
        max: usize,
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
                    "The board {board} does not support chip type {chip_type} with this firmware version"
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
            Error::UnsupportedMcuFamily { family, version } => write!(
                f,
                "The MCU family {family} is not supported by this firmware version {version}"
            ),
            Error::UnsupportedBoardConfig { board, reason } => write!(
                f,
                "The board {board} does not support this configuration: {reason}"
            ),
            Error::MetadataOverflow { size } => write!(
                f,
                "The configuration's metadata exceeds the {size}-byte metadata region"
            ),
            Error::MissingImageData { chip_type, index } => write!(
                f,
                "Internal error: chip {index} ({chip_type}) in this set has no image data"
            ),
            Error::RomTableTooLarge { size, max } => write!(
                f,
                "ROM table is {size} bytes, exceeds maximum of {max} bytes for a single slot"
            ),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::InvalidConfig {
            error: e.to_string(),
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

/// License details for validation by caller
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct License {
    /// License ID provided for information only.
    pub id: usize,

    /// File ID that this license applies to, provided for information only.
    pub file_id: usize,

    /// License URL/identifier.  Used by caller to retrieve and present to user
    /// for acceptance.
    pub url: String,

    // Whether this license has been validated by the caller
    validated: bool,
}

impl License {
    /// Create new license
    pub fn new(id: usize, file_id: usize, url: String) -> Self {
        Self {
            id,
            file_id,
            url,
            validated: false,
        }
    }
}

/// One ROM chip configuration format.
///
/// Used to indicate:
/// - What ROM chips, RAM chips and any other devices to emulate
/// - What ROM images to include
/// - Any overrides for the firmware build-time setting
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(title = "One ROM Configuration"))]
pub struct Config {
    /// Configuration format version.
    #[cfg_attr(feature = "schemars", schemars(schema_with = "version_schema"))]
    pub version: u32,

    /// Optional name for this configuration.  Is included in the description
    /// output by the builder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Mandatory description for this configuration.  This is included in the
    /// description output by the builder, following the name.
    pub description: String,

    /// Optional detailed description for this configuration.  This is included
    /// in the description output by the builder, following name and
    /// description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,

    /// Array of chip set configurations.  Note that even if not using complex
    /// features like dynamic banking and multi-ROM sets, each ROM image, or
    /// other chip types is in its own set.
    ///
    /// The builder description output lists either "Images" or "Sets"
    /// depending on whether there are any multi-set or banked sets in use.
    #[serde(alias = "rom_sets")]
    pub chip_sets: Vec<ChipSetConfig>,

    /// Optional notes for this configuration.  This is included in the
    /// description output by the builder, following the list of images/sets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,

    /// Optional categories for this configuration, to aid in grouping,
    /// sorting, and searching of configurations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,

    /// Optional name for this One ROM instance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_name: Option<String>,

    /// Optional serial number override for this One ROM, overriding the stock
    /// USB serial number (which is the MCU's unique chip ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial_override: Option<String>,

    /// Whether to enable boot logging, using SWD.
    #[serde(default = "default_boot_logging")]
    pub boot_logging: bool,

    /// Whether to enable SWD.  Must be enabled for boot logging.
    #[serde(default = "default_swd_enabled")]
    pub swd_enabled: bool,

    /// Whethher to boot fast.  Disables reading the image select jumpers.
    /// The first non-plugin image is served.
    #[serde(default = "default_turbo_boot")]
    pub turbo_boot: bool,
}

pub(crate) fn default_boot_logging() -> bool {
    false
}
pub(crate) fn default_swd_enabled() -> bool {
    true
}
pub(crate) fn default_turbo_boot() -> bool {
    false
}

#[cfg(feature = "schemars")]
fn version_schema(_gen: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "const": 1
    })
}

/// Chip Set configuration structure
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ChipSetConfig {
    /// Type of ROM set
    #[serde(rename = "type")]
    #[cfg_attr(feature = "schemars", schemars(default))]
    pub set_type: ChipSetType,

    /// Optional description for this chip set.  This is included in the
    /// description output by the builder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Array of chip configurations in this set.  Contains 1 member for single
    /// chip sets, and multiple members for multi-ROM and banked ROM sets.
    ///
    /// For multi-ROM sets, the array order determines X pin assignment:
    ///   chip0 — primary socket (One ROM physically installed here)
    ///   chip1 — X1 pin monitors this socket's chip select via fly-lead
    ///   chip2 — X2 pin monitors this socket's chip select via fly-lead
    /// Maximum 3 chips per multi-ROM set (primary + 2 X pins).
    #[serde(alias = "roms")]
    pub chips: Vec<ChipConfig>,

    /// Optional serving algorithm override for this chip set.  Only valid
    /// when using CPU serving - Ice boards and Fire 24 A/B by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serve_alg: Option<ServeAlg>,

    /// Optional firmware overrides when serving this chip set.  Takes
    /// precedence over any global configuration firmware overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware_overrides: Option<FirmwareConfig>,
}

/// Chip configuration structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ChipConfig {
    /// Filename or URL of any ROM image - filename is only valid if using a
    /// generator tool with local file access.  This is passed to the generator
    /// tool to retrieve the ROM image.
    #[serde(default)]
    pub file: String,

    /// Optional license URL/identifier for the ROM.  This is passed to the
    /// generator tool to retrieve and ask the user to accept before building.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// Optional description for this configuration.  This is included in the
    /// description output by the builder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Type of ROM
    #[serde(rename = "type")]
    pub chip_type: ChipType,

    /// Optional Chip Select 1 logic - only valid for Chip Types that have CS1
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cs1: Option<CsLogic>,

    /// Optional Chip Select 2 logic - only valid for Chip Types that have CS2
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cs2: Option<CsLogic>,

    /// Optional Chip Select 3 logic - only valid for Chip Types that have CS3
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cs3: Option<CsLogic>,

    /// Optional Chip Select 4 logic - only valid for Chip Types that have CS4
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cs4: Option<CsLogic>,

    /// Optional Chip Enable logic override - only valid for chip types that
    /// have a /CE control line.  In V2 multi-ROM sets, may be set to Ignore
    /// when /CE is tied active and /OE is the fly-leaded chip select, or vice
    /// versa.  Not valid for V1 configurations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ce: Option<CsLogic>,

    /// Optional Output Enable logic override - only valid for chip types that
    /// have an /OE control line.  Not valid for V1 configurations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oe: Option<CsLogic>,

    /// Explicitly permit CS/CE/OE lines to be set to Ignore outside the
    /// contexts where this is implicitly allowed:
    ///   - V2 multi-ROM set chips[1+] (secondary sockets — free pass)
    ///   - Lines with allow_ignore in chip_types.json (datasheet-defined)
    ///
    /// Required for chip0 in multi-ROM sets and for single-chip sets
    /// where a line needs ignoring for custom circuit reasons.
    /// Misuse can cause bus contention — only set when intentional.
    #[serde(default)]
    pub allow_cs_ignore: bool,

    /// Optional size handling configuration for this Chip.  Used to specify
    /// handling when the image supplied isn't the correct size for this Chip
    /// type.
    #[serde(default, skip_serializing_if = "SizeHandling::is_none")]
    pub size_handling: SizeHandling,

    /// Optional extract path within an archive (zip/tar) if the file pointed
    /// to is an archive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract: Option<String>,

    /// Optional label for this ROM image.  If specified, this is used in
    /// metadata instead of the filename (which itself can be complex if
    /// extracting a file from an image and providing location information)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Optional location within a larger image file.  Used to specify start
    /// offset and length within the file.  Useful when multiple ROM images
    /// are concatenated into a single file and one needs to be extracted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
}

impl ChipConfig {
    // Constructs the filename string for metadata.  Note label will be used
    // in metadata instead if specified.
    fn filename(&self) -> String {
        if let Some(label) = &self.label {
            // Return label if we have one
            return label.clone();
        }

        // Base of filename is "file|extract" or just "file"
        let filename_base = if let Some(extract) = &self.extract {
            format!("{}|{}", self.file, extract)
        } else {
            self.file.clone()
        };

        // If location specified, append "|start=0x...,length=0x..."
        if let Some(location) = &self.location {
            format!(
                "{}|start={:#X},length={:#X}",
                filename_base, location.start, location.length
            )
        } else {
            filename_base
        }
    }
}

/// Details about a file to be loaded by the caller
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileSpec {
    /// File ID to be used when adding the loaded file to the builder
    pub id: usize,

    /// Optional description for this file.  Provided for information only.
    pub description: Option<String>,

    /// Filename or URL of the ROM image to be loaded
    pub source: String,

    /// Optional extract path within an archive (zip/tar) if the file pointed
    /// to is an archive.  If extract is present, the file at that path within
    /// the archive should be extracted before returning the data to the
    /// builder.
    pub extract: Option<String>,

    /// Size handling configuration for this ROM.  Provided for information
    /// only.
    pub size_handling: SizeHandling,

    /// Type of Chip.  Provided for information only.
    pub chip_type: ChipType,

    /// Size of the ROM in bytes.  Provided for information only.
    pub rom_size: usize,

    /// Optional Chip Select 1 logic - only valid for ROM Types that have CS1.
    /// Provided for information only.
    pub cs1: Option<CsLogic>,

    /// Optional Chip Select 2 logic - only valid for ROM Types that have CS2.
    /// Provided for information only.
    pub cs2: Option<CsLogic>,

    /// Optional Chip Select 3 logic - only valid for ROM Types that have CS3.
    /// Provided for information only.
    pub cs3: Option<CsLogic>,

    /// Optional Chip Select 4 logic - only valid for ROM Types that have CS4.
    /// Provided for information only.
    pub cs4: Option<CsLogic>,

    /// Optional Chip Enable logic override.  Provided for information only.
    pub ce: Option<CsLogic>,

    /// Optional Output Enable logic override.  Provided for information only.
    pub oe: Option<CsLogic>,

    /// ROM Set ID that this file belongs to.  Provided for information only.
    pub set_id: usize,

    /// ROM Set type that this file belongs to.  Provided for information only.
    pub set_type: ChipSetType,

    /// Optional ROM Set description that this file belongs to.  Provided for
    /// information only.
    pub set_description: Option<String>,
}

/// File data loaded by the caller, passed back to the builder.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FileData {
    /// File ID as per FileSpec
    pub id: usize,

    /// File data
    pub data: alloc::vec::Vec<u8>,
}

/// Location within a larger Chip image that the specific image to use resides
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Location {
    /// Start of the image within the larger Chip image
    pub start: usize,

    /// Length of the image within the larger Chip image.  Must match the
    /// selected Chip type, or SizeHandling will be applied.
    pub length: usize,
}
