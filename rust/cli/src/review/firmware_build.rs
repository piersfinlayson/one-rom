// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `firmware build`.

use tempfile::TempDir;

use super::{command_of, failed};
use crate::args::Commands;
use crate::args::firmware::FirmwareCommands;
use crate::firmware::cmd_build;
use crate::test_board::{IMAGE_27C400, IMAGE_2364, base_firmware, config_27c400, config_2364};

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

    /// Base firmware for v0.`minor`.0 and a config of 2364 chip sets.
    pub(super) fn with_2364(minor: u16, sets: &[&str], reserved: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("base.bin"), base_firmware(minor)).unwrap();
        std::fs::write(
            dir.path().join("sets.json"),
            config_2364(sets, dir.path(), reserved),
        )
        .unwrap();
        let chips: usize = sets
            .iter()
            .map(|set_type| if *set_type == "single" { 1 } else { 2 })
            .sum();
        for n in 0..chips {
            let image = vec![n as u8; IMAGE_2364];
            std::fs::write(dir.path().join(format!("{n}.bin")), image).unwrap();
        }
        Self { dir }
    }

    /// `line`'s words, with each file it refers to in the directory.
    pub(super) fn words(&self, line: &str) -> Vec<String> {
        line.split_whitespace()
            .map(|word| match word {
                "base.bin" | "sets.json" | "out.bin" => {
                    self.dir.path().join(word).display().to_string()
                }
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

#[tokio::test]
async fn an_image_larger_than_its_chip_type() {
    let files = Files::with_2364(8, &["single"], &[]);
    std::fs::write(files.dir.path().join("0.bin"), vec![0; 2 * IMAGE_2364]).unwrap();
    let line = "onerom firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin";
    build(line, &files).await;
}

/// An Intel HEX image that doesn't decode, in the first ROM slot behind a
/// system plugin.
#[tokio::test]
async fn an_intel_hex_image_that_does_not_decode() {
    let files = Files::with_2364(8, &["single"], &[]);
    let dir = files.dir.path();
    std::fs::write(dir.join("basic.hex"), ":10000000ZZ\n").unwrap();
    let plugin = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../images/test/stub-system-plugin.bin"
    );
    let config = serde_json::json!({
        "version": 1,
        "description": "Plugin and a damaged Intel HEX image",
        "rom_sets": [
            { "type": "single", "roms": [{ "file": plugin, "type": "system_plugin" }] },
            { "type": "single", "roms": [{
                "file": dir.join("basic.hex"),
                "label": "basic.hex",
                "type": "2364",
                "cs1": "active_low",
                "format": "ihex"
            }] }
        ]
    });
    std::fs::write(dir.join("sets.json"), config.to_string()).unwrap();
    let line = "onerom firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin";
    build(line, &files).await;
}

#[tokio::test]
async fn verbose_images_and_their_jumpers() {
    let files = Files::with_2364(8, &["single"; 5], &[]);
    let line = "onerom --verbose firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin";
    build(line, &files).await;
}

/// SEL_D becomes image select bit 2.
#[tokio::test]
async fn verbose_images_with_a_reserved_pin() {
    let files = Files::with_2364(8, &["single"; 5], &[]);
    let line = "onerom --verbose firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin --reserve-pin sel_c";
    build(line, &files).await;
}

/// Four image select pins less two reserved provide 4 combinations for 5 images.
#[tokio::test]
async fn verbose_an_image_the_jumpers_cannot_select() {
    let files = Files::with_2364(8, &["single"; 5], &[]);
    let line = "onerom --verbose firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin --reserve-pin sel_c --reserve-pin sel_d";
    build(line, &files).await;
}

/// Every jumper setting selects the one image.
#[tokio::test]
async fn verbose_one_image() {
    let files = Files::with_2364(8, &["single"], &[]);
    let line = "onerom --verbose firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin";
    build(line, &files).await;
}

#[tokio::test]
async fn verbose_turbo_boot() {
    let files = Files::with_2364(8, &["single"], &[]);
    let line = "onerom --verbose firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin --turbo-boot";
    build(line, &files).await;
}

#[tokio::test]
async fn slots_the_jumpers_cannot_select() {
    let files = Files::with_2364(8, &["single"; 5], &[]);
    let line = "onerom firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin --reserve-pin sel_c --reserve-pin sel_d";
    build(line, &files).await;
    println!();
    let line = "onerom firmware build --board fire-24-c --config sets.json --base-firmware base.bin --output out.bin";
    build(line, &files).await;
    println!();
    let files = Files::with_2364(8, &["single"; 3], &[]);
    let line = "onerom firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin --reserve-pin sel_b --reserve-pin sel_c --reserve-pin sel_d";
    build(line, &files).await;
    println!();
    let files = Files::with_2364(8, &["single"; 2], &[]);
    let line = "onerom firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin --reserve-pin sel_a --reserve-pin sel_b --reserve-pin sel_c --reserve-pin sel_d";
    build(line, &files).await;
}

/// - GPIO 23 isn't wired to an image select pin, X1 or X2.
/// - A fire-24-f doesn't have SEL_E.
#[tokio::test]
async fn a_pin_that_cannot_be_reserved() {
    let files = Files::with_2364(8, &["single"], &[]);
    let line = "onerom firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin --reserve-pin gpio23";
    build(line, &files).await;
    for entry in ["sel_e", "banana"] {
        println!();
        let files = Files::with_2364(8, &["single"], &[entry]);
        let line = "onerom firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin";
        println!("~ sets.json has \"reserved_pins\": [\"{entry}\"]");
        build(line, &files).await;
    }
}

#[test]
fn a_reserve_pin_that_isnt_a_pin_name() {
    super::help(&[
        "onerom",
        "firmware",
        "build",
        "--board",
        "fire-24-f",
        "--reserve-pin",
        "banana",
    ]);
}

#[tokio::test]
async fn reserved_pins_with_firmware_before_0_8_0() {
    let files = Files::with_2364(7, &["single"], &[]);
    let line = "onerom firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin --reserve-pin sel_c";
    build(line, &files).await;
}

/// - A banked set fails with X1 reserved.
/// - A single set on a fire-24-a builds with X1 and X2 reserved, although its
///   address read window spans them.
#[tokio::test]
async fn a_banked_set_with_x1_reserved() {
    let files = Files::with_2364(8, &["single", "banked"], &[]);
    let line = "onerom firmware build --board fire-24-f --config sets.json --base-firmware base.bin --output out.bin --reserve-pin x1";
    build(line, &files).await;
    println!();
    let files = Files::with_2364(8, &["single"], &[]);
    let line = "onerom firmware build --board fire-24-a --config sets.json --base-firmware base.bin --output out.bin --reserve-pin x1 --reserve-pin x2";
    build(line, &files).await;
}
