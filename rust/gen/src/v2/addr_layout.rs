// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Address-range layout derivation for One ROM v2 metadata generation.
//!
//! This covers point C from the design discussion: for a chip set, find
//! the smallest contiguous GPIO range (>= 16 bits / 64KB) that covers all
//! the address-line GPIOs for the chip(s) involved, plus X1/X2 for
//! Multi/Banked sets, *and* fits within a single PIO GPIO window (see
//! `gpio_window`). That range becomes `alg_addr`'s `gpio_base` /
//! `num_addr_pins` / `num_rom_table_bits` (the latter two being equal).
//!
//! For Multi/Banked sets, the resolved GPIOs for X1/X2 *within this
//! range* are also recorded (`x1_gpio`/`x2_gpio`). Note these may differ
//! from X1/X2's GPIOs as resolved by `cs_data_layout` (Multi only): a
//! dual-bonded X pin can legitimately appear at different GPIOs in the
//! address-range PIO vs the CS/data-range PIO.
//!
//! X1 is included for any Multi/Banked set (always >= 2 chips). X2 is
//! only included for sets with >= 3 chips (4-bank Banked, or Multi with 3+
//! chips) - mirroring `cs_data_layout`'s `chip_types.len() >= 3` condition
//! for X2. For a 2-chip set, `x2_gpio` is `None`: including X2 in the
//! range unconditionally would (a) waste a table-index bit that
//! `build_rom_image` never reads for 2-bank sets, inflating
//! `num_addr_pins`/the ROM table size beyond what's needed, and (b)
//! require an X2 GPIO mapping on boards that don't have/need one for a
//! 2-chip set.
//!
//! `bit_mode` (the *effective* bit mode for chip0 - from
//! `alg_config::bit_mode_for`, accounting for any `force_16_bit`/future
//! `force_8_bit` override, computed once by the caller and shared with
//! `build_alg_config`) determines whether `chip0.address_pins()[0]` is
//! included as an address-PIO line:
//!
//! - `BitMode8`: included, as today - `num_addr_lines ==
//!   chip0.num_addr_lines()`, starting from `address_pins()[0]`.
//! - `BitMode16`: excluded. By convention, any chip with `bit_modes`
//!   including 16 has `address_pins()[0] == "A-1"` - the chip pin shared
//!   with its highest data line (e.g. 27C400/27C200's D15/A-1), used by
//!   `AlgData1`'s data-write PIO (`a_minus_1_pin`, derived separately from
//!   `cs_data_layout`) rather than the address-read PIO.
//!   `num_addr_lines == chip0.num_addr_lines() - 1`, starting from
//!   `address_pins()[1]`.
//!
//! `addr_pin_gpios` records, for each of chip0's address lines actually
//! included (in `chip0.address_pins()` order, from the starting index
//! above), which GPIO (within `[gpio_base, gpio_base + num_addr_pins)`)
//! it resolved to under the winning dual-bond combo - needed by
//! `build_rom_pin_map` to populate `OneromRomPinMap::addr`.
//!
//! Deliberately decoupled from `Chip`/`ChipSet` (which carry ROM image
//! data, CS config, filenames etc. - none of which this needs): takes just
//! the `ChipSetType` and the `ChipType`s involved, so it's directly
//! testable without constructing full chip-set objects.

use alloc::collections::BTreeSet;
use alloc::string::ToString;
use alloc::vec::Vec;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_metadata::BitModes;

use crate::image::ChipSetType;
use crate::Error;

use super::gpio_window::fits_pio_window;

/// Minimum width (in GPIOs/bits) of the address-read range, i.e. the
/// minimum 64KB ROM table size.
const MIN_ADDR_PINS: u8 = 16;

/// Resolved address-range layout for one chip set: the address PIO reads
/// `num_addr_pins` contiguous GPIOs starting at `gpio_base`, and that
/// value indexes the ROM table directly
/// (`num_rom_table_bits == num_addr_pins`, per agreement).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddrLayout {
    pub gpio_base: u8,
    pub num_addr_pins: u8,

    /// Resolved GPIO for X1, within the address range, for Multi/Banked
    /// sets. `None` for Single.
    pub x1_gpio: Option<u8>,

    /// Resolved GPIO for X2, within the address range, for Multi/Banked
    /// sets with >= 3 chips (4-bank Banked, or Multi with 3+ chips).
    /// `None` for Single, and for 2-chip Multi/Banked sets.
    pub x2_gpio: Option<u8>,

    /// Resolved GPIO for each of chip0's *included* address lines (see
    /// module docs re: `BitMode16` excluding `address_pins()[0]`), in
    /// `chip0.address_pins()` order. Length == `chip0.num_addr_lines()`
    /// for `BitMode8`, or `chip0.num_addr_lines() - 1` for `BitMode16`.
    pub addr_pin_gpios: Vec<u8>,
}

