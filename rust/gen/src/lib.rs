// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Generates firmware artifacts for One ROM.

#![no_std]

extern crate alloc;

pub mod builder;
pub mod chip_type_spec;
pub mod compat;
pub mod firmware;
pub mod ihex;
pub mod image;
pub mod meta;
pub mod transform;
pub mod v1;
pub mod v2;

pub use builder::Builder;
pub use chip_type_spec::ChipTypeSpec;
pub use firmware::{
    DebugConfig, FireConfig, FireCpuFreq, FireServeMode, FireVreg, FirmwareConfig, IceConfig,
    IceCpuFreq, LedConfig, ServeAlgParams,
};
pub use ihex::{
    AddressParseError, IHEX_BLANK_BYTE, IhexError, LoadAddress, decode_ihex, encode_ihex,
};
pub use image::{Chip, ChipSet, ChipSetType, CsConfig, CsLogic, FileFormat, SizeHandling};
pub use image::{MAX_IMAGE_SIZE, PAD_BLANK_BYTE, PAD_NO_CHIP_BYTE};
pub use image::{num_excess_addr_lines, requires_half_select_cs1};
pub use meta::{MAX_METADATA_LEN, Metadata, PAD_METADATA_BYTE};
use onerom_config::mcu::Family;
pub use transform::{
    TRANSFORM_LIST_SEPARATOR, Transform, TransformError, apply_transforms, format_transform_list,
    parse_transform_list,
};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
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
pub use v2::alg_preference::{AddrAlgPreference, CsAlgPreference, DataAlgPreference};
pub use v2::slot_context::socket_pin_offset;

/// Version of metadata produced by this version of the crate
pub const METADATA_VERSION: u32 = 1;
const METADATA_VERSION_STR: &str = "1";

/// Firmware size reserved at the start of flash, before metadata
pub const FIRMWARE_SIZE: usize = 48 * 1024; // 48KB

// The V1 and V2 metadata regions are the same size, which is what lets
// [`rom_data_space`] serve both build paths. `MAX_METADATA_LEN` bounds what the
// V1 writer emits; `METADATA_SIZE` is the fixed region the V2 layout reserves.
// If they ever diverge, `rom_data_space` needs to take the metadata size (or
// the schema) as an argument rather than assuming one value.
const _: () = assert!(MAX_METADATA_LEN == onerom_metadata::METADATA_SIZE);

/// Flash available for ROM image data on `mcu_variant`, in bytes.
///
/// This is the whole flash less the two fixed regions that precede the ROM
/// images: the firmware ([`FIRMWARE_SIZE`], 48KB) and the metadata region
/// (16KB). On the RP2350's 2MB flash that leaves 1984KB.
///
/// Both builder paths bound their composed ROM data with this, and it is the
/// budget a caller should size a set of images against. Note the separate,
/// smaller [`MAX_IMAGE_SIZE`] cap that applies to any *single* slot - that is a
/// RAM limit, not a flash one, so a set of images can be within this budget yet
/// still contain a slot that is too large to serve.
pub fn rom_data_space(mcu_variant: onerom_config::mcu::Variant) -> usize {
    mcu_variant.flash_storage_bytes() - FIRMWARE_SIZE - MAX_METADATA_LEN
}

pub const MIN_FIRMWARE_OVERRIDES_VERSION: FirmwareVersion = FirmwareVersion::new(0, 6, 0, 0);

/// Error type
///
/// This enum is `#[non_exhaustive]`: new variants may be added in a
/// backwards-compatible release, so a `match` on it outside this crate needs a
/// wildcard arm.  Enabling `clippy::wildcard_enum_match_arm` will then point at
/// that arm whenever a new variant appears, rather than letting it be absorbed
/// silently.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
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
    /// An Intel HEX image failed to decode.
    IntelHex {
        index: usize,
        source: ihex::IhexError,
    },
    /// `size_handling: duplicate` was requested for an Intel HEX image, which
    /// places data by address and cannot be meaningfully duplicated.
    IhexDuplicateUnsupported {
        index: usize,
    },
    /// A non-zero `load_address` was set on a chip that is not Intel HEX.
    LoadAddressWithoutIhex {
        index: usize,
    },
    /// A chip's `transform` list could not be applied to its image.
    Transform {
        index: usize,
        source: transform::TransformError,
    },
    /// Turbo boot was enabled on a config with more than one non-plugin ROM
    /// slot.  Accept it with [`ConfigOverrides::allow_turbo_boot_multi_slot`],
    /// which turns this into [`ConfigWarning::TurboBootMultiSlot`].
    TurboBootMultiSlot {
        slots: usize,
    },
}
type Result<T> = core::result::Result<T, Error>;

/// Config checks the caller is willing to accept, downgrading each from an
/// error to a [`ConfigWarning`].
///
/// The default enforces every check, which is what [`Builder::from_json`]
/// uses; [`Builder::from_json_with_overrides`] takes a value of this type.
#[derive(Debug, Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct ConfigOverrides {
    /// Whether to accept turbo boot on a config with more than one non-plugin
    /// ROM slot.
    pub turbo_boot_multi_slot: bool,
}

