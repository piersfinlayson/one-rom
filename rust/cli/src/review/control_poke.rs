// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `control poke live` and its alias `poke`.

use onerom_config::chip::ChipType;

use super::{command_of, written_failure};
use crate::args::Commands;
use crate::control::{applied_live_line, poke_data, wrote_live_line};
use crate::utils::live_range;

/// Prints the transcript of `line`, a `poke` on a One ROM serving half a
/// 27C080.
fn poke_27c080(line: &str) {
    println!("$ {line}");
    let words: Vec<&str> = line.split_whitespace().collect();
    let (command, _) = command_of(&words);
    let Commands::Poke(args) = command else {
        panic!("not poke");
    };
    let data = poke_data(args.byte, args.input.as_ref()).unwrap();
    let chip = ChipType::try_from_str("27C080").unwrap();
    let length = Some(data.len() as u32);
    match live_range(chip.name(), chip.size_bytes(), args.address, length) {
        Ok((address, length)) => println!("~ (writes {length} byte(s) to 0x{address:08x})"),
        Err(error) => written_failure(&error),
    }
}

#[test]
fn live_help() {
    super::help(&["onerom", "control", "poke", "live", "--help"]);
}

/// The last byte of the half.
#[test]
fn the_end_of_a_27c080_half() {
    poke_27c080("onerom poke --address 0x7ffff --byte 0xea");
}

/// The first byte past the end of the half.
#[test]
fn past_the_end_of_a_27c080_half() {
    poke_27c080("onerom poke --address 0x80000 --byte 0xea");
}

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
