// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! GPIO pull configuration (point I, pull portion).
//!
//! For Banked sets, X1 (and X2, for 3-bank or 4-bank sets) need an internal
//! pull in the direction *opposite* `board.x_jumper_pull()`: the bank-select
//! jumper pulls the pin one way when fitted, so the GPIO's internal pull
//! is set the other way - giving a clean binary "jumper fitted / not
//! fitted" read on that pin, with both values then populated in the ROM
//! table.
//!
//! Single and Multi sets need no pull entries: Multi's X1/X2 are driven by
//! the PIO via dedicated CS lines (see `cs_data_layout`), not jumpers.

use alloc::vec::Vec;

use onerom_config::hw::Board;
use onerom_metadata::OneromAlgPullConfig;

use crate::image::ChipSetType;

use super::addr_layout::AddrLayout;

/// Encode one pull entry per `OneromAlgPullConfig`'s byte format: MSB=1 ->
/// pull up, MSB=0 -> pull down, low bits = GPIO number.
fn encode_pull(gpio: u8, pull_up: bool) -> u8 {
    if pull_up { 0x80 | gpio } else { gpio }
}

/// Build the GPIO pull configuration for a chip set, if any is needed.
///
/// `num_chips` is `chips.len()` for the set: 2 -> 2-bank (X1 only), 3 or
/// 4 -> 3-bank or 4-bank (X1+X2), for Banked sets. Single/Multi always
/// return `None`.
///
/// For a 3-chip set, bank index 3 (X1 and X2 both set) maps to "no chip"
/// in `build_rom_image`, but both jumpers still exist and both GPIOs need
/// internal pulls so the RP2350 reads them cleanly when neither is fitted.
///
/// For belt and braces we set the pulls on both GPIOs for each X pin (if
/// there are more than 1).  This isn't strictly necessary, as, by definition,
/// the two GPIOs for each X pin are connected together, so a single pull
/// affects both.  But it makes it easier to test using `fw-tester`.
pub fn build_gpio_pull_config(
    _layout: &AddrLayout,
    set_type: ChipSetType,
    num_chips: usize,
    board: Board,
) -> Option<OneromAlgPullConfig> {
    if !matches!(set_type, ChipSetType::Banked) {
        return None;
    }

    // Pull direction is the inverse of the jumper's fitted direction.
    let pull_up = board.x_jumper_pull() == 0;

    let mut params: Vec<u8> = Vec::with_capacity(2);

    for &gpio in board.gpios_for_x_pin(1) {
        params.push(encode_pull(gpio, pull_up));
    }

    #[allow(clippy::collapsible_if)]
    if num_chips >= 3 {
        for &gpio in board.gpios_for_x_pin(2) {
            params.push(encode_pull(gpio, pull_up));
        }
    }

    if params.is_empty() {
        None
    } else {
        Some(OneromAlgPullConfig { params })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Fire24A: x_jumper_pull() == 0 (jumper pulls low when fitted), so the
    /// GPIO pull is the inverse: pull-up (MSB set).
    ///
    /// The x1/x2 values passed here are no longer used by
    /// `build_gpio_pull_config` — pull GPIOs are now derived from
    /// `board.x_pin_map()` so that all bond GPIOs for each X pin are covered.
    fn layout_with_x(x1: u8, x2: u8) -> AddrLayout {
        AddrLayout {
            gpio_base: 0,
            num_addr_pins: 16,
            x1_gpio: Some(x1),
            x2_gpio: Some(x2),
            addr_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12],
            excess_addr_pin_gpios: alloc::vec![],
        }
    }

    /// 2-bank Banked set: only X1 gets a pull entry.
    #[test]
    fn banked_2bank_pulls_x1_only() {
        let layout = layout_with_x(14, 15);

        let pulls = build_gpio_pull_config(&layout, ChipSetType::Banked, 2, Board::Fire24A)
            .expect("2-bank Banked set should have pull config");

        assert_eq!(pulls.params, alloc::vec![encode_pull(9, true)]);
    }

    /// 3-bank Banked set: both X1 and X2 get pull entries. Bank index 3
    /// (X1 and X2 both set) maps to "no chip" in build_rom_image, but both
    /// jumpers still exist and both GPIOs need pulls to read cleanly when
    /// not fitted.
    #[test]
    fn banked_3bank_pulls_x1_and_x2() {
        let layout = layout_with_x(14, 15);

        let pulls = build_gpio_pull_config(&layout, ChipSetType::Banked, 3, Board::Fire24A)
            .expect("3-bank Banked set should have pull config");

        assert_eq!(
            pulls.params,
            alloc::vec![encode_pull(9, true), encode_pull(8, true)]
        );
    }

    /// 4-bank Banked set: both X1 and X2 get pull entries.
    #[test]
    fn banked_4bank_pulls_x1_and_x2() {
        let layout = layout_with_x(14, 15);

        let pulls = build_gpio_pull_config(&layout, ChipSetType::Banked, 4, Board::Fire24A)
            .expect("4-bank Banked set should have pull config");

        assert_eq!(
            pulls.params,
            alloc::vec![encode_pull(9, true), encode_pull(8, true)]
        );
    }

    /// Single sets never need pull entries.
    #[test]
    fn single_no_pulls() {
        let layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 16,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12],
            excess_addr_pin_gpios: alloc::vec![],
        };

        let pulls = build_gpio_pull_config(&layout, ChipSetType::Single, 1, Board::Fire24A);
        assert_eq!(pulls, None);
    }

    /// Multi sets never need pull entries (X1/X2 are CS lines for chip1/2,
    /// not jumpers).
    #[test]
    fn multi_no_pulls() {
        let layout = layout_with_x(14, 15);

        let pulls = build_gpio_pull_config(&layout, ChipSetType::Multi, 3, Board::Fire24A);
        assert_eq!(pulls, None);
    }
}
