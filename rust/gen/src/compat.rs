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
//! per-board chip tables, and by the CLI's `chips` command - which share
//! [`supported_chips`], [`format_size`] and [`CompatResult::fit_description`]
//! so the tool and the document cannot disagree.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use onerom_config::chip::{CHIP_TYPE_NAMES, ChipType};
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

    /// How the chip sits in the board's socket, as a short human-readable
    /// phrase: `native`, `overhang`, `larger socket (no fly-leads)`, or
    /// `fly-lead to X1[ and X2]`.
    ///
    /// The `larger socket (no fly-leads)` case is a chip with more pins than
    /// the board whose extra pins carry no address lines: One ROM sits
    /// bottom-justified in the larger socket and nothing needs wiring. It is
    /// spelled out rather than left as a bare "no fly-leads" because these rows
    /// sit under a "(with fly-leads)" heading, which on its own reads as a
    /// contradiction. It still does not simply drop in - the socket's VCC is
    /// among the pins One ROM cannot reach, so power must be rerouted, as for
    /// any cross-size fit.
    ///
    /// Used by the `compat` binary for `docs/COMPATIBILITY.md`'s per-board
    /// tables and by the CLI's `chips` command, so the two agree.
    pub fn fit_description(&self) -> String {
        if self.is_native() {
            "native".to_string()
        } else if self.requires_fly_leads() {
            match self.num_fly_lead_pins {
                0 => "larger socket (no fly-leads)".to_string(),
                1 => "fly-lead to X1".to_string(),
                2 => "fly-lead to X1 and X2".to_string(),
                n => alloc::format!("fly-lead ({n} pins)"),
            }
        } else {
            "overhang".to_string()
        }
    }
}

