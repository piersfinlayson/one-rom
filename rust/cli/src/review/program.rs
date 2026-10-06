// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `program`'s checks of the image and its flash steps.

use onerom_app::{BoardSize, FlashPlan, FlashStep};
use onerom_cli::Options;
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_config::mcu::Variant;
use onerom_gen::FlashChips;
use onerom_metadata::{MaybeKnown, OneromBoardSize};

use super::firmware_build::Files;
use super::{command_of, failed};
use crate::firmware::{build_rom_image, verify_assembled_firmware};
use crate::program::{plan_lines, verify_line};
use crate::test_board::{image_file, move_slot};
use onerom_cli::error::plan_error;

/// The first flash chip's length.
const FIRST_CHIP: usize = 2 * 1024 * 1024;

/// The options `line` sets. There isn't a device.
fn options(line: &str) -> Options {
    command_of(&line.split_whitespace().collect::<Vec<_>>()).1
}

fn chips(size: BoardSize) -> FlashChips {
    FlashChips::new(Variant::RP2350, size)
}

/// Prints the transcript of `line` programming `image` on a fire-40-a, up to
/// the image check.
async fn check_image(line: &str, image: &[u8]) {
    println!("$ {line}");
    let options = options(line);
    let force = line.contains("--force");
    match verify_assembled_firmware(&options, image, force, Some(Board::Fire40A)).await {
        Ok(_) => println!("~ (continues)"),
        Err(e) => failed(e),
    }
}

/// An image file shorter than its slots, one with a slot between the flash
/// chips, then one longer than the first chip without a slot on the second.
/// Each without and with `--force`.
#[tokio::test]
async fn an_image_file_that_doesnt_match_its_slots() {
    let mut short = image_file(BoardSize::M, 3);
    short.truncate(short.len() - 4096);
    let damaged = move_slot(image_file(BoardSize::M, 3), 2, 0x1030_0000).await;
    let mut long = image_file(BoardSize::M, 3);
    long.resize(FIRST_CHIP + 4096, 0xFF);
    for (name, image) in [
        ("an image file 4KB shorter than its slots", short),
        ("an image file with slot 2 between the flash chips", damaged),
        ("an M image file padded past the first chip", long),
    ] {
        println!("### {name}");
        check_image("onerom program --firmware image.bin", &image).await;
        println!();
        check_image("onerom program --firmware image.bin --force", &image).await;
        println!();
    }
}

/// Prints the transcript of `onerom program --firmware image.bin` refusing
/// `image` for a stopped One ROM with `chips` whose recorded size is `size`.
/// The lines before the refusal are the same as for any image.
fn refuse_plan(image: &[u8], chips: &FlashChips, size: Option<MaybeKnown<OneromBoardSize>>) {
    println!("$ onerom program --firmware image.bin");
    let error = FlashPlan::new(image, chips).unwrap_err();
    failed(plan_error(error, image.len(), size));
}

/// An image that uses the second chip, for a board without one of each
/// recorded size. Each has an M board's chips.
#[test]
fn the_second_chip_on_a_board_without_one() {
    use OneromBoardSize::{BoardSizeM, BoardSizeOther, BoardSizeUnknown};
    for (name, size) in [
        ("an M fire-40-a", Some(MaybeKnown::Known(BoardSizeM))),
        (
            "a fire-40-a neither M nor L",
            Some(MaybeKnown::Known(BoardSizeOther)),
        ),
        (
            "a fire-40-a whose size isn't recorded",
            Some(MaybeKnown::Known(BoardSizeUnknown)),
        ),
        ("a fire-40-a whose size couldn't be read", None),
    ] {
        println!("### an L image on {name}");
        refuse_plan(&image_file(BoardSize::L, 4), &chips(BoardSize::M), size);
        println!();
    }
}

/// An image larger than an L board's two chips.
#[test]
fn an_image_larger_than_an_l_board() {
    let mut image = image_file(BoardSize::L, 4);
    image.resize(2 * FIRST_CHIP + 4096, 0xFF);
    println!("### an image 4KB larger than an L fire-40-a's flash");
    let l = Some(MaybeKnown::Known(OneromBoardSize::BoardSizeL));
    refuse_plan(&image, &chips(BoardSize::L), l);
}

