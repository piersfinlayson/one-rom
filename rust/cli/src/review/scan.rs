// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `scan`'s commissioning lines and the lines `scan --slots` shows for firmware
//! this build doesn't recognise.

use onerom_app::{BoardSize, MemoryOtp};
use onerom_config::hw::Board;

use super::{
    UNRECOGNISED, damaged_header_flash, device, flash_without_firmware, newer_boards,
    newer_firmware_flash, replaced_instance_board, size_of, unrecognised_reasons,
};
use crate::commissioning::device_lines;
use crate::inspect::unrecognised_firmware_lines;
use crate::test_board::{blank_board, commissioned_board, table};

/// Prints the lines `scan` prints for a stopped One ROM whose firmware is for
/// `board` and whose OTP is `otp`. Only its commissioning lines come from
/// running the code.
async fn scan(otp: &mut MemoryOtp, board: &str, verbose: bool) {
    let area = onerom_app::read_commissioning(otp).await.unwrap();
    let commissioning = onerom_cli::otp::Commissioning::Read(area);
    let line = device(board, size_of(otp).await);
    println!("$ onerom scan{}", if verbose { " --verbose" } else { "" });
    println!("~ Scanning ... ");
    println!("~ found 1 connected device:");
    println!("~   {line}");
    if verbose {
        println!("~     MCU: RP235xA Chip ID: DE3F9C232F655B6B");
    }
    let board = Board::try_from_str(board);
    for line in device_lines(board, &commissioning, (verbose, verbose), &table(None)) {
        println!("    {line}");
    }
}

/// `scan` without and with `--verbose` on `otp`, whose firmware is for
/// `board`.
async fn both(otp: &mut MemoryOtp, board: &str) {
    scan(otp, board, false).await;
    println!();
    scan(otp, board, true).await;
}

#[tokio::test]
async fn a_blank_board() {
    both(&mut blank_board(), "fire-24-f").await;
}

#[tokio::test]
async fn a_commissioned_m_board() {
    let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
    both(&mut otp, "fire-24-f").await;
}

#[tokio::test]
async fn a_commissioned_l_board() {
    let mut otp = commissioned_board("fire-40-a", BoardSize::L).await;
    both(&mut otp, "fire-40-a").await;
}

/// A board with two instances, the second written by `--force` with another
/// date.
#[tokio::test]
async fn two_instances() {
    scan(&mut replaced_instance_board().await, "fire-24-f", true).await;
}

/// Data from a newer version. First an area holding version 2, then an
/// instance holding an unknown key.
#[tokio::test]
async fn newer_data() {
    for (name, mut otp) in newer_boards() {
        println!("### {name}");
        both(&mut otp, "fire-24-f").await;
        println!();
    }
}

/// Prints the lines `scan --slots --unrecognised` prints for a stopped board
/// whose flash holds `image` and whose OTP isn't commissioned. Only the lines
/// beneath the device's line come from running the code.
async fn scan_slots(image: Vec<u8>) {
    let reasons = unrecognised_reasons(image).await;
    println!("$ onerom scan --slots --unrecognised");
    println!("~ Scanning ... ");
    println!("~ found 1 connected device:");
    println!("~ ---");
    println!("~ {UNRECOGNISED}");
    for line in unrecognised_firmware_lines(&reasons) {
        println!("  {line}");
    }
}

/// Firmware newer than this build reads. The parser provides one reason.
#[tokio::test]
async fn newer_firmware() {
    scan_slots(newer_firmware_flash()).await;
}

/// A v0.8.0 header with a null build date pointer, whose metadata is zeros.
/// The parser provides two reasons.
#[tokio::test]
async fn a_damaged_header() {
    scan_slots(damaged_header_flash()).await;
}

/// Erased flash, then flash reading all zeros. The parser doesn't provide a
/// reason for either.
#[tokio::test]
async fn no_one_rom_firmware() {
    for (name, image) in flash_without_firmware() {
        println!("### {name}");
        scan_slots(image).await;
        println!();
    }
}
