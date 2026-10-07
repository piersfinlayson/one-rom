// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Shared error type for the One ROM CLI library.

use onerom_app::FlashPlanError;
use onerom_config::fw::FirmwareVersion;
use onerom_fw_parser::ImageFileError;
use onerom_gen::FileFormat;
use onerom_metadata::{MaybeKnown, OneromBoardSize};

use crate::device::flash_chips;
use crate::hint;
use crate::otp::{board_size_text, escape_controls};
use crate::plugin::{CompatibleRelease, PluginType, PluginVersion};

/// Render the way out of a plugin incompatibility as a further indented line.
///
/// A build that stops here cannot proceed until the user changes something, so
/// naming the release that would work - and the URL a config has to point at to
/// use it - is worth the extra line.
fn plugin_way_out(newest_compatible: &Option<CompatibleRelease>) -> String {
    match newest_compatible {
        Some(r) => format!(
            "\n  Plugin version {} supports it: {}",
            r.version, r.binary_url
        ),
        None => "\n  No version of this plugin supports the selected firmware version.".to_string(),
    }
}

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
        "No One ROM was found or specified.\n  Specify a One ROM using --serial.\n  Use '{scan}' to list connected One ROMs.",
        scan = hint::SCAN
    )]
    NoDevice,

    #[error("The '{0}' command has not been implemented")]
    Unimplemented(String),

    #[error("Hit an error accessing the serial port:\n  {0}")]
    SerialPort(String),

    /// No CDC serial port belongs to the selected One ROM.
    ///
    /// The device is running, so it is on the USB bus, but nothing on this host
    /// presents a serial port for it. Either its USB plugin does not offer the
    /// CDC interface, or the platform has not bound its CDC driver to it.
    #[error(
        "No serial port was found for this One ROM.\n  {detail}\n  A serial port needs the USB system plugin - flash one with\n  '{usb}'.",
        detail = .0,
        usb = hint::PROGRAM_WITH_USB
    )]
    SerialPortNotFound(String),

    #[error(
        "Could not open this One ROM's serial port {0}:\n  {1}\n  Another program may already have it open."
    )]
    SerialPortOpen(String, String),

    /// Nothing arrived within the attach window.
    ///
    /// The USB plugin writes a banner whenever a terminal opens the port - even
    /// when the firmware is too old for the logging API, in which case the
    /// banner says so. So silence is not any of the things that merely stop the
    /// log flowing. It means the plugin is older than the banner.
    #[error(
        "No logs were received in {0} seconds.\n  This suggests this One ROM's USB plugin is not new enough to support logging."
    )]
    LogSilent(f32),

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
        "The operation attempted to access past the end of a live ROM image.\n  The live {0} image is {1} bytes"
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
        "Cannot determine the board type.\n  The connected One ROM reports a board type this build does not recognise.\n  Name it with --board, or use '{by_name}' to draw a\n  board by name.",
        by_name = hint::board_view(.0)
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

    #[error(
        "Cannot program {0} because it is One ROM Lab firmware.\n  onerom program only programs One ROM firmware."
    )]
    LabFirmware(String),

    #[error("Failed to stop device, cannot proceed.\n  This is likely a bug.  Please report it.")]
    DeviceStillRunning,

    #[error("Flash verification failed at offset {0:#010x}:\n  Expected {1:#04x}, got {2:#04x}")]
    VerifyFailed(usize, u8, u8),

    /// An image that uses the second flash chip, for a One ROM without one.
    /// The `String` is the One ROM's size, as text for the `Board size:` line.
    #[error(
        "Cannot program this image because it requires a board size larger than M.\n  Board size: {0}"
    )]
    SecondChipRequired(String),

    /// An image longer than the One ROM's flash chips together.
    #[error(
        "Cannot program this image because it is larger than this One ROM's flash.\n  {image} bytes supplied vs {flash} bytes maximum"
    )]
    ImageTooLarge { image: usize, flash: usize },

    /// A flash operation this build doesn't know, from a newer onerom-app.
    #[error(
        "Cannot program this image.\n  It requires a flash operation this CLI doesn't support.\n  This is likely a bug.  Please report it."
    )]
    UnknownFlashStep,

    /// An image file whose slots don't match its length or the flash chips.
    #[error("{}", image_file_error_text(.0))]
    ImageFile(ImageFileError),

    /// A chip set that doesn't fit on the flash. `advise_second_chip` where
    /// `firmware build` built for a board without a second flash chip, and
    /// the board and the firmware both support one.
    #[error(
        "{error}{}",
        if *.advise_second_chip { "\n  If the board size is larger than M, use --size." } else { "" }
    )]
    SlotDoesNotFit {
        error: onerom_fw::Error,
        advise_second_chip: bool,
    },

    #[error("Invalid '{0}' argument found:\n  {1}")]
    InvalidArgument(String, String),

    #[error("Aborted:\n  {0}")]
    Aborted(String),

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
        "Plugin '{name}' not found in the release manifest.\n  Use '{list}' to list available plugins.",
        name = .0,
        list = hint::PLUGIN_LIST
    )]
    PluginNotFound(String),

    #[error(
        "Plugin '{name}' version '{version}' not found in the release manifest.\n  Use '{list}' to list available versions.",
        name = .0,
        version = .1,
        list = hint::PLUGIN_ALL_VERSIONS
    )]
    PluginVersionNotFound(String, String),

    #[error(
        "Plugin '{name}' version '{version}' requires firmware {min_fw} or later.\n  The selected firmware version is {fw}.{}",
        plugin_way_out(.newest_compatible)
    )]
    PluginIncompatible {
        name: String,
        version: PluginVersion,
        min_fw: FirmwareVersion,
        fw: FirmwareVersion,
        newest_compatible: Option<CompatibleRelease>,
    },

    #[error(
        "Plugin binary from '{0}' is too small to contain a valid header: {1} bytes (minimum {2})"
    )]
    PluginBinaryTooSmall(String, usize, usize),

    #[error("Plugin binary from '{0}' has invalid magic: {1:#010x} (expected {2:#010x})")]
    PluginInvalidMagic(String, u32, u32),

    #[error("Plugin type mismatch for '{0}': manifest says {1}, binary header says {2}")]
    PluginTypeMismatch(String, String, String),

    #[error("'{name}' is a {plugin_type} plugin but is configured as the {configured} plugin")]
    PluginWrongChipType {
        name: String,
        plugin_type: PluginType,
        configured: PluginType,
    },

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
        "Firmware board type '{firmware}' does not match the expected board type '{expected}'.\n  Use --force to override."
    )]
    BoardMismatch { firmware: String, expected: String },

    #[error(
        "{0}\n  Use --force to program it anyway - for example when the first slot holds a bootloader that selects the others itself."
    )]
    TurboBootMultiSlot(onerom_gen::Error),

    #[error(
        "Plugin '{name}' version '{version}' is not compatible with firmware {from} or later.\n  The selected firmware version is {fw}.{}",
        plugin_way_out(.newest_compatible)
    )]
    PluginIncompatibleNewer {
        name: String,
        version: PluginVersion,
        from: FirmwareVersion,
        fw: FirmwareVersion,
        newest_compatible: Option<CompatibleRelease>,
    },

    #[error("Failed to decode {} from '{path}':\n  {message}", .format.display_name())]
    ImageDecode {
        path: String,
        format: FileFormat,
        message: String,
    },

    #[error("Failed to transform ROM image '{0}':\n  {1}")]
    ImageTransform(String, String),

    #[error("Invalid --pin value '{0}':\n  {1}")]
    InvalidPin(String, String),

    #[error(
        "Invalid --reserve-pin value '{0}':\n  Only image select pins and X pins can be reserved - for example 'sel_c' or 'x1'."
    )]
    InvalidReservePin(String),

    #[error(
        "This One ROM's USB system plugin predates GPIO control.\n  {detail}\n  Reprogram it with the v0.7.1 or later USB system plugin, for example:\n    {usb}",
        detail = .0,
        usb = hint::PROGRAM_WITH_USB
    )]
    PluginTooOldForGpio(String),

    #[error(
        "This One ROM's firmware predates GPIO control.\n  {0}\n  Its USB system plugin supports GPIO control but its firmware does not.\n  Update the device to One ROM firmware v0.7.1 or later."
    )]
    FirmwareTooOldForGpio(String),

    #[error(
        "This One ROM's USB system plugin predates standby.\n  {detail}\n  Reprogram it with the v0.8.0 or later USB system plugin, for example:\n    {usb}",
        detail = .0,
        usb = hint::PROGRAM_WITH_USB
    )]
    PluginTooOldForStandby(String),

    #[error(
        "This One ROM's firmware predates standby.\n  {0}\n  Its USB system plugin supports standby but its firmware does not.\n  Update the device to One ROM firmware v0.8.0 or later."
    )]
    FirmwareTooOldForStandby(String),

    #[error(
        "This One ROM cannot hold a GPIO for a bounded period.\n  {0}\n  Update the device to One ROM firmware v0.7.1 or later, or omit --hold."
    )]
    GpioHoldUnsupported(String),

    #[error("A hold of {0}ms is longer than this One ROM allows.\n  Its maximum is {1}ms.")]
    GpioHoldTooLong(u32, u32),

    #[error(
        "This One ROM does not support --hold or --period.\n  {0}\n  Its USB system plugin is too old."
    )]
    LedArgsUnsupported(String),

    #[error(
        "This One ROM does not support querying LED state.\n  {0}\n  Its firmware or USB system plugin is too old."
    )]
    LedQueryUnsupported(String),

    #[error(
        "This One ROM does not support RGB LED control.\n  {0}\n  Its firmware or USB system plugin is too old."
    )]
    RgbUnsupported(String),

    #[error("This One ROM has no RGB LED.\n  {0}")]
    RgbAbsent(String),

    #[error("This One ROM has no GPIO{0}.\n  It reports {1} GPIOs, GPIO0 upwards.")]
    GpioOutOfRange(u8, u8),

    #[error(
        "GPIO{gpio} is in use by One ROM.\n  Use --force to drive it anyway - see '{inspect}' for what it is doing.",
        gpio = .0,
        inspect = hint::INSPECT_GPIO
    )]
    GpioInUse(u8),

    #[error(
        "This One ROM rejected the request for GPIO{0} as invalid.\n  This is likely a bug.  Please report it."
    )]
    GpioRejected(u8),

    #[error("This One ROM returned a response that could not be decoded:\n  {0}")]
    PicobootxDecode(String),

    #[error(
        "This One ROM is not running, so its GPIOs cannot be read or driven.\n  {detail}\n  A stopped One ROM sits in the RP2350 bootloader, where One ROM's own\n  command handler is not running.\n  Start it with '{start}'.",
        detail = .0,
        start = hint::CONTROL_REBOOT_RUNNING
    )]
    DeviceNotRunning(String),

    #[error("{0} is in use by One ROM: {1}.\n  {2}\n  {3}")]
    GpioInUseNamed(String, String, String, String),

    #[error(
        "This One ROM is already holding as many GPIOs as it can.\n  Release one first - drive it with no --hold, or wait for a hold to expire."
    )]
    GpioHoldLimit,

    #[error(
        "No One ROM CLI build is published for this platform ({0}).\n  Published platforms: {1}\n  Name one explicitly with --target to download it anyway."
    )]
    CliPlatformUnsupported(String, String),

    #[error("Unknown --target '{0}'.\n  Published platforms: {1}")]
    CliTargetUnknown(String, String),

    #[error("One ROM CLI v{0} was not built for '{1}'.\n  It was built for: {2}")]
    CliTargetNotInRelease(String, String, String),

    #[error("Could not parse version '{0}':\n  {1}")]
    CliVersionParse(String, String),

    #[error(
        "SHA256 mismatch for downloaded file '{file}':\n  expected {expected}\n  got      {got}\n  The download was discarded.  Try again, or download from {}.",
        crate::release::DOWNLOAD_PAGE
    )]
    DownloadSha256Mismatch {
        file: String,
        expected: String,
        got: String,
    },

    #[error("File already exists: {0}\n  Use --force to overwrite it.")]
    OutputExists(String),

    #[error("Output directory does not exist: {0}")]
    OutputDirMissing(String),

    /// One of these was refused:
    /// - a signer table
    /// - the table's pointer
    /// - a retired key's record file
    #[error("Can't use the signing keys:\n  {0}.")]
    Signer(onerom_app::SignerError),

    #[error("Hit an error accessing OTP:\n  {0}")]
    Otp(onerom_app::OtpError),

    /// A command needed OTP from a running One ROM Lab, which refuses every
    /// OTP read and write while it runs. Carries the device's line.
    #[error(
        "Cannot access OTP while One ROM Lab is running.\n  {detail}\n  Stop it with '{stop}'.",
        detail = .0,
        stop = hint::CONTROL_REBOOT_STOPPED
    )]
    OtpLabRunning(String),

    #[error("{}", commission_text(.0))]
    Commission(onerom_app::CommissionError),

    /// `hardware commission` failed after it began writing. Where the
    /// connection was lost the text says to run it again.
    #[error("{}", commission_failed_text(.0))]
    CommissionFailed(onerom_app::CommissionError),

    /// `hardware commission` found the One ROM commissioned with other values.
    /// `instance` holds the commissioning instance's values, a line each, as
    /// the command shows them.
    #[error(
        "Cannot commission this One ROM:\n  It is already commissioned:\n{instance}\n  {RE_COMMISSION}"
    )]
    AlreadyCommissioned { instance: String },

    /// An image for a board other than the one the One ROM is commissioned
    /// as.
    #[error(
        "Image board type '{image}' does not match the commissioned board type '{commissioned}'.\n  Use --force to program it anyway."
    )]
    CommissionedBoardMismatch { commissioned: String, image: String },

    /// `hardware set-size` refused the One ROM.
    #[error("{}", set_size_text(.0))]
    SetSize(onerom_app::CommissionError),

    /// `hardware request-signature` refused the One ROM because `hardware
    /// commission` would refuse it.
    #[error("{}", request_signature_text(.0))]
    RequestSignature(onerom_app::CommissionError),

    /// `hardware set-size` failed after it began writing. Where the connection
    /// was lost the text says to run it again.
    #[error("{}", set_size_failed_text(.0))]
    SetSizeFailed(onerom_app::CommissionError),

    /// `hardware commission` found firmware for a board other than `--board`.
    #[error(
        "Firmware board type '{firmware}' does not match the commissioned board type '{board}'.\n  Use --force to override."
    )]
    FirmwareForAnotherBoard { firmware: String, board: String },

    /// `hardware set-size` found firmware for a board other than `--board`.
    #[error(
        "Firmware board type '{firmware}' does not match board type '{board}'.\n  Use --force to override."
    )]
    SetSizeFirmwareForAnotherBoard { firmware: String, board: String },

    /// `hardware request-signature` found firmware for a board other than
    /// `--board`. It doesn't have `--force` so there's no advice.
    #[error("Firmware board type '{firmware}' does not match board type '{board}'.")]
    RequestSignatureFirmwareForAnotherBoard { firmware: String, board: String },

    /// A `hardware` command found an RP2350 stepping One ROM doesn't support.
    /// `stepping` is `A2` or the bootrom version of one this CLI doesn't know.
    /// `--force` doesn't override it so there's no advice.
    #[error(
        "This One ROM's RP2350 is stepping {stepping}, which One ROM doesn't support. It supports A3 and A4."
    )]
    UnsupportedStepping { stepping: String },

    /// A `hardware` command found the bootrom's package and OTP's NUM_GPIOS
    /// disagree.
    #[error("This One ROM's bootloader reports an {package} but it reports {num_gpios} GPIOs.")]
    PackageConflict { package: String, num_gpios: u16 },

    /// A `hardware` command found an RP2350 package other than the one
    /// `--board` is for. `--force` doesn't override it so there's no advice.
    #[error("Board type '{board}' is for an {board_package} but this One ROM has an {package}.")]
    PackageForAnotherBoard {
        board: String,
        board_package: String,
        package: String,
    },

    /// A `hardware` command couldn't read the RP2350 package to check it
    /// against `--board`, or OTP's NUM_GPIOS holds neither package's count.
    #[error(
        "Couldn't read this One ROM's RP2350 package to check it against board type '{board}'."
    )]
    PackageUnknown { board: String },

    /// An encrypted key file without a PIN.
    #[error("Key file {0} is encrypted.\n  Use --pin or run the command in a terminal.")]
    KeyFileEncrypted(String),

    #[error("The PIN doesn't decrypt key file {0}.")]
    KeyFileWrongPin(String),

    #[error("Key file {0} is encrypted in a form this CLI doesn't support.")]
    KeyFileEncryptionUnsupported(String),

    #[error("Key file {0} doesn't contain an Ed25519 key.")]
    KeyFileNotEd25519(String),

    #[error("Key file {0} isn't a PKCS#8 PEM private key.")]
    KeyFileNotPkcs8(String),

    #[error("The signing server requires a PIN.\n  Use --pin or run the command in a terminal.")]
    NoPin,

    /// The signing server replied with an error status and a one-line reason.
    /// `url` is the key's URL as `--signer` gave it.
    #[error("{}", signing_server_text(.url, .status, .message))]
    SigningServer {
        url: String,
        status: u16,
        message: String,
    },

    /// A request to the signing server at the key's URL failed without an
    /// HTTP status, for example because it timed out.
    #[error("Couldn't reach the signing server at {0}.")]
    SigningServerUnreachable(String),

    #[error("The signing server replied with {len} bytes where {expected} were expected.\n  {url}")]
    SigningServerReply {
        url: String,
        len: usize,
        expected: usize,
    },

    #[error("This signing key is invalid.\n  Its public key is {0}.")]
    SigningKeyUnknown(String),

    /// `--key-id` identifies a key the signing key table doesn't contain.
    #[error("Signing key {0} is invalid.")]
    SigningKeyIdUnknown(u16),

    /// The signing server's key `id` isn't the signing key table's key `id`.
    /// `url` is the server's address as `--signer` gave it.
    #[error(
        "Signing key {id} on the signing server at {url} is invalid.\n  Its public key doesn't match key {id} in the signing key table."
    )]
    SigningServerKeyMismatch { url: String, id: u16 },

    #[error("Signing key {name} ({id}) has been retired.")]
    SigningKeyRetired { id: u16, name: String },

    /// The signing key table doesn't allow key `id` to sign `manufacturer`.
    #[error("Signing key {name} ({id}) cannot sign manufacturer '{manufacturer}'.")]
    ManufacturerNotAllowed {
        id: u16,
        name: String,
        manufacturer: String,
    },

    /// A signature that doesn't verify with the signer's key in the table.
    #[error("The signature is invalid with key {name} ({id}).\n  No changes have been made.")]
    BadSignature { id: u16, name: String },

    /// The signature the signing server recorded differs from the one it
    /// returned before the user was asked.
    #[error(
        "The signing server recorded a different signature from the one it signed first.\n  This is likely a bug. Please report it."
    )]
    RecordedSignatureDiffers,

    /// `hardware validate` didn't accept the board's commissioning. It
    /// carries the reason.
    #[error("ERROR: Commissioning information invalid\n  {0}")]
    NotValidated(String),
}

