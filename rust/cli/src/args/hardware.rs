// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom hardware`.

use std::path::PathBuf;
use std::str::FromStr;

use clap::error::ErrorKind;
use clap::{ArgGroup, Args, Subcommand};
use enum_dispatch::enum_dispatch;
use onerom_app::BoardSize;
use onerom_config::hw::{Board, Model};
use onerom_gen::board_supports_size;
use onerom_metadata::otp::{BuildError, check_manufacturer};
use time::{Date, Month, OffsetDateTime};

use crate::args::{CommandTrait, arg_error};
use crate::utils::get_supported_boards;

#[derive(Debug, Args)]
pub struct HardwareArgs {
    #[command(subcommand)]
    pub command: HardwareCommands,
}

impl CommandTrait for HardwareArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }

    fn check_args(&self) -> Result<(), clap::Error> {
        self.command.check_args()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum HardwareCommands {
    /// Commission a One ROM
    ///
    /// Writes to the RP2350's OTP (One Time Programmable memory):
    /// - a signed and dated commissioning instance containing the board's type
    ///   and its manufacturer
    /// - the bootloader's USB info
    /// - the settings for a second flash chip if present
    ///
    /// This commissioning instance's pages are then locked. OTP cannot be
    /// erased so commission shows what it will write and asks for confirmation.
    /// A run that stopped part way can be completed later by re-running
    /// commission.
    ///
    /// The signature comes from one of:
    /// - a key on a One ROM signing server (--signer)
    /// - a key file (--key)
    /// - a signature made with hardware sign (--signature)
    ///
    /// A One ROM that is running or in limp mode is stopped first and rebooted
    /// into running mode once complete. A stopped One ROM is rebooted into
    /// stopped mode once OTP has been written.
    ///
    /// Examples:
    ///
    ///   onerom hardware commission --board fire-24-f --manufacturer piers.rocks --signer https://HOST --key-id 1
    ///
    ///   onerom hardware commission --board fire-40-a --size L --manufacturer piers.rocks --key key.pem
    ///
    ///   onerom hardware commission --board fire-24-f --size M --manufacturer onerom.org --date 20261001 --key-id 2 --signature SIGNATURE
    #[command(verbatim_doc_comment)]
    Commission(HardwareCommissionArgs),

    /// Request a signature to commission a One ROM
    ///
    /// Checks the connected One ROM can be commissioned and produces a link to
    /// a filled in signing request on GitHub. The request is answered with the
    /// hardware commission command to run.
    ///
    /// The request is for signing with the piers.rocks Community signing key.
    /// The manufacturer of a One ROM signed this way is always onerom.org and
    /// cannot be specified when signing with this mechanism.
    ///
    /// A One ROM that is running or in limp mode is stopped first and rebooted
    /// into running mode once complete.
    ///
    /// Examples:
    ///
    ///   onerom hardware request-signature --board fire-24-f
    ///
    ///   onerom hardware request-signature --board fire-40-a --size L
    #[command(verbatim_doc_comment)]
    RequestSignature(HardwareRequestSignatureArgs),

    /// Sign a One ROM's commissioning instance
    ///
    /// Generates a commissioning instance's signature for the One ROM with the
    /// given Chip ID which can be used with hardware commission's --signature
    /// option.
    ///
    /// This command does not operate on One ROM hardware directly.
    ///
    /// The signature comes from one of:
    /// - a key on a One ROM signing server (--signer)
    /// - a key file (--key)
    ///
    /// It asks for confirmation before signing. A signing server records the
    /// signature once confirmed.
    ///
    /// Examples:
    ///
    ///   onerom hardware sign --chip-id E126C9F97C10ADAC --board fire-24-f --manufacturer onerom.org --signer https://HOST --key-id 2
    ///
    ///   onerom hardware sign --chip-id E126C9F97C10ADAC --board fire-40-a --size L --manufacturer piers.rocks --key key.pem --json
    #[command(verbatim_doc_comment)]
    Sign(HardwareSignArgs),

    /// Set a One ROM's size
    ///
    /// Writes the settings for a second flash chip to the RP2350's OTP
    /// (One Time Programmable memory) without commissioning the One ROM.
    /// hardware commission writes the same settings, so this is only required
    /// when changing the One ROM's size after or without commissioning.
    ///
    /// OTP cannot be erased so set-size shows what it will write and asks for
    /// confirmation. Once a size other than M is set it cannot be changed.
    /// Size M writes nothing.
    ///
    /// A One ROM that is running or in limp mode is stopped first and rebooted
    /// into running mode once complete. A stopped One ROM is rebooted into
    /// stopped mode once OTP has been written.
    ///
    /// Examples:
    ///
    ///   onerom hardware set-size --board fire-40-a --size L
    #[command(verbatim_doc_comment)]
    SetSize(HardwareSetSizeArgs),

    /// Check a One ROM's commissioning information
    ///
    /// Checks the signature of each commissioning instance present on the One
    /// ROM against valid signing keys.
    ///
    /// It fails unless the board's latest commissioning instance is valid.
    ///
    /// A One ROM that is running or in limp mode is stopped first and rebooted
    /// into running mode once complete.
    ///
    /// Examples:
    ///
    ///   onerom hardware validate
    ///
    ///   onerom hardware validate --json
    #[command(verbatim_doc_comment)]
    Validate(HardwareValidateArgs),
}

/// `--size`'s help for commission, request-signature and sign. It leaves out
/// the final full stop, which clap drops from help it reads from a doc
/// comment, so it prints as the options beside it do.
const HELP_SIZE: &str = "The board's size. M has 2MB of flash. L has an additional 2MB \
     flash chip on chip select 1. Must be supplied for a board that supports external flash \
     being populated. Boards that don't support external flash are always M";

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("signing").required(true).args(["signer", "key", "signature"])))]
pub struct HardwareCommissionArgs {
    /// The board's type (e.g. fire-24-f). Written to OTP.
    #[arg(long, short, value_name = "BOARD", value_parser = parse_fire_board)]
    pub board: Board,

    #[arg(long, value_name = "SIZE", value_parser = BoardSize::from_str, help = HELP_SIZE)]
    pub size: Option<BoardSize>,

    /// The manufacturer's name. Written to OTP.
    #[arg(long, value_name = "NAME", value_parser = parse_manufacturer)]
    pub manufacturer: String,

    /// The address of a One ROM signing server (e.g. https://HOST).
    #[arg(long, value_name = "URL", value_parser = parse_signer, requires = "key_id")]
    pub signer: Option<String>,