impl ConfigOverrides {
    /// Accept turbo boot on a config with more than one non-plugin ROM slot.
    ///
    /// Turbo boot does not read the image select jumpers, so only the first
    /// non-plugin slot is served at boot.  The remaining slots are still
    /// programmed, and are reachable at runtime - by a bootloader in the
    /// first slot, or by a plugin switching slots - so the combination is
    /// deliberate in some configurations.
    ///
    /// [`ConfigOverrides`] is `#[non_exhaustive]`, so this (rather than a
    /// struct literal) is how callers outside this crate set the field.
    pub fn allow_turbo_boot_multi_slot(mut self, allow: bool) -> Self {
        self.turbo_boot_multi_slot = allow;
        self
    }
}

/// A config problem the caller accepted via [`ConfigOverrides`], and which
/// would otherwise have been an [`Error`].
///
/// Returned by [`Builder::from_json_with_overrides`], for the caller to
/// report as it sees fit.
///
/// This enum is `#[non_exhaustive]`: new variants may be added in a
/// backwards-compatible release, so a `match` on it outside this crate needs
/// a wildcard arm.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum ConfigWarning {
    /// Turbo boot is enabled on a config with `slots` non-plugin ROM slots.
    TurboBootMultiSlot { slots: usize },
}

impl core::fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ConfigWarning::TurboBootMultiSlot { slots } => {
                write!(f, "{}", turbo_boot_multi_slot_msg(*slots))
            }
        }
    }
}

/// The description shared by [`Error::TurboBootMultiSlot`] and
/// [`ConfigWarning::TurboBootMultiSlot`], so the rejected and the accepted
/// case describe the config identically.
fn turbo_boot_multi_slot_msg(slots: usize) -> alloc::string::String {
    alloc::format!(
        "Turbo boot is enabled with {slots} non-plugin ROM slots.  Turbo boot does not read the image select jumpers, so only the first non-plugin slot is served at boot."
    )
}

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
            Error::IntelHex { index, source } => write!(
                f,
                "The Intel HEX image for chip {index} could not be decoded:\n  {source}"
            ),
            Error::IhexDuplicateUnsupported { index } => write!(
                f,
                "Chip {index}: the duplicate size-handling option is not supported for Intel HEX images, which place data by address"
            ),
            Error::LoadAddressWithoutIhex { index } => write!(
                f,
                "Chip {index}: load_address is only valid for Intel HEX images (format: ihex)"
            ),
            Error::Transform { index, source } => {
                write!(f, "Chip {index}: {source}")
            }
            Error::TurboBootMultiSlot { slots } => {
                write!(f, "{}", turbo_boot_multi_slot_msg(*slots))
            }
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
#[non_exhaustive]
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

impl ChipSetConfig {
    /// Creates a chip set of `set_type` containing `chips`, with every
    /// optional setting at its default.
    ///
    /// [`ChipSetConfig`] is `#[non_exhaustive]`, so this (rather than a struct
    /// literal) is how callers outside this crate build one; assign the
    /// optional fields afterwards.
    pub fn new(set_type: ChipSetType, chips: Vec<ChipConfig>) -> Self {
        Self {
            set_type,
            description: None,
            chips,
            serve_alg: None,
            firmware_overrides: None,
        }
    }
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
#[non_exhaustive]
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

    /// Whether to enable boot logging.  Logging is emitted over RTT, so a
    /// debug probe must be attached to see it.  Compatible with
    /// swd_enabled = false, as SWD stays up for the whole of boot.
    #[serde(default = "default_boot_logging")]
    pub boot_logging: bool,

    /// Whether to leave SWD enabled once the One ROM starts serving.
    ///
    /// When false, SWD is available for the whole of boot and is shut off
    /// immediately before serving starts, staying off until the next reset.
    /// This stops debug port accesses to SRAM stealing cycles from the
    /// serving DMAs.  Nothing is logged past that point, and plugins get no
    /// logging.
    ///
    /// This is not a debug lockout - the boot ROM runs before the One ROM
    /// firmware does, and BOOTSEL/PICOBOOT are unaffected.
    #[serde(default = "default_swd_enabled")]
    pub swd_enabled: bool,

    /// Whether to boot fast.  Disables reading the image select jumpers.
    /// The first non-plugin image is served.
    #[serde(default = "default_turbo_boot")]
    pub turbo_boot: bool,
}

impl Config {
    /// Creates a configuration containing `chip_sets`, with every optional
    /// setting at its default.
    ///
    /// [`Config`] is `#[non_exhaustive]`, so this (rather than a struct
    /// literal) is how callers outside this crate build one; assign the
    /// optional fields afterwards.  The defaults match those applied when
    /// deserialising a config file that omits them.
    pub fn new(description: String, chip_sets: Vec<ChipSetConfig>) -> Self {
        Self {
            version: 1,
            name: None,
            description,
            detail: None,
            chip_sets,
            notes: None,
            categories: None,
            instance_name: None,
            serial_override: None,
            boot_logging: default_boot_logging(),
            swd_enabled: default_swd_enabled(),
            turbo_boot: default_turbo_boot(),
        }
    }
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
#[non_exhaustive]
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
#[non_exhaustive]
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
    pub chip_type: ChipTypeSpec,

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

