// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Build `OneromAlgCsConfig` (`AlgCs0`/`AlgCs1`/`AlgCs2`) from the CS/data layout.
//!
//! `serve_cs_low_0` (AlgCs0 only) reflects the PIO's required CS
//! polarity for this set type (0 = active-low for Single/Banked, 1 =
//! active-high for Multi) - NOT the chip's configured `CsLogic`.
//! Reconciling configured polarity with this requirement is
//! `cs_overrides::build_cs_overrides`'s job (GpioOverInvert), run
//! separately.
//!
//! `AlgCs1` (non-contiguous, one gap - `cs_ignore_index`) has no
//! `serve_cs_low_0`/`byte_pin`/`first_rom_*` fields: it's always
//! active-low (Single/Banked only, per `derive_cs_data_layout`), and has
//! no multi-rom or byte-mode use cases.
//!
//! `AlgCs2` (enable + address-qualified, e.g. 23QL384) has only the three
//! qualifier fields (`base_qualifier_pin`, `num_qualifier_pins`,
//! `qualifier_inactive_pattern`) beyond the common base fields. No
//! `serve_cs_low_0`, `byte_pin`, or `first_rom_*`.

use onerom_metadata::{GPIO_NONE, OneromAlgCsConfig, OneromAlgDataConfig};

use crate::image::ChipSetType;

use super::alg_config::{DEFAULT_CLKDIV_FRAC, DEFAULT_CLKDIV_INT};
use super::cs_data_layout::CsDataLayout;

/// The CS-detect PIO's required polarity, as `serve_cs_low_0`.
/// Only used for `AlgCs0` - `AlgCs2` has no such field.
fn serve_cs_low_0(set_type: ChipSetType) -> u8 {
    match set_type {
        ChipSetType::Single | ChipSetType::Banked => 0,
        ChipSetType::Multi => 1,
    }
}

/// `(first_rom_cs_base, first_rom_num_cs_pins)`: the CS sub-range
/// belonging to chip0 of the set.
///
/// - Single/Banked: there's only one rom - its range is the whole select
///   range.
/// - Multi: chip0's select line is always the first entry in
///   `select_lines` (placed there by `derive_cs_data_layout` in
///   per_chip_select, X1, X2 order). It may be `Cs1`, `Ce`, or `Oe`
///   depending on the chip type and which line is fly-leaded.
///
/// Only used for `AlgCs0` - `AlgCs2` has no `first_rom_*` fields.
fn first_rom_cs(layout: &CsDataLayout, set_type: ChipSetType) -> (u8, u8) {
    match set_type {
        ChipSetType::Single | ChipSetType::Banked => (layout.base_cs_pin, layout.num_cs_pins),
        ChipSetType::Multi => {
            let chip0_select = layout
                .select_lines
                .first()
                .expect("Multi sets must have at least one select line");
            (chip0_select.gpio - layout.gpio_base, 1)
        }
    }
}

