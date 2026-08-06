// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom scan`.

use crate::args::CommandTrait;
use clap::Args;

/// Discover and list all connected One ROMs.
///
/// Displays each device's serial number, USB location, user-assigned name
/// (if set), board type, MCU, and currently loaded firmware version.
///
/// Example:
///
///   onerom scan
///
///   onerom scan --board fire-24-e
#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Only show devices matching this board type (e.g. fire-24-e).
    #[arg(long, short, value_name = "BOARD", conflicts_with = "list_boards")]
    pub board: Option<String>,

    /// List all known board types.
    #[arg(long)]
    pub list_boards: bool,

    /// Show the slot contents for each One ROM found.
    #[arg(long, visible_alias = "slot", conflicts_with = "list_boards")]
    pub slots: bool,

    // Private argument to pass the serial from the parent command for
    // filtering in the scan command.
    #[arg(skip)]
    pub serial: Option<String>,
}

impl CommandTrait for ScanArgs {
    fn requires_device(&self) -> bool {
        false
    }
}