/// The text of an [`Error::SigningServer`] from the key at `url` for HTTP
/// status `status` and the server's `message`. A 404's text is one line
/// without the message.
fn signing_server_text(url: &str, status: &u16, message: &str) -> String {
    let first = match status {
        400 => "The signing server refused the request.".to_string(),
        401 => "The signing server refused the PIN.".to_string(),
        404 => return format!("Invalid signing key {url}"),
        503 => "Signing failed - the signing record cannot be written".to_string(),
        status => format!("The signing server replied with HTTP status {status}."),
    };
    format!("{first}\n  {url}: {message}")
}

/// What to do about a One ROM commissioned with other values.
const RE_COMMISSION: &str = "Use --force to re-commission it.";

/// Who may have written commissioning data this CLI doesn't know.
pub const NEWER_DATA: &str = "It may have been written by a newer version of the CLI.";

/// A command refusing a One ROM with [`refusal`]'s text. Advice beneath a
/// refusal identifies only options the command has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusing {
    Commission,
    SetSize,
    /// `hardware request-signature`, which doesn't have `--force`.
    RequestSignature,
}

/// The text of an [`Error::Commission`].
fn commission_text(error: &onerom_app::CommissionError) -> String {
    format!(
        "Cannot commission this One ROM:\n  {}",
        refusal(error, Refusing::Commission)
    )
}

