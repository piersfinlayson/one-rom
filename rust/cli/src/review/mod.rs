// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Review harness for the CLI's output.
//!
//! Each test prints what the CLI prints for one case. It runs the command
//! against onerom-app's in-memory OTP so the output can be shown without a One
//! ROM. It asserts nothing, and fails only where the code it drives panics.
//!
//! A transcript is the command line as typed, then what the command prints. A
//! line starting `~` is written from the code rather than printed by running
//! it. Device lines are all written this way. `Device` holds nusb's
//! `DeviceInfo`, which only USB enumeration creates, so a test can't build a
//! `Device` to print through its `Display`. The signature comes from the test
//! key, signer 1 in a test table.
//!
//! There's a file for each command and a test for each case. To run one
//! command's cases:
//!
//!   cargo test -p onerom-cli --bin onerom review::set_size -- --nocapture --test-threads=1

mod commission;
mod control_erase;
mod control_pin;
mod control_poke;
mod control_reset;
mod firmware_build;
mod firmware_inspect;
mod hardware;
mod inspect_gpio;
mod inspect_header;
mod inspect_otp;
mod inspect_peek;
mod inspect_slots;
mod program;
mod request_signature;
mod scan;
mod set_size;
mod sign;
mod validate;

use std::cell::RefCell;
use std::fmt::Display;
use std::io::{BufRead, Cursor, Read, Write};
use std::rc::Rc;

use clap::Parser;
use ed25519_dalek::Signer as _;
use onerom_app::{
    BoardSize, CommissionError, Interruption, MemoryOtp, Request, RequestDate, prepare,
};
use onerom_cli::signing::KeyFile;
use onerom_cli::usb::{FLASH_BASE, FLASH_READ_SIZE_BYTES, RAM_BASE};
use onerom_cli::{Error, Options};
use onerom_config::hw::Board;
use onerom_fw_parser::ParseError;
use onerom_fw_parser::readers::MemoryReader;
use onerom_metadata::otp::CommissioningValues;
use onerom_metadata::otp::pico_otp::ecc_encode;

use crate::args::hardware::HardwareCommands;
use crate::args::{Cli, CommandTrait, Commands};
use crate::hardware::{SignatureSource, Signing, ToSign};
use crate::test_board::{
    ACME_PIN, CHIP_ID, acme_key_file, blank_board, commissioned_board, header_image, key,
};