/// Prints the lines `--verbose` shows programming and verifying `image` on a
/// board with `chips`. The lines before and after are unchanged.
fn verbose_steps(image: &[u8], chips: &FlashChips) {
    println!("$ onerom program --firmware image.bin --verify --verbose");
    println!("~ (the lines before are unchanged)");
    println!("~ Programming device - DO NOT DISCONNECT");
    let plan = FlashPlan::new(image, chips).unwrap();
    for line in plan_lines(&plan) {
        println!("{line}");
    }
    for step in plan.steps() {
        if let FlashStep::Write { addr, data } = *step {
            println!("{}", verify_line(addr, data));
        }
    }
    println!("~ Verification passed");
    println!("~ (the lines after are unchanged)");
}

/// An image on the first chip, then one that uses the second.
#[test]
fn verbose_steps_for_each_image() {
    println!("### an M image on an L fire-40-a");
    verbose_steps(&image_file(BoardSize::M, 3), &chips(BoardSize::L));
    println!();
    println!("### an L image on an L fire-40-a");
    verbose_steps(&image_file(BoardSize::L, 4), &chips(BoardSize::L));
}

/// Four 512KB sets for a running M fire-40-a. `program` builds for the One
/// ROM's size, so it doesn't advise `--size`.
#[tokio::test]
async fn a_set_that_doesnt_fit_an_m_one_rom() {
    let files = Files::new(8, 4);
    let line = "onerom program --config sets.json --base-firmware base.bin";
    println!("### a running M fire-40-a");
    println!("$ {line}");
    let words = files.words(line);
    let config = std::fs::read_to_string(&words[3]).unwrap();
    let result = build_rom_image(
        &options(line),
        &config,
        FirmwareVersion::new(0, 8, 0, 0),
        Board::Fire40A,
        Variant::RP2350,
        BoardSize::M,
        false,
        |_| Ok(()),
    )
    .await;
    match result {
        Ok(_) => println!("~ (built)"),
        Err(e) => failed(e),
    }
}

#[tokio::test]
async fn a_reset_host_pin_that_isnt_reserved() {
    use crate::program::unreserved_reset_pin;
    use crate::test_board::image_2364;
    use onerom_cli::image::parse_firmware;
    use onerom_cli::pin::parse_pin;

    let pin = parse_pin("x1")
        .unwrap()
        .resolve(Some(&Board::Fire24F))
        .unwrap();
    for (name, reserved) in [
        ("an image that doesn't reserve X1", &[][..]),
        ("an image that reserves X1", &["x1"][..]),
    ] {
        println!("### {name}");
        println!("$ onerom program --firmware image.bin --reset-host x1");
        let image = parse_firmware(&image_2364(1, reserved)).await;
        if let Some(warning) = unreserved_reset_pin(&image, pin) {
            println!("Warning: {warning}");
        }
        println!("~ (continues)");
        println!();
    }
}

#[tokio::test]
async fn a_reset_host_pin_a_slot_uses() {
    use crate::test_board::image_2364_sets;
    use onerom_cli::image::parse_firmware;
    use onerom_cli::pin::parse_pin;

    for (name, board, sets, pin) in [
        ("an address line", Board::Fire24F, &["single"][..], "gpio16"),
        (
            "bank select on X1",
            Board::Fire24F,
            &["single", "banked"][..],
            "x1",
        ),
        (
            "bank select on X1, the GPIO a banked set doesn't read",
            Board::Fire28C,
            &["banked"][..],
            "gpio9",
        ),
        (
            "bank select on X1 in two slots",
            Board::Fire24F,
            &["banked", "single", "banked"][..],
            "x1",
        ),
    ] {
        println!("### {name}, {board}");
        println!("$ onerom program --firmware image.bin --reset-host {pin}");
        let pin = parse_pin(pin).unwrap().resolve(Some(&board)).unwrap();
        let image = parse_firmware(&image_2364_sets(board, sets, &[])).await;
        match crate::control::refuse_reset_pin_in_use(&image, pin) {
            Ok(()) => println!("~ (continues)"),
            Err(e) => failed(e),
        }
        println!();
    }
}

#[test]
fn a_reset_host_pin_the_board_uses() {
    use onerom_cli::pin::parse_pin;

    // GPIO29 is fire-24-f's status LED and RGB LED.
    let line = "onerom program --firmware image.bin --reset-host gpio29";
    println!("### fire-24-f");
    println!("$ {line}");
    let pin = parse_pin("gpio29").unwrap();
    match crate::control::check_reset_pin(&options(line), &pin, Some(&Board::Fire24F)) {
        Ok(_) => println!("~ (continues)"),
        Err(e) => failed(e),
    }
}

#[test]
fn help() {
    super::help(&["onerom", "program", "--help"]);
}