/// The text of an [`Error::SetSize`].
fn set_size_text(error: &onerom_app::CommissionError) -> String {
    format!(
        "Cannot set this One ROM's size:\n  {}",
        refusal(error, Refusing::SetSize)
    )
}

/// The text of an [`Error::RequestSignature`]. It is `hardware commission`'s
/// refusal without advice identifying an option `hardware request-signature`
/// doesn't have.
fn request_signature_text(error: &onerom_app::CommissionError) -> String {
    format!(
        "Cannot commission this One ROM:\n  {}",
        refusal(error, Refusing::RequestSignature)
    )
}

/// Why `command` refused the One ROM, with advice beneath where it helps.
///
/// A value read from OTP is printed with its control characters escaped. Where
/// an option of `command` overrides the refusal the text says which.
fn refusal(error: &onerom_app::CommissionError, command: Refusing) -> String {
    use onerom_app::CommissionError as E;
    use onerom_metadata::otp::BuildError;
    match error {
        // hardware commission refuses with Error::AlreadyCommissioned, which
        // holds the instance's values. Only --force re-commissions.
        E::AlreadyCommissioned { .. } => match command {
            Refusing::Commission | Refusing::SetSize => {
                format!("It is already commissioned.\n  {RE_COMMISSION}")
            }
            Refusing::RequestSignature => "It is already commissioned.".to_string(),
        },
        // Only hardware set-size refuses this.
        E::CommissionedAsAnotherBoard { board, requested } => {
            format!(
                "It is commissioned as {} not {}.",
                escape_controls(board),
                requested.name()
            )
        }
        // Only prepare() refuses this, and hardware commission's --force
        // overrides it. --size L is refused for a board that doesn't support
        // external flash. hardware request-signature doesn't have --force,
        // and guessing at --size L there confuses more than it helps.
        E::SecondChipConfigured(board) => {
            let supports_l = board.external_flash_cs_pin().is_some();
            let advice = match (command, supports_l) {
                (Refusing::Commission | Refusing::SetSize, true) => {
                    Some("Use --size L, or --force to commission it as M anyway.")
                }
                (Refusing::Commission | Refusing::SetSize, false) => {
                    Some("Use --force to commission it as M anyway.")
                }
                (Refusing::RequestSignature, _) => None,
            };
            match advice {
                Some(advice) => format!("{}\n  {advice}", sentence(error)),
                None => sentence(error),
            }
        }
        E::NewerData { .. } | E::UnknownKey { .. } => {
            format!("{}\n  {NEWER_DATA}", sentence(error))
        }
        // The manufacturer's name is the only value whose length the user
        // chooses.
        E::Build(BuildError::DoesNotFit) => {
            "The manufacturer's name is too long to fit in OTP.".to_string()
        }
        E::Otp { .. }
        | E::AreaFull
        | E::Build(_)
        | E::NotFire(_)
        | E::NoExternalFlash(_)
        | E::SizeAlreadySet { .. }
        | E::SlotSizeInvalid { .. }
        | E::RowWritten { .. }
        | E::PageLocked { .. }
        | E::ReadBack { .. } => sentence(error),
    }
}

