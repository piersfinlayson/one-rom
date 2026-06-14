// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! CS-line polarity overrides (point I, CS portion).
//!
//! The CS-detect PIO requires a fixed convention regardless of how a
//! chip's CS lines are configured:
//! - Single/Banked: all select lines must read active-low.
//! - Multi: all select lines must read active-high.
//!
//! Each select line's *configured* `CsLogic` (from `CsConfig`) may not
//! match that convention - e.g. a 23xx mask ROM configured with
//! `CsLogic::ActiveHigh` CS1 on a Single set. Where it doesn't match, the
//! corresponding GPIO gets a `GpioOverInvert` so the PIO always sees the
//! convention it expects.

use alloc::vec::Vec;

use onerom_metadata::GpioOverride;

use crate::image::{ChipSetType, CsConfig, CsLogic};

use super::cs_data_layout::{CsDataLayout, SelectRole};

/// The `CsLogic` a select line is configured as, for override comparison.
///
/// - `Cs1`/`X1`/`X2` all read `cs_config.cs1_logic()` - for Multi,
///   `multi_cs_logic()` validates all chips share the same `cs1_logic()`,
///   so chip0's value applies to X1 (chip1) and X2 (chip2) too.
/// - `Cs2`/`Cs3` read the corresponding logic; `select_phys_pins` only
///   includes them when `Some(ActiveLow|ActiveHigh)`, so `unwrap_or` here
///   is just a defensive fallback, not expected to be hit.
/// - `Ce` (27xx-style fixed chip-enable) is always active-low.
fn cs_logic_for_role(role: SelectRole, cs_config: &CsConfig) -> CsLogic {
    match role {
        SelectRole::Cs1 | SelectRole::X1 | SelectRole::X2 => cs_config.cs1_logic(),
        SelectRole::Cs2 => cs_config.cs2_logic().unwrap_or(CsLogic::ActiveLow),
        SelectRole::Cs3 => cs_config.cs3_logic().unwrap_or(CsLogic::ActiveLow),
        SelectRole::Ce => CsLogic::ActiveLow,
    }
}

/// The CS polarity the PIO requires, for this set type.
fn required_cs_logic(set_type: ChipSetType) -> CsLogic {
    match set_type {
        ChipSetType::Single | ChipSetType::Banked => CsLogic::ActiveLow,
        ChipSetType::Multi => CsLogic::ActiveHigh,
    }
}

fn encode_override(gpio: u8, ov: GpioOverride) -> u8 {
    ((ov as u8) << 6) | (gpio & 0x3F)
}

/// Build the `GpioOverInvert` entries (point I) needed to make every
/// select line in `layout` conform to the PIO's required CS polarity for
/// `set_type`. The `cs_ignore_index` gap (if any) isn't in
/// `layout.select_lines`, so is untouched.
pub fn build_cs_overrides(layout: &CsDataLayout, set_type: ChipSetType, cs_config: &CsConfig) -> Vec<u8> {
    let required = required_cs_logic(set_type);

    layout
        .select_lines
        .iter()
        .filter_map(|line| {
            let configured = cs_logic_for_role(line.role, cs_config);
            if configured != required {
                Some(encode_override(line.gpio, GpioOverride::GpioOverInvert))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::cs_data_layout::SelectLine;

    fn layout_with(select_lines: Vec<SelectLine>) -> CsDataLayout {
        CsDataLayout {
            gpio_base: 0,
            base_data_pin: 16,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![16, 17, 18, 19, 20, 21, 22, 23],
            base_cs_pin: 13,
            num_cs_pins: select_lines.len() as u8,
            cs_ignore_index: None,
            select_lines,
        }
    }

    /// Single set, CS1 configured ActiveLow (matches required ActiveLow) ->
    /// no override.
    #[test]
    fn single_active_low_no_override() {
        let layout = layout_with(alloc::vec![SelectLine { role: SelectRole::Cs1, gpio: 13 }]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None);

        let overrides = build_cs_overrides(&layout, ChipSetType::Single, &cs_config);
        assert!(overrides.is_empty());
    }

    /// Single set, CS1 configured ActiveHigh (required ActiveLow) ->
    /// GpioOverInvert on GPIO13.
    #[test]
    fn single_active_high_inverted() {
        let layout = layout_with(alloc::vec![SelectLine { role: SelectRole::Cs1, gpio: 13 }]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveHigh), None, None);

        let overrides = build_cs_overrides(&layout, ChipSetType::Single, &cs_config);
        assert_eq!(overrides, alloc::vec![encode_override(13, GpioOverride::GpioOverInvert)]);
    }

    /// Multi set, CS1 configured ActiveLow (required ActiveHigh for Multi)
    /// -> CS1, X1 and X2 all inverted (multi_cs_logic validates they share
    /// chip0's cs1_logic).
    #[test]
    fn multi_active_low_all_inverted() {
        let layout = layout_with(alloc::vec![
            SelectLine { role: SelectRole::Cs1, gpio: 13 },
            SelectLine { role: SelectRole::X1, gpio: 14 },
            SelectLine { role: SelectRole::X2, gpio: 15 },
        ]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None);

        let overrides = build_cs_overrides(&layout, ChipSetType::Multi, &cs_config);
        assert_eq!(
            overrides,
            alloc::vec![
                encode_override(13, GpioOverride::GpioOverInvert),
                encode_override(14, GpioOverride::GpioOverInvert),
                encode_override(15, GpioOverride::GpioOverInvert),
            ]
        );
    }

    /// Multi set, CS1 configured ActiveHigh (already matches required
    /// ActiveHigh) -> no overrides.
    #[test]
    fn multi_active_high_no_override() {
        let layout = layout_with(alloc::vec![
            SelectLine { role: SelectRole::Cs1, gpio: 13 },
            SelectLine { role: SelectRole::X1, gpio: 14 },
        ]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveHigh), None, None);

        let overrides = build_cs_overrides(&layout, ChipSetType::Multi, &cs_config);
        assert!(overrides.is_empty());
    }
}