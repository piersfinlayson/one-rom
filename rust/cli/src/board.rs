// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::args::BoardArgs;
use onerom_cli::{Error, Options};
use onerom_config::hw::BOARDS;

pub async fn cmd_boards(_options: &Options, _args: &BoardArgs) -> Result<(), Error> {
    println!("Supported One ROM board types:");
    // Comma separate them
    let boards = BOARDS
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    println!("  {boards}");
    Ok(())
}