/// The text of an [`Error::CommissionFailed`].
fn commission_failed_text(error: &onerom_app::CommissionError) -> String {
    failed_text(
        "Commissioning failed part way through due to an error:",
        error,
    )
}

/// The text of an [`Error::SetSizeFailed`].
fn set_size_failed_text(error: &onerom_app::CommissionError) -> String {
    failed_text(
        "Setting the board size failed part way through due to an error:",
        error,
    )
}

/// `heading`, then the reason a run that began writing failed with `error`.
/// Where running it again completes it the text says so.
fn failed_text(heading: &str, error: &onerom_app::CommissionError) -> String {
    let text = format!("{heading}\n  {}", sentence(error));
    if run_again_completes(error) {
        format!("{text}\n  Run the same command again to complete.")
    } else {
        text
    }
}

/// Whether running the same command again completes a run that failed with
/// `error`. Only a lost connection is completed this way. A second run skips
/// the rows already written. Any other failure stops it again.
fn run_again_completes(error: &onerom_app::CommissionError) -> bool {
    matches!(
        error,
        onerom_app::CommissionError::Otp {
            error: onerom_app::OtpError::Transport(_),
            ..
        }
    )
}

/// The text of an [`Error::ImageFile`].
fn image_file_error_text(error: &ImageFileError) -> String {
    format!(
        "{}\n  Download or build the image file again, or use --force to override.",
        image_file_text(error)
    )
}

