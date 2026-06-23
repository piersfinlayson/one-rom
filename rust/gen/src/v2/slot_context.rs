// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Per-slot derivation context and socket-pin offset calculation.
//!
//! [`SlotContext`] bundles the board-level and chip-set-level parameters
//! computed once in `build_rom_slot` and shared across
//! `derive_addr_layout`, `derive_cs_data_layout`, `build_alg_config`, and
//! `build_rom_image`. Consolidating them here avoids long individual
//! parameter lists and ensures all derivation functions operate on a
//! consistent view of the same inputs.
//!
//! [`socket_pin_offset`] computes the physical pin number translation
//! needed when a chip with fewer pins than the board socket is installed:
//! the chip sits in the centre of the socket, with its pin 1 offset from
//! socket pin 1 by half the pin-count difference. Only 24/28/32-pin
//! cross-size combinations are supported; 40-pin boards have a different
//! physical layout and are excluded.

use alloc::vec::Vec;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_metadata::BitModes;

use crate::image::{ChipSetType, CsConfig};

use super::multi_cs_config::MultiChipCsConfig;

/// Returns the socket-pin offset for a chip of `chip_pins` pins placed in
/// a board socket of `board_pins` pins, or `None` if the combination is
/// not supported.
///
/// The offset is added to the chip's physical pin number to obtain the
/// corresponding socket pin number: `socket_pin = chip_pin + offset`.
///
/// Supported combinations:
/// - `Some(0)`: same-size (24/24, 28/28, 32/32, 40/40) — no translation.
/// - `Some(2)`: 24-pin chip in 28-pin socket, or 28-pin chip in 32-pin socket.
/// - `Some(4)`: 24-pin chip in 32-pin socket.
/// - `Some(-2)`: 28-pin chip in 24-pin socket, or 32-pin chip in 28-pin socket
///   (fly-lead mode: chip overhangs by 2 pins on each side; address pins in
///   the overhanging zone must be fly-leaded to X pins via `derive_addr_layout`).
/// - `Some(-4)`: 32-pin chip in 24-pin socket
///   (fly-lead mode: chip overhangs by 4 pins on each side).
/// - `None`: all other combinations — including any cross-size combination
///   involving a 40-pin socket.
pub fn socket_pin_offset(chip_pins: u8, board_pins: u8) -> Option<i16> {
    match (chip_pins, board_pins) {
        (24, 24) | (28, 28) | (32, 32) | (40, 40) => Some(0),
        (24, 28) | (28, 32) => Some(2),
        (24, 32) => Some(4),
        (28, 24) | (32, 28) => Some(-2),
        (32, 24) => Some(-4),
        _ => None,
    }
}

/// Per-slot derivation context: parameters computed once in `build_rom_slot`
/// and shared across `derive_addr_layout`, `derive_cs_data_layout`,
/// `build_alg_config`, and `build_rom_image`.
///
/// Bundles the board, set-level configuration, and chip-type information
/// that would otherwise be threaded individually through each derivation
/// function.
pub struct SlotContext {
    /// The board being targeted.
    pub board: Board,

    /// Set type: Single, Multi, or Banked.
    pub set_type: ChipSetType,

    /// Chip types in slot order. `chip_types[0]` is the primary chip whose
    /// address lines define the GPIO range; for Multi/Banked sets,
    /// subsequent entries identify the additional chips.
    /// `chip_types.len()` gives `num_chips`.
    pub chip_types: Vec<ChipType>,

    /// CS configuration for `chip_types[0]`, used for CS polarity and
    /// override derivation.
    pub cs_config: CsConfig,

    /// Effective bit mode for `chip_types[0]`, computed once by
    /// `bit_mode_for`. Shared between `derive_addr_layout` (which excludes
    /// `address_pins()[0]` / "A-1" for `BitMode16`) and `build_alg_config`
    /// (which selects `AlgData0` vs `AlgData1` based on it).
    pub bit_mode: BitModes,

    /// Socket-pin translation offset: `socket_pin = chip_pin + pin_offset`.
    /// `0` for same-size combinations. Positive for chips smaller than the
    /// socket (chip centres in the socket). Negative for chips larger than
    /// the socket (fly-lead mode: chip overhangs; address pins in the
    /// overhanging zone are fly-leaded to X pins by `derive_addr_layout`).
    /// Derived from `socket_pin_offset(chip_types[0].chip_pins(),
    /// board.chip_pins())`.
    pub pin_offset: i16,

    /// When `true` and `bit_mode == BitMode16`, forces `AlgData0` with
    /// `word_size: 16` (native 16-bit word mode, ignoring `/BYTE`) rather
    /// than the default `AlgData1`. No effect for `BitMode8`.
    pub force_16_bit: bool,

    /// Per-chip select and commoned control-line configuration for Multi
    /// sets, derived by `derive_multi_cs_config`. `None` for Single and
    /// Banked sets.
    pub multi_cs_config: Option<MultiChipCsConfig>,
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_size_no_offset() {
        assert_eq!(socket_pin_offset(24, 24), Some(0i16));
        assert_eq!(socket_pin_offset(28, 28), Some(0i16));
        assert_eq!(socket_pin_offset(32, 32), Some(0i16));
        assert_eq!(socket_pin_offset(40, 40), Some(0i16));
    }

    #[test]
    fn valid_cross_size_offsets() {
        assert_eq!(socket_pin_offset(24, 28), Some(2i16));
        assert_eq!(socket_pin_offset(28, 32), Some(2i16));
        assert_eq!(socket_pin_offset(24, 32), Some(4i16));
    }

    #[test]
    fn fly_lead_offsets() {
        // Chip larger than socket: negative offset, fly-lead mode.
        assert_eq!(socket_pin_offset(28, 24), Some(-2i16));
        assert_eq!(socket_pin_offset(32, 28), Some(-2i16));
        assert_eq!(socket_pin_offset(32, 24), Some(-4i16));
    }

    #[test]
    fn unsupported_combinations_return_none() {
        // Chip larger than socket, but not a supported fly-lead pair.
        assert_eq!(socket_pin_offset(40, 24), None);
        assert_eq!(socket_pin_offset(40, 28), None);
        assert_eq!(socket_pin_offset(40, 32), None);
        // 40-pin cross-size not supported (different physical layout).
        assert_eq!(socket_pin_offset(24, 40), None);
        assert_eq!(socket_pin_offset(28, 40), None);
        assert_eq!(socket_pin_offset(32, 40), None);
    }
}
