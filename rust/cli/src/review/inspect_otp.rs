// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `inspect otp`.

use onerom_app::{BoardSize, MemoryOtp};
use onerom_cli::Error;
use onerom_metadata::otp::pico_otp::ecc_encode;

use super::{
    RUNNING_LAB, device, json_boards, neither_m_nor_l_board, newer_boards, written_failure,
};
use crate::commissioning::report_lines;
use crate::test_board::{acme_board, blank_board, commissioned_board, table};

/// Prints the lines `inspect otp` prints for `otp` on a One ROM whose firmware
/// is for `board`.
async fn inspect_otp(otp: &mut MemoryOtp, board: &str, verbose: bool) {
    let report = onerom_app::read_report(otp).await.unwrap();
    println!(
        "$ onerom inspect otp{}",
        if verbose { " --verbose" } else { "" }
    );
    println!("~ {}", device(board, report.size));
    for line in report_lines(&report, verbose, &table(None)) {
        println!("  {line}");
    }
}

/// `inspect otp` without and with `--verbose` on `otp`, whose firmware is for
/// `board`.
async fn both(otp: &mut MemoryOtp, board: &str) {
    inspect_otp(otp, board, false).await;
    println!();
    inspect_otp(otp, board, true).await;
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

#[tokio::test]
async fn a_board_neither_m_nor_l() {
    both(&mut neither_m_nor_l_board(), "fire-40-a").await;
}

/// A blank board whose FLASH_DEVINFO holds size code 13 for chip select 1, a
/// code [`OneromFlashSize`](onerom_metadata::OneromFlashSize) has no name for.
#[tokio::test]
async fn an_unknown_flash_size_code() {
    let mut otp = blank_board();
    otp.set_raw(0x054, ecc_encode(0xd9af));
    for row in 0x048..=0x04a {
        otp.set_raw(row, 0x000020);
    }
    inspect_otp(&mut otp, "fire-40-a", true).await;
}

/// An M fire-40-b commissioned by Acme Retro with its key.
#[tokio::test]
async fn a_manufacturers_board() {
    inspect_otp(&mut acme_board().await, "fire-40-b", false).await;
}

/// Each state `--verbose` shows an instance in.
#[tokio::test]
async fn instance_states() {
    for (name, mut otp) in super::instance_states().await {
        println!("### {name}");
        inspect_otp(&mut otp, "fire-24-f", true).await;
        println!();
    }
}

/// Data from a newer version. First an area holding version 2, then an
/// instance holding an unknown key.
#[tokio::test]
async fn newer_data() {
    for (name, mut otp) in newer_boards() {
        println!("### {name}");
        inspect_otp(&mut otp, "fire-24-f", false).await;
        println!();
    }
}

/// A One ROM running One ROM Lab, which refuses OTP access. The CLI refuses
/// before it sends a command.
#[test]
fn a_running_lab() {
    println!("$ onerom inspect otp");
    written_failure(&Error::OtpLabRunning(RUNNING_LAB.to_string()));
}

/// `--json` for each board.
#[tokio::test]
async fn json() {
    for (name, mut otp) in json_boards().await {
        println!("### {name}");
        let report = onerom_app::read_report(&mut otp).await.unwrap();
        println!("$ onerom inspect otp --json");
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        println!();
    }
}