/// The text for an image file whose slots don't match its length or the flash
/// chips. The advice isn't included.
pub fn image_file_text(error: &ImageFileError) -> String {
    let (fault, detail) = match *error {
        ImageFileError::TooShort { short_by } => {
            ("too short", format!("It ends {short_by} bytes short"))
        }
        ImageFileError::BadAddress { slot, addr } => (
            "damaged",
            format!("Slot {slot} is at {addr:#010x}, which isn't a valid address"),
        ),
        ImageFileError::TooLong { too_long_by } => {
            ("too long", format!("It is {too_long_by} bytes too long"))
        }
    };
    format!("Cannot use this image file because it is {fault}.\n  {detail}.")
}

/// The error for an image of `image_len` bytes that [`FlashPlan::new`] fails
/// to plan on a board whose size is `size`.
///
/// [`FlashPlan::new`]: onerom_app::FlashPlan::new
pub fn plan_error(
    error: FlashPlanError,
    image_len: usize,
    size: Option<MaybeKnown<OneromBoardSize>>,
) -> Error {
    match error {
        FlashPlanError::SecondChipRequired => {
            // A size that couldn't be read shows as not known.
            let size = size.unwrap_or(MaybeKnown::Known(OneromBoardSize::BoardSizeUnknown));
            Error::SecondChipRequired(board_size_text(size))
        }
        FlashPlanError::TooLarge => {
            let chips = flash_chips(size);
            Error::ImageTooLarge {
                image: image_len,
                flash: chips.first().len() + chips.second().map_or(0, |chip| chip.len()),
            }
        }
    }
}