    /// The signing key's ID in the signing key table, from 1 to 65535. Must be
    /// supplied with --signer or --signature.
    #[arg(long, value_name = "ID", value_parser = clap::value_parser!(u16).range(1..))]
    pub key_id: Option<u16>,

    /// The PIN of the signing server's key or of an encrypted key file. It is
    /// asked for interactively if omitted.
    #[arg(long, value_name = "PIN", value_parser = parse_pin)]
    pub pin: Option<String>,

    /// A file containing an Ed25519 private key in PKCS#8 PEM form, to be used
    /// to sign a commissioning instance. If the key is encrypted, --pin
    /// decrypts it.
    #[arg(long, value_name = "FILE", conflicts_with = "key_id")]
    pub key: Option<PathBuf>,

    /// The commissioning instance's signature as 128 hex digits, generated by
    /// hardware sign.
    #[arg(
        long,
        value_name = "SIGNATURE",
        value_parser = parse_signature,
        requires = "key_id",
        requires = "date",
        conflicts_with = "pin"
    )]
    pub signature: Option<[u8; 64]>,

    /// The commissioning date as YYYYMMDD. Defaults to today's UTC date.
    #[arg(long, value_name = "DATE", value_parser = parse_date)]
    pub date: Option<String>,

    /// Continue despite non-fatal problems.
    #[arg(long, short)]
    pub force: bool,

    /// Show what commissioning data would be written without writing
    /// it.
    #[arg(long, visible_alias = "dryrun")]
    pub dry_run: bool,
}

impl HardwareCommissionArgs {
    /// The board's size. It's `--size` where given and M otherwise.
    ///
    /// A board that supports external flash being populated requires `--size`
    /// so a board with external flash fitted isn't commissioned as M by
    /// leaving it out. Any other board is M.
    pub fn board_size(&self) -> Result<BoardSize, SizeError> {
        board_size(self.board, self.size)
    }
}

impl CommandTrait for HardwareCommissionArgs {
    fn requires_device(&self) -> bool {
        true
    }

    fn check_args(&self) -> Result<(), clap::Error> {
        let path = ["hardware", "commission"];
        check_board_size(&path, self.board, self.size)?;
        // --force allows a date later than today.
        if !self.force {
            check_date(&path, self.date.as_deref())?;
        }
        Ok(())
    }
}

/// `size` for `board`, or M where `size` is `None` and `board` doesn't
/// support external flash being populated.
///
/// A board that supports external flash being populated requires `--size`
/// so a board with external flash fitted isn't commissioned as M by leaving
/// it out.
fn board_size(board: Board, size: Option<BoardSize>) -> Result<BoardSize, SizeError> {
    match size {
        Some(size) => supported_size(board, size),
        None if board.external_flash_cs_pin().is_some() => Err(SizeError::Missing(board)),
        None => Ok(BoardSize::M),
    }
}

/// Refuses `--size` as `size` where [`board_size`] refuses it for `board`.
/// `path` is the command's path below `onerom`.
fn check_board_size(
    path: &[&str],
    board: Board,
    size: Option<BoardSize>,
) -> Result<(), clap::Error> {
    match size {
        Some(size) => check_size(path, board, size),
        None => match board_size(board, size) {
            Ok(_) => Ok(()),
            Err(e) => Err(arg_error(path, ErrorKind::MissingRequiredArgument, e)),
        },
    }
}

/// Refuses `--date` as `date` where it's later than today in UTC. `path` is
/// the command's path below `onerom`.
fn check_date(path: &[&str], date: Option<&str>) -> Result<(), clap::Error> {
    let today = OffsetDateTime::now_utc().date();
    match date.and_then(|text| Some((text, future_date(text, today)?))) {
        Some((text, date)) => Err(arg_error(
            path,
            ErrorKind::ValueValidation,
            format!("invalid value '{text}' for '--date <DATE>': {date} is in the future"),
        )),
        None => Ok(()),
    }
}

/// A `--size` that doesn't suit the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SizeError {
    /// The board supports external flash being populated and `--size` isn't
    /// given.
    #[error(
        "--size must be supplied for {} because it supports external flash being populated. Use --size M if external flash is unpopulated",
        .0.name()
    )]
    Missing(Board),

    /// The board doesn't support external flash and `--size` has a second
    /// flash chip.
    #[error("{} doesn't support --size other than M", .0.name())]
    ExternalFlashUnsupported(Board),
}

/// `size` where `board` supports it. A size with a second flash chip needs a
/// board that supports external flash.
pub(crate) fn supported_size(board: Board, size: BoardSize) -> Result<BoardSize, SizeError> {
    if board_supports_size(board, size) {
        Ok(size)
    } else {
        Err(SizeError::ExternalFlashUnsupported(board))
    }
}

/// Refuses `--size` as `size` for a board that doesn't support external flash.
/// `path` is the command's path below `onerom`, such as `["hardware",
/// "set-size"]`.
fn check_size(path: &[&str], board: Board, size: BoardSize) -> Result<(), clap::Error> {
    match supported_size(board, size) {
        Ok(_) => Ok(()),
        Err(e) => Err(arg_error(
            path,
            ErrorKind::ValueValidation,
            format!("invalid value '{size}' for '--size <SIZE>': {e}"),
        )),
    }
}

#[derive(Debug, Args)]
pub struct HardwareSetSizeArgs {
    /// The board's type (e.g. fire-40-a).
    #[arg(long, short, value_name = "BOARD", value_parser = parse_board_to_size)]
    pub board: Board,

    /// The board's size. M has 2MB of flash. L has an additional 2MB flash
    /// chip on chip select 1. Boards that don't support external flash are
    /// always M.
    #[arg(long, value_name = "SIZE", value_parser = BoardSize::from_str)]
    pub size: BoardSize,

    /// Continue despite non-fatal problems.
    #[arg(long, short)]
    pub force: bool,

    /// Show what would be written without writing it.
    #[arg(long, visible_alias = "dryrun")]
    pub dry_run: bool,
}

impl CommandTrait for HardwareSetSizeArgs {
    fn requires_device(&self) -> bool {
        true
    }

    fn check_args(&self) -> Result<(), clap::Error> {
        check_size(&["hardware", "set-size"], self.board, self.size)
    }
}

#[derive(Debug, Args)]
pub struct HardwareRequestSignatureArgs {
    /// The board's type (e.g. fire-24-f).
    #[arg(long, short, value_name = "BOARD", value_parser = parse_fire_board)]
    pub board: Board,

