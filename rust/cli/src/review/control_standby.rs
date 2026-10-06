// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `control standby`.

use onerom_cli::Error;

use super::{command_of, help, written_failure};
use crate::control::standby_line;

/// A running fire-24-f's line. It's written from `Device`'s `Display`.
fn running(version: &str) -> String {
    format!("One ROM Fire 24 F - Firmware: {version} State: Running Serial: DE3F9C232F655B6B")
}

#[test]
fn helps() {
    for command in [
        &["control"][..],
        &["control", "standby"],
        &["control", "standby", "on"],
        &["control", "standby", "off"],
    ] {
        let mut words = vec!["onerom"];
        words.extend(command);
        words.push("--help");
        help(&words);
        println!();
    }
}

/// Success prints nothing unless `--verbose` is provided.
#[test]
fn on_and_off() {
    for (line, standby) in [
        ("onerom control standby on", true),
        ("onerom control standby on --verbose", true),
        ("onerom control standby off", false),
        ("onerom control standby off --verbose", false),
    ] {
        println!("$ {line}");
        let (_, options) = command_of(&line.split_whitespace().collect::<Vec<_>>());
        if options.verbose {
            println!("{}", standby_line(standby));
        }
        println!();
    }
}

/// The USB system plugin's GET_CAPS fails as too old or reports extension
/// version 1.0.
#[test]
fn a_plugin_that_predates_standby() {
    println!("$ onerom control standby on");
    written_failure(&Error::PluginTooOldForStandby(running("v0.8.0")));
}

/// The USB system plugin reports extension version 1.1 without the standby
/// feature bit.
#[test]
fn firmware_that_predates_standby() {
    println!("$ onerom control standby on");
    written_failure(&Error::FirmwareTooOldForStandby(running("v0.7.3")));
}
