// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `update firmware`, `update slot`, `update plugin` and `clone`.
//!
//! Each reads a One ROM back and programs it. The cases print the help, the
//! command lines that fail to parse before a One ROM is looked for, and the
//! lines and errors these commands add to `program`'s, written from the code.

use std::path::Path;

use onerom_app::{BoardSize, ReadBack, RecoveryProblem, published_plugins, read_back_file};
use onerom_cli::Error;
use onerom_cli::image::parse_firmware;
use onerom_cli::read_back::{device_read_back_error, inline_files};
use onerom_cli::usb::{FLASH_BASE, RAM_BASE};
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_config::mcu::Variant;
use onerom_fw::net::Releases;
use onerom_fw_parser::readers::MemoryReader;
use onerom_fw_parser::{Parser, SDRR_INFO_FW_OFFSET};
use onerom_metadata::{
    FIRMWARE_RELEASE, ONEROM_INFO_MAJOR_VERSION_OFFSET, ONEROM_INFO_MINOR_VERSION_OFFSET,
    ONEROM_INFO_PATCH_VERSION_OFFSET,
};

use super::{command_of, failed, refused_lines, written_failure};
use crate::args::Commands;
use crate::firmware::{build_rom_image, resolve_release};
use crate::test_board::{
    Files, IMAGE_2364, NoFetch, USB_URL, base_firmware, image_2364_sets, image_with_plugins,
    image_with_usb,
};
use crate::update::{
    Command, build_failure, check_recovered, clone_config, firmware_change, plugin_changes,
    plugin_config, read_source, replace_rom, source_error,
};

async fn read_back() -> ReadBack {
    let image = image_2364_sets(Board::Fire24F, &["single", "single", "banked"], &[]).await;
    read_back_file(&image).await.unwrap()
}

#[test]
fn help() {
    for command in ["firmware", "slot", "plugin"] {
        super::help(&["onerom", "update", command, "--help"]);
        println!();
    }
    super::help(&["onerom", "clone", "--help"]);
}

#[test]
fn refused() {
    refused_lines(&[
        "onerom update firmware --version 0.8.1 --base-firmware base.bin",
        "onerom update slot --slot 0",
        "onerom update slot --image kernal.bin",
        "onerom update plugin",
        "onerom clone --from E126C9F97C10ADAC",
    ]);
}

/// A rebuild that moves the firmware or a published plugin to another release
/// prints a line for it before programming.
#[tokio::test]
async fn the_firmware_and_a_plugin_move_to_another_release() {
    println!("$ onerom update firmware --version 0.8.1");
    let release = |patch| FirmwareVersion::new(0, 8, patch, 0);
    if let Some(line) = firmware_change(Some(release(0)), release(1)) {
        println!("~ {line}");
    }
    let before = published_plugins(&parse_firmware(&image_with_usb("0.3.1").await).await);
    let after = published_plugins(&parse_firmware(&image_with_usb("0.3.2").await).await);
    for line in plugin_changes(&before, &after) {
        println!("~ {line}");
    }
    println!("~ Programming device - DO NOT DISCONNECT");
    println!("~ Programming complete");
}

/// Slot 3's chip-select settings and slot 1's vreg override aren't
/// recognised, so a rebuild would lose them.
#[tokio::test]
async fn a_setting_that_isnt_recognised() {
    let image = image_2364_sets(Board::Fire24F, &["single"; 4], &[]).await;
    let read_back = read_back_file(&image).await.unwrap();
    for problem in [
        RecoveryProblem::ChipSelect { set: 3 },
        RecoveryProblem::Override {
            set: 1,
            name: "vreg",
        },
    ] {
        println!("$ onerom update slot --slot 0 --image kernal.bin");
        let mut read_back = read_back.clone();
        read_back.problems.push(problem);
        written_failure(&check_recovered(&read_back).unwrap_err());
        println!();
    }
}

