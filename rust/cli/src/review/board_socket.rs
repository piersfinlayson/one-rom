// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `board socket`.

use onerom_config::hw::Board;

use crate::board::show_rom_socket;

/// A 28-pin 2764 on 24-pin fire-24-f, which sits at pins 3-26 of the socket.
#[test]
fn a_larger_chip_on_a_smaller_board() {
    println!("$ onerom board socket --board fire-24-f --chip-type 2764");
    show_rom_socket(&Board::Fire24F, &Some("2764".to_string()), false).unwrap();
}
