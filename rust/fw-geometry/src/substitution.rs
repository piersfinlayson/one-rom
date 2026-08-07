// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Board-specific chip substitution.
//!
//! A board that cannot drive a requested chip directly may serve it as a
//! pin-compatible substitute.  Every consumer that drives the socket has to
//! apply the same substitution the firmware does, or it drives the wrong pins.

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;

/// Board-specific chip substitution: some boards cannot drive a requested chip
/// directly and serve it as a pin-compatible substitute instead.
///
/// Returns `Some(substitute)` when a substitution applies, `None` otherwise.
/// Shared by the pio-tester and by One ROM Lens so both drive the substituted
/// chip identically.
pub fn chip_substitution(board: Board, chip_type: ChipType) -> Option<ChipType> {
    match (board, chip_type) {
        // fire-32-a cannot drive SST39SF040 directly; a pin-remap shim allows
        // it to serve the image as a 27C040 instead.
        (Board::Fire32A, ChipType::ChipSST39SF040) => Some(ChipType::Chip27C040),
        _ => None,
    }
}
