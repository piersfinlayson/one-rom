// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use onerom_config::hw::BOARDS;
use onerom_cli::{Error, Options};
use crate::args::BoardArgs;

pub async fn cmd_boards(_options: &Options, _args: &BoardArgs) -> Result<(), Error> {
    println!("Supported One ROM board types:");
    // Comma separate them
    let boards = BOARDS.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", ");
    println!("  {boards}");
    Ok(())
}