/// Build `OneromAlgCsConfig` from the CS/data layout, set type, and the
/// already-built `AlgData` config (for `byte_pin` in `AlgCs0`).
pub fn build_alg_cs(
    layout: &CsDataLayout,
    set_type: ChipSetType,
    alg_data: &OneromAlgDataConfig,
) -> OneromAlgCsConfig {
    // AlgCs2 takes priority: it implies a distinct algorithm regardless of
    // whether the CS pins happen to be contiguous. Fields are only the
    // common base fields plus the three qualifier params - no serve_cs_low_0,
    // byte_pin, or first_rom_*.
    if let Some(cs2) = &layout.alg_cs2 {
        return OneromAlgCsConfig::AlgCs2 {
            clkdiv_int: DEFAULT_CLKDIV_INT,
            clkdiv_frac: DEFAULT_CLKDIV_FRAC,
            gpio_base: layout.gpio_base,
            base_cs_pin: layout.base_cs_pin,
            num_cs_pins: layout.num_cs_pins,
            base_data_pin: layout.base_data_pin,
            num_data_pins: layout.num_data_pins,
            cs_active_delay: 0,
            cs_inactive_delay: 0,
            base_qualifier_pin: cs2.base_qualifier_pin,
            num_qualifier_pins: cs2.num_qualifier_pins,
            qualifier_inactive_pattern: cs2.qualifier_inactive_pattern,
        };
    }

    match layout.cs_ignore_index {
        None => {
            let byte_pin = match alg_data {
                OneromAlgDataConfig::AlgData1 { byte_pin, .. } => *byte_pin,
                _ => GPIO_NONE,
            };
            let (first_rom_cs_base, first_rom_num_cs_pins) = first_rom_cs(layout, set_type);

            OneromAlgCsConfig::AlgCs0 {
                clkdiv_int: DEFAULT_CLKDIV_INT,
                clkdiv_frac: DEFAULT_CLKDIV_FRAC,
                gpio_base: layout.gpio_base,
                base_cs_pin: layout.base_cs_pin,
                num_cs_pins: layout.num_cs_pins,
                base_data_pin: layout.base_data_pin,
                num_data_pins: layout.num_data_pins,
                cs_active_delay: 0,
                cs_inactive_delay: 0,
                serve_cs_low_0: serve_cs_low_0(set_type),
                byte_pin,
                first_rom_cs_base,
                first_rom_num_cs_pins,
            }
        }
        Some(cs_ignore_index) => OneromAlgCsConfig::AlgCs1 {
            clkdiv_int: DEFAULT_CLKDIV_INT,
            clkdiv_frac: DEFAULT_CLKDIV_FRAC,
            gpio_base: layout.gpio_base,
            base_cs_pin: layout.base_cs_pin,
            num_cs_pins: layout.num_cs_pins,
            base_data_pin: layout.base_data_pin,
            num_data_pins: layout.num_data_pins,
            cs_active_delay: 0,
            cs_inactive_delay: 0,
            cs_ignore_index,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::cs_data_layout::{AlgCs2Config, SelectLine, SelectRole};
    use super::*;

    fn fire24a_2364_layout() -> CsDataLayout {
        CsDataLayout {
            gpio_base: 0,
            base_data_pin: 16,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![16, 17, 18, 19, 20, 21, 22, 23],
            base_cs_pin: 13,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: alloc::vec![SelectLine {
                role: SelectRole::Cs1,
                gpio: 13
            }],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        }
    }

    fn alg_data_8bit() -> OneromAlgDataConfig {
        OneromAlgDataConfig::AlgData0 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            base_data_pin: 16,
            word_size: 8,
        }
    }

    #[test]
    fn fire24a_2364_single() {
        let layout = fire24a_2364_layout();
        let alg_data = alg_data_8bit();

        let alg_cs = build_alg_cs(&layout, ChipSetType::Single, &alg_data);

        assert_eq!(
            alg_cs,
            OneromAlgCsConfig::AlgCs0 {
                clkdiv_int: 1,
                clkdiv_frac: 0,
                gpio_base: 0,
                base_cs_pin: 13,
                num_cs_pins: 1,
                base_data_pin: 16,
                num_data_pins: 8,
                cs_active_delay: 0,
                cs_inactive_delay: 0,
                serve_cs_low_0: 0,
                byte_pin: GPIO_NONE,
                first_rom_cs_base: 13,
                first_rom_num_cs_pins: 1,
            }
        );
    }

    /// Multi set: serve_cs_low_0 = 1 (active-high), and first_rom_cs_base
    /// picks out chip0's Cs1 specifically (not the whole 3-pin range).
    #[test]
    fn multi_serve_cs_low_0_and_first_rom() {
        let layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 16,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![16, 17, 18, 19, 20, 21, 22, 23],
            base_cs_pin: 13,
            num_cs_pins: 3,
            cs_ignore_index: None,
            select_lines: alloc::vec![
                SelectLine {
                    role: SelectRole::Cs1,
                    gpio: 13
                },
                SelectLine {
                    role: SelectRole::X1,
                    gpio: 14
                },
                SelectLine {
                    role: SelectRole::X2,
                    gpio: 15
                },
            ],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };
        let alg_data = alg_data_8bit();

        let alg_cs = build_alg_cs(&layout, ChipSetType::Multi, &alg_data);

        match alg_cs {
            OneromAlgCsConfig::AlgCs0 {
                clkdiv_int: _,
                clkdiv_frac: _,
                gpio_base: _,
                base_cs_pin: _,
                num_cs_pins: _,
                base_data_pin: _,
                num_data_pins: _,
                cs_active_delay: _,
                cs_inactive_delay: _,
                serve_cs_low_0,
                byte_pin: _,
                first_rom_cs_base,
                first_rom_num_cs_pins,
            } => {
                assert_eq!(serve_cs_low_0, 1);
                assert_eq!(first_rom_cs_base, 13);
                assert_eq!(first_rom_num_cs_pins, 1);
            }
            _ => panic!("expected AlgCs0"),
        }
    }

    /// Fire24A/2316 with CS2 active: `cs_ignore_index = Some(1)` ->
    /// `AlgCs1`.
    #[test]
    fn fire24a_2316_cs2_active_alg_cs1() {
        let layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 16,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![16, 17, 18, 19, 20, 21, 22, 23],
            base_cs_pin: 13,
            num_cs_pins: 3,
            cs_ignore_index: Some(1),
            select_lines: alloc::vec![
                SelectLine {
                    role: SelectRole::Cs1,
                    gpio: 13
                },
                SelectLine {
                    role: SelectRole::Cs2,
                    gpio: 15
                },
            ],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };
        let alg_data = alg_data_8bit();

        let alg_cs = build_alg_cs(&layout, ChipSetType::Single, &alg_data);

        assert_eq!(
            alg_cs,
            OneromAlgCsConfig::AlgCs1 {
                clkdiv_int: 1,
                clkdiv_frac: 0,
                gpio_base: 0,
                base_cs_pin: 13,
                num_cs_pins: 3,
                base_data_pin: 16,
                num_data_pins: 8,
                cs_active_delay: 0,
                cs_inactive_delay: 0,
                cs_ignore_index: 1,
            }
        );
    }

    /// 23QL384-shaped layout with `alg_cs2` populated: `build_alg_cs`
    /// must emit `AlgCs2` with the qualifier params from `alg_cs2` and the
    /// standard base fields. Synthetic GPIO values - Fire28A-specific
    /// values are covered by the end-to-end test in `alg_config::tests`.
    #[test]
    fn alg_cs2_layout_emits_alg_cs2() {
        let layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 16,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![16, 17, 18, 19, 20, 21, 22, 23],
            base_cs_pin: 13,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: alloc::vec![SelectLine {
                role: SelectRole::Cs1,
                gpio: 13
            }],
            commoned_lines: alloc::vec![],
            alg_cs2: Some(AlgCs2Config {
                base_qualifier_pin: 5,
                num_qualifier_pins: 2,
                qualifier_inactive_pattern: 0b11,
            }),
        };
        let alg_data = alg_data_8bit();

        let alg_cs = build_alg_cs(&layout, ChipSetType::Single, &alg_data);

        assert_eq!(
            alg_cs,
            OneromAlgCsConfig::AlgCs2 {
                clkdiv_int: 1,
                clkdiv_frac: 0,
                gpio_base: 0,
                base_cs_pin: 13,
                num_cs_pins: 1,
                base_data_pin: 16,
                num_data_pins: 8,
                cs_active_delay: 0,
                cs_inactive_delay: 0,
                base_qualifier_pin: 5,
                num_qualifier_pins: 2,
                qualifier_inactive_pattern: 0b11,
            }
        );
    }
}
