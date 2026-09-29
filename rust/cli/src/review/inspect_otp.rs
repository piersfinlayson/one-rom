// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `inspect otp`.

use onerom_app::{BoardSize, MemoryOtp};
use onerom_cli::Error;

use super::{RUNNING_LAB, device, json_boards, newer_boards, written_failure};
use crate::commissioning::report_lines;
use crate::test_board::{blank_board, commissioned_board, table};

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
