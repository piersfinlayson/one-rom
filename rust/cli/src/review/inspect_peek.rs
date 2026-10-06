// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `inspect peek`.

use crate::inspect::read_live_line;
use crate::utils::print_hex_dump;

#[test]
fn help() {
    super::help(&["onerom", "inspect", "peek", "memory", "--help"]);
}

/// 16 bytes from live ROM offset 0x100, serving and in standby. The bytes
/// are made up.
#[test]
fn live_serving_and_standby() {
    let data: Vec<u8> = (0..16).collect();
    for (state, standby) in [("serving", false), ("in standby", true)] {
        println!("### {state}");
        println!("$ onerom inspect peek live --address 0x100 --length 16");
        println!("{}", read_live_line(16, 0x100, standby));
        print_hex_dump(0x100, &data);
        println!();
        println!("$ onerom inspect peek live --address 0x100 --length 16 --output rom.bin");
        println!("{}", read_live_line(16, 0x100, standby));
        println!();
    }
}
