// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `--pin`, and `control pin`'s refusals.

use onerom_cli::Error;
use onerom_cli::image::parse_firmware;
use onerom_config::chip::ChipType;
use onerom_config::hw::Board;

use super::inspect_gpio::served_entries;
use super::{failed, help, written_failure};
use crate::control::{PIN_FORCE_HINT, describe_gpio, gpio_in_use};
use crate::test_board::image_2364;

#[test]
fn image_select_pads_past_sel_e() {
    help(&["onerom", "inspect", "gpio", "--pin", "sel_f"]);
    println!();
    help(&["onerom", "inspect", "gpio", "--pin", "sel_g"]);
}

#[test]
fn names_that_arent_pins() {
    help(&["onerom", "inspect", "gpio", "--pin", "sel_h"]);
    println!();
    help(&["onerom", "inspect", "gpio", "--pin", "a17"]);
}

#[test]
fn helps() {
    for command in [
        &["control", "pin"][..],
        &["control", "reset"],
        &["inspect", "gpio"],
    ] {
        let mut words = vec!["onerom"];
        words.extend(command);
        words.push("--help");
        help(&words);
        println!();
    }
}

/// GPIO0 is D3 of a 2364 on a fire-24-f. Only the error and the pin's use come
/// from running the code.
#[tokio::test]
async fn a_data_pin_serving_and_in_standby() {
    let board = Board::Fire24F;
    let image = parse_firmware(&image_2364(1, &[])).await;
    let gpio_use = served_entries(&image, &board)[0].gpio_use().unwrap();
    let name = describe_gpio(Some(&board), Some(ChipType::Chip2364), 0);
    for (state, standby) in [("serving", false), ("in standby", true)] {
        println!("### {state}");
        println!("$ onerom control pin --pin gpio0 --state low");
        failed(gpio_in_use(&name, gpio_use, standby, PIN_FORCE_HINT));
        println!();
    }
}

/// The USB system plugin reports GPIO control without the feature bits.
#[test]
fn firmware_that_predates_gpio_control() {
    println!("$ onerom control pin --pin x1 --state low");
    written_failure(&Error::FirmwareTooOldForGpio(
        "One ROM Fire 24 F - Firmware: v0.7.0 State: Running Serial: DE3F9C232F655B6B".to_string(),
    ));
}
