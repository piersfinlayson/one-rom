// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Chip-board compatibility checking for v2 (Fire/RP2350) boards.
//!
//! [`check_chip_on_board`] runs the full v2 address and CS/data layout
//! derivation for a (board, chip_type) pair and returns a [`CompatResult`]
//! describing the ROM table parameters, or `None` if the combination is not
//! supportable.
//!
//! Used by the `compat` binary to generate the compatibility matrix and
//! per-board chip tables.

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_metadata::BitModes;

use crate::image::{ChipSetType, CsConfig, CsLogic};
use crate::v2::addr_layout::derive_addr_layout;
use crate::v2::alg_config::bit_mode_for;
use crate::v2::cs_data_layout::derive_cs_data_layout;
use crate::v2::slot_context::{SlotContext, socket_pin_offset};

/// ROM table parameters for a supported chip-board combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatResult {
    /// Address bits in the ROM table index. The table has `2^num_addr_pins`
    /// entries and `slot_size_bytes` bytes total.
    pub num_addr_pins: u8,

    /// ROM table size in bytes: `2^num_addr_pins * bytes_per_word`
    /// (bytes_per_word is 1 for BitMode8, 2 for BitMode16).
    pub slot_size_bytes: u32,

    /// Socket-pin translation offset: 0 for native (chip pins == board socket
    /// pins), positive for smaller chip in larger socket (One ROM overhangs
    /// the target socket), negative for larger chip in smaller socket
    /// (fly-leads required from the chip socket's address pins to One ROM's
    /// X1/X2 header pins).
    pub pin_offset: i16,

    /// Number of fly-lead connections required. 0 for native and overhang
    /// combinations; 1 for a single fly-lead to X1, 2 for fly-leads to both
    /// X1 and X2.
    pub num_fly_lead_pins: u8,
}

impl CompatResult {
    /// True if the chip and board socket are the same size — no adapter or
    /// fly-leads needed.
    pub fn is_native(&self) -> bool {
        self.pin_offset == 0
    }

    /// True if the chip has fewer pins than the board socket and One ROM
    /// overhangs the target socket when installed.
    pub fn is_overhang(&self) -> bool {
        self.pin_offset > 0
    }

    /// True if the chip has more pins than the board socket and fly-leads are
    /// required from the chip socket's address pin(s) to One ROM's X1 (and
    /// optionally X2) header pins.
    pub fn requires_fly_leads(&self) -> bool {
        self.pin_offset < 0
    }
}

/// Returns true if `chip_type` is supported in v2 firmware at all, regardless
/// of board. False for plugins and chips not in `SUPPORTED_CHIP_TYPES_V2`.
pub fn is_v2_chip(chip_type: ChipType) -> bool {
    !chip_type.is_plugin() && crate::SUPPORTED_CHIP_TYPES_V2.contains(&chip_type)
}

/// Check if `chip_type` can be served on `board` in a single-chip slot.
///
/// Returns `Some(CompatResult)` if both the address-layout and CS/data-layout
/// derivation succeed. Returns `None` if:
/// - The chip is a plugin or not in `SUPPORTED_CHIP_TYPES_V2`.
/// - `socket_pin_offset` returns `None` (pin counts not a supported pair).
/// - `derive_addr_layout` fails — e.g. the GPIO span for the chip's address
///   lines does not fit any PIO window, or an overhanging address pin cannot
///   be assigned to an X pin.
/// - `derive_cs_data_layout` fails.
///
/// Native, overhang (smaller chip in larger socket), and fly-lead (larger
/// chip in smaller socket) combinations are all evaluated where
/// `socket_pin_offset` permits. For fly-lead results, `num_fly_lead_pins`
/// indicates how many connections from the chip socket's address pins to One
/// ROM's X1/X2 header pins are required.
///
/// CS configuration for the layout check: cs1 = `ActiveLow`, cs2/cs3 =
/// `Ignore`. This activates only the primary select line and is sufficient
/// for layout compatibility checking — polarity does not affect GPIO
/// placement. For CE/OE chips, the cs1 value is irrelevant (their
/// control_lines() do not include cs1); CE and OE fall back to `ActiveLow`
/// via `control_line_logic`'s `ChipSelect` match arm.
pub fn check_chip_on_board(board: Board, chip_type: ChipType) -> Option<CompatResult> {
    if !is_v2_chip(chip_type) {
        return None;
    }

    let pin_offset = socket_pin_offset(chip_type.chip_pins(), board.chip_pins())?;
    let bit_mode = bit_mode_for(chip_type, board);

    let cs_config = CsConfig::new(
        Some(CsLogic::ActiveLow),
        Some(CsLogic::Ignore),
        Some(CsLogic::Ignore),
        None,
    );

    let ctx = SlotContext {
        board,
        set_type: ChipSetType::Single,
        chip_types: alloc::vec![chip_type],
        cs_config,
        bit_mode,
        pin_offset,
        force_16_bit: false,
        multi_cs_config: None,
    };

    let addr_layout = derive_addr_layout(&ctx).ok()?;
    derive_cs_data_layout(&ctx, Some(&addr_layout)).ok()?;

    let bytes_per_word: u32 = if matches!(bit_mode, BitModes::BitMode16) {
        2
    } else {
        1
    };

    // Count overhanging address pins that required fly-leads. Mirrors the
    // logic in derive_addr_layout so the count matches what was actually
    // wired to X pins during layout derivation.
    let num_fly_lead_pins = if pin_offset < 0 {
        let addr_line_start = if matches!(bit_mode, BitModes::BitMode16) {
            1
        } else {
            0
        };
        chip_type.address_pins()[addr_line_start..]
            .iter()
            .filter(|&&ap| {
                let sp = ap as i16 + pin_offset;
                sp < 1 || sp > board.chip_pins() as i16
            })
            .count() as u8
    } else {
        0
    };

    Some(CompatResult {
        num_addr_pins: addr_layout.num_addr_pins,
        slot_size_bytes: (1u32 << addr_layout.num_addr_pins) * bytes_per_word,
        pin_offset,
        num_fly_lead_pins,
    })
}
