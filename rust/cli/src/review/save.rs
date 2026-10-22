// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `save` from a firmware file. Saving from a One ROM prints the same lines.

use std::path::Path;

use onerom_config::hw::Board;

use onerom_app::{ReadBackError, RecoveryProblem, read_back_file};
use onerom_cli::read_back::{device_read_back_error, read_back_error};
use onerom_cli::usb::{FLASH_BASE, RAM_BASE};
use onerom_fw_parser::Parser;
use onerom_fw_parser::readers::MemoryReader;
use onerom_gen::{RecoverError, SlotName, UnscrambleError};

use super::{command_of, failed, flash_without_firmware, refused_lines, written_failure};
use crate::args::Commands;
use crate::save::{save, save_read_back};
use crate::test_board::{base_firmware, image_2364_sets, image_with_usb, original_image};

/// Prints the transcript of `line` with `image` in `image.bin`. `line` is an
/// `onerom save --firmware image.bin` command line. Every other file in it is
/// in the same directory. Prints any `config.json` saved.
async fn run(line: &str, image: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    run_in(dir.path(), line, image).await;
}

/// [`run`] with `image.bin` and the other files in `dir`, which may already
/// hold some of them. Prints `c64/config.json` where it exists.
async fn run_in(dir: &Path, line: &str, image: &[u8]) {
    println!("$ {line}");
    let prefix = format!("{}/", dir.display());
    std::fs::write(dir.join("image.bin"), image).unwrap();
    let words: Vec<String> = line
        .split_whitespace()
        .map(|word| match word.chars().next() {
            Some('-') => word.to_string(),
            _ if word == "onerom" || word == "save" => word.to_string(),
            _ if word.contains('.') || word == "c64" => format!("{prefix}{word}"),
            _ => word.to_string(),
        })
        .collect();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let (command, options) = command_of(&words);
    let Commands::Save(args) = command else {
        panic!("not save");
    };
    match save(&options, &args).await {
        Ok(lines) => {
            for line in lines {
                println!("{}", line.replace(&prefix, ""));
            }
        }
        Err(e) => failed(e.to_string().replace(&prefix, "")),
    }
    if let Ok(config) = std::fs::read_to_string(dir.join("c64/config.json")) {
        println!("~ c64/config.json:");
        print!("{}", config.replace(&prefix, ""));
    }
}

async fn image() -> Vec<u8> {
    image_2364_sets(Board::Fire24F, &["single", "single", "banked"], &[]).await
}

#[test]
fn help() {
    super::help(&["onerom", "save", "--help"]);
}

#[tokio::test]
async fn a_config_and_its_roms() {
    run(
        "onerom save --firmware image.bin --path c64",
        &image().await,
    )
    .await;
}

#[tokio::test]
async fn one_rom() {
    let image = image().await;
    run(
        "onerom save --firmware image.bin --slot 0 --output basic.bin",
        &image,
    )
    .await;
    println!();
    run(
        "onerom save --firmware image.bin --slot 2 --chip 1 --output char.bin",
        &image,
    )
    .await;
}

#[tokio::test]
async fn a_slot_or_chip_it_doesnt_have() {
    let image = image().await;
    for line in [
        "onerom save --firmware image.bin --slot 2 --output char.bin",
        "onerom save --firmware image.bin --slot 0 --chip 1 --output basic.bin",
        "onerom save --firmware image.bin --slot 3 --output basic.bin",
    ] {
        run(line, &image).await;
        println!();
    }
}

/// Base firmware doesn't contain a config. Firmware before v0.7.0 isn't read
/// back.
#[tokio::test]
async fn firmware_it_cant_read_back() {
    run(
        "onerom save --firmware image.bin --path c64",
        &base_firmware(8),
    )
    .await;
    println!();
    run(
        "onerom save --firmware image.bin --path c64",
        &original_image(),
    )
    .await;
}

/// image.bin and the One ROM's flash are erased, so neither contains One ROM
/// firmware.
#[tokio::test]
async fn firmware_that_isnt_one_rom_firmware() {
    let [(_, erased), _] = flash_without_firmware();
    run("onerom save --firmware image.bin --path c64", &erased).await;
    println!();
    println!("$ onerom save --path c64");
    let mut reader = MemoryReader::new(erased, FLASH_BASE);
    let parsed = Parser::with_base_flash_address(&mut reader, FLASH_BASE, RAM_BASE)
        .parse_device()
        .await;
    let e = onerom_app::read_back(&parsed, &mut reader, None)
        .await
        .unwrap_err();
    written_failure(&device_read_back_error(e));
}

/// image.bin's firmware is v0.9.0, newer than this build supports.
#[tokio::test]
async fn newer_firmware() {
    run(
        "onerom save --firmware image.bin --path c64",
        &base_firmware(9),
    )
    .await;
}

/// Slot 0's chip-select settings and vreg override aren't recognised. The
/// config and ROMs are saved anyway.
#[tokio::test]
async fn a_setting_that_isnt_recognised() {
    println!("$ onerom save --path c64");
    let mut read_back = read_back_file(&image_with_usb("0.3.1").await)
        .await
        .unwrap();
    // Chip set 0 is the USB plugin's.
    read_back.problems.extend([
        RecoveryProblem::ChipSelect { set: 1 },
        RecoveryProblem::Override {
            set: 1,
            name: "vreg",
        },
    ]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c64").to_string_lossy().into_owned();
    let prefix = format!("{}/", dir.path().display());
    for line in save_read_back(read_back, &path, false).unwrap() {
        for text in line.replace(&prefix, "").lines() {
            println!("~ {text}");
        }
    }
}

/// Slot 1's pin layout in image.bin is inconsistent.
#[test]
fn corrupt_firmware() {
    println!("$ onerom save --firmware image.bin --path c64");
    let error = RecoverError::Unscramble {
        slot: SlotName::Rom(1),
        error: UnscrambleError::BadLayout,
    };
    written_failure(&read_back_error(
        "image.bin",
        ReadBackError::<String>::Recover(error),
    ));
}

/// basic.bin already exists so the save fails and the file is left as it was.
/// With --force, the file is overwritten.
#[tokio::test]
async fn an_output_file_that_already_exists() {
    let image = image().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("basic.bin"), "old").unwrap();
    for line in [
        "onerom save --firmware image.bin --slot 0 --output basic.bin",
        "onerom save --firmware image.bin --slot 0 --output basic.bin --force",
    ] {
        run_in(dir.path(), line, &image).await;
        println!();
    }
}

/// c64/config.json already exists so the save fails and the directory is left
/// as it was. With --force, the files are overwritten.
#[tokio::test]
async fn a_file_that_already_exists() {
    let image = image().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("c64")).unwrap();
    std::fs::write(dir.path().join("c64/config.json"), "{}\n").unwrap();
    run_in(
        dir.path(),
        "onerom save --firmware image.bin --path c64",
        &image,
    )
    .await;
    println!();
    run_in(
        dir.path(),
        "onerom save --firmware image.bin --path c64 --force",
        &image,
    )
    .await;
}

/// --path is a file.
#[tokio::test]
async fn a_path_thats_a_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("one.bin"), "one").unwrap();
    run_in(
        dir.path(),
        "onerom save --firmware image.bin --path one.bin",
        &image().await,
    )
    .await;
}

#[test]
fn refused() {
    refused_lines(&[
        "onerom save",
        "onerom save --slot 0",
        "onerom save --output a.bin --path c64",
        "onerom save --slot 0 --output a.bin --path c64",
        "onerom save --chip 1 --path c64",
    ]);
}