/// Render a ROM or image size the way `docs/COMPATIBILITY.md` and the CLI do:
/// whole `MB`/`KB` units where the value divides exactly, `B` below 1KB.
///
/// Every size this is applied to is a power of two, so the truncating division
/// is exact; it is not a general-purpose byte formatter.
pub fn format_size(bytes: u32) -> String {
    if bytes >= 1024 * 1024 {
        alloc::format!("{}MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        alloc::format!("{}KB", bytes / 1024)
    } else {
        alloc::format!("{bytes}B")
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

/// One chip type a board can emulate, as listed by [`supported_chips`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChipCompat {
    /// The chip type.
    pub chip_type: ChipType,

    /// The name this entry was listed under. Chip types with several accepted
    /// spellings appear once per alias (e.g. `2316` and `9316A`), since a user
    /// looking up the part number stamped on their chip needs to find it.
    pub alias: &'static str,

    /// The chip's own storage capacity, which may be smaller than the flash
    /// the image occupies ([`CompatResult::slot_size_bytes`]).
    pub rom_size_bytes: u32,

    /// How the chip fits this board, and how much flash its image uses.
    pub result: CompatResult,
}

/// Sort key ordering fit classes: native, then overhang, then fly-lead.
pub fn pin_offset_order(pin_offset: i16) -> i32 {
    match pin_offset {
        0 => 0,
        n if n > 0 => 1,
        _ => 2,
    }
}

/// Every chip type `board` can emulate, with the flash each one's image uses.
///
/// Ordered as `docs/COMPATIBILITY.md` presents it - native fits first, then
/// overhang, then fly-lead; within a class by how far the chip's pin count is
/// from the board's, then ascending ROM size, then name - so a caller can group
/// consecutive runs of equal `result.pin_offset` into that document's sections.
///
/// Sizes are for a chip served alone in its slot. A banked or multi-chip set
/// draws X1/X2 (and, for a multi set, the per-chip select and commoned control
/// lines) into the slot's address window, which can make its table larger than
/// the figure here; only the builder knows that, since it depends on the set's
/// exact composition.
pub fn supported_chips(board: Board) -> Vec<ChipCompat> {
    let mut entries: Vec<ChipCompat> = CHIP_TYPE_NAMES
        .iter()
        .filter_map(|alias| {
            let chip_type = ChipType::try_from_str(alias)?;
            let result = check_chip_on_board(board, chip_type)?;
            Some(ChipCompat {
                chip_type,
                alias,
                rom_size_bytes: chip_type.size_bytes() as u32,
                result,
            })
        })
        .collect();

    entries.sort_by_key(|e| {
        (
            pin_offset_order(e.result.pin_offset),
            e.result.pin_offset.abs(),
            e.rom_size_bytes,
            e.alias,
        )
    });

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(board: Board, alias: &str) -> ChipCompat {
        *supported_chips(board)
            .iter()
            .find(|e| e.alias == alias)
            .unwrap_or_else(|| panic!("{alias} should be listed for {}", board.name()))
    }

    #[test]
    fn format_size_picks_whole_units() {
        assert_eq!(format_size(512), "512B");
        assert_eq!(format_size(1024), "1KB");
        assert_eq!(format_size(48 * 1024), "48KB");
        assert_eq!(format_size(1024 * 1024), "1MB");
    }

    /// The image sizes and fits `supported_chips` reports are what
    /// `docs/COMPATIBILITY.md` publishes, since the document is generated from
    /// it. Spot-check one entry of each fit class, including the two cases the
    /// figure exists to expose: a chip whose image is far larger than the chip
    /// (2364 overhanging a 28-pin board, 8KB served from a 256KB table) and one
    /// where the two match.
    #[test]
    fn reports_the_documented_image_sizes() {
        let native = find(Board::Fire24F, "2364");
        assert_eq!(native.rom_size_bytes, 8 * 1024);
        assert_eq!(native.result.slot_size_bytes, 8 * 1024);
        assert_eq!(native.result.fit_description(), "native");

        let overhang = find(Board::Fire28C, "2364");
        assert_eq!(overhang.rom_size_bytes, 8 * 1024);
        assert_eq!(overhang.result.slot_size_bytes, 256 * 1024);
        assert_eq!(overhang.result.fit_description(), "overhang");

        let fly_lead = find(Board::Fire24F, "2764");
        assert_eq!(fly_lead.rom_size_bytes, 8 * 1024);
        assert_eq!(fly_lead.result.slot_size_bytes, 32 * 1024);
        assert_eq!(fly_lead.result.fit_description(), "fly-lead to X1");
    }

    /// A chip with more pins than the board, but no address line among the
    /// extra ones, needs no fly-leads - One ROM just sits bottom-justified in
    /// the larger socket. The 32-pin 28C512 on a 28-pin board is the case.
    /// It is still a cross-size fit, not a native one.
    #[test]
    fn larger_socket_without_fly_leads_says_so() {
        let entry = find(Board::Fire28C, "28C512");
        assert!(entry.result.requires_fly_leads());
        assert_eq!(entry.result.num_fly_lead_pins, 0);
        assert!(!entry.result.is_native());
        assert_eq!(
            entry.result.fit_description(),
            "larger socket (no fly-leads)"
        );
    }

    /// Callers group consecutive runs of equal `pin_offset` into the document's
    /// sections, which only works if the entries are ordered by fit class - so
    /// each class must appear exactly once in the listing.
    #[test]
    fn orders_by_fit_class_without_interleaving() {
        for board in [Board::Fire24F, Board::Fire28C, Board::Fire32B] {
            let entries = supported_chips(board);
            assert!(!entries.is_empty(), "{} lists no chips", board.name());

            let classes: Vec<i32> = entries
                .iter()
                .map(|e| pin_offset_order(e.result.pin_offset))
                .collect();
            assert!(
                classes.windows(2).all(|w| w[0] <= w[1]),
                "{} entries are not ordered by fit class: {classes:?}",
                board.name()
            );

            let mut offsets: Vec<i16> = entries.iter().map(|e| e.result.pin_offset).collect();
            offsets.dedup();
            let unique = offsets.len();
            offsets.sort_unstable();
            offsets.dedup();
            assert_eq!(
                unique,
                offsets.len(),
                "{} has a pin offset split across sections",
                board.name()
            );
        }
    }

    /// Every alias of a chip type is listed, so a user can look up the part
    /// number stamped on the chip rather than One ROM's preferred name for it.
    #[test]
    fn lists_each_alias_separately() {
        let entries = supported_chips(Board::Fire24F);
        for alias in ["2316", "9316", "9316A"] {
            assert!(
                entries.iter().any(|e| e.alias == alias),
                "{alias} missing from the fire-24-f listing"
            );
        }
    }

    /// A chip the board cannot serve has no size to report.
    #[test]
    fn omits_unsupported_chips() {
        let entries = supported_chips(Board::Fire24F);
        assert!(entries.iter().all(|e| e.alias != "27C400"));
        assert!(check_chip_on_board(Board::Fire24F, ChipType::Chip27C400).is_none());
    }
}
