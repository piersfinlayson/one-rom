// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `hardware set-size`.

use onerom_app::{BoardSize, Interruption, MemoryOtp, plan_size};
use onerom_cli::Options;
use onerom_config::hw::Board;
use onerom_metadata::otp::pico_otp::ecc_encode;

use super::{Keyboard, Screen, device, failed, hardware_of, put, refused_lines, size_of};
use crate::args::hardware::{HardwareCommands, HardwareSetSizeArgs};
use crate::hardware::{OtpCommand, check_firmware, firmware_warning, set_size_otp};
use crate::test_board::{blank_board, commissioned_board};

/// Prints the transcript of `line`, which starts `onerom hardware set-size`,
/// on `otp`. `typed` is what the user types. The device line is for firmware
/// for the board in `line`.
pub(super) async fn set_size(otp: &mut MemoryOtp, line: &str, typed: &str) {
    println!("$ {line}");
    let (args, options) = set_size_args(&line.split_whitespace().collect::<Vec<_>>());
    // A size the CLI can't read adds nothing to the device line.
    let size = match onerom_app::read_board_size(otp).await {
        Ok(_) => size_of(otp).await,
        Err(_) => Some(BoardSize::M),
    };
    println!("~ {}", device(args.board.name(), size));
    run_set_size(otp, &args, &options, typed).await;
}

/// `words`, which start `onerom hardware set-size`, parsed and checked as the
/// CLI does. Returns the arguments and the options `--yes` and `--verbose`
/// set.
fn set_size_args(words: &[&str]) -> (HardwareSetSizeArgs, Options) {
    let (command, options) = hardware_of(words);
    let HardwareCommands::SetSize(args) = command else {
        panic!("not hardware set-size");
    };
    (args, options)
}

/// Prints what `hardware set-size` prints on `otp` after the device line.
/// `typed` is what the user types.
async fn run_set_size(
    otp: &mut MemoryOtp,
    args: &HardwareSetSizeArgs,
    options: &Options,
    typed: &str,
) {
    let mut screen = Screen::default();
    let mut keyboard = Keyboard::new(typed, &screen);
    let result = set_size_otp(otp, args, options, &mut screen, &mut keyboard).await;
    print!("{}", screen.take());
    if let Err(e) = result {
        failed(e);
    }
}

/// A fire-40-a set to L, printing nothing.
async fn l_board() -> MemoryOtp {
    let mut otp = blank_board();
    let board = Board::try_from_str("fire-40-a").unwrap();
    let plan = plan_size(&mut otp, board, BoardSize::L).await.unwrap();
    plan.execute(&mut otp, |_| {}).await.unwrap();
    otp
}

#[test]
fn help() {
    super::help(&["onerom", "hardware", "set-size", "--help"]);
}

/// M on a blank fire-40-a.
#[tokio::test]
async fn m_on_a_blank_board() {
    let line = "onerom hardware set-size --board fire-40-a --size M";
    set_size(&mut blank_board(), line, "").await;
}

/// L on a blank fire-40-a, without and with `--verbose`, each answered n.
/// Then with `--dry-run`, then answered y.
#[tokio::test]
async fn l_on_a_blank_board() {
    let l = "onerom hardware set-size --board fire-40-a --size L";
    let mut otp = blank_board();
    set_size(&mut otp, l, "n\n").await;
    println!();
    set_size(&mut otp, &format!("{l} --verbose"), "n\n").await;
    println!();
    set_size(&mut otp, &format!("{l} --dry-run"), "").await;
    println!();
    set_size(&mut otp, l, "y\n").await;
}

/// L on a board that's already L, without and with `--verbose`.
#[tokio::test]
async fn l_on_an_l_board() {
    let l = "onerom hardware set-size --board fire-40-a --size L";
    let mut otp = l_board().await;
    set_size(&mut otp, l, "").await;
    println!();
    set_size(&mut otp, &format!("{l} --verbose"), "").await;
}

/// L with `--yes` on a blank board.
#[tokio::test]
async fn l_with_yes() {
    let line = "onerom hardware set-size --board fire-40-a --size L --yes";
    set_size(&mut blank_board(), line, "").await;
}

/// An L run that stops after FLASH_DEVINFO because the connection is lost,
/// then the same command again.
#[tokio::test]
async fn again_after_an_interruption() {
    let line = "onerom hardware set-size --board fire-40-a --size L --yes";
    let mut otp = blank_board();
    otp.interrupt(2, Interruption::NotLanded);
    set_size(&mut otp, line, "").await;
    otp.reset();
    println!();
    set_size(&mut otp, line, "").await;
}

/// Command lines refused before the CLI looks for a device.
#[test]
fn refused_command_lines() {
    refused_lines(&[
        "onerom hardware set-size --board fire-40-a",
        "onerom hardware set-size --size L",
        "onerom hardware set-size --board fire-40-a --size XL",
        "onerom hardware set-size --board fire-40-a --size S",
        "onerom hardware set-size --board fire-24-f --size L",
        "onerom hardware set-size --board ice-24-d --size M",
        "onerom hardware set-size --board fire-99-z --size M",
    ]);
}