/// The line a stopped One ROM whose firmware is for `board` is shown with.
/// It carries `(L)` or `(other)` for the size its OTP configures, `size`.
/// It's written from `Device`'s `Display`.
fn device(board: &str, size: Option<BoardSize>) -> String {
    let name: Vec<String> = board
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or(String::new(), |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect();
    let size = match size {
        Some(BoardSize::M) => "",
        Some(BoardSize::L) => " (L)",
        None => " (other)",
    };
    format!(
        "One ROM {}{size} - Firmware: v0.8.0 State: Stopped Serial: DE3F9C232F655B6B",
        name.join(" ")
    )
}

/// The line a stopped board is shown with where this build doesn't recognise
/// its firmware and its OTP isn't commissioned. It's written from `Device`'s
/// `Display`.
const UNRECOGNISED: &str =
    "Unknown           - Firmware: n/a   State: Stopped Serial: DE3F9C232F655B6B";

/// The line a One ROM running One ROM Lab for fire-24-e is shown with. It's
/// written from `Device`'s `Display`.
const RUNNING_LAB: &str =
    "One ROM Lab Fire 24 E - Firmware: v0.4.0 State: Running Serial: DE3F9C232F655B6B";

/// The parser's reasons for not recognising the firmware of a stopped board
/// whose flash holds `image`. The parser reads `image` as enumeration reads a
/// board.
async fn unrecognised_reasons(image: Vec<u8>) -> Vec<ParseError> {
    let mut reader = MemoryReader::new(image, FLASH_BASE);
    let parsed =
        onerom_fw_parser::Parser::with_base_flash_address(&mut reader, FLASH_BASE, RAM_BASE)
            .parse_device()
            .await;
    parsed.parse_errors().to_vec()
}

/// Flash holding v0.9.0, newer than this build reads.
fn newer_firmware_flash() -> Vec<u8> {
    header_image(9, FLASH_BASE + 0x300, 0)
}

/// Flash holding a v0.8.0 header with a null build date pointer, whose
/// metadata is zeros.
fn damaged_header_flash() -> Vec<u8> {
    header_image(8, 0, FLASH_BASE + 0x400)
}

/// Erased flash and flash reading all zeros, each with its description.
fn flash_without_firmware() -> [(&'static str, Vec<u8>); 2] {
    let size = FLASH_READ_SIZE_BYTES as usize;
    [
        ("erased flash", vec![0xff; size]),
        ("flash reading all zeros", vec![0; size]),
    ]
}

/// A blank board whose OTP configures a size that's neither M nor L.
/// FLASH_DEVINFO sets chip select 1 to 4MB.
fn neither_m_nor_l_board() -> MemoryOtp {
    let mut otp = blank_board();
    otp.set_raw(0x054, ecc_encode(0xa9af));
    for row in 0x048..=0x04a {
        otp.set_raw(row, 0x000020);
    }
    otp
}

/// The size `otp` configures, as a stopped board's device line has it.
async fn size_of(otp: &mut MemoryOtp) -> Option<BoardSize> {
    use onerom_metadata::OneromBoardSize;
    match onerom_app::read_board_size(otp).await.unwrap() {
        OneromBoardSize::BoardSizeM => Some(BoardSize::M),
        OneromBoardSize::BoardSizeL => Some(BoardSize::L),
        OneromBoardSize::BoardSizeUnknown | OneromBoardSize::BoardSizeOther => None,
    }
}

/// Output shared by the command and the echo of the user's answer, as a
/// terminal shows them.
#[derive(Clone, Default)]
struct Screen(Rc<RefCell<Vec<u8>>>);

impl Screen {
    /// What the screen shows, taken off it.
    fn take(&self) -> String {
        String::from_utf8(self.0.take()).unwrap()
    }
}

impl Write for Screen {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The user's typing. What the command reads is echoed to the screen.
struct Keyboard {
    typed: Cursor<Vec<u8>>,
    screen: Screen,
}

impl Keyboard {
    /// A keyboard on which the user types `typed`, echoed to `screen`.
    fn new(typed: &str, screen: &Screen) -> Self {
        Self {
            typed: Cursor::new(typed.as_bytes().to_vec()),
            screen: screen.clone(),
        }
    }
}

impl Read for Keyboard {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.fill_buf()?.len().min(buf.len());
        buf[..n].copy_from_slice(&self.fill_buf()?[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for Keyboard {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.typed.fill_buf()
    }

    fn consume(&mut self, n: usize) {
        let start = self.typed.position() as usize;
        let read = &self.typed.get_ref()[start..start + n];
        self.screen.0.borrow_mut().extend_from_slice(read);
        self.typed.consume(n);
    }
}

/// `words` parsed and checked as the CLI does. Returns the command and the
/// options `--yes` and `--verbose` set. There isn't a device.
fn command_of(words: &[&str]) -> (Commands, Options) {
    let cli = Cli::try_parse_from(words).unwrap();
    cli.command.check_args().unwrap();
    let options = Options {
        verbose: cli.verbose,
        log_level: onerom_cli::LogLevel::Warn,
        yes: cli.yes,
        unrecognised: false,
        device: None,
        vid_pid: Vec::new(),
    };
    (cli.command, options)
}

/// `words`, which start `onerom hardware`, parsed and checked as the CLI
/// does. Returns the command and the options `--yes` and `--verbose` set.
fn hardware_of(words: &[&str]) -> (HardwareCommands, Options) {
    let (command, options) = command_of(words);
    let Commands::Hardware(hardware) = command else {
        panic!("not a hardware command");
    };
    (hardware.command, options)
}

/// Prints `error` as the CLI does when a command fails.
fn failed(error: impl Display) {
    println!("Failed to execute command.\n{error}");
}

/// Prints `error` as the CLI prints it, written from the code.
fn written_failure(error: &Error) {
    for text in format!("Failed to execute command.\n{error}").lines() {
        println!("~ {text}");
    }
}

/// The prompt for the PIN of `key`, such as `signing key #1` or a key file, as
/// a terminal shows it once the user has pressed Enter, Escape or Ctrl-C. The
/// PIN isn't echoed.
fn pin_prompt(key: &str) {
    println!("~ PIN for {key}: ");
}

/// Acme Retro's key from its key file, `acme.pem` on the command line, once
/// the user has typed its PIN at the prompt this prints. The CLI asks for the
/// PIN before it prints anything else.
fn acme_signing() -> Signing {
    pin_prompt("acme.pem");
    let (_dir, path) = acme_key_file();
    Signing::File(KeyFile::read(&path, Some(ACME_PIN)).unwrap())
}

/// `words` as typed at a shell. An empty word or one holding a space is
/// quoted and a control character escaped.
fn shell_line(words: &[&str]) -> String {
    words
        .iter()
        .map(|word| {
            if word.chars().any(char::is_control) {
                format!("$'{}'", onerom_cli::otp::escape_controls(word))
            } else if word.is_empty() || word.contains(' ') {
                format!("\"{word}\"")
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Prints the transcript of `words`. clap or the check after parsing refuses
/// that command line before the CLI looks for a device.
fn refused_line(words: &[&str]) {
    println!("$ {}", shell_line(words));
    let error = match Cli::try_parse_from(words) {
        Ok(cli) => match cli.command.check_args() {
            Ok(()) => {
                println!("~ parses");
                return;
            }
            Err(e) => e,
        },
        Err(e) => e,
    };
    print!("{error}");
}

/// Prints the transcript of each of `lines`, as [`refused_line`] does, a
/// blank line after each.
fn refused_lines<S: AsRef<str>>(lines: &[S]) {
    for line in lines {
        refused_line(&line.as_ref().split_whitespace().collect::<Vec<_>>());
        println!();
    }
}

/// Prints the help `words` asks for, as clap prints it.
fn help(words: &[&str]) {
    println!("$ {}", words.join(" "));
    match Cli::try_parse_from(words) {
        Ok(_) => println!("~ parses"),
        Err(e) => print!("{e}"),
    }
}

/// Writes `values` with ECC from `row`.
fn put(otp: &mut MemoryOtp, row: u16, values: &[u16]) {
    for (row, &value) in (row..).zip(values) {
        otp.set_raw(row, ecc_encode(value));
    }
}

/// An entry's rows: its key, its length and its value. The value takes two
/// bytes a row with the low byte first.
fn entry(key: u16, value: &[u8]) -> Vec<u16> {
    let mut rows = vec![key, value.len() as u16];
    rows.extend(
        value
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair.get(1).copied().unwrap_or(0)])),
    );
    rows
}

/// A blank board whose commissioning area holds version 2.
fn version_2_board() -> MemoryOtp {
    let mut otp = blank_board();
    put(&mut otp, 0x0c0, &[onerom_metadata::OTP_STORE_MAGIC, 2]);
    otp
}

/// A blank board holding a signed fire-24-f instance with key 16 after its
/// board.
fn board_with_an_unknown_key() -> MemoryOtp {
    use onerom_metadata::{OTP_STORE_MAGIC, OTP_STORE_VERSION};
    let rows = |signature: &[u8]| -> Vec<u16> {
        [
            vec![OTP_STORE_MAGIC, OTP_STORE_VERSION],
            entry(1, b"fire-24-f"),
            entry(16, &[1, 2]),
            entry(3, b"piers.rocks"),
            entry(4, b"20260101"),
            entry(5, &1_u16.to_le_bytes()),
            entry(2, signature),
        ]
        .concat()
    };
    let area = onerom_metadata::otp::CommissioningArea::parse(&rows(&[0; 64]));
    let message = area.instances()[0].message(CHIP_ID).unwrap();
    let signature = key().sign(&message).to_bytes();
    let mut otp = blank_board();
    put(&mut otp, 0x0c0, &rows(&signature));
    otp
}

/// Boards holding data from a newer version, each with its description.
fn newer_boards() -> [(&'static str, MemoryOtp); 2] {
    [
        ("an area holding version 2", version_2_board()),
        ("an instance holding key 16", board_with_an_unknown_key()),
    ]
}

/// Commissions `otp` as an M fire-24-f by piers.rocks on `date`, printing
/// nothing. `force` is `--force`.
async fn commission_quietly(
    otp: &mut MemoryOtp,
    date: &str,
    force: bool,
) -> Result<(), CommissionError> {
    let request = Request {
        board: Board::try_from_str("fire-24-f").unwrap(),
        size: BoardSize::M,
        manufacturer: "piers.rocks".to_string(),
        date: RequestDate::Given(date.to_string()),
        signer: 1,
        force,
    };
    let prepared = prepare(otp, &request).await?;
    let signature = key().sign(prepared.message()).to_bytes();
    prepared.plan(&signature)?.execute(otp, |_| {}).await
}

/// A board holding an instance from 20260101 and one from 20260102 that
/// `--force` wrote to replace it.
async fn replaced_instance_board() -> MemoryOtp {
    let mut otp = blank_board();
    commission_quietly(&mut otp, "20260101", false)
        .await
        .unwrap();
    commission_quietly(&mut otp, "20260102", true)
        .await
        .unwrap();
    otp
}

/// A board in each state `inspect otp --verbose` and `validate --verbose` show
/// an instance in, each with its description.
async fn instance_states() -> Vec<(&'static str, MemoryOtp)> {
    let mut one = blank_board();
    commission_quietly(&mut one, "20260101", false)
        .await
        .unwrap();

    let mut force_interrupted = blank_board();
    commission_quietly(&mut force_interrupted, "20260101", false)
        .await
        .unwrap();
    let write = force_interrupted.write_count() + 10;
    force_interrupted.interrupt(write, Interruption::NotLanded);
    commission_quietly(&mut force_interrupted, "20260102", true)
        .await
        .unwrap_err();

    let mut first_interrupted = blank_board();
    first_interrupted.interrupt(10, Interruption::NotLanded);
    commission_quietly(&mut first_interrupted, "20260101", false)
        .await
        .unwrap_err();

    let mut missing_signer = replaced_instance_board().await;
    // The second instance's COMMISSIONING_SIGNER key row, deleted.
    missing_signer.set_raw(0x117, 0xff_ffff);

    vec![
        ("A: one instance", one),
        (
            "B: an instance replaced by --force with another date",
            replaced_instance_board().await,
        ),
        (
            "C: a --force run interrupted part way through its instance",
            force_interrupted,
        ),
        (
            "D: a first run interrupted part way through its instance",
            first_interrupted,
        ),
        (
            "E: a later complete instance missing its signer",
            missing_signer,
        ),
    ]
}

/// The boards whose JSON the JSON cases show, each with its description.
async fn json_boards() -> Vec<(&'static str, MemoryOtp)> {
    let [version_2, unknown_key] = newer_boards();
    vec![
        ("a blank board", blank_board()),
        (
            "a board commissioned as M fire-24-f",
            commissioned_board("fire-24-f", BoardSize::M).await,
        ),
        (
            "a board commissioned as L fire-40-a",
            commissioned_board("fire-40-a", BoardSize::L).await,
        ),
        (
            "an instance replaced by --force with another date",
            replaced_instance_board().await,
        ),
        version_2,
        unknown_key,
    ]
}

/// A signing server's address.
const SIGNER: &str = "https://onerom-sign.example";

/// Key 1's URL on [`SIGNER`].
fn key_1() -> String {
    onerom_cli::signing::key_url(SIGNER, 1)
}

/// A signing server. `dry_run_key` signs the dry run. The recorded signature
/// is the test key's. It covers another message where `records_another`.
struct Server {
    dry_run_key: ed25519_dalek::SigningKey,
    records_another: bool,
}

impl Server {
    /// A server holding the test key.
    fn test_key() -> Self {
        Self {
            dry_run_key: key(),
            records_another: false,
        }
    }

    /// A server whose dry run is signed with a key other than the test key.
    fn another_key() -> Self {
        Self {
            dry_run_key: ed25519_dalek::SigningKey::from_bytes(&[2; 32]),
            records_another: false,
        }
    }
}

impl SignatureSource for Server {
    async fn dry_run(&self, to_sign: &ToSign<'_>) -> Result<[u8; 64], Error> {
        Ok(self.dry_run_key.sign(to_sign.message).to_bytes())
    }

    async fn record(&self, to_sign: &ToSign<'_>) -> Result<Option<[u8; 64]>, Error> {
        let message = if self.records_another {
            b"another message".as_slice()
        } else {
            to_sign.message
        };
        Ok(Some(key().sign(message).to_bytes()))
    }
}

/// `signing`'s signature over the instance of `board` by `manufacturer` on
/// `date` for signer `id`, on the test board's CHIPID, in hex.
fn signature_with(
    signing: &ed25519_dalek::SigningKey,
    board: &str,
    manufacturer: &str,
    date: &str,
    id: u16,
) -> String {
    let board = Board::try_from_str(board).unwrap();
    let values = CommissioningValues::new(board, manufacturer, date, id).unwrap();
    hex::encode(signing.sign(&values.message(CHIP_ID)).to_bytes())
}

/// The test key's signature over the instance of `board` by `manufacturer` on
/// `date` for signer `id`, on the test board's CHIPID, in hex.
fn signature_hex(board: &str, manufacturer: &str, date: &str, id: u16) -> String {
    signature_with(&key(), board, manufacturer, date, id)
}