/// The One ROM's firmware is the release after this build's, and new.bin's
/// is v0.9.0, newer than the parser reads.
#[tokio::test]
async fn newer_firmware() {
    let image = image_2364_sets(Board::Fire24F, &["single"], &[]).await;
    let next = FirmwareVersion::new(
        FIRMWARE_RELEASE.major(),
        FIRMWARE_RELEASE.minor(),
        FIRMWARE_RELEASE.patch() + 1,
        0,
    );
    let flash = with_version(image, next);
    // The error reading back a One ROM whose flash holds `flash`.
    let device_error = async || {
        let mut reader = MemoryReader::new(flash.clone(), FLASH_BASE);
        let parsed = Parser::with_base_flash_address(&mut reader, FLASH_BASE, RAM_BASE)
            .parse_device()
            .await;
        let e = onerom_app::read_back(&parsed, &mut reader, None)
            .await
            .unwrap_err();
        device_read_back_error(e)
    };

    println!("$ onerom update slot --slot 0 --image kernal.bin");
    written_failure(&device_error().await);
    println!();

    let dir = tempfile::tempdir().unwrap();
    let new = dir.path().join("new.bin");
    std::fs::write(&new, base_firmware(9)).unwrap();
    clone_from(&new, "new.bin").await;
    println!();

    println!("$ onerom clone --from E126C9F97C10ADAC --to FC9D67248E8E8023");
    written_failure(&source_error(device_error().await));
}

