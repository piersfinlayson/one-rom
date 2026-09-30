// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `hardware commission`.

use std::io::IsTerminal;
use std::path::Path;

use onerom_app::{BoardSize, Interruption, MemoryOtp, SignerTable};
use onerom_cli::otp::escape_controls;
use onerom_cli::signing::KeyFile;
use onerom_cli::{Error, Options};
use onerom_config::hw::Board;
use onerom_metadata::otp::format_chip_id;
use onerom_metadata::otp::pico_otp::ecc_encode;
use serde::Serialize;

use super::{
    Keyboard, SIGNER, Screen, Server, acme_signing, commission_quietly, device, failed,
    hardware_of, key_1, newer_boards, pin_prompt, refused_line, refused_lines, shell_line,
    signature_hex, signature_with, size_of, written_failure,
};
use crate::args::hardware::{HardwareCommands, HardwareCommissionArgs};
use crate::hardware::{
    OtpCommand, SignatureSource, Signing, SigningKeys, ToSign, check_commissioned_otp,
    check_firmware, check_server_key, commission_otp, firmware_warning, signer_for, signing,
    signing_key,
};
use crate::test_board::{
    Files, blank_board, commissioned_board, key, key_file, table, table_allowing,
};

/// `line`, which starts `onerom hardware commission`, parsed and checked as the
/// CLI does. `key.pem` stands for `key`. Returns the arguments and the options
/// `--yes` and `--verbose` set.
fn commission_args(line: &str, key: &Path) -> (HardwareCommissionArgs, Options) {
    let key = key.to_str().unwrap();
    let words: Vec<&str> = line
        .split_whitespace()
        .map(|word| if word == "key.pem" { key } else { word })
        .collect();
    args_of(&words)
}

/// `words`, which start `onerom hardware commission`, parsed and checked as
/// the CLI does. Returns the arguments and the options `--yes` and
/// `--verbose` set.
fn args_of(words: &[&str]) -> (HardwareCommissionArgs, Options) {
    let (command, options) = hardware_of(words);
    let HardwareCommands::Commission(args) = command else {
        panic!("not hardware commission");
    };
    (args, options)
}

/// Prints the transcript of `line` commissioning `otp` with the test key file.
/// `typed` is what the user types. Returns whether it wrote to OTP.
async fn commission(otp: &mut MemoryOtp, line: &str, typed: &str) -> bool {
    let (_dir, key) = key_file();
    let signing = Signing::File(KeyFile::read(&key, None).unwrap());
    commission_signed(otp, line, typed, &signing).await
}

/// Prints the transcript of `line` commissioning `otp` with `signing`
/// signing. `typed` is what the user types. Returns whether it wrote to OTP.
async fn commission_signed<S: SignatureSource>(
    otp: &mut MemoryOtp,
    line: &str,
    typed: &str,
    signing: &S,
) -> bool {
    println!("$ {}", short_line(line));
    run_signed(otp, line, typed, signing, None).await
}

/// Prints what `line` prints commissioning `otp` with `signing` signing, from
/// the device line on. `typed` is what the user types. `lines` is how many
/// lines after the device line are printed. All of them where `None`.
/// Returns whether it wrote to OTP.
async fn run_signed<S: SignatureSource>(
    otp: &mut MemoryOtp,
    line: &str,
    typed: &str,
    signing: &S,
    lines: Option<usize>,
) -> bool {
    let (_dir, key) = key_file();
    let (args, options) = commission_args(line, &key);
    let table = table(None);
    let signer = table.get(1).unwrap();

    println!("~ {}", device(args.board.name(), size_of(otp).await));
    let mut screen = Screen::default();
    let mut keyboard = Keyboard::new(typed, &screen);
    let result = commission_otp(
        otp,
        &args,
        signing,
        (signer, &table),
        &options,
        &mut screen,
        &mut keyboard,
    )
    .await;
    let shown = screen.take();
    if let Some(lines) = lines {
        for text in shown.lines().take(lines) {
            println!("{text}");
        }
        return matches!(result, Ok(true));
    }
    print!("{shown}");
    match result {
        Ok(wrote) => wrote,
        Err(e) => {
            failed(e);
            false
        }
    }
}

