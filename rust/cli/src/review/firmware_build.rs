// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `firmware build`.

use tempfile::TempDir;

use super::{command_of, failed};
use crate::args::Commands;
use crate::args::firmware::FirmwareCommands;
use crate::firmware::cmd_build;
use crate::test_board::{IMAGE_27C400, base_firmware, config_27c400};

/// The files a `firmware build` line refers to, in a temporary directory:
/// - `base.bin`, base firmware
/// - `sets.json`, a config of 27C400 chip sets whose images are beside it
pub(super) struct Files {
    dir: TempDir,
}

impl Files {
    /// Base firmware for v0.`minor`.0 and a config of `sets` chip sets.
    pub(super) fn new(minor: u16, sets: usize) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("base.bin"), base_firmware(minor)).unwrap();
        std::fs::write(
            dir.path().join("sets.json"),
            config_27c400(sets, dir.path()),
        )
        .unwrap();
        for n in 0..sets {
            let image = vec![n as u8; IMAGE_27C400];
            std::fs::write(dir.path().join(format!("{n}.bin")), image).unwrap();
        }
        Self { dir }
    }

    /// `line`'s words, with each file it refers to in the directory.
    pub(super) fn words(&self, line: &str) -> Vec<String> {
        line.split_whitespace()
            .map(|word| match word {
                "base.bin" | "sets.json" => self.dir.path().join(word).display().to_string(),
                word => word.to_string(),
            })
            .collect()
    }
}

/// Prints the transcript of `line`, which starts `onerom firmware build`, with
/// `files`. There isn't a One ROM connected.
async fn build(line: &str, files: &Files) {
    println!("$ {line}");
    let words = files.words(line);
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let (command, options) = command_of(&words);
    let Commands::Firmware(firmware) = command else {
        panic!("not firmware");
    };
    let FirmwareCommands::Build(args) = firmware.command else {
        panic!("not firmware build");
    };
    match cmd_build(&options, &args).await {
        Ok(()) => println!("~ (built)"),
        Err(e) => failed(e),
    }
}

#[test]
fn help() {
    super::help(&["onerom", "firmware", "build", "--help"]);
}

/// `--size L` for fire-24-f, which doesn't support external flash. The CLI
/// refuses before it reads a file.
#[tokio::test]
async fn size_l_for_a_board_without_external_flash() {
    let files = Files::new(8, 1);
    let line = "onerom firmware build --board fire-24-f --size L --config sets.json --base-firmware base.bin";
    build(line, &files).await;
}

/// Four 512KB sets for fire-40-a, which supports external flash. The fourth
/// doesn't fit on the first chip, so an M build fails.
#[tokio::test]
async fn a_set_that_doesnt_fit_an_m_board() {
    let files = Files::new(8, 4);
    let line =
        "onerom firmware build --board fire-40-a --config sets.json --base-firmware base.bin";
    build(line, &files).await;
}

/// The same four sets with v0.7.0 firmware, which supports only M, so
/// `--size` isn't advised.
#[tokio::test]
async fn a_set_that_doesnt_fit_with_firmware_before_0_8_0() {
    let files = Files::new(7, 4);
    let line =
        "onerom firmware build --board fire-40-a --config sets.json --base-firmware base.bin";
    build(line, &files).await;
}

/// Three sets for L with v0.7.0 firmware, which supports only M. They'd fit
/// an M board.
#[tokio::test]
async fn a_size_other_than_m_with_firmware_before_0_8_0() {
    let files = Files::new(7, 3);
    let line = "onerom firmware build --board fire-40-a --size L --config sets.json --base-firmware base.bin";
    build(line, &files).await;
}