    #[arg(long, value_name = "SIZE", value_parser = BoardSize::from_str, help = HELP_SIZE)]
    pub size: Option<BoardSize>,
}

impl HardwareRequestSignatureArgs {
    /// The board's size, as [`HardwareCommissionArgs::board_size`] finds it.
    pub fn board_size(&self) -> Result<BoardSize, SizeError> {
        board_size(self.board, self.size)
    }
}

impl CommandTrait for HardwareRequestSignatureArgs {
    fn requires_device(&self) -> bool {
        true
    }

    fn check_args(&self) -> Result<(), clap::Error> {
        check_board_size(&["hardware", "request-signature"], self.board, self.size)
    }
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("signing").required(true).args(["signer", "key"])))]
pub struct HardwareSignArgs {
    /// The One ROM's Chip ID as 16 hex digits (e.g. E126C9F97C10ADAC).
    #[arg(long, value_name = "CHIPID", value_parser = parse_chip_id)]
    pub chip_id: [u16; 4],

    /// The board's type (e.g. fire-24-f).
    #[arg(long, short, value_name = "BOARD", value_parser = parse_fire_board)]
    pub board: Board,

    #[arg(long, value_name = "SIZE", value_parser = BoardSize::from_str, help = HELP_SIZE)]
    pub size: Option<BoardSize>,

    /// The manufacturer's name.
    #[arg(long, value_name = "NAME", value_parser = parse_manufacturer)]
    pub manufacturer: String,

    /// The commissioning date as YYYYMMDD. Defaults to today's UTC date.
    #[arg(long, value_name = "DATE", value_parser = parse_date)]
    pub date: Option<String>,

    /// The address of a One ROM signing server (e.g. https://HOST).
    #[arg(long, value_name = "URL", value_parser = parse_signer, requires = "key_id")]
    pub signer: Option<String>,

    /// The signing key's ID in the signing key table, from 1 to 65535. Must be
    /// supplied with --signer.
    #[arg(long, value_name = "ID", value_parser = clap::value_parser!(u16).range(1..))]
    pub key_id: Option<u16>,

    /// The PIN of the signing server's key or of an encrypted key file. It is
    /// asked for interactively if omitted.
    #[arg(long, value_name = "PIN", value_parser = parse_pin)]
    pub pin: Option<String>,

    /// A file containing an Ed25519 private key in PKCS#8 PEM form, to be used
    /// to sign the commissioning instance. If the key is encrypted, --pin
    /// decrypts it.
    #[arg(long, value_name = "FILE", conflicts_with = "key_id")]
    pub key: Option<PathBuf>,

    /// Sign without asking for confirmation and without the signing server
    /// publicly recording the signature.
    #[arg(long, visible_alias = "dryrun", conflicts_with = "key")]
    pub dry_run: bool,

    /// Show the result as JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

impl HardwareSignArgs {
    /// The board's size, as [`HardwareCommissionArgs::board_size`] finds it.
    pub fn board_size(&self) -> Result<BoardSize, SizeError> {
        board_size(self.board, self.size)
    }
}

impl CommandTrait for HardwareSignArgs {
    fn requires_device(&self) -> bool {
        false
    }

