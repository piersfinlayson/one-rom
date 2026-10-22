// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `monitor log`.

use std::path::Path;

use onerom_cli::Error;

use super::{command_of, help, written_failure};
use crate::args::Commands;
use crate::args::monitor::MonitorCommands;
use crate::monitor::{Source, ended_line, monitoring_line, source_line, wrote_line};

const PORT: &str = "/dev/cu.usbmodem0123456789ABCDEF1";

const SERIAL_OUTPUT: &str = "\
----- One ROM USB log -----
One ROM fire-28-a v0.8.1
Serial: 0123456789ABCDEF
Logging: boot, error, plugin-application
---------------------------
-----
One ROM v0.8.1.1 https://onerom.org
-----
RP235XA
";

const RAM_OUTPUT: &str = "\
-----
One ROM v0.8.1.1 https://onerom.org
-----
RP235XA
";

/// Prints the transcript of `line` reading `output` from `source`. `stopped`
/// is whether Ctrl-C ended the session.
fn transcript(line: &str, source: &Source, output: &str, stopped: bool) {
    println!("$ {line}");
    let (command, options) = command_of(&line.split_whitespace().collect::<Vec<_>>());
    let Commands::Monitor(monitor) = command else {
        panic!("not a monitor command");
    };
    let MonitorCommands::Log(args) = monitor.command;
    let capture = args.output.as_deref().map(Path::new);

    if options.verbose {
        println!("~ {}", source_line(source));
    }
    println!("~ {}", monitoring_line(capture));
    for text in output.lines() {
        println!("~ {text}");
    }
    println!("~ {}", ended_line(stopped));
    if let Some(path) = capture {
        // The One ROM ends each line with CRLF.
        let copied = output.lines().map(|text| text.len() as u64 + 2).sum();
        println!("~ {}", wrote_line(copied, path));
    }
    println!();
}

#[test]
fn helps() {
    help(&["onerom", "monitor", "log", "--help"]);
}

#[test]
fn from_the_serial_port() {
    let source = Source::Serial(PORT.to_string());
    transcript("onerom monitor log", &source, SERIAL_OUTPUT, true);
    transcript("onerom monitor log --verbose", &source, SERIAL_OUTPUT, true);
    transcript(
        "onerom monitor log --output boot.txt",
        &source,
        SERIAL_OUTPUT,
        false,
    );
}

/// A One ROM with a plugin that has exclusive use of the serial port.
#[test]
fn from_ram() {
    transcript("onerom monitor log", &Source::Ram, RAM_OUTPUT, true);
    transcript(
        "onerom monitor log --verbose",
        &Source::Ram,
        RAM_OUTPUT,
        true,
    );
    transcript(
        "onerom monitor log --output boot.txt",
        &Source::Ram,
        RAM_OUTPUT,
        false,
    );
}

/// The details are copied from `onerom_cli::ram_log`.
#[test]
fn a_log_not_found_in_ram() {
    for detail in [
        "The firmware doesn't provide the log's location.",
        "RAM at 0x20001000 doesn't contain the log's control block.",
        "The log's control block has a buffer of 0 bytes at 0x00000000.",
        "The log's write offset 2048 is outside its 2048 bytes.",
    ] {
        println!("$ onerom monitor log");
        println!("~ {}", monitoring_line(None));
        written_failure(&Error::LogNotFound(detail.to_string()));
        println!();
    }
}
