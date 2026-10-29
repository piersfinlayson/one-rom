// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom console`.

use crate::args::CommandTrait;
use clap::{Args, ValueEnum};

/// What Enter sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LineEnding {
    /// A carriage return, 0x0D, which retro systems expect.
    Cr,
    /// A line feed, 0x0A.
    Lf,
    /// Both, 0x0D then 0x0A.
    Crlf,
}

impl LineEnding {
    pub fn bytes(self) -> &'static [u8] {
        match self {
            LineEnding::Cr => b"\r",
            LineEnding::Lf => b"\n",
            LineEnding::Crlf => b"\r\n",
        }
    }
}

#[derive(Debug, Args)]
pub struct ConsoleArgs {
    /// Also write One ROM's output to this file, replacing its contents.
    ///
    /// The file contains only what One ROM sends.  Output is still displayed.
    #[arg(long, short, visible_alias = "out", value_name = "FILE")]
    pub output: Option<String>,

    /// What Enter sends: cr, lf or crlf.
    ///
    /// Default cr, which retro systems expect.
    #[arg(long, value_name = "ENDING", default_value = "cr")]
    pub line_ending: LineEnding,

    /// Send each key as you press it, not a line at a time.
    ///
    /// For programs that read single keys.  No line editing.  Enter sends the
    /// line ending.  Ctrl-C exits.
    #[arg(long)]
    pub raw: bool,

    /// Don't display what you type.
    ///
    /// Use when the retro system echoes it back.  Only applies with --raw.  In
    /// line mode the terminal displays the line as you edit it.
    #[arg(long)]
    pub no_echo: bool,

    /// Serial port to open: 1 or 2.  Default 1.
    ///
    /// - 1 is the USB plugin's default serial port.
    /// - 2 requires the USB plugin and a user plugin that supports it.
    #[arg(
        long,
        value_name = "PORT",
        default_value_t = 1,
        value_parser = clap::value_parser!(u8).range(1..=2),
        verbatim_doc_comment
    )]
    pub serial_port: u8,
}

impl CommandTrait for ConsoleArgs {
    fn requires_device(&self) -> bool {
        true
    }
}
