// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use crate::usb::enumerate_devices;
use crate::{Device, Error, Options};
use onerom_config::hw::Board;

/// Scan for connected One ROM Fire (RP2350) devices.
///
/// Returns all discovered devices. The board_filter argument is reserved
/// for future use when additional device metadata is available post-scan.
pub async fn scan(options: &Options, board_filter: Option<Board>) -> Result<Vec<Device>, Error> {
    let mut devices = enumerate_devices(options.unrecognised, &options.vid_pid).await?;

    // Now filter based on the board_type
    if let Some(board) = board_filter.as_ref() {
        devices.retain(|d| {
            if let Some(onerom) = d.onerom.as_ref()
                && let Some(sdrr) = onerom.as_original()
                && let Some(flash) = sdrr.flash.as_ref()
                && let Some(flash_board) = flash.board.as_ref()
            {
                flash_board == board
            } else {
                false
            }
        });
    }

    // And/or the device
    if let Some(device) = options.device.as_ref() {
        devices.retain(|d| d.serial == device.serial);
    }

    devices.sort_by_key(|d| d.sort_key());

    Ok(devices)
}