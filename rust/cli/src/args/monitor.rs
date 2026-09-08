// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom monitor`.

use crate::args::CommandTrait;
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;

#[derive(Debug, Args)]
pub struct MonitorArgs {
    #[command(subcommand)]
    pub command: MonitorCommands,
}

impl CommandTrait for MonitorArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum MonitorCommands {
    /// Show a running One ROM's log as it is written.
    ///
    /// Attaches to the One ROM's USB serial port and prints the firmware and
    /// plugin logging it sends, until the One ROM is disconnected, rebooted or
    /// stopped.
    ///
    /// What the One ROM has logged since anything last listened arrives first,
    /// so attaching after a reboot still shows the boot log.
    ///
    /// The One ROM must be running, and must have been programmed with the USB
    /// system plugin.  A debug probe reading the log over SWD consumes the same
    /// bytes, so do not use one at the same time.
    ///
    /// Examples:
    ///
    ///   onerom monitor log
    ///
    ///   onerom --serial 1234abcd monitor log
    ///
    ///   onerom monitor log --output boot.txt
    Log(MonitorLogArgs),
}

#[derive(Debug, Args)]
pub struct MonitorLogArgs {
    /// Also write the One ROM's output to this file, replacing its contents.
    ///
    /// The file receives what the One ROM sends and nothing else, so it is a
    /// transcript of the device rather than of this command.  The output still
    /// appears on screen as well.
    #[arg(long, short, visible_alias = "out", value_name = "FILE")]
    pub output: Option<String>,
}

impl CommandTrait for MonitorLogArgs {
    fn requires_device(&self) -> bool {
        true
    }
}