/// Layout-derivation failure: this chip type (and configuration) isn't
/// servable on this board.
///
/// Carries `board` so [`From<LayoutError> for Error`] can build a
/// user-facing [`Error`] without needing extra context from the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// A physical pin required by this chip type has no GPIO mapping on
    /// this board (`socket_pin_map`/`x_pin_map` returned empty).
    UnmappedPin { board: Board, chip_type: ChipType, phys_pin: u8 },

    /// Multi/Banked set requires X1 and/or X2, but the board doesn't
    /// define them. X1 is required for any Multi/Banked set; X2 is only
    /// required for sets with >= 3 chips.
    MissingXPin { board: Board, x_pin: u8 },

    /// No combination of pin-bond choices produced a valid address range:
    /// either none met `MIN_ADDR_PINS`/contiguity, or none fit within a
    /// single PIO GPIO window (see `gpio_window::fits_pio_window`).
    NoValidLayout { board: Board },

    /// This chip type has no recognised "select" control line (no `ce` or
    /// `cs1` in `control_lines()`). Used by CS/data layout derivation.
    NoSelectLine { board: Board, chip_type: ChipType },

    /// The select-line GPIOs (CS1[/CS2/CS3] or CS1+X1[+X2]) aren't
    /// contiguous on this board, under any dual-bond combination. The PIO
    /// CS-detect algorithm requires a contiguous range, so this chip type
    /// (with this CS configuration) isn't currently servable on this
    /// board.
    NonContiguousSelect { board: Board, chip_type: ChipType, gpios: Vec<u8> },
}

impl From<LayoutError> for Error {
    fn from(err: LayoutError) -> Self {
        match err {
            LayoutError::UnmappedPin { board, chip_type, .. }
            | LayoutError::NoSelectLine { board, chip_type }
            | LayoutError::NonContiguousSelect { board, chip_type, .. } => {
                Error::UnsupportedBoardChipType { board, chip_type }
            }
            LayoutError::MissingXPin { board, x_pin } => Error::UnsupportedBoardConfig {
                board,
                reason: alloc::format!(
                    "Board does not define X{x_pin} pin, required for Multi/Banked sets"
                ),
            },
            LayoutError::NoValidLayout { board } => Error::UnsupportedBoardConfig {
                board,
                reason: "No valid GPIO layout found for this chip set on this board".to_string(),
            },
        }
    }
}

/// Get the candidate GPIO(s) for chip_type's address line at
/// `address_pins()` index `n`, on `board`.
fn addr_line_candidates(
    board: Board,
    chip_type: ChipType,
    n: usize,
) -> Result<&'static [u8], LayoutError> {
    let phys_pin = chip_type.address_pins()[n];
    let gpios = board.gpios_for_socket_pin(phys_pin);
    if gpios.is_empty() {
        return Err(LayoutError::UnmappedPin { board, chip_type, phys_pin });
    }
    Ok(gpios)
}

