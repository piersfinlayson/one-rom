// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `firmware inspect`.

use onerom_app::BoardSize;
use onerom_gen::FIRMWARE_SIZE;

use super::{command_of, failed};
use crate::args::Commands;
use crate::args::firmware::FirmwareCommands;
use crate::firmware::cmd_inspect;
use crate::test_board::{base_firmware, image_file, original_image};

/// Prints the transcript of `line`, which starts `onerom firmware inspect` and
/// refers to `image.bin`, with `image` in `image.bin`. There isn't a One ROM
/// connected.
async fn inspect(line: &str, image: &[u8]) {
    println!("$ {line}");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("image.bin");
    std::fs::write(&path, image).unwrap();
    let words: Vec<String> = line
        .split_whitespace()
        .map(|word| match word {
            "image.bin" => path.display().to_string(),
            word => word.to_string(),
        })
        .collect();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let (command, options) = command_of(&words);
    let Commands::Firmware(firmware) = command else {
        panic!("not firmware");
    };
    let FirmwareCommands::Inspect(args) = firmware.command else {
        panic!("not firmware inspect");
    };
    if let Err(e) = cmd_inspect(&options, &args).await {
        failed(e);
    }
}

/// Prints the transcript of inspecting `image`, then of inspecting it with
/// `--verbose`.
async fn inspect_both(image: &[u8]) {
    inspect("onerom firmware inspect --firmware image.bin", image).await;
    println!();
    inspect(
        "onerom firmware inspect --firmware image.bin --verbose",
        image,
    )
    .await;
}

#[test]
fn help() {
    super::help(&["onerom", "firmware", "inspect", "--help"]);
}

/// An image built for an M fire-40-a with v0.8.0 firmware, holding two single
/// 27C400 chip sets.
#[tokio::test]
async fn a_built_image() {
    inspect_both(&image_file(BoardSize::M, 2)).await;
}

/// v0.8.0 base firmware, which doesn't contain metadata.
#[tokio::test]
async fn base_firmware_alone() {
    inspect_both(&base_firmware(8)).await;
}

/// A built image cut off part way through its metadata.
#[tokio::test]
async fn an_image_cut_off_in_its_metadata() {
    let mut image = image_file(BoardSize::M, 1);
    image.truncate(FIRMWARE_SIZE + 0x100);
    inspect("onerom firmware inspect --firmware image.bin", &image).await;
}

/// A fire-24-e's v0.6.0 firmware, from before v0.7.0, without ROM sets.
#[tokio::test]
async fn firmware_from_before_0_7_0() {
    inspect(
        "onerom firmware inspect --firmware image.bin",
        &original_image(),
    )
    .await;
}