/// OTP containing something that stops set-size, a case each. The M cases
/// with FLASH_DEVINFO written on a board OTP still configures as M go ahead.
#[tokio::test]
async fn refused_otp() {
    let m = "onerom hardware set-size --board fire-40-a --size M";
    let l = "onerom hardware set-size --board fire-40-a --size L";
    let m_24 = "onerom hardware set-size --board fire-24-f --size M";
    // FLASH_DEVINFO with 4MB on chip select 1, enabled by every BOOT_FLAGS0
    // copy.
    fn neither_m_nor_l(otp: &mut MemoryOtp) {
        otp.set_raw(0x054, ecc_encode(0xa9af));
        for row in 0x048..=0x04a {
            otp.set_raw(row, 0x000020);
        }
    }
    type Case = (&'static str, &'static str, fn(&mut MemoryOtp));
    let cases: [Case; 14] = [
        ("a commissioning area containing version 2", l, |otp| {
            put(otp, 0x0c0, &[onerom_metadata::OTP_STORE_MAGIC, 2])
        }),
        ("M on an L board", m, |otp| {
            otp.set_raw(0x054, ecc_encode(0x99af));
            for row in 0x048..=0x04a {
                otp.set_raw(row, 0x000020);
            }
        }),
        ("M on a board neither M nor L", m, neither_m_nor_l),
        ("L on a board neither M nor L", l, neither_m_nor_l),
        ("M on a fire-40-a with FLASH_DEVINFO written", m, |otp| {
            otp.set_raw(0x054, ecc_encode(0x99af))
        }),
        (
            "M on a fire-40-a with one BOOT_FLAGS0 copy enabling FLASH_DEVINFO",
            m,
            |otp| otp.set_raw(0x04a, 0x000020),
        ),
        ("M on a fire-24-f with FLASH_DEVINFO written", m_24, |otp| {
            otp.set_raw(0x054, ecc_encode(0x99af))
        }),
        ("L with a bad FLASH_PARTITION_SLOT_SIZE", l, |otp| {
            otp.set_raw(0x055, 0x000100)
        }),
        ("L with FLASH_DEVINFO containing another value", l, |otp| {
            otp.set_raw(0x054, ecc_encode(0x99a0))
        }),
        (
            "L with FLASH_DEVINFO containing another value with 2MB on chip select 1, enabled",
            l,
            |otp| {
                otp.set_raw(0x054, ecc_encode(0x99a0));
                for row in 0x048..=0x04a {
                    otp.set_raw(row, 0x000020);
                }
            },
        ),
        ("L with page 1 locked", l, |otp| {
            otp.set_raw(0xf81 + 2, 0x151515);
            otp.reset();
        }),
        ("L with page 1 locked on a board already L", l, |otp| {
            otp.set_raw(0x054, ecc_encode(0x99af));
            for row in 0x048..=0x04a {
                otp.set_raw(row, 0x000020);
            }
            otp.set_raw(0xf81 + 2, 0x151515);
            otp.reset();
        }),
        ("L with page 1 unreadable", l, |otp| {
            otp.set_raw(0xf81 + 2, 0x303030);
            otp.reset();
        }),
        (
            "L on a board whose last complete instance is for fire-24-f",
            l,
            |_| {},
        ),
    ];
    for (name, line, set_up) in cases {
        println!("### {name}");
        let mut otp = if name.ends_with("fire-24-f") {
            commissioned_board("fire-24-f", BoardSize::M).await
        } else {
            blank_board()
        };
        set_up(&mut otp);
        set_size(&mut otp, line, "").await;
        println!();
    }
}

/// FLASH_DEVINFO reads back wrong once written, then the same command again.
#[tokio::test]
async fn part_way() {
    let line = "onerom hardware set-size --board fire-40-a --size L --yes";
    let mut otp = blank_board();
    otp.corrupt(1, 0x000100);
    set_size(&mut otp, line, "").await;
    println!();
    set_size(&mut otp, line, "").await;
}

/// Firmware for another board, then with `--force`. The CLI refuses before it
/// prints the device line. With `--force` it prints the warning and goes on.
#[tokio::test]
async fn firmware_for_another_board() {
    let board = |name| Board::try_from_str(name).unwrap();
    let line = "onerom hardware set-size --board fire-40-a --size L";
    println!("$ {line}");
    let error = check_firmware(
        OtpCommand::SetSize,
        Some(board("fire-40-b")),
        board("fire-40-a"),
        false,
    )
    .unwrap_err();
    failed(error);
    println!();

    println!("$ {line} --force --yes");
    println!(
        "~ {}",
        firmware_warning(OtpCommand::SetSize, board("fire-40-b"), board("fire-40-a"))
    );
    let mut otp = blank_board();
    println!("~ {}", device("fire-40-b", size_of(&mut otp).await));
    let words: Vec<&str> = line
        .split_whitespace()
        .chain(["--force", "--yes"])
        .collect();
    let (args, options) = set_size_args(&words);
    run_set_size(&mut otp, &args, &options, "").await;
}
