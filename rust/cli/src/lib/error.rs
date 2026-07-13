// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Shared error type for the One ROM CLI library.

use onerom_config::fw::FirmwareVersion;

use crate::plugin::{PluginType, PluginVersion};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Hit an error accessing USB:\n  {0}")]
    Usb(String),

    #[error("No One ROMs found")]
    NoDevices,

    #[error("Multiple One ROMs found.  Use --serial to select one.\n  Found: {}", .0.join(", "))]
    MultipleDevices(Vec<String>),

    #[error("One ROM not found: {0}")]
    DeviceNotFound(String),

    #[error("Hit an input/output error: {0}")]
    Io(String),

    #[error("{0}")]
    Other(String),

    #[error("Unknown board type: {0}\n  Known board types: {1}")]
    InvalidBoard(String, String),

    #[error(
        "You must not specify both --serial and --board together.\n  If --serial is specified, this is used to determine the board type automatically if possible."
    )]
    DeviceAndBoard,

    #[error("The selected operation does not apply to a One ROM.\n  Do not specify --serial.")]
    Device,

    #[error(
        "No One ROM was found or specified.\n  Specify a One ROM using --serial.\n  Use 'onerom scan' to list connected One ROMs."
    )]
    NoDevice,

    #[error("The '{0}' command has not been implemented")]
    Unimplemented(String),

    #[error(
        "The operation attempted to access an unsupported memory region\n  Address {0:#010x}, length {1:#010x}"
    )]
    InvalidMemoryRange(u32, u32),

    #[error("The specified memory range is not accessible when One ROM isn't running")]
    MemoryDeviceNotRunning,

    #[error("The specificied memory range is not writeable")]
    MemoryNotWriteable,

    #[error("This operation can only be performed on a One ROM that is running")]
    NotRunning,

    #[error("This operation cannot be performed as the ROM type is unknown")]
    UnknownRomType,

    #[error(
        "The operation attempted to access past the end of a live ROM image.\n  The {0} size is {1} bytes"
    )]
    LiveOutOfBounds(String, usize),

    #[error("Cannot determine the board type.\n  Either --board or --serial must be specified.")]
    NoBoardOrDevice,

    #[error("Specified version '{0}' not found.\n  Available releases: {1}")]
    VersionNotFound(String, String),

    #[error("No latest release found in manifest.\n  This is likely a bug.  Please report it.")]
    NoLatestRelease,

    #[error("License was not accepted.\n  You must accept the license to proceed.")]
    LicenseNotAccepted,

    #[error(
        "Above stock value for {0} was not accepted.\n  You must accept or modify the configuration to proceed."
    )]
    AboveStockNotAccepted(String),

    #[error(
        "The base firmware image supplied is larger than the maximum supported\n  {0} bytes supplied vs {1} bytes maximum"
    )]
    BaseFirmwareTooLarge(usize, usize),

    #[error(
        "Assembled firmware has parse errors (use --force to override):\n  {0}\n  This is likely a bug.  Please report it."
    )]
    FirmwareValidation(String),

    #[error("Failed to stop device, cannot proceed.\n  This is likely a bug.  Please report it.")]
    DeviceStillRunning,

    #[error("Flash verification failed at offset {0:#010x}:\n  Expected {1:#04x}, got {2:#04x}")]
    VerifyFailed(usize, u8, u8),

    #[error("Invalid '{0}' argument found:\n  {1}")]
    InvalidArgument(String, String),

    #[error(
        "Cannot program One ROM as no configuration or firmware specified.\n  Use --config, --slot, --firmware, or --base-firmware."
    )]
    NoFirmwareSource,

    #[error("Unexpected reboot state specified.\n  This is likely a bug.  Please report it.")]
    NoReboot,

    #[error("Unsupported chip type '{0}'.\n  Supported types for this board: {1}")]
    UnsupportedChipType(String, String),

    #[error(
        "This board does not support chip types {1}.\n  Supported types: {2}\n  Use --allow-unsupported-chip-type to override."
    )]
    UnsupportedBoardChipType(String, String, String),

    #[error(
        "Could not determine board type from the connected device {0}.\n  It may be an unprogrammed One ROM or have corrupt firmware.\n  Supply the board type with --board"
    )]
    NoBoardFromDevice(String),

    #[error(
        "The selected One ROM does not support that operation.\n  {0}\n  The firmware may be too old, or the USB system plugin may not be present."
    )]
    CannotRun(String),

    #[error(
        "The selected One ROM does not support being rebooted into running mode.\n  {0}\n  The firmware may be too old, or the USB system plugin may not be present."
    )]
    NoRebootIntoRunning(String),

    #[error("Hit a network error accessing URL {0}.\n  {1}")]
    Network(String, String),

    #[error("Hit an HTTP error accessing URL {0}.\n  Status code {1}")]
    Http(String, u16),

    #[error("Hit an error parsing JSON from {0}.\n  {1}")]
    Json(String, String),

    #[error(
        "A {0} plugin has already been specified.\n  At most one system plugin and one user plugin are supported."
    )]
    DuplicatePlugin(PluginType),

    #[error(
        "A user plugin was specified without a system plugin.\n  A system plugin is required when using a user plugin."
    )]
    UserPluginWithoutSystem,

    #[error(
        "Plugin binary is too large to fit in a plugin slot.\n  {0} bytes supplied vs {1} bytes maximum"
    )]
    PluginTooLarge(usize, usize),

    #[error(
        "Plugin '{0}' not found in the release manifest.\n  Use 'onerom plugin' to list available plugins."
    )]
    PluginNotFound(String),

    #[error(
        "Plugin '{0}' version '{1}' not found in the release manifest.\n  Use 'onerom plugin --all-versions' to list available versions."
    )]
    PluginVersionNotFound(String, String),

    #[error(
        "Plugin '{0}' version '{1}' requires firmware {2} or later.\n  The selected firmware version is {3}."
    )]
    PluginIncompatible(String, PluginVersion, FirmwareVersion, FirmwareVersion),

    #[error(
        "Plugin binary from '{0}' is too small to contain a valid header: {1} bytes (minimum {2})"
    )]
    PluginBinaryTooSmall(String, usize, usize),

    #[error("Plugin binary from '{0}' has invalid magic: {1:#010x} (expected {2:#010x})")]
    PluginInvalidMagic(String, u32, u32),

    #[error("Plugin type mismatch for '{0}': manifest says {1}, binary header says {2}")]
    PluginTypeMismatch(String, String, String),

    #[error("Plugin version mismatch for '{0}': manifest says {1}, binary header says {2}")]
    PluginVersionMismatch(String, PluginVersion, PluginVersion),

    #[error("SHA256 mismatch for plugin binary '{0}':\n  expected {1}\n  got      {2}")]
    PluginSha256Mismatch(String, String, String),

    #[error("Plugin binary from '{0}' is a PIO plugin, which is not currently supported")]
    PluginPioNotSupported(String),

    #[error("Plugin binary from '{0}' has unrecognised plugin type: {1}")]
    PluginUnknownBinaryType(String, u8),

    #[error("Plugin '{0}' has unrecognised type '{1}' in manifest")]
    PluginUnknownManifestType(String, String),

    #[error(
        "ROM image '{0}' has an odd number of bytes ({1}).\n  Byte swapping requires an even-length input file."
    )]
    OddLengthImage(String, usize),

    #[error(
        "Firmware board type '{0}' does not match the expected board type '{1}'.\n  Use --force to override."
    )]
    BoardMismatch(String, String),

    #[error(
        "Plugin '{0}' version '{1}' is not compatible with firmware {2} or later.\n  The selected firmware version is {3}."
    )]
    PluginIncompatibleNewer(String, PluginVersion, FirmwareVersion, FirmwareVersion),
}

