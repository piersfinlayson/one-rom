// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::utils::check_device_nand_board;
use crate::{args, utils};
use onerom_cli::{Error, Options};

pub async fn cmd_scan(options: &Options, args: &args::scan::ScanArgs) -> Result<(), Error> {
    if args.list_boards {
        let supported = utils::get_supported_boards();
        println!("One ROM board types: {supported}");
        return Ok(());
    }

    check_device_nand_board(options, &args.board)?;

    let board = if let Some(board) = &args.board {
        let board = onerom_config::hw::Board::try_from_str(board)
            .ok_or_else(|| Error::InvalidBoard(board.clone(), utils::get_supported_boards()))?;
        println!("Scanning for {board} ... ");
        Some(board)
    } else {
        println!("Scanning ... ");
        None
    };

    // Scan for devices
    let mut devices = onerom_cli::scan::scan(options, board).await?;

    // Do serial number filtering if requested
    if let Some(serial) = args.serial.as_ref() {
        devices.retain(|d| d.matches_serial(serial));
    }

    if devices.is_empty() {
        println!("No matching One ROM devices found.");
        return Ok(());
    }

    let num_devices = devices.len();
    println!(
        "found {} connected device{}:",
        num_devices,
        if num_devices == 1 { "" } else { "s" }
    );

    for d in &devices {
        if args.slots {
            println!("---");
            crate::inspect::output_slot_info(d, options, "")
                .await
                .inspect_err(|_| log::error!("Failed to read slots"))
                .ok();
        } else {
            println!("  {d}");
        }
    }

    Ok(())
}