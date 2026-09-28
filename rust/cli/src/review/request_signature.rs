// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `hardware request-signature`.

use onerom_app::{BoardSize, MemoryOtp, Request, RequestDate, prepare};
use onerom_config::hw::Board;
use onerom_metadata::otp::pico_otp::ecc_encode;

use super::{board_with_an_unknown_key, device, failed, hardware_of, put, refused_lines, size_of};
use crate::args::hardware::HardwareCommands;
use crate::hardware::{OtpCommand, check_firmware, request_signature_otp};
use crate::signing_request::{KEY_ID, MANUFACTURER};
use crate::test_board::{blank_board, commissioned_board, key};

/// Prints the transcript of `hardware request-signature` with `options` on
/// `otp`. The device line is for firmware for the board in `options`.
async fn request_run(otp: &mut MemoryOtp, options: &str) {
    let line = format!("onerom hardware request-signature {options}");
    println!("$ {line}");
    let (command, _) = hardware_of(&line.split_whitespace().collect::<Vec<_>>());
    let HardwareCommands::RequestSignature(args) = command else {
        panic!("not hardware request-signature");
    };
    println!("~ {}", device(args.board.name(), size_of(otp).await));
    let mut out = Vec::new();
    let result = request_signature_otp(otp, &args, &mut out).await;
    print!("{}", String::from_utf8(out).unwrap());
    if let Err(e) = result {
        failed(e);
    }
}

#[test]
fn help() {
    super::help(&["onerom", "hardware", "request-signature", "--help"]);
}

/// Command lines refused before the CLI looks for a device.
#[test]
fn refused_command_lines() {
    let request = "onerom hardware request-signature";
    refused_lines(&[
        request.to_string(),
        format!("{request} --board fire-40-a"),
        format!("{request} --board fire-24-f --size L"),
        format!("{request} --board ice-24-d"),
        format!("{request} --board fire-24-f --manufacturer onerom.org"),
        format!("{request} --board fire-24-f --force"),
    ]);
}

/// A blank fire-24-f, then a blank fire-40-a as L with `-b`.
#[tokio::test]
async fn a_blank_board() {
    request_run(&mut blank_board(), "--board fire-24-f").await;
    println!();
    request_run(&mut blank_board(), "-b fire-40-a --size L").await;
}

/// Firmware for another board. It's written from the code.
#[test]
fn firmware_for_another_board() {
    let board = |name| Board::try_from_str(name).unwrap();
    println!("$ onerom hardware request-signature --board fire-40-a");
    let error = check_firmware(
        OtpCommand::RequestSignature,
        Some(board("fire-40-b")),
        board("fire-40-a"),
        false,
    )
    .unwrap_err();
    failed(error);
}

/// OTP holding something that stops `hardware commission`, a case each.
#[tokio::test]
async fn refused_otp() {
    use ed25519_dalek::Signer as _;

    println!("### commissioned with the request's values on 20260101");
    let mut otp = blank_board();
    let request = Request {
        board: Board::try_from_str("fire-24-f").unwrap(),
        size: BoardSize::M,
        manufacturer: MANUFACTURER.to_string(),
        date: RequestDate::Given("20260101".to_string()),
        signer: KEY_ID,
        force: false,
    };
    let prepared = prepare(&mut otp, &request).await.unwrap();
    let signature = key().sign(prepared.message()).to_bytes();
    let plan = prepared.plan(&signature).unwrap();
    plan.execute(&mut otp, |_| {}).await.unwrap();
    request_run(&mut otp, "--board fire-24-f").await;
    println!();

    type Case = (&'static str, &'static str, fn(&mut MemoryOtp));
    let cases: [Case; 8] = [
        (
            "commissioned by piers.rocks with key 1",
            "--board fire-24-f",
            |_| {},
        ),
        ("an area containing version 2", "--board fire-24-f", |otp| {
            put(otp, 0x0c0, &[onerom_metadata::OTP_STORE_MAGIC, 2])
        }),
        (
            "a fire-24-f with FLASH_DEVINFO written",
            "--board fire-24-f",
            |otp| otp.set_raw(0x054, ecc_encode(0x99af)),
        ),
        (
            "a fire-40-a with FLASH_DEVINFO written, as M",
            "--board fire-40-a --size M",
            |otp| otp.set_raw(0x054, ecc_encode(0x99af)),
        ),
        ("an L board, as M", "--board fire-40-a --size M", |otp| {
            otp.set_raw(0x054, ecc_encode(0x99af));
            for row in 0x048..=0x04a {
                otp.set_raw(row, 0x000020);
            }
        }),
        ("page 59 locked", "--board fire-24-f", |otp| {
            otp.set_raw(0xf81 + 2 * 59, 0x151515);
            otp.reset();
        }),
        (
            "USB_WHITE_LABEL_ADDR contains another value",
            "--board fire-24-f",
            |otp| otp.set_raw(0x05c, ecc_encode(0x0100)),
        ),
        (
            "L with a bad FLASH_PARTITION_SLOT_SIZE",
            "--board fire-40-a --size L",
            |otp| otp.set_raw(0x055, 0x000001),
        ),
    ];
    for (name, options, set_up) in cases {
        println!("### {name}");
        let mut otp = if name.starts_with("commissioned") {
            commissioned_board("fire-24-f", BoardSize::M).await
        } else {
            blank_board()
        };
        set_up(&mut otp);
        request_run(&mut otp, options).await;
        println!();
    }
    println!("### an instance containing key 16");
    request_run(&mut board_with_an_unknown_key(), "--board fire-24-f").await;
}
