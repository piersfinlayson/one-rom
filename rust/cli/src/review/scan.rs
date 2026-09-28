// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `scan`'s commissioning lines.

use onerom_app::{BoardSize, MemoryOtp};
use onerom_config::hw::Board;

use super::{device, newer_boards, replaced_instance_board, size_of};
use crate::commissioning::device_lines;
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