/// The warning `--force` shows in place of `Error::ImageFile(error)`.
pub fn image_file_warning(error: &ImageFileError) -> String {
    let reason = match *error {
        ImageFileError::TooShort { short_by } => format!("ends {short_by} bytes short"),
        ImageFileError::BadAddress { .. } => "is damaged".to_string(),
        ImageFileError::TooLong { too_long_by } => format!("is {too_long_by} bytes too long"),
    };
    format!("Warning: This image file {reason} (continuing due to --force)")
}

/// `error`'s text as a sentence, with a capital letter and a full stop.
fn sentence(error: &impl std::fmt::Display) -> String {
    let text = error.to_string();
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => text,
    }
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
                newest_compatible,
            } => Error::PluginIncompatible {
                name,
                version,
                min_fw,
                fw,
                newest_compatible,
            },
            P::IncompatibleNewer {
                name,
                version,
                from,
                fw,
                newest_compatible,
            } => Error::PluginIncompatibleNewer {
                name,
                version,
                from,
                fw,
                newest_compatible,
            },
            P::BinaryTooSmall(src, actual, min) => Error::PluginBinaryTooSmall(src, actual, min),
            P::InvalidMagic(src, got, expected) => Error::PluginInvalidMagic(src, got, expected),
            P::TypeMismatch(src, expected, got) => {
                Error::PluginTypeMismatch(src, expected.to_string(), got.to_string())
            }
            P::WrongChipType {
                name,
                plugin_type,
                configured,
            } => Error::PluginWrongChipType {
                name,
                plugin_type,
                configured,
            },
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
            onerom_app::Error::Signer(e) => Error::Signer(e),
        }
    }
}

