// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `inspect header`.

use onerom_config::hw::Board;

use crate::board::show_pin_header;

/// fire-24-a's header has pins fitted but not connected.
#[test]
fn a_header_with_unconnected_pins() {
    println!("$ onerom inspect header --board fire-24-a");
    show_pin_header(&Board::Fire24A);
}