/// Prints the transcript of `line` commissioning the stopped board `otp` with
/// the test key file, then what `--validate` and `--inspect-otp` print where
/// `line` sets them. `typed` is what the user types.
async fn commission_then_check(otp: &mut MemoryOtp, line: &str, typed: &str) {
    let wrote = commission(otp, line, typed).await;
    let (_dir, key) = key_file();
    let (args, options) = commission_args(line, &key);
    if !(wrote && (args.validate || args.inspect_otp)) {
        return;
    }
    if options.verbose {
        println!("~ Rebooting device into stopped mode...");
    }
    let table = table(None);
    let files = Files(Vec::new());
    let device = format!("~ {}", device(args.board.name(), size_of(otp).await));
    let mut out = Vec::new();
    let result = check_commissioned_otp(
        otp,
        &device,
        args.validate
            .then_some(((&table, SigningKeys::Downloaded), &files)),
        args.inspect_otp.then_some(&table),
        options.verbose,
        &mut out,
    )
    .await;
    print!("{}", String::from_utf8(out).unwrap());
    if let Err(e) = result {
        failed(e);
    }
}

/// `line` with a manufacturer's name longer than 40 characters shortened.
fn short_line(line: &str) -> String {
    line.split_whitespace()
        .map(|word| {
            if word.len() > 40 {
                format!("<{} characters>", word.len())
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Prints the transcript of `line`, which starts `onerom hardware
/// commission`, on `otp` with the signing key table `table`. The signature's
/// source is `server` where it's given and what the line asks for otherwise.
/// `typed` is what the user types.
async fn commission_with_table(
    otp: &mut MemoryOtp,
    line: &str,
    typed: &str,
    table: &SignerTable,
    server: Option<&Server>,
) {
    println!("$ {line}");
    let (_dir, key) = key_file();
    let (args, options) = commission_args(line, &key);
    // The CLI makes the signature's source and checks its key before it stops
    // the One ROM and shows the device line. A signing server's key is found
    // by its ID and checked against the manufacturer before its public key is
    // fetched, so a refused key doesn't reach the network.
    let signing = match signing(&args) {
        Ok(signing) => signing,
        Err(e) => return failed(e),
    };
    let signer = match signer_for(table, &signing, &args.manufacturer).await {
        Ok(signer) => signer,
        Err(e) => return failed(e),
    };
    println!("~ {}", device(args.board.name(), size_of(otp).await));
    let mut screen = Screen::default();
    let mut keyboard = Keyboard::new(typed, &screen);
    let result = match server {
        Some(server) => {
            commission_otp(
                otp,
                &args,
                server,
                (signer, table),
                &options,
                &mut screen,
                &mut keyboard,
            )
            .await
        }
        None => {
            commission_otp(
                otp,
                &args,
                &signing,
                (signer, table),
                &options,
                &mut screen,
                &mut keyboard,
            )
            .await
        }
    };
    print!("{}", screen.take());
    if let Err(e) = result {
        failed(e);
    }
}

#[test]
fn help() {
    super::help(&["onerom", "hardware", "commission", "--help"]);
}

/// An M board. A dry run, then with `--verbose`, then a real run answered no,
/// then yes.
#[tokio::test]
async fn an_m_board() {
    let line =
        "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem";
    let mut otp = blank_board();
    commission(&mut otp, &format!("{line} --dry-run"), "").await;
    println!();
    commission(&mut otp, &format!("{line} --dry-run --verbose"), "").await;
    println!();
    commission(&mut otp, line, "n\n").await;
    println!();
    commission(&mut otp, line, "y\n").await;
}

/// An L board. A dry run, then with `--verbose`, then a real run answered
/// yes.
#[tokio::test]
async fn an_l_board() {
    let line = "onerom hardware commission --board fire-40-a --size L --manufacturer piers.rocks --key key.pem";
    let mut otp = blank_board();
    commission(&mut otp, &format!("{line} --dry-run"), "").await;
    println!();
    commission(&mut otp, &format!("{line} --dry-run --verbose"), "").await;
    println!();
    commission(&mut otp, line, "y\n").await;
}

/// An L board set by set-size, then commissioned.
#[tokio::test]
async fn an_l_board_set_by_set_size() {
    let mut otp = blank_board();
    super::set_size::set_size(
        &mut otp,
        "onerom hardware set-size --board fire-40-a --size L --yes",
        "",
    )
    .await;
    println!();
    let line = "onerom hardware commission --board fire-40-a --size L --manufacturer piers.rocks --key key.pem --date 20260101";
    commission(&mut otp, &format!("{line} --dry-run --verbose"), "").await;
    println!();
    commission(&mut otp, line, "y\n").await;
}

/// A second run on a board it commissioned, with today's date.
#[tokio::test]
async fn again_on_a_commissioned_board() {
    let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
    commission(
        &mut otp,
        "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem",
        "",
    )
    .await;
}

/// Another date on a commissioned board, then with `--force`, then another
/// manufacturer.
#[tokio::test]
async fn another_date_and_force() {
    let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
    let line =
        "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem";
    commission(&mut otp, &format!("{line} --date 20260102"), "").await;
    println!();
    commission(&mut otp, &format!("{line} --date 20260102 --force"), "y\n").await;
    println!();
    commission(
        &mut otp,
        "onerom hardware commission --board fire-24-f --manufacturer acme --key key.pem",
        "",
    )
    .await;
}

/// A run after one that stopped part way through the bootloader USB strings.
#[tokio::test]
async fn after_one_that_stopped() {
    let mut otp = blank_board();
    otp.interrupt(70, Interruption::NotLanded);
    let line = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem --date 20260101";
    commission(&mut otp, line, "y\n").await;
    println!();
    commission(&mut otp, line, "y\n").await;
}

/// `--validate` and `--inspect-otp` on an M board, after a run answered yes.
#[tokio::test]
async fn then_validate_and_inspect_otp() {
    let line = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem --validate --inspect-otp";
    commission_then_check(&mut blank_board(), line, "y\n").await;
}

/// `--validate` and `--inspect-otp` with `--verbose` on an L board, after a
/// run answered yes.
#[tokio::test]
async fn then_validate_and_inspect_otp_verbose() {
    let line = "onerom hardware commission --board fire-40-a --size L --manufacturer piers.rocks --key key.pem --validate --inspect-otp --verbose";
    commission_then_check(&mut blank_board(), line, "y\n").await;
}

/// `--inspect-otp` without `--validate`, after a run answered yes.
#[tokio::test]
async fn then_inspect_otp() {
    let line = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem --inspect-otp";
    commission_then_check(&mut blank_board(), line, "y\n").await;
}

/// Neither `--validate` nor `--inspect-otp` runs after a run answered no, or
/// on a board that already holds everything.
#[tokio::test]
async fn no_checks_without_writing() {
    let line = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem --validate --inspect-otp";
    commission_then_check(&mut blank_board(), line, "n\n").await;
    println!();
    let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
    commission_then_check(&mut otp, line, "").await;
}

/// An M fire-40-b commissioned by Acme Retro with its key file, which is
/// encrypted with a PIN. A real run answered yes.
#[tokio::test]
async fn a_manufacturers_encrypted_key_file() {
    let words = [
        "onerom",
        "hardware",
        "commission",
        "--board",
        "fire-40-b",
        "--size",
        "M",
        "--manufacturer",
        "Acme Retro",
        "--key",
        "acme.pem",
    ];
    println!("$ {}", shell_line(&words));
    let signing = acme_signing();
    let (args, options) = args_of(&words);
    let table = table(None);
    let signer = signer_for(&table, &signing, &args.manufacturer)
        .await
        .unwrap();

    let mut otp = blank_board();
    println!("~ {}", device(args.board.name(), size_of(&mut otp).await));
    let mut screen = Screen::default();
    let mut keyboard = Keyboard::new("y\n", &screen);
    let result = commission_otp(
        &mut otp,
        &args,
        &signing,
        (signer, &table),
        &options,
        &mut screen,
        &mut keyboard,
    )
    .await;
    print!("{}", screen.take());
    if let Err(e) = result {
        failed(e);
    }
}

/// Command lines refused before the CLI looks for a device.
#[test]
fn refused_command_lines() {
    let signature = signature_hex("fire-24-f", "piers.rocks", "20260101", 1);
    let commission = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks";
    refused_lines(&[
        "onerom hardware commission --board fire-99-z --manufacturer piers.rocks --key key.pem".to_string(),
        "onerom hardware commission --board ice-24-d --manufacturer piers.rocks --key key.pem".to_string(),
        "onerom hardware commission --board fire-40-a --size XL --manufacturer piers.rocks --key key.pem".to_string(),
        "onerom hardware commission --board fire-40-a --size S --manufacturer piers.rocks --key key.pem".to_string(),
        "onerom hardware commission --board fire-40-a --manufacturer piers.rocks --key key.pem".to_string(),
        "onerom hardware commission --board fire-24-f --size L --manufacturer piers.rocks --key key.pem".to_string(),
        format!("{commission} --key key.pem --date 2026-01-01"),
        format!("{commission} --key key.pem --date 20260230"),
        format!("{commission} --key key.pem --date 20991231"),
        format!("{commission} --key key.pem --date 20991231 --force"),
        format!("{commission} --signer http://HOST"),
        format!("{commission} --signer http://HOST --key-id 1"),
        commission.to_string(),
        format!("{commission} --pin 1234"),
        format!("{commission} --key key.pem --signer https://HOST --key-id 1"),
        format!("{commission} --signer https://HOST"),
        format!("{commission} --key-id 1"),
        format!("{commission} --key key.pem --key-id 1"),
        format!("{commission} --signer https://HOST --key-id 0"),
        format!("{commission} --signer https://HOST --key-id 65536"),
        format!("{commission} --signer https://HOST --key-id 1 --key key.pem"),
        format!("{commission} --signature {signature} --date 20260101"),
        format!("{commission} --key-id 1 --signature {signature}"),
        format!("{commission} --key-id 1 --signature {signature} --date 20260101 --pin 1234"),
        format!(
            "{commission} --signer https://HOST --key-id 1 --signature {signature} --date 20260101"
        ),
        format!("{commission} --key key.pem --signature {signature} --date 20260101"),
        format!(
            "{commission} --key-id 1 --signature {} --date 20260101",
            &signature[..126]
        ),
        format!(
            "{commission} --key-id 1 --signature {}xy --date 20260101",
            &signature[..126]
        ),
        format!("{commission} --key key.pem --dry-run --validate"),
        format!("{commission} --key key.pem --dry-run --inspect-otp"),
    ]);
    let words: Vec<&str> = commission.split_whitespace().collect();
    for manufacturer in ["", "piers\u{1b}rocks"] {
        let mut line = words[..words.len() - 1].to_vec();
        line.extend([manufacturer, "--key", "key.pem"]);
        refused_line(&line);
        println!();
    }
    refused_line(&[&words[..], &["--key", "key.pem", "--pin", ""]].concat());
}

/// From `openssl genpkey -algorithm ed25519 | openssl pkcs8 -topk8 -scrypt
/// -passout pass:test`.
const ENCRYPTED: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----
MIGbMFcGCSqGSIb3DQEFDTBKMCkGCSsGAQQB2kcECzAcBBCygZFSouxgAoz03h2W
pWvSAgJAAAIBCAIBATAdBglghkgBZQMEASoEEI5DlJCcGZ+k1qjMlzDf9qIEQESu
mDfuv/uA/EVN4DQZajeb6QaS3jYuKexSVklfDT0DMynvVcfZ5BTWXojk99Ph/N2o
Mx/cjBQLObAI6LuJncI=
-----END ENCRYPTED PRIVATE KEY-----
";

/// From `openssl genpkey -algorithm ed25519 | openssl pkcs8 -topk8 -v1
/// PBE-MD5-DES -passout pass:test -provider legacy -provider default`.
const PBES1: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----
MFcwGwYJKoZIhvcNAQUDMA4ECDOcgymcx8QvAgIIAAQ4Gf4s+PWJj+uviCpH2fEf
elx0zT7/O11DuJzzHIqInF2PhTr4nNIWsbXT7oe3ZQj5Li678VcAnxo=
-----END ENCRYPTED PRIVATE KEY-----
";

/// From `openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256`.
const P256: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgSdnk/0fIpjdrJrqr
SlmbDWYPwkDuw0eu2dVahTAFkDahRANCAAQygyWZl7meGBtGp6jwBmgf/YixAvEq
iMcga2y1OrfxMbb+71DBh9AOyszeZljXjWv/ywweOfVl3rsEdSIEHNPO
-----END PRIVATE KEY-----
";

/// Prints the transcript of `line`. Its signing options are refused before the
/// One ROM is stopped. A word ending `.pem` is a key file in `files`. `files`
/// pairs names with contents. `""` is an empty word.
fn refused_signing(line: &str, files: &[(&str, &str)]) {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let words: Vec<String> = line
        .split_whitespace()
        .map(|word| {
            if word.ends_with(".pem") {
                dir.path().join(word).display().to_string()
            } else if word == "\"\"" {
                String::new()
            } else {
                word.to_string()
            }
        })
        .collect();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let (args, _) = args_of(&words);
    println!("$ {line}");
    let prefix = format!("{}/", dir.path().display());
    match signing(&args) {
        Ok(_) => println!("~ accepted"),
        Err(e) => failed(e.to_string().replace(&prefix, "")),
    }
}

/// Signing options refused before the One ROM is stopped. Then keys the table
/// of signing keys refuses.
#[test]
fn refused_keys() {
    use ed25519_dalek::pkcs8::EncodePrivateKey as _;
    let line = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks";
    let pem = key()
        .to_pkcs8_pem(ed25519_dalek::pkcs8::spki::der::pem::LineEnding::LF)
        .unwrap();
    for (options, files) in [
        ("--key missing.pem", vec![]),
        ("--key notes.pem", vec![("notes.pem", "some notes\n")]),
        ("--key p256.pem", vec![("p256.pem", P256)]),
        ("--key key.pem --pin 1234", vec![("key.pem", pem.as_str())]),
        (
            "--key encrypted.pem --pin wrong",
            vec![("encrypted.pem", ENCRYPTED)],
        ),
        ("--key pbes1.pem --pin test", vec![("pbes1.pem", PBES1)]),
    ] {
        refused_signing(&format!("{line} {options}"), &files);
        println!();
    }

    let other = ed25519_dalek::SigningKey::from_bytes(&[2; 32]);
    for (private, table) in [
        (&other, table(None)),
        (&key(), table(Some(serde_json::json!({})))),
    ] {
        println!("$ {line} --key key.pem");
        match signing_key(&table, &private.verifying_key().to_bytes()) {
            Ok(_) => println!("~ accepted"),
            Err(e) => failed(e),
        }
        println!();
    }
}

/// Signing options that need a PIN, without `--pin` and without a terminal to
/// ask for it: a signing server's key, then an encrypted key file.
///
/// Ignored because the CLI asks the terminal for the PIN where stdin is one,
/// and a test's stdin is the terminal `cargo test` runs in. Run it with stdin
/// from /dev/null:
///
///   cargo test -p onerom-cli --bin onerom review::commission::no_pin -- --ignored --nocapture < /dev/null
#[tokio::test]
#[ignore]
async fn no_pin_without_a_terminal() {
    assert!(
        !std::io::stdin().is_terminal(),
        "run with stdin from /dev/null so the CLI doesn't ask for a PIN"
    );
    let line = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks";
    let signer = format!("{line} --signer {SIGNER} --key-id 1");
    let (args, _) = args_of(&signer.split_whitespace().collect::<Vec<_>>());
    println!("$ {signer}");
    match signing(&args) {
        Ok(_) => println!("~ accepted"),
        Err(e) => failed(e),
    }
    println!();
    refused_signing(
        &format!("{line} --key encrypted.pem"),
        &[("encrypted.pem", ENCRYPTED)],
    );
}

/// A signature that doesn't verify, then a recorded signature that differs
/// from the one shown.
#[tokio::test]
async fn refused_signatures() {
    let line = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --signer https://HOST --key-id 1 --pin 1234 --date 20260101";
    let mut otp = blank_board();
    commission_signed(&mut otp, line, "", &Server::another_key()).await;
    println!();
    let differs = Server {
        records_another: true,
        ..Server::test_key()
    };
    commission_signed(&mut otp, line, "y\n", &differs).await;
}

/// Firmware for another board, then with `--force`. The CLI refuses before it
/// prints the device line. With `--force` it prints the warning and goes on.
#[tokio::test]
async fn firmware_for_another_board() {
    let board = |name| Board::try_from_str(name).unwrap();
    let line = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem --date 20260101";
    println!("$ {line}");
    let error = check_firmware(
        OtpCommand::Commission,
        Some(board("fire-24-e")),
        board("fire-24-f"),
        false,
    )
    .unwrap_err();
    failed(error);
    println!();

    println!("$ {line} --force --yes");
    println!(
        "~ {}",
        firmware_warning(
            OtpCommand::Commission,
            board("fire-24-e"),
            board("fire-24-f")
        )
    );
    let mut otp = blank_board();
    println!("~ {}", device("fire-24-e", size_of(&mut otp).await));
    let (_dir, key) = key_file();
    let signing = Signing::File(KeyFile::read(&key, None).unwrap());
    let (args, options) = commission_args(&format!("{line} --force --yes"), &key);
    let table = table(None);
    let mut out = Vec::new();
    let result = commission_otp(
        &mut otp,
        &args,
        &signing,
        (table.get(1).unwrap(), &table),
        &options,
        &mut out,
        &mut std::io::empty(),
    )
    .await;
    print!("{}", String::from_utf8(out).unwrap());
    if let Err(e) = result {
        failed(e);
    }
}

/// Data from a newer version. First an area holding version 2, then an
/// instance holding an unknown key. Each without and with `--force`.
#[tokio::test]
async fn newer_data() {
    let line = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem --date 20260101";
    for (name, mut otp) in newer_boards() {
        println!("### {name}");
        commission(&mut otp, line, "").await;
        commission(&mut otp, &format!("{line} --force"), "").await;
        println!();
    }
}

/// OTP holding something that stops commissioning, a case each.
#[tokio::test]
async fn refused_otp() {
    let m = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem --date 20260101 --dry-run";
    let l = "onerom hardware commission --board fire-40-a --size L --manufacturer piers.rocks --key key.pem --date 20260101 --dry-run";
    type Case = (&'static str, &'static str, fn(&mut MemoryOtp));
    let cases: [Case; 11] = [
        ("a bootloader string's row holds another value", m, |otp| {
            otp.set_raw(0xed0, ecc_encode(0x1234))
        }),
        ("USB_WHITE_LABEL_ADDR holds another value", m, |otp| {
            otp.set_raw(0x05c, ecc_encode(0x0100))
        }),
        ("a USB_BOOT_FLAGS copy holds other flags", m, |otp| {
            otp.set_raw(0x05a, 0x000001)
        }),
        ("page 3's lock word holds another lock", m, |otp| {
            otp.set_raw(0xf87, 0x000010)
        }),
        ("page 3 is locked", m, |otp| {
            otp.set_raw(0xf87, 0x151515);
            otp.reset();
        }),
        ("page 3 can't be read", m, |otp| {
            otp.set_raw(0xf87, 0x303030);
            otp.reset();
        }),
        ("an M board with FLASH_DEVINFO written", m, |otp| {
            otp.set_raw(0x054, ecc_encode(0x99af))
        }),
        ("an M board with FLASH_DEVINFO enabled", m, |otp| {
            otp.set_raw(0x054, ecc_encode(0x99af));
            for row in 0x048..=0x04a {
                otp.set_raw(row, 0x000020);
            }
        }),
        (
            "an L board whose FLASH_DEVINFO holds another value",
            l,
            |otp| otp.set_raw(0x054, ecc_encode(0x0900)),
        ),
        (
            "an L board with a bad FLASH_PARTITION_SLOT_SIZE",
            l,
            |otp| otp.set_raw(0x055, 0x000001),
        ),
        ("the manufacturer's name is too long", "", |_| {}),
    ];
    for (name, line, set_up) in cases {
        println!("### {name}");
        let mut otp = blank_board();
        set_up(&mut otp);
        if line.is_empty() {
            let long = "x".repeat(2100);
            let line = format!(
                "onerom hardware commission --board fire-24-f --manufacturer {long} --key key.pem --date 20260101"
            );
            commission(&mut otp, &line, "").await;
        } else {
            commission(&mut otp, line, "").await;
        }
        println!();
    }

    println!("### an M board with FLASH_DEVINFO enabled, with --force");
    let mut otp = blank_board();
    otp.set_raw(0x054, ecc_encode(0x99af));
    for row in 0x048..=0x04a {
        otp.set_raw(row, 0x000020);
    }
    commission(&mut otp, &format!("{m} --force"), "").await;
    println!();

    println!("### the commissioning area full");
    let mut otp = blank_board();
    for day in 1..=16 {
        commission_quietly(&mut otp, &format!("202601{day:02}"), day > 1)
            .await
            .unwrap();
    }
    commission(
        &mut otp,
        "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem --date 20260117 --force",
        "",
    )
    .await;
}

/// Runs that stop part way, each followed by a second run.
#[tokio::test]
async fn part_way() {
    let line = "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --key key.pem --date 20260101 --yes";
    println!("### a row of the commissioning instance reads back wrong");
    let mut otp = blank_board();
    otp.corrupt(5, 0x000100);
    commission(&mut otp, line, "").await;
    commission(&mut otp, line, "").await;
    println!();

    println!("### a bootloader USB string's row reads back wrong");
    let mut otp = blank_board();
    otp.corrupt(70, 0x000100);
    commission(&mut otp, line, "").await;
    commission(&mut otp, line, "").await;
    println!();

    println!("### the bootloader USB strings' first page is locked");
    let mut otp = blank_board();
    otp.set_raw(0xf81 + 2 * 59, 0x151515);
    otp.reset();
    commission(&mut otp, line, "").await;
    commission(&mut otp, line, "").await;
}

// ---------------------------------------------------------------------------
// Signing through a signing server
// ---------------------------------------------------------------------------

/// A run signing through key 1 on [`SIGNER`], with `--pin` where `pin`.
fn server_line(pin: bool) -> String {
    let line = format!(
        "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --signer {SIGNER} --key-id 1"
    );
    if pin {
        format!("{line} --pin 1234")
    } else {
        line
    }
}

/// A signing server holding the test key. One request fails with the error
/// `error` makes of what it's asked to sign.
struct FailingServer {
    /// Whether the recorded signature's request fails rather than the dry
    /// run's.
    at_record: bool,
    error: Box<dyn Fn(&ToSign<'_>) -> Error>,
}

impl SignatureSource for FailingServer {
    async fn dry_run(&self, to_sign: &ToSign<'_>) -> Result<[u8; 64], Error> {
        use ed25519_dalek::Signer as _;
        if self.at_record {
            Ok(key().sign(to_sign.message).to_bytes())
        } else {
            Err((self.error)(to_sign))
        }
    }

    async fn record(&self, to_sign: &ToSign<'_>) -> Result<Option<[u8; 64]>, Error> {
        use ed25519_dalek::Signer as _;
        if self.at_record {
            Err((self.error)(to_sign))
        } else {
            Ok(Some(key().sign(to_sign.message).to_bytes()))
        }
    }
}

/// The error the CLI makes of the server replying with HTTP status `status`.
/// The server's `text` makes the body a line holding `message`, and
/// `decode_reply` makes the error.
fn replied(status: u16, message: &str) -> Error {
    let body = format!("{message}\n");
    Error::SigningServer {
        url: key_1(),
        status,
        message: escape_controls(body.trim()),
    }
}

/// A dry-run `/sign` request's body, as the CLI's `SignRequest` serializes it.
#[derive(Serialize)]
struct DryRunBody<'a> {
    chip_id: String,
    board: &'a str,
    manufacturer: &'a str,
    date: &'a str,
    dry_run: bool,
}

/// A `/sign` request's body as a server from before the dry-run option reads
/// it. It refuses a field it doesn't know.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct OldSignRequest {
    chip_id: String,
    board: String,
    manufacturer: String,
    date: String,
}

/// The error a server from before the dry-run option refuses the dry run of
/// `to_sign` with.
fn too_old(to_sign: &ToSign<'_>) -> Error {
    let body = serde_json::to_vec(&DryRunBody {
        chip_id: format_chip_id(to_sign.chip_id),
        board: to_sign.board.name(),
        manufacturer: to_sign.manufacturer,
        date: to_sign.date,
        dry_run: true,
    })
    .unwrap();
    let error = serde_json::from_slice::<OldSignRequest>(&body)
        .err()
        .unwrap();
    replied(400, &format!("the request isn't valid: {error}"))
}

/// Prints the transcript of a run with `--pin` that fails with `error`
/// fetching the public key. The CLI fetches it before it stops the One ROM and
/// shows the device line. It's written from the code.
fn public_key_fails(error: &Error) {
    println!("$ {}", server_line(true));
    written_failure(error);
}

/// Signing through a signing server, and each way the server can fail.
#[tokio::test]
async fn signing_server() {
    let with_pin = server_line(true);
    let without_pin = server_line(false);
    let fails_dry_run = |error: fn(&ToSign<'_>) -> Error| FailingServer {
        at_record: false,
        error: Box::new(error),
    };

    println!("### a full run with --pin, answered y");
    let server = Server::test_key();
    commission_signed(&mut blank_board(), &with_pin, "y\n", &server).await;
    println!();

    println!("### the PIN asked for at the terminal");
    println!("$ {without_pin}");
    pin_prompt("signing key #1");
    run_signed(&mut blank_board(), &without_pin, "", &server, Some(6)).await;
    println!("~ (continues as in the full run)");
    println!();

    println!("### Escape or Ctrl-C at the PIN prompt");
    println!("$ {without_pin}");
    pin_prompt("signing key #1");
    written_failure(&Error::Aborted("The PIN wasn't entered".to_string()));
    println!();

    println!("### an empty PIN entered at the prompt");
    println!("$ {without_pin}");
    pin_prompt("signing key #1");
    written_failure(&Error::Aborted("The PIN wasn't entered".to_string()));
    println!();

    println!(
        "### the server refuses the PIN (401). Fetching the public key doesn't check the PIN so the dry-run sign is refused"
    );
    let server = fails_dry_run(|_| replied(401, "the PIN is incorrect"));
    commission_signed(&mut blank_board(), &with_pin, "", &server).await;
    println!();

    println!("### the server doesn't have key 1 (404)");
    public_key_fails(&replied(404, "the key doesn't exist"));
    println!();

    println!("### the server refuses the request (400) because it's too old to know dry_run");
    let server = fails_dry_run(too_old);
    commission_signed(&mut blank_board(), &with_pin, "", &server).await;
    println!();

    println!("### the server refuses the request (400) because it doesn't know the board");
    let server = fails_dry_run(|to_sign| {
        let message = format!("unknown board {}", to_sign.board.name());
        replied(400, &message)
    });
    commission_signed(&mut blank_board(), &with_pin, "", &server).await;
    println!();

    println!(
        "### the server can't record the signature (503), here because its record file holds a bad line"
    );
    let server = FailingServer {
        at_record: true,
        error: Box::new(|_| {
            replied(
                503,
                "the record can't be written, so the signature isn't returned: line 3 of signatures/1.txt isn't a record line",
            )
        }),
    };
    commission_signed(&mut blank_board(), &with_pin, "y\n", &server).await;
    println!();

    println!("### the server can't be reached");
    public_key_fails(&Error::SigningServerUnreachable(key_1()));
    println!();

    println!("### the public key's reply has the wrong number of bytes");
    public_key_fails(&Error::SigningServerReply {
        url: key_1(),
        len: 615,
        expected: 32,
    });
    println!();

    println!("### the dry-run signature's reply has the wrong number of bytes");
    let server = fails_dry_run(|_| Error::SigningServerReply {
        url: key_1(),
        len: 0,
        expected: 64,
    });
    commission_signed(&mut blank_board(), &with_pin, "", &server).await;
    println!();

    println!("### the server replies with another error status (500)");
    let server = fails_dry_run(|_| replied(500, "the key's files don't match"));
    commission_signed(&mut blank_board(), &with_pin, "", &server).await;
}

/// `--signer` and `--key-id`, as far as it runs without a network. The PIN
/// prompt, a full run against a server holding the test key, a key ID the
/// table doesn't contain, a retired key and a server whose key 1 isn't the
/// table's.
#[tokio::test]
async fn signing_server_key_ids() {
    let line = |id: u16| {
        format!(
            "onerom hardware commission --board fire-24-f --manufacturer piers.rocks --date 20260101 --signer {SIGNER} --key-id {id} --pin 1234"
        )
    };
    let table = table(None);

    println!("### the PIN asked for at the terminal");
    println!("$ {}", line(1).replace(" --pin 1234", ""));
    pin_prompt("signing key #1");
    println!("~ (continues as below)");
    println!();

    println!("### a full run with --pin, answered y, against a server holding the test key");
    // The fake server's public key is the test key, the table's key 1.
    println!("$ {}", line(1));
    run_signed(
        &mut blank_board(),
        &line(1),
        "y\n",
        &Server::test_key(),
        None,
    )
    .await;
    println!();

    println!("### key ID 7, which the table doesn't contain");
    commission_with_table(&mut blank_board(), &line(7), "", &table, None).await;
    println!();

    println!("### key 1 retired");
    let retired = crate::test_board::table(Some(serde_json::json!({})));
    commission_with_table(&mut blank_board(), &line(1), "", &retired, None).await;
    println!();

    println!("### the server's key 1 isn't the table's key 1 (written from the code)");
    println!("$ {}", line(1));
    let other = ed25519_dalek::SigningKey::from_bytes(&[2; 32])
        .verifying_key()
        .to_bytes();
    written_failure(&check_server_key(&table, 1, &other, SIGNER).unwrap_err());
}

/// `--signature`. A good signature answered y and then run again. Then one
/// from another key, one for another date, a key ID the table doesn't contain
/// and a retired key.
#[tokio::test]
async fn with_a_signature() {
    let line = |signature: &str, id: u16| {
        format!(
            "onerom hardware commission --board fire-24-f --size M --manufacturer piers.rocks --date 20260101 --key-id {id} --signature {signature}"
        )
    };
    let good = signature_hex("fire-24-f", "piers.rocks", "20260101", 1);
    let table = table(None);

    println!("### a signature from key 1");
    let mut otp = blank_board();
    commission_with_table(&mut otp, &line(&good, 1), "y\n", &table, None).await;
    println!();
    println!("### the same command again");
    commission_with_table(&mut otp, &line(&good, 1), "", &table, None).await;
    println!();

    println!("### a signature from another key");
    let another = ed25519_dalek::SigningKey::from_bytes(&[2; 32]);
    let other = signature_with(&another, "fire-24-f", "piers.rocks", "20260101", 1);
    commission_with_table(&mut blank_board(), &line(&other, 1), "", &table, None).await;
    println!();

    println!("### key 1's signature for 20260102");
    let dated = signature_hex("fire-24-f", "piers.rocks", "20260102", 1);
    commission_with_table(&mut blank_board(), &line(&dated, 1), "", &table, None).await;
    println!();

    println!("### key ID 7, which the table doesn't contain");
    commission_with_table(&mut blank_board(), &line(&good, 7), "", &table, None).await;
    println!();

    println!("### key 1 retired");
    let retired = crate::test_board::table(Some(serde_json::json!({})));
    commission_with_table(&mut blank_board(), &line(&good, 1), "", &retired, None).await;
}

/// A manufacturer key 1 doesn't allow, with a key file, a signing server and
/// `--signature`. Key 1 may sign only onerom.org.
#[tokio::test]
async fn a_manufacturer_the_key_doesnt_allow() {
    let table = table_allowing(&["onerom.org"]);
    let line = "onerom hardware commission --board fire-24-f --size M --manufacturer piers.rocks --date 20260101";
    let signature = signature_hex("fire-24-f", "piers.rocks", "20260101", 1);

    println!("### a key file");
    let key_file = format!("{line} --key key.pem");
    commission_with_table(&mut blank_board(), &key_file, "", &table, None).await;
    println!();

    println!("### a signing server");
    let server = format!("{line} --signer {SIGNER} --key-id 1 --pin 1234");
    commission_with_table(&mut blank_board(), &server, "", &table, None).await;
    println!();

    println!("### --signature");
    let given = format!("{line} --key-id 1 --signature {signature}");
    commission_with_table(&mut blank_board(), &given, "", &table, None).await;
}
