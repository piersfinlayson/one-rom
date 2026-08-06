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

    /// A board the CLI can describe but cannot act on.
    ///
    /// Every firmware and device path here is RP2350-only - images are composed
    /// for [`Variant::RP2350`](onerom_config::mcu::Variant) and devices are
    /// reached over picoboot - so an Ice (STM32) board has no image to build and
    /// no bootloader to talk to. Saying so here beats letting it surface as a
    /// missing-release error from the manifest lookup, which describes a symptom
    /// rather than the cause.
    #[error(
        "Board '{0}' is an Ice (STM32) board, which this command does not support.\n  This command supports Fire (RP2350) boards only."
    )]
    IceBoardUnsupported(String),

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

    /// A device-oriented view could not identify the connected One ROM's board.
    ///
    /// Reached only with a One ROM *connected*: the caller checks for a device
    /// first, so a missing one is [`Error::NoDevice`]. What is left is a One ROM
    /// reporting a board type this build does not recognise, which the
    /// command's own `--board` override exists to answer. Unlike
    /// [`Error::NoBoardOrDevice`] there is no point offering `--serial`, which
    /// would only select a different One ROM.
    #[error(
        "Cannot determine the board type.\n  The connected One ROM reports a board type this build does not recognise.\n  Name it with --board, or use 'onerom board {0} --board <board>' to draw a\n  board by name."
    )]
    NoDeviceForBoardView(String),

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

    #[error("This board cannot serve chip types {1}.\n  Supported types: {2}")]
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
        "{0}\n  Use --force to program it anyway - for example when the first slot holds a bootloader that selects the others itself."
    )]
    TurboBootMultiSlot(onerom_gen::Error),

    #[error(
        "Plugin '{0}' version '{1}' is not compatible with firmware {2} or later.\n  The selected firmware version is {3}."
    )]
    PluginIncompatibleNewer(String, PluginVersion, FirmwareVersion, FirmwareVersion),

    #[error("Failed to decode Intel HEX from '{0}':\n  {1}")]
    IhexDecode(String, String),

    #[error("Failed to transform ROM image '{0}':\n  {1}")]
    ImageTransform(String, String),

    #[error("Invalid --pin value '{0}':\n  {1}")]
    InvalidPin(String, String),

    #[error(
        "This One ROM's USB system plugin predates GPIO control.\n  {0}\n  Reprogram it with the v0.7.1 or later USB system plugin, for example:\n    onerom program --config <your config> --plugin usb"
    )]
    PluginTooOldForGpio(String),

    #[error(
        "This One ROM's firmware predates GPIO control.\n  {0}\n  Its USB system plugin supports GPIO control but the firmware beneath it does not.\n  Update the device to One ROM firmware v0.7.1 or later."
    )]
    FirmwareTooOldForGpio(String),

    #[error(
        "This One ROM cannot hold a GPIO for a bounded period.\n  {0}\n  Update the device to One ROM firmware v0.7.1 or later, or omit --hold."
    )]
    GpioHoldUnsupported(String),

    #[error("A hold of {0}ms is longer than this One ROM allows.\n  Its maximum is {1}ms.")]
    GpioHoldTooLong(u32, u32),

    #[error("This One ROM has no GPIO{0}.\n  It reports {1} GPIOs, GPIO0 upwards.")]
    GpioOutOfRange(u8, u8),

    #[error(
        "GPIO{0} is in use by One ROM.\n  Use --force to drive it anyway - see 'onerom inspect gpio' for what it is doing."
    )]
    GpioInUse(u8),

    #[error(
        "This One ROM rejected the request for GPIO{0} as invalid.\n  This is likely a bug.  Please report it."
    )]
    GpioRejected(u8),

    #[error("This One ROM returned a response that could not be decoded:\n  {0}")]
    PicobootxDecode(String),

    #[error(
        "This One ROM is not running, so its GPIOs cannot be read or driven.\n  {0}\n  A stopped One ROM sits in the RP2350 bootloader, where One ROM's own\n  command handler is not running.\n  Start it with 'onerom control reboot --running'."
    )]
    DeviceNotRunning(String),

    #[error("{0} is in use by One ROM: {1}.\n  {2}\n  {3}")]
    GpioInUseNamed(String, String, String, String),

    #[error(
        "This One ROM is already holding as many GPIOs as it can.\n  Release one first - drive it with no --hold, or wait for a hold to expire."
    )]
    GpioNoHoldSlot,
}

impl Error {
    pub fn io(path: impl AsRef<std::path::Path>, e: std::io::Error) -> Self {
        Self::Io(format!("{}: {e}", path.as_ref().display()))
    }
}

/// Phase 2 left `DecodeError` local to `picobootx.rs` so that module stays a
/// pure description of the wire format. This is where a wire response the host
/// could not make sense of becomes something a user sees.
impl From<crate::picobootx::DecodeError> for Error {
    fn from(e: crate::picobootx::DecodeError) -> Self {
        Self::PicobootxDecode(e.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The board-view error offers only advice that would actually work.
    ///
    /// Both routes it names have to be spelled the way the CLI accepts them,
    /// and both have to be reachable from where the user is: a One ROM *is*
    /// connected (the caller has already checked), it is just one whose board
    /// this build does not know. So `--board` is the fix, and `--serial` -
    /// which the shared [`Error::NoBoardOrDevice`] offers - would only pick a
    /// different One ROM.
    ///
    /// This is easy to break by renaming an argument and not the message, which
    /// is exactly what happened when `board header` took its board
    /// positionally.
    #[test]
    fn board_view_error_offers_only_advice_that_works() {
        for view in ["header", "socket"] {
            let msg = Error::NoDeviceForBoardView(view.to_string()).to_string();
            // The override on this very command, which resolves the situation.
            assert!(msg.contains("--board"), "{view}: {msg}");
            // The escape hatch, spelled as the `board` command actually parses
            // it - not the positional form it once took.
            assert!(
                msg.contains(&format!("onerom board {view} --board <board>")),
                "{view}: {msg}"
            );
            // Would only select a different One ROM, not name this one's board.
            assert!(!msg.contains("--serial"), "{view}: {msg}");
        }
    }

    /// The shared error still gives the `--board` advice, which is correct for
    /// the commands that have one (`program`, `firmware build`, `board ...`).
    #[test]
    fn shared_no_board_error_still_advises_board_or_serial() {
        let msg = Error::NoBoardOrDevice.to_string();
        assert!(msg.contains("--board"), "{msg}");
        assert!(msg.contains("--serial"), "{msg}");
    }
}