    fn check_args(&self) -> Result<(), clap::Error> {
        let path = ["hardware", "sign"];
        check_board_size(&path, self.board, self.size)?;
        check_date(&path, self.date.as_deref())
    }
}

#[derive(Debug, Args)]
pub struct HardwareValidateArgs {
    /// Show the result as JSON instead of text.
    #[arg(long)]
    pub json: bool,
}

impl CommandTrait for HardwareValidateArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

/// Parse a Fire board's name for `hardware commission`. An Ice board is
/// refused because commissioning writes an RP2350's OTP.
fn parse_fire_board(text: &str) -> Result<Board, String> {
    fire_board(text, "Ice boards do not support commissioning")
}

/// Parse a Fire board's name for `hardware set-size`. An Ice board is refused
/// because a board's size is set in an RP2350's OTP.
fn parse_board_to_size(text: &str) -> Result<Board, String> {
    fire_board(text, "Ice boards do not support setting a size")
}

/// Parse a Fire board's name. `ice` is the refusal for an Ice board.
fn fire_board(text: &str, ice: &str) -> Result<Board, String> {
    let board = Board::try_from_str(text)
        .ok_or_else(|| format!("unknown board\n  Fire boards: {}", get_supported_boards()))?;
    match board.model() {
        Model::Fire => Ok(board),
        Model::Ice => Err(ice.to_string()),
    }
}

/// Parse a manufacturer's name. onerom-metadata's `check_manufacturer` says
/// which names are valid.
fn parse_manufacturer(text: &str) -> Result<String, String> {
    match check_manufacturer(text) {
        Ok(()) => Ok(text.to_string()),
        Err(BuildError::EmptyManufacturer) => Err("manufacturer is empty".to_string()),
        Err(_) => Err(
            "the manufacturer must be printable ASCII without '*' or a leading or trailing space"
                .to_string(),
        ),
    }
}

/// Parse a signing server's address. It must be https.
fn parse_signer(text: &str) -> Result<String, String> {
    if onerom_cli::signing::is_https(text) {
        Ok(text.to_string())
    } else {
        Err("signing server must use https".to_string())
    }
}

/// Parse a signature as 128 hex digits in either case.
fn parse_signature(text: &str) -> Result<[u8; 64], String> {
    let refused = || "the signature must be 128 hex digits".to_string();
    hex::decode(text)
        .map_err(|_| refused())?
        .try_into()
        .map_err(|_| refused())
}

/// Parse a Chip ID as 16 hex digits in either case, in the order the
/// bootloader's USB serial number shows CHIPID.
fn parse_chip_id(text: &str) -> Result<[u16; 4], String> {
    onerom_metadata::otp::parse_chip_id(&text.to_ascii_uppercase())
        .ok_or_else(|| "the Chip ID must be 16 hex digits".to_string())
}

/// Parse a PIN. It can't be empty.
fn parse_pin(text: &str) -> Result<String, String> {
    if text.is_empty() {
        Err("the PIN is empty".to_string())
    } else {
        Ok(text.to_string())
    }
}

/// Parse a real `YYYYMMDD` date. `check_args()` refuses one later than today
/// in UTC unless `--force` is given.
fn parse_date(text: &str) -> Result<String, String> {
    if !is_yyyymmdd(text) {
        return Err("use date format YYYYMMDD".to_string());
    }
    calendar_date(text).ok_or("not a valid date")?;
    Ok(text.to_string())
}

/// The date `text` holds where it's later than `today`.
fn future_date(text: &str, today: Date) -> Option<Date> {
    calendar_date(text).filter(|&date| date > today)
}

/// Whether `text` is 8 ASCII digits.
fn is_yyyymmdd(text: &str) -> bool {
    text.len() == 8 && text.bytes().all(|b| b.is_ascii_digit())
}

/// The date `text` holds as `YYYYMMDD`.
fn calendar_date(text: &str) -> Option<Date> {
    if !is_yyyymmdd(text) {
        return None;
    }
    let year = text[..4].parse().ok()?;
    let month = Month::try_from(text[4..6].parse::<u8>().ok()?).ok()?;
    let day = text[6..].parse().ok()?;
    Date::from_calendar_date(year, month, day).ok()
}

/// Today's UTC date as `YYYYMMDD`.
pub fn today() -> String {
    let today = OffsetDateTime::now_utc().date();
    format!(
        "{:04}{:02}{:02}",
        today.year(),
        u8::from(today.month()),
        today.day()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        calendar_date(text).unwrap()
    }

    #[test]
    fn a_date_is_eight_digits() {
        assert_eq!(parse_date("20260926").unwrap(), "20260926");
        for text in [
            "2026092",
            "202609260",
            "2026O926",
            "2026-9-26",
            " 20260926",
            "２０２６０９２６",
            "",
        ] {
            assert!(parse_date(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_date_is_a_real_calendar_date() {
        for text in ["20261301", "20260001", "20260230", "20250229", "20260900"] {
            assert!(parse_date(text).is_err(), "{text}");
        }
        assert!(parse_date("20240229").is_ok());
        // A date that doesn't exist has its own refusal.
        assert_ne!(parse_date("20260230"), parse_date("2026-2-30"));
    }

    #[test]
    fn a_date_later_than_today_is_in_the_future() {
        let today = date("20260926");
        assert_eq!(future_date("20260927", today), Some(date("20260927")));
        assert_eq!(future_date("20270101", today), Some(date("20270101")));
        assert_eq!(future_date("20260926", today), None);
        assert_eq!(future_date("20260925", today), None);
    }

    /// A manufacturer's name is printable ASCII without `*` or a leading or
    /// trailing space. An empty name has its own refusal.
    #[test]
    fn a_manufacturer_is_named_in_printable_ascii() {
        for text in ["piers.rocks", "Piers's Boards & Co", "a b"] {
            assert_eq!(parse_manufacturer(text).unwrap(), text);
        }
        let empty = parse_manufacturer("").unwrap_err();
        for text in [
            " piers.rocks",
            "piers.rocks ",
            "a*b",
            "*",
            "Café Ltd",
            "piers\nrocks",
            "\u{1b}[2J",
            "tab\there",
        ] {
            let refused = parse_manufacturer(text).unwrap_err();
            assert_ne!(refused, empty, "{text:?}");
        }
    }

    /// Each Fire board needs `--size` exactly when its config sets an external
    /// flash chip select pin.
    #[test]
    fn every_boards_size_follows_its_external_flash_support() {
        for board in onerom_config::hw::BOARDS {
            if board.model() != Model::Fire {
                continue;
            }
            let args = |size| HardwareCommissionArgs {
                board,
                size,
                manufacturer: "piers.rocks".to_string(),
                signer: None,
                key_id: None,
                pin: None,
                key: Some(PathBuf::from("key.pem")),
                signature: None,
                date: None,
                force: false,
                dry_run: false,
            };
            let name = board.name();
            let (left_out, l) = if board.external_flash_cs_pin().is_some() {
                (Err(SizeError::Missing(board)), Ok(BoardSize::L))
            } else {
                (
                    Ok(BoardSize::M),
                    Err(SizeError::ExternalFlashUnsupported(board)),
                )
            };
            assert_eq!(args(None).board_size(), left_out, "{name}");
            assert_eq!(args(Some(BoardSize::L)).board_size(), l, "{name}");
            assert_eq!(
                args(Some(BoardSize::M)).board_size(),
                Ok(BoardSize::M),
                "{name}"
            );
        }
    }

    #[test]
    fn only_a_fire_board_is_accepted() {
        assert_eq!(
            parse_fire_board("fire-24-f").unwrap(),
            Board::try_from_str("fire-24-f").unwrap()
        );
        let ice = parse_fire_board("ice-24-d").unwrap_err();
        let unknown = parse_fire_board("not-a-board").unwrap_err();
        // An Ice board isn't refused as an unknown board.
        assert_ne!(ice, unknown);
        // An unknown board's refusal lists the Fire boards.
        assert!(unknown.contains(&get_supported_boards()), "{unknown}");
    }
}

/// The command lines `hardware` and `inspect otp` accept and refuse. The tests
/// parse them as the CLI does. Parsing doesn't look for a device.
#[cfg(test)]
mod command_lines {
    use super::*;

    use std::error::Error as _;

    use clap::error::ErrorKind;
    use clap::{CommandFactory, Parser};
    use onerom_app::BoardSizeError;

    use crate::args::{Cli, Commands};

    /// The options every commission of a board that doesn't support external
    /// flash needs apart from a signature's source.
    const REQUIRED: &str = "--board fire-24-f --manufacturer piers.rocks";

    const SIGNER: &str = "--signer https://example.invalid --key-id 1";

    /// A signature as `--signature` takes it.
    const SIGNATURE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";

    fn cli(line: &str) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(["onerom"].into_iter().chain(line.split_whitespace()))
    }

    fn commission(options: &str) -> Result<HardwareCommissionArgs, clap::Error> {
        Ok(commission_args(cli(&format!(
            "hardware commission {options}"
        ))?))
    }

    /// `hardware commission OPTIONS` parsed and checked, as the CLI does before
    /// it looks for a device.
    fn checked(options: &str) -> Result<HardwareCommissionArgs, clap::Error> {
        let cli = cli(&format!("hardware commission {options}"))?;
        cli.command.check_args()?;
        Ok(commission_args(cli))
    }

    fn commission_args(cli: Cli) -> HardwareCommissionArgs {
        let Commands::Hardware(hardware) = cli.command else {
            panic!("not a hardware command");
        };
        let HardwareCommands::Commission(args) = hardware.command else {
            panic!("not hardware commission");
        };
        args
    }

    fn refused(options: &str) -> ErrorKind {
        match commission(options) {
            Ok(args) => panic!("{options} parsed as {args:?}"),
            Err(e) => e.kind(),
        }
    }

    /// `hardware commission`'s usage as clap shows it.
    fn usage() -> String {
        usage_of("commission")
    }

    /// The usage of `hardware NAME` as clap shows it.
    fn usage_of(name: &str) -> String {
        let mut onerom = Cli::command().bin_name("onerom");
        onerom.build();
        let hardware = onerom.find_subcommand_mut("hardware").unwrap();
        let command = hardware.find_subcommand_mut(name).unwrap();
        command.render_usage().to_string()
    }

    #[test]
    fn a_commission_with_a_signing_server_parses() {
        let args = commission(&format!("{REQUIRED} {SIGNER} --pin secret")).unwrap();
        assert_eq!(args.board, Board::try_from_str("fire-24-f").unwrap());
        assert_eq!(args.size, None);
        assert_eq!(args.manufacturer, "piers.rocks");
        assert_eq!(args.signer.as_deref(), Some("https://example.invalid"));
        assert_eq!(args.key_id, Some(1));
        assert_eq!(args.pin.as_deref(), Some("secret"));
        assert_eq!(args.key, None);
        assert_eq!(args.signature, None);
        assert_eq!(args.date, None);
        assert!(!args.force);
    }

    #[test]
    fn a_commission_with_a_key_file_parses() {
        let args = commission(&format!("{REQUIRED} --key key.pem --date 20260101 -f")).unwrap();
        assert_eq!(args.key, Some(PathBuf::from("key.pem")));
        assert_eq!(args.signer, None);
        assert_eq!(args.key_id, None);
        assert_eq!(args.date.as_deref(), Some("20260101"));
        assert!(args.force);
    }

    #[test]
    fn a_dry_run_parses() {
        assert!(!commission(&format!("{REQUIRED} {SIGNER}")).unwrap().dry_run);
        for flag in ["--dry-run", "--dryrun"] {
            let args = commission(&format!("{REQUIRED} {SIGNER} {flag}")).unwrap();
            assert!(args.dry_run, "{flag}");
        }
    }

    #[test]
    fn size_is_m_or_l_in_either_case() {
        for (text, size) in [
            ("m", BoardSize::M),
            ("M", BoardSize::M),
            ("l", BoardSize::L),
            ("L", BoardSize::L),
        ] {
            let args = commission(&format!(
                "--board fire-40-a --size {text} --manufacturer piers.rocks --key key.pem"
            ))
            .unwrap();
            assert_eq!(args.size, Some(size), "{text}");
        }
    }

    #[test]
    fn size_can_be_left_out_where_external_flash_isnt_supported() {
        for options in [
            REQUIRED,
            "--board fire-24-f --size M --manufacturer piers.rocks",
        ] {
            let args = checked(&format!("{options} --key key.pem")).unwrap();
            assert_eq!(args.board_size(), Ok(BoardSize::M), "{options}");
        }
    }

    #[test]
    fn l_is_refused_where_external_flash_isnt_supported() {
        let error = checked(&format!("{REQUIRED} --size L --key key.pem")).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
        // The refusal carries its reason and `hardware commission`'s usage.
        let board = Board::try_from_str("fire-24-f").unwrap();
        let reason = SizeError::ExternalFlashUnsupported(board).to_string();
        let text = error.to_string();
        assert!(text.contains(&reason), "{text}");
        assert!(text.contains(&usage()), "{text}");
    }

    #[test]
    fn size_is_required_where_external_flash_is_supported() {
        let options = "--board fire-40-a --manufacturer piers.rocks --key key.pem";
        let error = checked(options).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        // The refusal carries its reason and `hardware commission`'s usage.
        let board = Board::try_from_str("fire-40-a").unwrap();
        let reason = SizeError::Missing(board).to_string();
        let text = error.to_string();
        assert!(text.contains(&reason), "{text}");
        assert!(text.contains(&usage()), "{text}");

        for (text, size) in [("M", BoardSize::M), ("L", BoardSize::L)] {
            let args = checked(&format!("{options} --size {text}")).unwrap();
            assert_eq!(args.board_size(), Ok(size), "{text}");
        }
    }

    /// XL is refused like any other size that isn't M or L.
    #[test]
    fn a_size_other_than_m_or_l_is_refused() {
        for text in ["XL", "xl", "Q", "S"] {
            let error = commission(&format!(
                "--board fire-40-a --size {text} --manufacturer piers.rocks --key key.pem"
            ))
            .unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{text}");
            let reason = error
                .source()
                .and_then(|e| e.downcast_ref::<BoardSizeError>());
            assert_eq!(reason, Some(&BoardSizeError::Unknown), "{text}");
        }
    }

    #[test]
    fn a_signature_needs_a_signing_server_a_key_file_or_a_signature() {
        assert_eq!(refused(REQUIRED), ErrorKind::MissingRequiredArgument);
        assert_eq!(
            refused(&format!("{REQUIRED} --key-id 1")),
            ErrorKind::MissingRequiredArgument
        );
    }

    /// A signing server is reached over https. An address that isn't is
    /// refused before the CLI looks for a device.
    #[test]
    fn a_signing_server_needs_an_https_url() {
        for url in ["http://example.invalid", "example.invalid"] {
            assert_eq!(
                refused(&format!("{REQUIRED} --signer {url} --key-id 1")),
                ErrorKind::ValueValidation,
                "{url}"
            );
        }
        let args = commission(&format!(
            "{REQUIRED} --signer HTTPS://example.invalid/sign/ --key-id 1"
        ));
        assert_eq!(
            args.unwrap().signer.as_deref(),
            Some("HTTPS://example.invalid/sign/")
        );
    }

    /// A signing server's key is identified by its ID. A key file's public
    /// key identifies its key so it refuses one.
    #[test]
    fn a_key_id_goes_with_a_signing_server_or_a_signature() {
        assert_eq!(
            refused(&format!("{REQUIRED} --signer https://example.invalid")),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            refused(&format!(
                "{REQUIRED} --signature {SIGNATURE} --date 20260101"
            )),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            refused(&format!("{REQUIRED} --key key.pem --key-id 1")),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn a_key_id_is_1_to_65535() {
        for (text, id) in [("1", 1), ("2", 2), ("65535", 65535)] {
            let args = commission(&format!(
                "{REQUIRED} --signer https://example.invalid --key-id {text}"
            ))
            .unwrap();
            assert_eq!(args.key_id, Some(id), "{text}");
        }
        for text in ["0", "65536", "-1", "x", "0x2"] {
            let line = format!("{REQUIRED} --signer https://example.invalid --key-id={text}");
            assert_eq!(refused(&line), ErrorKind::ValueValidation, "{text}");
        }
    }

    #[test]
    fn a_signature_is_128_hex_digits_in_either_case() {
        let args = commission(&format!(
            "{REQUIRED} --key-id 2 --signature {SIGNATURE} --date 20260101"
        ))
        .unwrap();
        let expected: Vec<u8> = hex::decode(SIGNATURE).unwrap();
        assert_eq!(args.signature.map(|s| s.to_vec()), Some(expected));
        assert_eq!(args.key_id, Some(2));
        assert_eq!(args.date.as_deref(), Some("20260101"));
        for text in [
            &SIGNATURE[..126],
            &format!("{SIGNATURE}00"),
            &format!("{}g", &SIGNATURE[..127]),
            &format!("0x{}", &SIGNATURE[2..]),
            &format!("{} ", &SIGNATURE[..127]),
        ] {
            let line = format!("{REQUIRED} --key-id 2 --signature {text} --date 20260101");
            let words = ["onerom", "hardware", "commission"]
                .into_iter()
                .chain(REQUIRED.split_whitespace())
                .chain(["--key-id", "2", "--date", "20260101", "--signature", text]);
            let error = Cli::try_parse_from(words).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{line}");
        }
    }

    /// A signature covers its date, so a run can't take today's.
    #[test]
    fn a_signature_requires_a_date() {
        assert_eq!(
            refused(&format!("{REQUIRED} --key-id 2 --signature {SIGNATURE}")),
            ErrorKind::MissingRequiredArgument
        );
    }

    /// A signature isn't made during the run so there isn't a PIN to give.
    #[test]
    fn a_signature_refuses_a_pin() {
        assert_eq!(
            refused(&format!(
                "{REQUIRED} --key-id 2 --signature {SIGNATURE} --date 20260101 --pin 1234"
            )),
            ErrorKind::ArgumentConflict
        );
    }

    /// A run takes exactly one of a signing server, a key file and a
    /// signature.
    #[test]
    fn a_run_takes_one_signature_source() {
        let signature = format!("--signature {SIGNATURE} --date 20260101");
        for sources in [
            format!("{SIGNER} --key key.pem"),
            format!("{SIGNER} {signature}"),
            format!("--key key.pem --key-id 1 {signature}"),
            format!("--key key.pem {signature}"),
        ] {
            assert_eq!(
                refused(&format!("{REQUIRED} {sources}")),
                ErrorKind::ArgumentConflict,
                "{sources}"
            );
        }
    }

    /// An empty PIN is refused before the CLI looks for a device.
    #[test]
    fn an_empty_pin_is_refused() {
        for source in [
            ["--signer", "https://example.invalid", "--key-id", "1"].as_slice(),
            &["--key", "key.pem"],
        ] {
            let line = ["onerom", "hardware", "commission"]
                .into_iter()
                .chain(REQUIRED.split_whitespace())
                .chain(source.iter().copied())
                .chain(["--pin", ""]);
            let error = Cli::try_parse_from(line).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{source:?}");
        }
    }

    /// A PIN is for a signing server's key or an encrypted key file. Whether
    /// a key file is encrypted is found once it's read.
    #[test]
    fn a_pin_needs_a_signing_server_or_a_key_file() {
        for source in [SIGNER, "--key key.pem"] {
            let args = commission(&format!("{REQUIRED} {source} --pin secret")).unwrap();
            assert_eq!(args.pin.as_deref(), Some("secret"), "{source}");
        }
        assert_eq!(
            refused(&format!("{REQUIRED} --pin secret")),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn an_ice_board_is_refused() {
        let error = commission(&format!(
            "--board ice-24-d --size M --manufacturer piers.rocks {SIGNER}"
        ))
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
        // The refusal is the one for an Ice board.
        let reason = error.source().map(|e| e.to_string());
        assert_eq!(reason, parse_fire_board("ice-24-d").err(), "{error}");
    }

    #[test]
    fn a_bad_date_is_refused() {
        for date in ["20261301", "2026092", "2026O926"] {
            assert_eq!(
                refused(&format!("{REQUIRED} {SIGNER} --date {date}")),
                ErrorKind::ValueValidation,
                "{date}"
            );
        }
    }

    /// A date later than today is refused after parsing unless `--force` is
    /// given.
    #[test]
    fn a_future_date_needs_force() {
        let line = format!("{REQUIRED} {SIGNER} --date 99991231");
        let error = checked(&line).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
        let args = checked(&format!("{line} --force")).unwrap();
        assert_eq!(args.date.as_deref(), Some("99991231"));
        assert!(checked(&format!("{REQUIRED} {SIGNER} --date {}", today())).is_ok());
    }

    #[test]
    fn a_bad_manufacturer_is_refused() {
        let args = [
            "hardware",
            "commission",
            "--board",
            "fire-24-f",
            "--size",
            "M",
        ];
        for name in ["", "piers\u{1b}rocks", " piers.rocks", "a*b"] {
            let line = ["onerom"].into_iter().chain(args).chain([
                "--manufacturer",
                name,
                "--key",
                "key.pem",
            ]);
            let error = Cli::try_parse_from(line).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{name:?}");
        }
    }

    #[test]
    fn validate_and_inspect_otp_parse() {
        for line in [
            "hardware validate",
            "hardware validate --json",
            "inspect otp",
            "inspect otp --json",
        ] {
            assert!(cli(line).is_ok(), "{line}");
        }
    }

    /// `update otp` was hidden and never implemented. `hardware` replaced it.
    #[test]
    fn update_otp_no_longer_parses() {
        for line in ["update otp", "update otp --read"] {
            let error = cli(line).err().unwrap_or_else(|| panic!("{line} parsed"));
            assert_eq!(error.kind(), ErrorKind::InvalidSubcommand, "{line}");
        }
    }

    /// `hardware set-size OPTIONS` parsed and checked, as the CLI does before
    /// it looks for a device.
    fn set_size(options: &str) -> Result<HardwareSetSizeArgs, clap::Error> {
        let cli = cli(&format!("hardware set-size {options}"))?;
        cli.command.check_args()?;
        let Commands::Hardware(hardware) = cli.command else {
            panic!("not a hardware command");
        };
        let HardwareCommands::SetSize(args) = hardware.command else {
            panic!("not hardware set-size");
        };
        Ok(args)
    }

    fn set_size_refused(options: &str) -> ErrorKind {
        match set_size(options) {
            Ok(args) => panic!("{options} parsed as {args:?}"),
            Err(e) => e.kind(),
        }
    }

    #[test]
    fn a_set_size_parses() {
        let args = set_size("--board fire-40-a --size L").unwrap();
        assert_eq!(args.board, Board::try_from_str("fire-40-a").unwrap());
        assert_eq!(args.size, BoardSize::L);
        assert!(!args.force);
        assert!(!args.dry_run);

        let args = set_size("-b fire-24-f --size m -f").unwrap();
        assert_eq!(args.board, Board::try_from_str("fire-24-f").unwrap());
        assert_eq!(args.size, BoardSize::M);
        assert!(args.force);

        for flag in ["--dry-run", "--dryrun"] {
            let args = set_size(&format!("--board fire-40-a --size L {flag}")).unwrap();
            assert!(args.dry_run, "{flag}");
        }
    }

    /// Unlike `hardware commission`, `set-size` requires `--size` on every
    /// board.
    #[test]
    fn set_size_requires_a_board_and_a_size() {
        for options in ["--board fire-24-f", "--board fire-40-a", "--size M", ""] {
            assert_eq!(
                set_size_refused(options),
                ErrorKind::MissingRequiredArgument,
                "{options}"
            );
        }
    }

    /// XL is refused like any other size that isn't M or L.
    #[test]
    fn set_size_refuses_a_size_it_doesnt_know() {
        for text in ["XL", "xl", "Q", ""] {
            let words = ["onerom", "hardware", "set-size", "--board", "fire-40-a"];
            let line = words.into_iter().chain(["--size", text]);
            let error = Cli::try_parse_from(line).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{text:?}");
            let reason = error
                .source()
                .and_then(|e| e.downcast_ref::<BoardSizeError>());
            assert_eq!(reason, Some(&BoardSizeError::Unknown), "{text:?}");
        }
    }

    /// A board that doesn't support external flash is always M. The refusal
    /// carries its reason and `hardware set-size`'s usage.
    #[test]
    fn set_size_refuses_a_size_other_than_m_where_external_flash_isnt_supported() {
        let board = Board::try_from_str("fire-24-f").unwrap();
        for &size in BoardSize::supported_values() {
            let options = format!("--board fire-24-f --size {size}");
            if size == BoardSize::M {
                assert_eq!(set_size(&options).unwrap().size, size);
                continue;
            }
            let error = set_size(&options).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{size}");
            let reason = SizeError::ExternalFlashUnsupported(board).to_string();
            let text = error.to_string();
            assert!(text.contains(&reason), "{text}");
            assert!(text.contains(&usage_of("set-size")), "{text}");
            // Every size is accepted where external flash is supported.
            let options = format!("--board fire-40-a --size {size}");
            assert_eq!(set_size(&options).unwrap().size, size);
        }
    }

    #[test]
    fn set_size_refuses_an_ice_board() {
        let error = set_size("--board ice-24-d --size M").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
        let reason = error.source().map(|e| e.to_string());
        assert_eq!(reason, parse_board_to_size("ice-24-d").err(), "{error}");
    }

    // -----------------------------------------------------------------------
    // sign
    // -----------------------------------------------------------------------

    /// The options every `hardware sign` of a board that doesn't support
    /// external flash requires apart from a signature's source.
    const TO_SIGN: &str = "--chip-id E126C9F97C10ADAC --board fire-24-f --manufacturer onerom.org";

    /// `hardware sign OPTIONS` parsed and checked, as the CLI does.
    fn sign(options: &str) -> Result<HardwareSignArgs, clap::Error> {
        let cli = cli(&format!("hardware sign {options}"))?;
        cli.command.check_args()?;
        let Commands::Hardware(hardware) = cli.command else {
            panic!("not a hardware command");
        };
        let HardwareCommands::Sign(args) = hardware.command else {
            panic!("not hardware sign");
        };
        Ok(args)
    }

    fn sign_refused(options: &str) -> ErrorKind {
        match sign(options) {
            Ok(args) => panic!("{options} parsed as {args:?}"),
            Err(e) => e.kind(),
        }
    }

    #[test]
    fn a_sign_with_a_signing_server_parses() {
        let args = sign(&format!("{TO_SIGN} {SIGNER} --pin secret --dry-run")).unwrap();
        // CHIPID's rows, row 0x000 first.
        assert_eq!(args.chip_id, [0xadac, 0x7c10, 0xc9f9, 0xe126]);
        assert_eq!(args.board, Board::try_from_str("fire-24-f").unwrap());
        assert_eq!(args.size, None);
        assert_eq!(args.manufacturer, "onerom.org");
        assert_eq!(args.date, None);
        assert_eq!(args.signer.as_deref(), Some("https://example.invalid"));
        assert_eq!(args.key_id, Some(1));
        assert_eq!(args.pin.as_deref(), Some("secret"));
        assert_eq!(args.key, None);
        assert!(args.dry_run);
        assert!(!args.json);
        assert!(!args.requires_device());
    }

    #[test]
    fn a_sign_with_a_key_file_parses() {
        let args = sign(&format!(
            "{TO_SIGN} --size M --key key.pem --date 20260101 --json"
        ))
        .unwrap();
        assert_eq!(args.size, Some(BoardSize::M));
        assert_eq!(args.key, Some(PathBuf::from("key.pem")));
        assert_eq!(args.key_id, None);
        assert_eq!(args.signer, None);
        assert_eq!(args.date.as_deref(), Some("20260101"));
        assert!(args.json);
        assert!(!args.dry_run);

        // -b is --board, as it is on every command.
        let args = sign(
            "--chip-id E126C9F97C10ADAC -b fire-40-a --size L --manufacturer onerom.org --key key.pem",
        )
        .unwrap();
        assert_eq!(args.board, Board::try_from_str("fire-40-a").unwrap());
    }

    /// A Chip ID is 16 hex digits, as the bootloader's USB serial number
    /// shows it, in either case.
    #[test]
    fn a_chip_id_is_16_hex_digits() {
        let options = "--board fire-24-f --manufacturer onerom.org --key key.pem";
        for text in ["E126C9F97C10ADAC", "e126c9f97c10adac"] {
            let args = sign(&format!("--chip-id {text} {options}")).unwrap();
            assert_eq!(args.chip_id, [0xadac, 0x7c10, 0xc9f9, 0xe126], "{text}");
        }
        for text in [
            "E126C9F97C10ADA",
            "E126C9F97C10ADAC0",
            "E126C9F97C10ADAG",
            "0xE126C9F97C10AD",
            "",
        ] {
            let words = ["onerom", "hardware", "sign", "--chip-id", text]
                .into_iter()
                .chain(options.split_whitespace());
            let error = Cli::try_parse_from(words).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{text:?}");
        }
    }

    #[test]
    fn a_sign_requires_its_values_and_one_signature_source() {
        for options in [
            "--board fire-24-f --manufacturer onerom.org --key key.pem",
            "--chip-id E126C9F97C10ADAC --manufacturer onerom.org --key key.pem",
            "--chip-id E126C9F97C10ADAC --board fire-24-f --key key.pem",
            TO_SIGN,
        ] {
            assert_eq!(
                sign_refused(options),
                ErrorKind::MissingRequiredArgument,
                "{options}"
            );
        }
        assert_eq!(
            sign_refused(&format!("{TO_SIGN} {SIGNER} --key key.pem")),
            ErrorKind::ArgumentConflict
        );
    }

    /// `hardware sign` has the same rules for --key-id as `hardware
    /// commission`, and doesn't have --signature.
    #[test]
    fn a_sign_takes_a_key_id_with_a_signing_server_only() {
        assert_eq!(
            sign_refused(&format!("{TO_SIGN} --signer https://example.invalid")),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            sign_refused(&format!("{TO_SIGN} --key key.pem --key-id 1")),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            sign_refused(&format!(
                "{TO_SIGN} --signer https://example.invalid --key-id 0"
            )),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            sign_refused(&format!("{TO_SIGN} --key key.pem --signature {SIGNATURE}")),
            ErrorKind::UnknownArgument
        );
    }

    /// A key file doesn't record a signature, so only a signing server has a
    /// dry run.
    #[test]
    fn a_sign_dry_run_requires_a_signing_server() {
        for flag in ["--dry-run", "--dryrun"] {
            assert!(sign(&format!("{TO_SIGN} {SIGNER} {flag}")).unwrap().dry_run);
            assert_eq!(
                sign_refused(&format!("{TO_SIGN} --key key.pem {flag}")),
                ErrorKind::ArgumentConflict,
                "{flag}"
            );
        }
    }

    /// The size follows `hardware commission`'s rules.
    #[test]
    fn a_sign_checks_the_size_as_commission_does() {
        let options = "--chip-id E126C9F97C10ADAC --manufacturer onerom.org --key key.pem";
        assert_eq!(
            sign_refused(&format!("{options} --board fire-40-a")),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            sign_refused(&format!("{options} --board fire-24-f --size L")),
            ErrorKind::ValueValidation
        );
        let args = sign(&format!("{options} --board fire-40-a --size L")).unwrap();
        assert_eq!(args.board_size(), Ok(BoardSize::L));
        let args = sign(&format!("{options} --board fire-24-f")).unwrap();
        assert_eq!(args.board_size(), Ok(BoardSize::M));
        assert_eq!(
            sign_refused(&format!("{options} --board ice-24-d --size M")),
            ErrorKind::ValueValidation
        );
    }

    /// `hardware sign` doesn't have --force, so a date later than today is
    /// always refused.
    #[test]
    fn a_sign_refuses_a_future_date() {
        let options = format!("{TO_SIGN} --key key.pem");
        assert_eq!(
            sign_refused(&format!("{options} --date 99991231")),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            sign_refused(&format!("{options} --date 20260230")),
            ErrorKind::ValueValidation
        );
        assert!(sign(&format!("{options} --date {}", today())).is_ok());
        assert_eq!(
            sign_refused(&format!("{options} --force")),
            ErrorKind::UnknownArgument
        );
    }

    // -----------------------------------------------------------------------
    // request-signature
    // -----------------------------------------------------------------------

    /// `hardware request-signature OPTIONS` parsed and checked, as the CLI
    /// does.
    fn request_signature(options: &str) -> Result<HardwareRequestSignatureArgs, clap::Error> {
        let cli = cli(&format!("hardware request-signature {options}"))?;
        cli.command.check_args()?;
        let Commands::Hardware(hardware) = cli.command else {
            panic!("not a hardware command");
        };
        let HardwareCommands::RequestSignature(args) = hardware.command else {
            panic!("not hardware request-signature");
        };
        Ok(args)
    }

    fn request_refused(options: &str) -> ErrorKind {
        match request_signature(options) {
            Ok(args) => panic!("{options} parsed as {args:?}"),
            Err(e) => e.kind(),
        }
    }

    #[test]
    fn a_signature_request_parses() {
        let args = request_signature("--board fire-24-f").unwrap();
        assert_eq!(args.board, Board::try_from_str("fire-24-f").unwrap());
        assert_eq!(args.board_size(), Ok(BoardSize::M));
        assert!(args.requires_device());
        let args = request_signature("--board fire-40-a --size l").unwrap();
        assert_eq!(args.board_size(), Ok(BoardSize::L));
        // -b is --board, as it is on every command.
        let args = request_signature("-b fire-40-a --size L").unwrap();
        assert_eq!(args.board, Board::try_from_str("fire-40-a").unwrap());
    }

    /// The size follows `hardware commission`'s rules.
    #[test]
    fn a_signature_request_checks_the_size_as_commission_does() {
        assert_eq!(
            request_refused("--board fire-40-a"),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            request_refused("--board fire-24-f --size L"),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            request_refused("--board fire-40-a --size XL"),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            request_refused("--board ice-24-d --size M"),
            ErrorKind::ValueValidation
        );
        assert_eq!(request_refused(""), ErrorKind::MissingRequiredArgument);
    }

    /// A community signing request's manufacturer and key are fixed, and it
    /// doesn't override anything `hardware commission` refuses.
    #[test]
    fn a_signature_request_takes_only_a_board_and_a_size() {
        for option in [
            "--manufacturer onerom.org",
            "--key-id 2",
            "--force",
            "--date 20260101",
            "--dry-run",
        ] {
            assert_eq!(
                request_refused(&format!("--board fire-24-f {option}")),
                ErrorKind::UnknownArgument,
                "{option}"
            );
        }
    }
}
