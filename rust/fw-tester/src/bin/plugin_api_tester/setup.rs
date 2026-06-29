// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Firmware boot and emulator initialisation.

use std::process;

use log::error;
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_fw_emulator::Emulator;

/// Boot the firmware for the given `sel_image`, retrieve and parse the
/// firmware version, and return a ready [`Emulator`] alongside the parsed
/// [`FirmwareVersion`].
///
/// `sel_image` drives the image-select pins, so the firmware boots, loads, and
/// serves the corresponding flash slot.  Called once per slot under test.
///
/// Exits the process immediately on limp mode or version parse failure.  A
/// boot failure on any slot is therefore fatal to the whole run; if per-slot
/// boot errors should instead be recorded and the run continued, this would
/// need to return a `Result`.
///
/// No epio setup is performed here — callers that need GPIO/cycle operations
/// must call [`Emulator::setup_epio`] themselves after boot.
pub fn setup(board: Board, log_enabled: bool, sel_image: u8) -> (Emulator, FirmwareVersion) {
    Emulator::set_logging(log_enabled);
    Emulator::set_rp_variant(board.rp_variant());
    Emulator::set_sel_image(sel_image);

    let emulator = Emulator::boot();

    if emulator.limp_mode() {
        error!("Firmware entered limp mode (sel_image={})", sel_image);
        process::exit(1);
    }

    let (result, version_str) = emulator.get_device_version(64);
    if !result.is_ok() {
        error!("Failed to get device version: {:?}", result);
        process::exit(1);
    }
    let version_str = match version_str {
        Some(s) => s,
        None => {
            error!("get_device_version returned OK but no version string");
            process::exit(1);
        }
    };

    // Strip leading 'v' prefix before parsing (e.g. "v0.7.0" → "0.7.0")
    let stripped = version_str
        .strip_prefix('v')
        .unwrap_or(&version_str)
        .to_string();
    let fw_version = FirmwareVersion::try_from_str(&stripped).unwrap_or_else(|e| {
        error!("Failed to parse firmware version '{}': {}", version_str, e);
        process::exit(1);
    });

    (emulator, fw_version)
}