impl Error {
    pub fn io(path: impl AsRef<std::path::Path>, e: std::io::Error) -> Self {
        Self::Io(format!("{}: {e}", path.as_ref().display()))
    }
}

impl From<onerom_fw::Error> for Error {
    fn from(e: onerom_fw::Error) -> Self {
        Self::Other(e.to_string())
    }
}

impl From<onerom_config::Error> for Error {
    fn from(e: onerom_config::Error) -> Self {
        Self::Other(format!("{e}"))
    }
}

impl From<onerom_app::PluginError> for Error {
    fn from(p: onerom_app::PluginError) -> Self {
        use onerom_app::PluginError as P;
        match p {
            P::DuplicatePlugin(t) => Error::DuplicatePlugin(t),
            P::UserPluginWithoutSystem => Error::UserPluginWithoutSystem,
            P::TooLarge(size, max) => Error::PluginTooLarge(size, max),
            P::NotFound(name) => Error::PluginNotFound(name),
            P::VersionNotFound(name, v) => Error::PluginVersionNotFound(name, v.to_string()),
            P::Incompatible {
                name,
                version,
                min_fw,
                fw,
            } => Error::PluginIncompatible(name, version, min_fw, fw),
            P::IncompatibleNewer {
                name,
                version,
                from,
                fw,
            } => Error::PluginIncompatibleNewer(name, version, from, fw),
            P::BinaryTooSmall(src, actual, min) => Error::PluginBinaryTooSmall(src, actual, min),
            P::InvalidMagic(src, got, expected) => Error::PluginInvalidMagic(src, got, expected),
            P::TypeMismatch(src, expected, got) => {
                Error::PluginTypeMismatch(src, expected.to_string(), got.to_string())
            }
            P::VersionMismatch(name, manifest, header) => {
                Error::PluginVersionMismatch(name, manifest, header)
            }
            P::Sha256Mismatch {
                binary,
                expected,
                got,
            } => Error::PluginSha256Mismatch(binary, expected, got),
            P::PioNotSupported(src) => Error::PluginPioNotSupported(src),
            P::UnknownBinaryType(src, v) => Error::PluginUnknownBinaryType(src, v),
            P::UnknownManifestType(name, ty) => Error::PluginUnknownManifestType(name, ty),
            P::SpecSyntax(msg) => Error::InvalidArgument("--plugin".to_string(), msg),
            P::ManifestJson(url, detail) => Error::Json(url, detail),
        }
    }
}

impl From<onerom_app::Error<onerom_fw::Error>> for Error {
    fn from(e: onerom_app::Error<onerom_fw::Error>) -> Self {
        match e {
            // Fetch failures carry onerom-fw's own error; map it as onerom-fw
            // errors are mapped elsewhere in the CLI (via From<onerom_fw::Error>).
            onerom_app::Error::Fetch { error, .. } => error.into(),
            onerom_app::Error::Plugin(p) => p.into(),
        }
    }
}
