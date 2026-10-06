// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `control reset`.

use onerom_cli::image::parse_firmware;
use onerom_config::hw::Board;

use super::command_of;
use super::inspect_gpio::served_entries;
use crate::args::Commands;
use crate::args::control::ControlCommands;
use crate::control::{check_reset_pin, needs_force, reset_asserted_line};
use crate::test_board::image_2364_for;

/// X1 is input forced on a fire-24-a serving a 2364.
#[tokio::test]
async fn an_input_forced_pin() {
    let line = ["onerom", "control", "reset", "--pin", "x1"];
    println!("$ {}", line.join(" "));
    let (command, _) = command_of(&line);
    let Commands::Control(control) = command else {
        panic!("not control");
    };
    let ControlCommands::Reset(args) = control.command else {
        panic!("not control reset");
    };

    let board = Board::Fire24A;
    let pin = args.pin.resolve(Some(&board)).unwrap();
    let image = parse_firmware(&image_2364_for(board, 1, &[])).await;
    let entries = served_entries(&image, &board);
    let gpio_use = entries[pin.gpio() as usize].gpio_use().unwrap();
    if needs_force(gpio_use) {
        println!("~ (fails, as One ROM reports {pin} as {gpio_use:?})");
    } else {
        println!("{}", reset_asserted_line(pin, args.hold));
    }
}

/// SEL_A on a fire-40-c is 3.3V-only.
#[test]
fn a_three_volt_three_pin() {
    let line = ["onerom", "--yes", "control", "reset", "--pin", "sel_a"];
    println!("$ {}", line.join(" "));
    let (command, options) = command_of(&line);
    let Commands::Control(control) = command else {
        panic!("not control");
    };
    let ControlCommands::Reset(args) = control.command else {
        panic!("not control reset");
    };
    check_reset_pin(&options, &args.pin, Some(&Board::Fire40C)).unwrap();
    println!("~ (continues)");
}
