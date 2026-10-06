// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `inspect peek` and its alias `peek`.

use onerom_config::chip::ChipType;

use super::{command_of, written_failure};
use crate::args::Commands;
use crate::utils::live_range;

/// Prints the transcript of `line`, a `peek` on a One ROM serving half a
/// 27C080.
fn peek_27c080(line: &str) {
    println!("$ {line}");
    let words: Vec<&str> = line.split_whitespace().collect();
    let (command, _) = command_of(&words);
    let Commands::Peek(args) = command else {
        panic!("not peek");
    };
    let chip = ChipType::try_from_str("27C080").unwrap();
    match live_range(chip.name(), chip.size_bytes(), args.address, args.length) {
        Ok((address, length)) => println!("~ (reads {length} byte(s) from 0x{address:08x})"),
        Err(error) => written_failure(&error),
    }
}

#[test]
fn help() {
    super::help(&["onerom", "inspect", "peek", "memory", "--help"]);
}

#[test]
fn live_help() {
    super::help(&["onerom", "inspect", "peek", "live", "--help"]);
}

/// The last 256 bytes of the half.
#[test]
fn the_end_of_a_27c080_half() {
    peek_27c080("onerom peek --address 0x7ff00");
}

/// The first byte past the end of the half.
#[test]
fn past_the_end_of_a_27c080_half() {
    peek_27c080("onerom peek --address 0x80000");
}

/// 512 bytes, half of them past the end of the half.
#[test]
fn across_the_end_of_a_27c080_half() {
    peek_27c080("onerom peek --address 0x7ff00 --length 0x200");
}
