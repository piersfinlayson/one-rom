// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom hardware`.

use std::path::PathBuf;
use std::str::FromStr;

use clap::{ArgGroup, Args, Subcommand};
use enum_dispatch::enum_dispatch;
use onerom_app::BoardSize;
use onerom_config::hw::{Board, Model};
use time::{Date, Month, OffsetDateTime};

use crate::args::CommandTrait;
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
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum HardwareCommands {
    /// Commission a One ROM by writing its identity to OTP
    ///
    /// Writes to the RP2350's OTP:
    /// - a signed and dated commissioning instance holding the board's type
    ///   and its manufacturer
    /// - the bootloader's USB strings
    /// - the settings for an L board's second flash chip
    ///
    /// The commissioning instance's pages are then locked. OTP can't be erased
    /// so every row is listed and confirmed before anything is written. A run
    /// that stopped part way can be run again.
    ///
    /// The signature comes from a key on a signing server (--signer) or a key
    /// file (--key).
    ///
    /// A One ROM that's running or in limp mode is stopped first and rebooted
    /// into running mode at the end.
    ///
    /// Examples:
    ///
    ///   onerom hardware commission --board fire-24-f --size M --manufacturer piers.rocks --signer https://HOST/v1/1
    ///
    ///   onerom hardware commission --board fire-40-a --size L --manufacturer piers.rocks --key key.pem
    #[command(verbatim_doc_comment)]
    Commission(HardwareCommissionArgs),

    /// Check the signatures of a One ROM's commissioning.
    ///
    /// Checks the signature of each commissioning instance in the One ROM's
    /// OTP against the table of signing keys. It downloads the current table
    /// and uses the one built into this CLI if the download fails.
    ///
    /// It succeeds only if the current commissioning instance's signature is
    /// accepted.
    ///
    /// A One ROM that's running or in limp mode is stopped first and rebooted
    /// into running mode at the end.
    ///
    /// Examples:
    ///
    ///   onerom hardware validate
    ///
    ///   onerom hardware validate --json
    Validate(HardwareValidateArgs),
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("signing").required(true).args(["signer", "key"])))]
pub struct HardwareCommissionArgs {
    /// The board's type (e.g. fire-24-f). Its name is written to OTP.
    #[arg(long, short, value_name = "BOARD", value_parser = parse_fire_board)]
    pub board: Board,

    /// The board's size. M has 2MB of flash. L also has a 2MB flash chip on
    /// chip select 1. XL is reserved.
    #[arg(long, value_name = "SIZE", value_parser = BoardSize::from_str)]
    pub size: BoardSize,

    /// The manufacturer's name to write to OTP.
    #[arg(long, value_name = "NAME", value_parser = parse_manufacturer)]
    pub manufacturer: String,

    /// The URL of a key on a signing server (e.g. https://HOST/v1/1).
    #[arg(long, value_name = "URL")]
    pub signer: Option<String>,

    /// The PIN of the key on the signing server. It's asked for when left
    /// out.
    // clap counts a requirement on one member of the "signing" group as met by
    // any member so `requires = "signer"` would accept --key. Conflicting with
    // --key refuses every --pin without --signer.
    #[arg(long, value_name = "PIN", conflicts_with = "key")]
    pub pin: Option<String>,

    /// A file holding an unencrypted Ed25519 private key in PKCS#8 PEM form.
    /// 'openssl genpkey -algorithm ed25519' writes one.
    #[arg(long, value_name = "FILE")]
    pub key: Option<PathBuf>,

    /// The UTC commissioning date as YYYYMMDD. Defaults to today's UTC date.
    #[arg(long, value_name = "DATE", value_parser = parse_date)]
    pub date: Option<String>,

    /// Go ahead despite:
    /// - a current commissioning instance holding other values
    /// - firmware for another board
    /// - OTP configuring a second flash chip for an M board
    #[arg(long, short, verbatim_doc_comment)]
    pub force: bool,
}

