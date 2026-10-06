// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `control poke live`.

use crate::control::{applied_live_line, wrote_live_line};

/// One byte, then a 16 byte file of which 3 bytes differ, serving and in
/// standby. Only the last line of each comes from running the code.
#[test]
fn serving_and_standby() {
    for (state, standby) in [("serving", false), ("in standby", true)] {
        println!("### {state}");
        println!("$ onerom control poke live --address 0x100 --byte 0xea");
        println!("{}", wrote_live_line(1, 0x100, standby));
        println!();
        println!("$ onerom control poke live --address 0x100 --input patch.bin --delta");
        println!("{}", applied_live_line(false, 3, 16, 0x100, standby));
        println!();
        println!("$ onerom control poke live --address 0x100 --input patch.bin --delta --dry-run");
        println!("{}", applied_live_line(true, 3, 16, 0x100, standby));
        println!();
    }
}