    /// Format of the supplied ROM image.  Defaults to raw binary.  Set to
    /// `ihex` to have the generator decode an Intel HEX file into a binary
    /// image before use.
    #[serde(default, skip_serializing_if = "FileFormat::is_binary")]
    pub format: FileFormat,

    /// For Intel HEX images ([`FileFormat::IntelHex`]), the absolute address
    /// that maps to byte 0 of the ROM.  Record addresses below it are an
    /// error; the highest address defines the image extent.  Accepts a decimal
    /// or a `0x`/`$`-prefixed hex value.  Must be 0 (unset) for binary images.
    /// Defaults to 0.
    #[serde(default, skip_serializing_if = "LoadAddress::is_zero")]
    pub load_address: LoadAddress,

    /// Byte-level transformations to apply to the supplied image, in order.
    ///
    /// Applied after any [`Location`] slice and before the image is reconciled
    /// against the chip size by [`SizeHandling`], and after an Intel HEX image
    /// has been decoded — so transforms behave the same whichever format the
    /// image arrived in.  Order is significant; see the
    /// [`transform`] module.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transform: Vec<Transform>,
}

impl ChipConfig {
    /// Creates a chip configuration for `file` as a `chip_type` part, with
    /// every optional setting at its default.
    ///
    /// [`ChipConfig`] is `#[non_exhaustive]`, so this (rather than a struct
    /// literal) is how callers outside this crate build one; assign the
    /// optional fields afterwards.  `file` and `chip_type` are the two
    /// settings with no meaningful default: a chip has to have a type, and
    /// only a RAM chip may lack an image.
    pub fn new(file: String, chip_type: ChipTypeSpec) -> Self {
        Self {
            file,
            license: None,
            description: None,
            chip_type,
            cs1: None,
            cs2: None,
            cs3: None,
            cs4: None,
            ce: None,
            oe: None,
            allow_cs_ignore: false,
            size_handling: SizeHandling::default(),
            extract: None,
            label: None,
            location: None,
            format: FileFormat::default(),
            load_address: LoadAddress::default(),
            transform: Vec::new(),
        }
    }

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
        let filename_base = if let Some(location) = &self.location {
            format!(
                "{}|start={:#X},length={:#X}",
                filename_base, location.start, location.length
            )
        } else {
            filename_base
        };

        // Record any transforms, so the metadata says how the served bytes
        // were derived from the named source and not just where they came
        // from.  Uses the same text encoding as the CLI's `transform=` key.
        if self.transform.is_empty() {
            filename_base
        } else {
            format!(
                "{}|transform={}",
                filename_base,
                format_transform_list(&self.transform)
            )
        }
    }
}

/// Details about a file to be loaded by the caller
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
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

    /// Format of the supplied ROM image.  The caller loads the raw bytes
    /// regardless of format; decoding (e.g. Intel HEX) is performed by the
    /// builder.  Provided for information only.
    #[serde(default)]
    pub format: FileFormat,

    /// For Intel HEX images, the load address mapping to ROM byte 0.  Provided
    /// for information only.
    #[serde(default)]
    pub load_address: LoadAddress,

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
#[non_exhaustive]
pub struct FileData {
    /// File ID as per FileSpec
    pub id: usize,

    /// File data
    pub data: alloc::vec::Vec<u8>,
}

impl FileData {
    /// Creates the loaded contents of the file identified by `id` in the
    /// corresponding [`FileSpec`].
    pub fn new(id: usize, data: alloc::vec::Vec<u8>) -> Self {
        Self { id, data }
    }
}

/// Location within a larger Chip image that the specific image to use resides
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct Location {
    /// Start of the image within the larger Chip image
    pub start: usize,

    /// Length of the image within the larger Chip image.  Must match the
    /// selected Chip type, or SizeHandling will be applied.
    pub length: usize,
}

impl Location {
    /// Creates a location `length` bytes long, starting at `start` within the
    /// supplied image.
    pub fn new(start: usize, length: usize) -> Self {
        Self { start, length }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onerom_config::mcu::Variant;

    /// The ROM budget is the flash less the firmware and metadata regions -
    /// 1984KB of the RP2350's 2MB. Both builder paths bound their composed ROM
    /// data with this, so a wrong value here silently changes what builds.
    #[test]
    fn rom_data_space_excludes_the_firmware_and_metadata_regions() {
        let space = rom_data_space(Variant::RP2350);
        assert_eq!(space, 1984 * 1024);
        assert_eq!(
            space,
            Variant::RP2350.flash_storage_bytes() - FIRMWARE_SIZE - MAX_METADATA_LEN
        );
        assert_eq!(rom_data_space(Variant::RP2350B), space);
    }

    /// A single slot can be at most MAX_IMAGE_SIZE, which must stay well inside
    /// the whole-flash budget - otherwise the per-slot RAM cap, not flash,
    /// would be what limits a single-image build.
    #[test]
    fn a_single_slot_fits_the_rom_budget() {
        assert!(MAX_IMAGE_SIZE < rom_data_space(Variant::RP2350));
    }
}
