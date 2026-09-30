// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `control erase`.

use onerom_app::BoardSize;
use onerom_config::mcu::Variant;
use onerom_gen::FlashChips;

use super::{command_of, device, failed};
use crate::args::Commands;
use crate::args::control::{ControlCommands, ControlEraseArgs};
use crate::control::{
    build_erase_ranges, erase_question, erase_range_lines, erased_line, validate_erase_ranges,
};

/// `line`, which starts `onerom control erase`, parsed and checked as the CLI
/// does.
fn erase_args(line: &str) -> (ControlEraseArgs, bool) {
    let (command, options) = command_of(&line.split_whitespace().collect::<Vec<_>>());
    let Commands::Control(control) = command else {
        panic!("not control");
    };
    let ControlCommands::Erase(args) = control.command else {
        panic!("not control erase");
    };
    (args, options.verbose)
}

/// Prints the transcript of `line`, which starts `onerom control erase`, on a
/// stopped `size` fire-40-a, up to the question. Only the lines about the
/// ranges come from running the code.
fn erase(line: &str, size: BoardSize) {
    println!("$ {line}");
    let (args, verbose) = erase_args(line);
    let chips = FlashChips::new(Variant::RP2350, size);
    let ranges = match build_erase_ranges(&args, &chips)
        .and_then(|ranges| validate_erase_ranges(&ranges, &chips).map(|()| ranges))
    {
        Ok(ranges) => ranges,
        Err(e) => {
            failed(e);
            return;
        }
    };
    println!("{}", erase_question(&ranges, verbose));
    println!("~   {}", device("fire-40-a", Some(size)));
    if verbose {
        for line in erase_range_lines(&ranges) {
            println!("{line}");
        }
    }
    println!("~ Auto-accepted (--yes)");
    println!("~ Erasing flash - DO NOT DISCONNECT");
    println!("~ (erases each range)");
    println!("{}", erased_line(&ranges));
}

#[test]
fn help() {
    super::help(&["onerom", "control", "erase", "--help"]);
}

/// `--all` on an M board and an L board, without and with `--verbose`.
#[test]
fn all() {
    for size in [BoardSize::M, BoardSize::L] {
        println!("### an {size} fire-40-a");
        erase("onerom control erase --all --yes", size);
        println!();
        erase("onerom control erase --all --yes --verbose", size);
        println!();
    }
}

/// Ranges on each chip, between the chips and across the end of the first.
#[test]
fn ranges() {
    for size in [BoardSize::M, BoardSize::L] {
        println!("### an {size} fire-40-a");
        for line in [
            "onerom control erase --address 0x101ff000 --length 0x1000 --yes",
            "onerom control erase --address 0x11000000 --length 0x1000 --yes",
            "onerom control erase --offset 0x1000000 --length 0x1000 --yes",
            "onerom control erase --address 0x10200000 --length 0x1000 --yes",
            "onerom control erase --address 0x101ff000 --length 0x2000 --yes",
        ] {
            erase(line, size);
            println!();
        }
    }
}