/// `image` with `version` in its `onerom_info_t`.
fn with_version(mut image: Vec<u8>, version: FirmwareVersion) -> Vec<u8> {
    let info = SDRR_INFO_FW_OFFSET as usize;
    for (offset, value) in [
        (ONEROM_INFO_MAJOR_VERSION_OFFSET, version.major()),
        (ONEROM_INFO_MINOR_VERSION_OFFSET, version.minor()),
        (ONEROM_INFO_PATCH_VERSION_OFFSET, version.patch()),
    ] {
        image[info + offset..info + offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    image
}

/// Prints the transcript of `onerom clone --from <from>`, which is `shown`
/// on the command line, as far as reading `from` back. `from` isn't a
/// connected One ROM's serial number.
async fn clone_from(from: &Path, shown: &str) {
    println!("$ onerom clone --from {shown} --to FC9D67248E8E8023");
    let from = from.to_string_lossy();
    let (command, options) = command_of(&[
        "onerom",
        "clone",
        "--from",
        &from,
        "--to",
        "FC9D67248E8E8023",
    ]);
    let Commands::Clone(args) = command else {
        panic!("not clone");
    };
    match read_source(&options, &args.from, async || Vec::new()).await {
        Ok(_) => println!("~ reads"),
        Err(e) => failed(e.to_string().replace(&*from, shown)),
    }
}

#[test]
fn a_slot_or_chip_it_doesnt_have() {
    println!("$ onerom update slot --slot 7 --image kernal.bin");
    written_failure(&Error::NoSlot { slot: 7, slots: 3 });
    println!();
    println!("$ onerom update slot --slot 2 --image char.bin");
    written_failure(&Error::ChipNeeded { slot: 2, chips: 2 });
}

#[test]
fn cloning_a_one_rom_onto_itself() {
    println!("$ onerom clone --from E126C9F97C10ADAC --to E126C9F97C10ADAC");
    written_failure(&Error::CloneOntoItself);
}

/// fire-32-b's releases start at v0.6.13.
#[test]
fn a_version_that_isnt_released_for_the_board() {
    let board = |name: &str| format!(r#"{{ "name": "{name}", "mcus": [{{ "name": "rp2350" }}] }}"#);
    let release = |version: &str, boards: &[&str]| {
        let boards: Vec<String> = boards.iter().map(|name| board(name)).collect();
        format!(
            r#"{{ "version": "{version}", "boards": [{}] }}"#,
            boards.join(", ")
        )
    };
    let json = format!(
        r#"{{ "version": 1, "latest": "0.8.0", "releases": [{}, {}, {}] }}"#,
        release("0.8.0", &["fire-24-a", "fire-32-b"]),
        release("0.6.13", &["fire-24-a", "fire-32-b"]),
        release("0.5.10", &["fire-24-a"]),
    );
    let releases = Releases::from_json(&json).unwrap();
    for version in ["9.9.9", "0.5.10"] {
        println!("$ onerom update firmware --version {version}");
        let version = Some(version.to_string());
        let e =
            resolve_release(&releases, &version, &Board::Fire32B, &Variant::RP2350).unwrap_err();
        failed(e);
        println!();
    }
}

/// The source contains the published usb plugin, the unpublished
/// activity.bin and kernal.bin. The clone reuses every slot.
#[tokio::test]
async fn verbose_reused_slots() {
    let line = "onerom --verbose clone --from source.bin --to FC9D67248E8E8023";
    println!("$ {line}");
    let image = image_with_plugins(&[USB_URL, "activity.bin"]).await;
    let read_back = read_back_file(&image).await.unwrap();
    let (_, options) = command_of(&line.split(' ').collect::<Vec<_>>());
    let json = serde_json::to_string(&clone_config(&read_back, None)).unwrap();

    println!("~ (the lines before are unchanged)");
    let (_, _, _, desc) = build_rom_image(
        &options,
        &json,
        read_back.version,
        read_back.board,
        Variant::RP2350,
        BoardSize::M,
        false,
        &NoFetch,
        |_| Ok(()),
    )
    .await
    .unwrap();
    // As `update` and `clone` print it.
    for line in format!("ROM configuration:\n---\n{desc}\n---").lines() {
        println!("~ {line}");
    }
    println!("~ (the lines after are unchanged)");
}

/// The One ROM has standby set in slot 0, which v0.7.3 predates.
#[tokio::test]
async fn a_setting_older_firmware_doesnt_support() {
    println!("$ onerom update firmware --version 0.7.3");
    let mut read_back = read_back().await;
    read_back.config.chip_sets[0].firmware_overrides =
        serde_json::from_str(r#"{ "fire": { "standby": true } }"#).unwrap();
    let version = FirmwareVersion::new(0, 7, 3, 0);
    let e = build_failure(
        Command::Update,
        &inline_files(&read_back),
        version,
        Board::Fire24F,
    )
    .await;
    failed(e);
}

/// kernal.bin is half a 2364.
#[tokio::test]
async fn an_undersized_image() {
    println!("$ onerom update slot --slot 1 --image kernal.bin");
    let dir = tempfile::tempdir().unwrap();
    let kernal = dir.path().join("kernal.bin");
    std::fs::write(&kernal, [0; IMAGE_2364 / 2]).unwrap();
    let config = replace_rom(&read_back().await, 1, None, &kernal.to_string_lossy()).unwrap();
    let version = FirmwareVersion::new(0, 8, 0, 0);
    let e = build_failure(Command::Update, &config, version, Board::Fire24F).await;
    let prefix = format!("{}/", dir.path().display());
    failed(e.to_string().replace(&prefix, ""));
}

/// usb is a system plugin, and the plugins manifest doesn't list nosuch.
#[tokio::test]
async fn a_plugin_that_isnt_published() {
    let catalogue = r#"{ "version": 1, "plugins": [
        { "name": "usb", "type": "system_plugin", "path": "system/usb" }
    ] }"#;
    let served = Files(vec![(
        "https://images.onerom.org/plugins/plugins.json".to_string(),
        catalogue.as_bytes().to_vec(),
    )]);
    let version = FirmwareVersion::new(0, 8, 0, 0);
    for (n, spec) in ["user/usb", "nosuch"].into_iter().enumerate() {
        if n > 0 {
            println!();
        }
        println!("$ onerom update plugin --plugin {spec}");
        let e = plugin_config(&read_back().await, spec, &version, &served)
            .await
            .unwrap_err();
        failed(e);
    }
}

/// nofile.bin doesn't exist and E126C9F97C10ADAC isn't connected.
#[tokio::test]
async fn a_source_that_isnt_found() {
    let dir = tempfile::tempdir().unwrap();
    clone_from(&dir.path().join("nofile.bin"), "nofile.bin").await;
    println!();
    println!("$ onerom clone --from E126C9F97C10ADAC --to FC9D67248E8E8023");
    written_failure(&Error::DeviceNotFound("E126C9F97C10ADAC".to_string()));
}

/// The destination is a fire-32-b, which doesn't support a 2364.
#[tokio::test]
async fn a_slot_the_destination_doesnt_support() {
    println!("$ onerom clone --from fire-24-f.bin --to FC9D67248E8E8023");
    let config = clone_config(&read_back().await, None);
    let version = FirmwareVersion::new(0, 8, 0, 0);
    let e = build_failure(Command::Clone, &config, version, Board::Fire32B).await;
    failed(e);
}