impl CommandTrait for HardwareCommissionArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct HardwareValidateArgs {
    /// Show the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl CommandTrait for HardwareValidateArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

/// Parse a Fire board's name. An Ice board is refused because commissioning
/// writes an RP2350's OTP.
fn parse_fire_board(text: &str) -> Result<Board, String> {
    let board = Board::try_from_str(text)
        .ok_or_else(|| format!("unknown board\n  Fire boards: {}", get_supported_boards()))?;
    match board.model() {
        Model::Fire => Ok(board),
        Model::Ice => Err("an Ice (STM32) board doesn't have OTP to commission".to_string()),
    }
}

/// Parse a manufacturer's name. It can't be empty or hold a control
/// character.
fn parse_manufacturer(text: &str) -> Result<String, String> {
    if text.is_empty() {
        Err("the name is empty".to_string())
    } else if text.chars().any(char::is_control) {
        Err("the name holds a control character".to_string())
    } else {
        Ok(text.to_string())
    }
}

/// Parse a `YYYYMMDD` date that is no later than today in UTC.
fn parse_date(text: &str) -> Result<String, String> {
    check_date(text, OffsetDateTime::now_utc().date())
}

/// `text` if it's a real `YYYYMMDD` date no later than `today`.
fn check_date(text: &str, today: Date) -> Result<String, String> {
    let date = calendar_date(text).ok_or("not a date as YYYYMMDD")?;
    if date > today {
        return Err(format!("{date} is later than today's UTC date {today}"));
    }
    Ok(text.to_string())
}

/// The date `text` holds as `YYYYMMDD`.
fn calendar_date(text: &str) -> Option<Date> {
    if text.len() != 8 || !text.bytes().all(|b| b.is_ascii_digit()) {
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
        let today = date("20260926");
        assert_eq!(check_date("20260926", today).unwrap(), "20260926");
        for text in [
            "2026092",
            "202609260",
            "2026O926",
            "2026-9-26",
            " 20260926",
            "２０２６０９２６",
            "",
        ] {
            assert!(check_date(text, today).is_err(), "{text}");
        }
    }

    #[test]
    fn a_date_is_a_real_calendar_date() {
        let today = date("20260926");
        for text in ["20261301", "20260001", "20260230", "20250229", "20260900"] {
            assert!(check_date(text, today).is_err(), "{text}");
        }
        assert!(check_date("20240229", today).is_ok());
    }

    #[test]
    fn a_date_later_than_today_is_refused() {
        let today = date("20260926");
        assert!(check_date("20260927", today).is_err());
        assert!(check_date("20270101", today).is_err());
        assert!(check_date("20260925", today).is_ok());
        // The real clock.
        assert!(parse_date("99991231").is_err());
        assert!(parse_date("20000101").is_ok());
        assert!(parse_date(&super::today()).is_ok());
    }

    #[test]
    fn a_manufacturer_is_named_without_control_characters() {
        assert_eq!(parse_manufacturer("piers.rocks").unwrap(), "piers.rocks");
        assert_eq!(parse_manufacturer("Café Ltd").unwrap(), "Café Ltd");
        for text in ["", "piers\nrocks", "\u{1b}[2J", "tab\there"] {
            assert!(parse_manufacturer(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn only_a_fire_board_is_accepted() {
        assert_eq!(
            parse_fire_board("fire-24-f").unwrap(),
            Board::try_from_str("fire-24-f").unwrap()
        );
        let error = parse_fire_board("ice-24-d").unwrap_err();
        assert!(error.contains("Ice"), "{error}");
        let error = parse_fire_board("not-a-board").unwrap_err();
        assert!(error.contains("fire-24-f"), "{error}");
    }
}

/// The command lines `hardware` and `inspect otp` accept and refuse. The tests
/// parse them as the CLI does. Parsing doesn't look for a device.
#[cfg(test)]
mod command_lines {
    use super::*;

    use clap::Parser;
    use clap::error::ErrorKind;

    use crate::args::{Cli, Commands};

    /// The options every commission needs apart from a signature's source.
    const REQUIRED: &str = "--board fire-24-f --size M --manufacturer piers.rocks";

    const SIGNER: &str = "--signer https://example.invalid/v1/1";

    fn cli(line: &str) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(["onerom"].into_iter().chain(line.split_whitespace()))
    }

    fn commission(options: &str) -> Result<HardwareCommissionArgs, clap::Error> {
        let cli = cli(&format!("hardware commission {options}"))?;
        let Commands::Hardware(hardware) = cli.command else {
            panic!("not a hardware command");
        };
        let HardwareCommands::Commission(args) = hardware.command else {
            panic!("not hardware commission");
        };
        Ok(args)
    }

    fn refused(options: &str) -> ErrorKind {
        match commission(options) {
            Ok(args) => panic!("{options} parsed as {args:?}"),
            Err(e) => e.kind(),
        }
    }

    #[test]
    fn a_commission_with_a_signing_server_parses() {
        let args = commission(&format!("{REQUIRED} {SIGNER} --pin secret")).unwrap();
        assert_eq!(args.board, Board::try_from_str("fire-24-f").unwrap());
        assert_eq!(args.size, BoardSize::M);
        assert_eq!(args.manufacturer, "piers.rocks");
        assert_eq!(args.signer.as_deref(), Some("https://example.invalid/v1/1"));
        assert_eq!(args.pin.as_deref(), Some("secret"));
        assert_eq!(args.key, None);
        assert_eq!(args.date, None);
        assert!(!args.force);
    }

    #[test]
    fn a_commission_with_a_key_file_parses() {
        let args = commission(&format!("{REQUIRED} --key key.pem --date 20260101 -f")).unwrap();
        assert_eq!(args.key, Some(PathBuf::from("key.pem")));
        assert_eq!(args.signer, None);
        assert_eq!(args.date.as_deref(), Some("20260101"));
        assert!(args.force);
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
            assert_eq!(args.size, size, "{text}");
        }
    }

    #[test]
    fn size_xl_is_reserved_and_other_sizes_are_refused() {
        for text in ["XL", "xl", "Q", "S"] {
            let error = commission(&format!(
                "--board fire-40-a --size {text} --manufacturer piers.rocks --key key.pem"
            ))
            .unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{text}");
            if text.eq_ignore_ascii_case("xl") {
                assert!(error.to_string().contains("reserved"), "{error}");
            }
        }
    }

    #[test]
    fn a_signature_needs_a_signing_server_or_a_key_file() {
        assert_eq!(refused(REQUIRED), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn a_signing_server_and_a_key_file_conflict() {
        assert_eq!(
            refused(&format!("{REQUIRED} {SIGNER} --key key.pem")),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn a_pin_needs_a_signing_server() {
        assert_eq!(
            refused(&format!("{REQUIRED} --key key.pem --pin secret")),
            ErrorKind::ArgumentConflict
        );
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
        assert!(error.to_string().contains("Ice"), "{error}");
    }

    #[test]
    fn a_bad_date_is_refused() {
        for date in ["99991231", "20261301", "2026092", "2026O926"] {
            assert_eq!(
                refused(&format!("{REQUIRED} {SIGNER} --date {date}")),
                ErrorKind::ValueValidation,
                "{date}"
            );
        }
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
        for name in ["", "piers\u{1b}rocks"] {
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
}