impl From<onerom_app::OtpError> for Error {
    fn from(e: onerom_app::OtpError) -> Self {
        Error::Otp(e)
    }
}

impl From<onerom_app::CommissionError> for Error {
    fn from(e: onerom_app::CommissionError) -> Self {
        Error::Commission(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An image file refusal and its warning contain the byte counts.
    #[test]
    fn an_image_file_refusal_contains_the_byte_counts() {
        for error in [
            ImageFileError::TooShort { short_by: 1111 },
            ImageFileError::TooLong { too_long_by: 1111 },
        ] {
            assert!(image_file_warning(&error).contains("1111"), "{error:?}");
            let text = Error::ImageFile(error.clone()).to_string();
            assert!(text.contains("1111"), "{error:?}");
        }
    }

    /// A refusal for a slot outside both flash chips contains the slot and its
    /// address.
    #[test]
    fn an_image_file_refusal_contains_the_bad_slot() {
        let error = ImageFileError::BadAddress {
            slot: 13,
            addr: 0x1030_0000,
        };
        let text = Error::ImageFile(error).to_string();
        assert!(text.contains("13"), "{text}");
        assert!(text.contains("0x10300000"), "{text}");
    }

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
            // it - not the positional form it once took. The spelling itself is
            // kept honest by `hint`'s own test, which parses every command line
            // the CLI hands out.
            assert!(
                msg.contains(&crate::hint::board_view(view)),
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

    /// `--size L` is refused for a board that doesn't support external flash
    /// so the refusal advises the option only for a board that does.
    #[test]
    fn a_second_chip_is_answered_with_size_l_only_where_it_works() {
        use onerom_app::CommissionError;
        use onerom_config::hw::Board;
        let text = |name| {
            let board = Board::try_from_str(name).unwrap();
            Error::Commission(CommissionError::SecondChipConfigured(board)).to_string()
        };
        assert!(text("fire-40-a").contains("--size"));
        assert!(!text("fire-24-f").contains("--size"));
    }

    /// `hardware request-signature`'s refusals don't have advice.
    /// `hardware commission`'s advise `--force`.
    #[test]
    fn a_signature_request_is_advised_only_its_own_options() {
        use onerom_app::CommissionError;
        use onerom_config::hw::Board;
        let second_chip = |name| {
            let board = Board::try_from_str(name).unwrap();
            let error = CommissionError::SecondChipConfigured(board);
            let request = Error::RequestSignature(error.clone()).to_string();
            (request, Error::Commission(error).to_string())
        };
        let (l, commission_l) = second_chip("fire-40-a");
        let (m, commission_m) = second_chip("fire-24-f");
        assert!(!l.contains("--"), "{l}");
        assert!(!m.contains("--"), "{m}");
        assert_eq!(l.lines().count(), 2, "{l}");
        assert!(commission_l.contains("--force"), "{commission_l}");
        assert!(commission_m.contains("--force"), "{commission_m}");

        let already = CommissionError::AlreadyCommissioned {
            row: 0x0c0,
            board: "fire-24-f".to_string(),
            manufacturer: "piers.rocks".to_string(),
            date: "20260101".to_string(),
            signer: 1,
            only_date_differs: false,
        };
        let request = Error::RequestSignature(already.clone()).to_string();
        assert!(!request.contains("--"), "{request}");
        assert!(Error::Commission(already).to_string().contains("--force"));
        // The same heading as hardware commission's.
        assert_eq!(request.lines().next(), m.lines().next());
        assert_eq!(request.lines().next(), commission_m.lines().next());
    }

    /// The board read from OTP is shown with its control characters escaped,
    /// beside the board the command was given. Nothing overrides the refusal
    /// so it doesn't have advice beneath it.
    #[test]
    fn another_board_is_shown_escaped_beside_the_board_given() {
        use onerom_app::CommissionError;
        use onerom_config::hw::Board;
        let requested = Board::try_from_str("fire-40-a").unwrap();
        let text = |board: &str| {
            Error::SetSize(CommissionError::CommissionedAsAnotherBoard {
                board: board.to_string(),
                requested,
            })
            .to_string()
        };
        let plain = text("fire-24-f");
        assert!(plain.contains("fire-24-f") && plain.contains("fire-40-a"));
        assert_eq!(plain.lines().count(), 2, "{plain}");
        let escaped = text("fire\u{1b}[2J");
        assert!(!escaped.contains('\u{1b}'), "{escaped}");
        assert!(escaped.contains("fire\\u{1b}[2J"), "{escaped}");
    }

    /// The refusal shows the size OTP configures and the size asked for.
    #[test]
    fn a_size_already_set_shows_both_sizes() {
        use onerom_app::{BoardSize, CommissionError};
        use onerom_metadata::OneromBoardSize;
        for (size, shown, requested) in [
            (OneromBoardSize::BoardSizeL, "L", BoardSize::M),
            (OneromBoardSize::BoardSizeOther, "other", BoardSize::M),
            (OneromBoardSize::BoardSizeOther, "other", BoardSize::L),
        ] {
            let text =
                Error::SetSize(CommissionError::SizeAlreadySet { size, requested }).to_string();
            let words: Vec<&str> = text.split(|c: char| !c.is_alphanumeric()).collect();
            let requested = requested.to_string();
            assert!(words.contains(&shown), "{text}");
            assert!(words.contains(&requested.as_str()), "{text}");
        }
    }

    #[test]
    fn only_a_lost_connection_is_completed_by_running_again() {
        use onerom_app::{CommissionError, OtpError, RowValue};
        let otp = |error| CommissionError::Otp { row: 0x0c0, error };
        let lost = otp(OtpError::Transport("timed out".to_string()));
        assert!(run_again_completes(&lost));
        for error in [
            otp(OtpError::NotPermitted),
            otp(OtpError::UnsupportedModification),
            CommissionError::ReadBack {
                row: 0x0c0,
                value: RowValue::Ecc(1),
                raw: 0,
            },
        ] {
            assert!(!run_again_completes(&error), "{error}");
        }
    }

    /// A running One ROM Lab is refused with its line and the command that
    /// stops it.
    #[test]
    fn a_running_lab_is_told_how_to_stop_it() {
        let device = "One ROM Lab Fire 24 E - State: Running".to_string();
        let msg = Error::OtpLabRunning(device.clone()).to_string();
        assert!(msg.contains(&device), "{msg}");
        assert!(msg.contains(hint::CONTROL_REBOOT_STOPPED), "{msg}");
    }

    #[test]
    fn a_reason_is_shown_as_a_sentence() {
        assert_eq!(sentence(&"row 0x0c5 is locked"), "Row 0x0c5 is locked.");
        assert_eq!(sentence(&"OTP row 0x0c5"), "OTP row 0x0c5.");
        assert_eq!(sentence(&""), "");
    }
}