/// Derive the address-range layout for a chip set.
///
/// `chip_types` is the chip type(s) in the set, in slot order
/// (`chip_types[0]` is the "primary" chip whose address lines define the
/// range; for Multi/Banked, X1 (and X2 for 3+ chip sets) are added on
/// top).
///
/// `bit_mode` is chip0's *effective* bit mode (see module docs) - for
/// `BitMode16`, `chip0.address_pins()[0]` ("A-1") is excluded from the
/// address-PIO range.
///
/// QUESTION/TODO: for Multi sets with heterogeneous chip types, chip[0]'s
/// address-line layout is assumed representative of the whole set. Revisit
/// if that's not actually true in practice.
pub fn derive_addr_layout(
    board: Board,
    set_type: ChipSetType,
    chip_types: &[ChipType],
    bit_mode: BitModes,
) -> Result<AddrLayout, LayoutError> {
    let chip0 = chip_types[0];

    // For BitMode16 (AlgData1), address_pins()[0] is "A-1" - handled
    // separately by the data-write PIO (a_minus_1_pin, derived from
    // cs_data_layout), not part of the address-PIO range. Skip it here.
    //
    // TODO: for BitMode8 (force_8_bit, not yet implemented),
    // address_pins()[0] is included as a normal address line via AlgData0
    // (the branch below). On boards where A-1's GPIO sits just outside
    // the natural span of the *other* address lines (e.g. fire-40-b,
    // where A17=GPIO36 and A-1=GPIO37 are adjacent but A-1 isn't itself a
    // "real" address line for a smaller chip like 27C200), including it
    // can drag an extra "padding pool" bit into the range, doubling the
    // ROM table beyond what's needed. A future BitMode8 variant that also
    // handles A-1 via the data-write PIO (like AlgData1 does) could avoid
    // this - revisit if/when force_8_bit is implemented.
    let addr_line_start = if matches!(bit_mode, BitModes::BitMode16) { 1 } else { 0 };
    let num_addr_lines = chip0.num_addr_lines() - addr_line_start;

    // --- Step 1: gather per-bit candidate GPIO sets -----------------------
    let mut candidates: Vec<&'static [u8]> = Vec::with_capacity(num_addr_lines + 2);
    for n in 0..num_addr_lines {
        candidates.push(addr_line_candidates(board, chip0, addr_line_start + n)?);
    }

    // Track which candidate slots (if any) are X1/X2, so we can read back
    // their resolved GPIO for the winning combo below.
    let mut x1_idx: Option<usize> = None;
    let mut x2_idx: Option<usize> = None;

    if matches!(set_type, ChipSetType::Multi | ChipSetType::Banked) {
        // X1: required for any Multi/Banked set (always >= 2 chips).
        let x1 = board.gpios_for_x_pin(1);
        if x1.is_empty() {
            return Err(LayoutError::MissingXPin { board, x_pin: 1 });
        }
        x1_idx = Some(candidates.len());
        candidates.push(x1);

        // X2: only needed (and only included in the address range) for
        // sets with >= 3 chips - 2-chip sets select between chips[0]/[1]
        // with X1 alone, and including an unused X2 here would just waste
        // a table-index bit (and require an X2 mapping the board may not
        // have).
        if chip_types.len() >= 3 {
            let x2 = board.gpios_for_x_pin(2);
            if x2.is_empty() {
                return Err(LayoutError::MissingXPin { board, x_pin: 2 });
            }
            x2_idx = Some(candidates.len());
            candidates.push(x2);
        }
    }

    // --- Step 2/3: enumerate dual-bond combinations, score each ----------
    //
    // Each candidate slot has 1 or 2 options. Enumerate the cartesian
    // product over slots with 2 options (2^k combinations - expect k to be
    // 0-2 in practice).
    let two_option_slots: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, opts)| opts.len() > 1)
        .map(|(i, _)| i)
        .collect();

    let num_combos: u32 = 1 << two_option_slots.len();

    let mut best: Option<(AddrLayout, u32)> = None;

    for combo in 0..num_combos {
        // Per-slot resolution for this combo, parallel to `candidates`.
        // Kept (not just collapsed into the BTreeSet below) so X1/X2 and
        // each address line's chosen GPIO can be read back for the
        // winning combo.
        let resolved_vec: Vec<u8> = candidates
            .iter()
            .enumerate()
            .map(|(i, opts)| {
                let choice = two_option_slots
                    .iter()
                    .position(|&s| s == i)
                    .map(|bit| ((combo >> bit) & 1) as usize)
                    .unwrap_or(0);
                opts[choice]
            })
            .collect();

        let resolved: BTreeSet<u8> = resolved_vec.iter().copied().collect();

        // resolved is non-empty (num_addr_lines >= 1 always).
        let min = *resolved.iter().next().unwrap();
        let max = *resolved.iter().last().unwrap();
        let span = max - min + 1;

        let num_addr_pins = span.max(MIN_ADDR_PINS);
        let gpio_base = min;

        // The address PIO can only access a single 32-GPIO window
        // ([0,32) or [16,48)); reject combos whose range doesn't fit in
        // either, even if they'd otherwise meet MIN_ADDR_PINS/be
        // contiguous-enough.
        if !fits_pio_window(gpio_base, num_addr_pins) {
            continue;
        }

        // Lower num_addr_pins wins; gpio_base as a deterministic tiebreak.
        let score = (num_addr_pins as u32) * 1000 + gpio_base as u32;

        if best.as_ref().is_none_or(|(_, best_score)| score < *best_score) {
            best = Some((
                AddrLayout {
                    gpio_base,
                    num_addr_pins,
                    x1_gpio: x1_idx.map(|i| resolved_vec[i]),
                    x2_gpio: x2_idx.map(|i| resolved_vec[i]),
                    addr_pin_gpios: resolved_vec[..num_addr_lines].to_vec(),
                },
                score,
            ));
        }
    }

    best.map(|(layout, _)| layout)
        .ok_or(LayoutError::NoValidLayout { board })
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Fire24A, single 2364.
    ///
    /// 2364's 13 address lines (physical pins 8,7,6,5,4,3,2,1,23,22,19,18,21)
    /// resolve via Fire24A's socket_pin_map to GPIOs
    /// 7,6,5,4,3,2,1,0,10,11,14,15,12 respectively - a 16-wide span (0..=15)
    /// that already meets the 16-bit minimum, so gpio_base=0,
    /// num_addr_pins=16.
    ///
    /// Single set, so no X1/X2: x1_gpio/x2_gpio are None. 2364 doesn't
    /// support bit_mode 16, so BitMode8 (address_pins()[0] included).
    ///
    /// Depends on Fire24A's `socket_pin_map()` being populated.
    #[test]
    fn fire24a_2364_single() {
        let layout = derive_addr_layout(Board::Fire24A, ChipSetType::Single, &[ChipType::Chip2364], BitModes::BitMode8)
            .expect("layout derivation should succeed");

        assert_eq!(
            layout,
            AddrLayout {
                gpio_base: 0,
                num_addr_pins: 16,
                x1_gpio: None,
                x2_gpio: None,
                addr_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12],
            }
        );
    }

    /// Fire28C, Banked (2x 27128).
    ///
    /// 27128's 14 address lines (physical pins
    /// 10,9,8,7,6,5,4,3,25,24,21,23,2,26) resolve via Fire28C's
    /// `socket_pin_to_gpio` to GPIOs 27,26,25,24,23,22,21,20,16,15,13,14,19,17
    /// respectively - spanning GPIO 13..=27 (15-wide).
    ///
    /// X1 (Fire28C's `x_pin_to_gpio["1"] = [9, 28]`) is dual-bonded:
    /// - X1=9 widens the span to 9..=27 (19-wide) -> num_addr_pins=19.
    /// - X1=28 widens the span to 13..=28 (16-wide) -> num_addr_pins=16,
    ///   exactly MIN_ADDR_PINS.
    ///
    /// Both fit within a single PIO window ([0,32)), so the window check
    /// doesn't change the outcome here: X1=28 wins on num_addr_pins
    /// alone, giving gpio_base=13, num_addr_pins=16, x1_gpio=Some(28).
    ///
    /// 2-chip Banked set, so X2 isn't included: x2_gpio=None. 27128
    /// doesn't support bit_mode 16, so BitMode8 (address_pins()[0]
    /// included).
    ///
    /// Depends on Fire28C's `socket_pin_to_gpio`/`x_pin_to_gpio` being
    /// populated.
    #[test]
    fn fire28c_27128_banked_2chip() {
        let layout = derive_addr_layout(
            Board::Fire28C,
            ChipSetType::Banked,
            &[ChipType::Chip27128, ChipType::Chip27128],
            BitModes::BitMode8,
        )
        .expect("layout derivation should succeed");

        assert_eq!(
            layout,
            AddrLayout {
                gpio_base: 13,
                num_addr_pins: 16,
                x1_gpio: Some(28),
                x2_gpio: None,
                addr_pin_gpios: alloc::vec![27, 26, 25, 24, 23, 22, 21, 20, 16, 15, 13, 14, 19, 17],
            }
        );
    }

    /// Fire40A, single 27C400, BitMode16 (AlgData1).
    ///
    /// 27C400's `address_pins()` has 19 entries; index 0 (physical pin 29)
    /// is "A-1" (shared with D15) and is skipped for BitMode16. Indices
    /// 1..19 are A0..A17 (physical pins
    /// 9,8,7,6,5,4,3,2,40,39,38,37,36,35,34,33,32,1), resolving via
    /// Fire40A's `socket_pin_to_gpio` (all single-option - no dual
    /// bonding for these pins) to GPIOs
    /// 36,35,34,33,32,31,30,29,27,26,25,24,23,22,21,20,19,28 - 18
    /// contiguous values (19..=36).
    ///
    /// span == 18 == MIN_ADDR_PINS-or-more, so num_addr_pins=18 exactly
    /// (no padding), gpio_base=19. [19,37) fits the [16,48) PIO window.
    ///
    /// Single set (Fire40A has one 27C400 socket), so x1_gpio/x2_gpio are
    /// None.
    ///
    /// Depends on Fire40A's `socket_pin_to_gpio` being populated.
    #[test]
    fn fire40a_27c400_bitmode16() {
        let layout = derive_addr_layout(
            Board::Fire40A,
            ChipSetType::Single,
            &[ChipType::Chip27C400],
            BitModes::BitMode16,
        )
        .expect("layout derivation should succeed");

        assert_eq!(
            layout,
            AddrLayout {
                gpio_base: 19,
                num_addr_pins: 18,
                x1_gpio: None,
                x2_gpio: None,
                addr_pin_gpios: alloc::vec![36, 35, 34, 33, 32, 31, 30, 29, 27, 26, 25, 24, 23, 22, 21, 20, 19, 28],
            }
        );
    }
}