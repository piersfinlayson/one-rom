// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! CS/data-range layout derivation for One ROM v2 metadata generation.
//!
//! Covers point D: find a shared `gpio_base` for the CS-detect +
//! data-output PIO, such that the data-line GPIOs and the CS-select
//! GPIO(s) are each expressible as `(base_pin_offset, count)` from that
//! base, where `count` covers a *contiguous* run of GPIOs (PIO `SET`/`IN`
//! requirement), and the overall range fits within a single PIO GPIO
//! window (see `gpio_window`).
//!
//! - Data lines are assumed always contiguous for a given chip+board - if
//!   they're not contiguous under every combo, that combo is rejected.
//!   `data_pin_gpios` records, for each of chip0's data lines (in
//!   `chip0.data_pins()` order), which GPIO it resolved to under the
//!   winning combo - needed by `build_rom_pin_map` to populate
//!   `OneromRomPinMap::data`.
//! - Select line(s):
//!     - Single/Banked: CS1 always, plus CS2/CS3 if the chip type has them
//!       (`control_lines()`) AND are configured `ActiveLow`/`ActiveHigh`
//!       (not `Ignore`/unset) in `CsConfig`.
//!     - Multi: CS1 (or CE) + X1 [+ X2], one per chip in the set.
//!
//!   The resulting select GPIO set must be contiguous (`AlgCs0`), *or*
//!   have exactly one gap for Single/Banked (`AlgCs1`,
//!   `cs_ignore_index` = gap position - Multi doesn't support this).
//!
//! - Additionally, `[gpio_base, gpio_base + 32)` (the PIO GPIO window
//!   anchored at 0 or 16) must cover all data + select + qualifier GPIOs.
//!   A combo can satisfy the contiguity/span checks above while still
//!   being unreachable by any single PIO, so this is checked independently.
//!
//! `gpio_base` is always exactly 0 or 16, matching the RP2350 GPIOBASE
//! register.  All `base_*_pin` offsets are relative to this value.
//!
//! Each select line is recorded in `select_lines` with its role
//! (CS1/CS2/CS3/CE/X1/X2) and resolved (absolute) GPIO, so that
//! `cs_overrides` can later look up its configured `CsLogic` and decide
//! whether it needs inverting.
//!
//! For chip types with `deselect_when_address_all_high()` (e.g. 23QL384),
//! `alg_cs2` is populated with the `ALG_CS_2` qualifier parameters.
//! `derive_cs_data_layout` requires `addr_pin_gpios` to be `Some` in this
//! case - it must be called after `derive_addr_layout` so the qualifier
//! GPIOs are already resolved.
//!
//! Like `addr_layout`, deliberately decoupled from `Chip`/`ChipSet` (takes
//! `CsConfig` directly - lightweight, no image data).

use alloc::collections::BTreeSet;
use alloc::vec;
use alloc::vec::Vec;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;

use super::addr_layout::{AddrLayout, LayoutError};
use super::alg_preference::{CsAlgPreference, cs_alg_preference};
use super::gpio_window::fits_pio_window;
use crate::image::{ChipSetType, CsConfig, CsLogic};

/// Which "select" role a GPIO plays in a chip set's CS-detect range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectRole {
    Cs1,
    Cs2,
    Cs3,
    /// Fixed active-low chip-enable (27xx-style EPROMs).
    Ce,
    /// Fixed active-low output-enable (27xx-style EPROMs).
    Oe,
    /// Multi-set extension select 1.
    X1,
    /// Multi-set extension select 2.
    X2,
    /// Top address pin(s) acting as a half-select for oversized ROMs
    /// (e.g. 27C080's A19). Active state determined by `cs1_logic`;
    /// polarity handled by `cs_overrides` (same path as `Cs1`).
    HalfSelect,
}

/// One resolved select line: its role and absolute GPIO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectLine {
    pub role: SelectRole,
    pub gpio: u8,
}

/// Parameters for the `ALG_CS_2` (enable + address-qualified) CS algorithm.
///
/// The chip is selected when the enable line is asserted AND the qualifier
/// pins do not match `qualifier_inactive_pattern`. Present only for chip
/// types where `chip_type.deselect_when_address_all_high()` returns `Some`
/// (e.g. 23QL384).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlgCs2Config {
    /// Offset of the first qualifier pin from `gpio_base`.
    pub base_qualifier_pin: u8,
    /// Span of qualifier pins (including any gap pins), i.e. `num_qualifier_pins`
    /// in the `ALG_CS_2` params struct.
    pub num_qualifier_pins: u8,
    /// Bit pattern (bit `n` = qualifier pin at `base_qualifier_pin + n`)
    /// on qualifier pins when the bank is NOT selected (Y preload value).
    /// For `deselect_when_address_all_high`, all qualifier bits are set.
    pub qualifier_inactive_pattern: u8,
}

/// Resolved CS/data-range layout for one chip set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsDataLayout {
    /// RP2350 PIO GPIOBASE: always exactly 0 or 16.
    pub gpio_base: u8,

    /// Offset of first data GPIO from gpio_base.
    pub base_data_pin: u8,
    pub num_data_pins: u8,

    /// Resolved GPIO for each of chip0's data lines, in
    /// `chip0.data_pins()` order. Length == `num_data_pins`.
    pub data_pin_gpios: Vec<u8>,

    /// Offset of first CS GPIO from gpio_base.
    pub base_cs_pin: u8,
    pub num_cs_pins: u8,

    /// `Some(index)` -> `AlgCs1`, position (0-based, within
    /// `[base_cs_pin, base_cs_pin+num_cs_pins)`) that isn't a real select
    /// line (e.g. an address line) and must be excluded from both the
    /// CS-active check and any override. `None` -> `AlgCs0`.
    pub cs_ignore_index: Option<u8>,

    /// The real select lines (excludes the `cs_ignore_index` gap, if any).
    pub select_lines: Vec<SelectLine>,

    /// `ALG_CS_2` qualifier config for chips with
    /// `deselect_when_address_all_high()` (e.g. 23QL384). `None` for all
    /// other chip types.
    pub alg_cs2: Option<AlgCs2Config>,
}

/// Physical pin for chip_type's single "select" line (CE for
/// FixedActiveLow chips, CS1 for Configurable). Used for Multi sets, where
/// CS2/CS3 must be `Ignore` (validated elsewhere).
fn primary_select_phys_pin(chip_type: ChipType) -> Option<(SelectRole, u8)> {
    let lines = chip_type.control_lines();
    if let Some(ce) = lines.iter().find(|l| l.name == "ce") {
        return Some((SelectRole::Ce, ce.pin));
    }
    lines
        .iter()
        .find(|l| l.name == "cs1")
        .map(|l| (SelectRole::Cs1, l.pin))
}

/// Physical pins (with roles) for chip_type's select line(s) for
/// Single/Banked sets: CE (FixedActiveLow), or CS1 plus any of CS2/CS3
/// that exist and are configured `ActiveLow`/`ActiveHigh` (not
/// `Ignore`/unset).
fn select_phys_pins(
    board: Board,
    chip_type: ChipType,
    cs_config: &CsConfig,
) -> Result<Vec<(SelectRole, u8)>, LayoutError> {
    let lines = chip_type.control_lines();
    let mut pins = vec![];

    if let Some(ce) = lines.iter().find(|l| l.name == "ce") {
        pins.push((SelectRole::Ce, ce.pin));
    }
    if let Some(oe) = lines.iter().find(|l| l.name == "oe") {
        pins.push((SelectRole::Oe, oe.pin));
    }

    let active =
        |l: Option<CsLogic>| matches!(l, Some(CsLogic::ActiveLow) | Some(CsLogic::ActiveHigh));

    let cs1_active = lines.iter().find(|l| l.name == "cs1").is_some_and(|cs1| {
        if active(cs_config.cs1_logic()) {
            pins.push((SelectRole::Cs1, cs1.pin));
            true
        } else {
            false
        }
    });

    let cs2_active = cs1_active
        && lines.iter().find(|l| l.name == "cs2").is_some_and(|cs2| {
            if active(cs_config.cs2_logic()) {
                pins.push((SelectRole::Cs2, cs2.pin));
                true
            } else {
                false
            }
        });

    #[allow(clippy::collapsible_if)]
    if cs2_active {
        if let Some(cs3) = lines.iter().find(|l| l.name == "cs3") {
            if active(cs_config.cs3_logic()) {
                pins.push((SelectRole::Cs3, cs3.pin));
            }
        }
    }

    if pins.is_empty() {
        return Err(LayoutError::NoSelectLine { board, chip_type });
    }

    Ok(pins)
}

fn gpios_for_pin(
    board: Board,
    chip_type: ChipType,
    phys_pin: u8,
) -> Result<&'static [u8], LayoutError> {
    let gpios = board.gpios_for_socket_pin(phys_pin);
    if gpios.is_empty() {
        return Err(LayoutError::UnmappedPin {
            board,
            chip_type,
            phys_pin,
        });
    }
    Ok(gpios)
}

/// Derive the CS/data-range layout for a chip set.
///
/// `addr_layout` must be `Some` for any chip type where
/// `chip_type.deselect_when_address_all_high()` returns `Some` (e.g.
/// 23QL384), and for any chip whose `addr_layout.excess_addr_pin_gpios` is
/// non-empty (oversized ROM using half-select). Pass `Some(&addr_layout)`
/// after calling `derive_addr_layout`; `None` is fine for chips that need
/// neither. Returns `LayoutError::MissingAddrPinGpios` or
/// `LayoutError::RomTooLargeNoCsConfig` as appropriate.
pub fn derive_cs_data_layout(
    board: Board,
    set_type: ChipSetType,
    chip_types: &[ChipType],
    cs_config: &CsConfig,
    addr_layout: Option<&AddrLayout>,
) -> Result<CsDataLayout, LayoutError> {
    let chip0 = chip_types[0];

    // Unpack addr_layout into its two slices used below.
    let addr_pin_gpios: Option<&[u8]> = addr_layout.map(|a| a.addr_pin_gpios.as_slice());
    let excess_addr_pin_gpios: &[u8] = addr_layout
        .map(|a| a.excess_addr_pin_gpios.as_slice())
        .unwrap_or(&[]);

    // Pre-check: ALG_CS_2 chips require addr_pin_gpios to resolve
    // qualifier GPIOs, which come from the already-resolved addr_layout.
    let qual_indices = chip0.deselect_when_address_all_high();
    if qual_indices.is_some() && addr_pin_gpios.is_none() {
        return Err(LayoutError::MissingAddrPinGpios {
            board,
            chip_type: chip0,
        });
    }

    // Pre-check: oversized ROMs (excess_addr_pin_gpios non-empty) require
    // cs1 to be active_low or active_high to act as a half-select.
    if !excess_addr_pin_gpios.is_empty() {
        match cs_config.cs1_logic() {
            Some(CsLogic::ActiveLow) | Some(CsLogic::ActiveHigh) => {}
            _ => {
                return Err(LayoutError::RomTooLargeNoCsConfig {
                    board,
                    chip_type: chip0,
                });
            }
        }
    }

    // Resolved qualifier GPIOs (fixed across all combos - they're
    // determined by the address layout, not the CS/data combo iteration).
    let qual_gpios: Option<Vec<u8>> = qual_indices.map(|indices| {
        let gpios = addr_pin_gpios.expect("pre-checked above");
        indices.iter().map(|&i| gpios[i as usize]).collect()
    });

    let mut data_candidates: Vec<&'static [u8]> = Vec::with_capacity(chip0.data_pins().len());
    for &phys_pin in chip0.data_pins() {
        data_candidates.push(gpios_for_pin(board, chip0, phys_pin)?);
    }
    let num_data_pins = chip0.data_pins().len() as u8;

    // select_roles[i] / select_candidates[i] are parallel.
    let mut select_roles: Vec<SelectRole> = Vec::new();
    let mut select_candidates: Vec<&'static [u8]> = Vec::new();

    match set_type {
        ChipSetType::Single | ChipSetType::Banked => {
            for (role, phys_pin) in select_phys_pins(board, chip0, cs_config)? {
                select_roles.push(role);
                select_candidates.push(gpios_for_pin(board, chip0, phys_pin)?);
            }
        }
        ChipSetType::Multi => {
            let (role, primary_pin) =
                primary_select_phys_pin(chip0).ok_or(LayoutError::NoSelectLine {
                    board,
                    chip_type: chip0,
                })?;
            select_roles.push(role);
            select_candidates.push(gpios_for_pin(board, chip0, primary_pin)?);

            if chip_types.len() >= 2 {
                let x1 = board.gpios_for_x_pin(1);
                if x1.is_empty() {
                    return Err(LayoutError::MissingXPin { board, x_pin: 1 });
                }
                select_roles.push(SelectRole::X1);
                select_candidates.push(x1);
            }
            if chip_types.len() >= 3 {
                let x2 = board.gpios_for_x_pin(2);
                if x2.is_empty() {
                    return Err(LayoutError::MissingXPin { board, x_pin: 2 });
                }
                select_roles.push(SelectRole::X2);
                select_candidates.push(x2);
            }
        }
    }

    let select_start = data_candidates.len();
    let mut all_candidates = data_candidates;
    all_candidates.extend_from_slice(&select_candidates);

    let two_option_slots: Vec<usize> = all_candidates
        .iter()
        .enumerate()
        .filter(|(_, opts)| opts.len() > 1)
        .map(|(i, _)| i)
        .collect();
    let num_combos: u32 = 1 << two_option_slots.len();

    let mut best: Option<(CsDataLayout, (CsAlgPreference, u32))> = None;
    let mut last_noncontig_select: Option<Vec<u8>> = None;

    for combo in 0..num_combos {
        let resolved: Vec<u8> = all_candidates
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

        let data_gpios: BTreeSet<u8> = resolved[..select_start].iter().copied().collect();
        let select_resolved = &resolved[select_start..];
        let mut select_gpios: BTreeSet<u8> = select_resolved.iter().copied().collect();
        // Excess address pin GPIOs (already resolved by derive_addr_layout,
        // fixed across all combos) are folded in as additional CS select
        // pins. The contiguity/gap check and algorithm selection naturally
        // account for them alongside CE/OE.
        for &gpio in excess_addr_pin_gpios {
            select_gpios.insert(gpio);
        }

        let data_min = *data_gpios.iter().next().unwrap();
        let data_max = *data_gpios.iter().last().unwrap();
        if data_max - data_min + 1 != data_gpios.len() as u8 {
            continue;
        }

        let sel_min = *select_gpios.iter().next().unwrap();
        let sel_max = *select_gpios.iter().last().unwrap();
        let sel_span = sel_max - sel_min + 1;
        let sel_len = select_gpios.len() as u8;

        let cs_ignore_index = if sel_span == sel_len {
            None
        } else if sel_span == sel_len + 1
            && matches!(set_type, ChipSetType::Single | ChipSetType::Banked)
        {
            let gap = (sel_min..=sel_max)
                .position(|g| !select_gpios.contains(&g))
                .expect("span - len == 1 implies exactly one gap") as u8;
            Some(gap)
        } else {
            last_noncontig_select = Some(select_gpios.into_iter().collect());
            continue;
        };
        let num_cs_pins = sel_span;

        // gpio_base must be the RP2350 PIO GPIOBASE: exactly 0 or 16.
        // Include qualifier GPIOs (if any) in the span so that the
        // chosen window covers all pins the CS PIO must observe.
        let mut all_min = data_min.min(sel_min);
        let mut all_max = data_max.max(sel_max);
        if let Some(ref qg) = qual_gpios {
            let q_min = *qg.iter().min().unwrap();
            let q_max = *qg.iter().max().unwrap();
            all_min = all_min.min(q_min);
            all_max = all_max.max(q_max);
        }

        let gpio_base: u8 = if all_min < 16 { 0 } else { 16 };
        let base_data_pin = data_min - gpio_base;
        let base_cs_pin = sel_min - gpio_base;

        let window_span = all_max - gpio_base + 1;
        if !fits_pio_window(gpio_base, window_span) {
            continue;
        }

        // Compute ALG_CS_2 config now that we have the resolved gpio_base.
        let alg_cs2 = qual_gpios.as_ref().map(|qg| {
            let q_min = *qg.iter().min().unwrap();
            let q_max = *qg.iter().max().unwrap();
            let base_qualifier_pin = q_min - gpio_base;
            let num_qualifier_pins = q_max - q_min + 1;
            // deselect_when_address_all_high: inactive when all qualifier
            // bits are high, so every qualifier pin's position within the
            // span contributes a 1 bit.
            let qualifier_inactive_pattern =
                qg.iter().fold(0u8, |acc, &g| acc | (1 << (g - q_min)));
            AlgCs2Config {
                base_qualifier_pin,
                num_qualifier_pins,
                qualifier_inactive_pattern,
            }
        });

        let alg_pref = cs_alg_preference(cs_ignore_index, alg_cs2.as_ref());
        let score = (alg_pref, (all_max - all_min + 1) as u32);

        if best.as_ref().is_none_or(|(_, s)| score < *s) {
            let mut select_lines: Vec<SelectLine> = select_roles
                .iter()
                .zip(select_resolved.iter())
                .map(|(&role, &gpio)| SelectLine { role, gpio })
                .collect();
            // Excess address pins are half-select lines. Polarity for
            // each is determined by cs1_logic in cs_overrides.
            for &gpio in excess_addr_pin_gpios {
                select_lines.push(SelectLine {
                    role: SelectRole::HalfSelect,
                    gpio,
                });
            }

            best = Some((
                CsDataLayout {
                    gpio_base,
                    base_data_pin,
                    num_data_pins,
                    data_pin_gpios: resolved[..select_start].to_vec(),
                    base_cs_pin,
                    num_cs_pins,
                    cs_ignore_index,
                    select_lines,
                    alg_cs2,
                },
                score,
            ));
        }
    }

    best.map(|(layout, _)| layout).ok_or({
        if let Some(gpios) = last_noncontig_select {
            LayoutError::NonContiguousSelect {
                board,
                chip_type: chip0,
                gpios,
            }
        } else {
            LayoutError::NoValidLayout { board }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use onerom_metadata::BitModes;

    #[test]
    fn fire24a_2364_single() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None);

        let layout = derive_cs_data_layout(
            Board::Fire24A,
            ChipSetType::Single,
            &[ChipType::Chip2364],
            &cs_config,
            None,
        )
        .expect("layout derivation should succeed");

        assert_eq!(layout.gpio_base, 0);
        assert_eq!(layout.base_data_pin, 16);
        assert_eq!(layout.num_data_pins, 8);
        assert_eq!(layout.base_cs_pin, 13);
        assert_eq!(layout.num_cs_pins, 1);
        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(layout.alg_cs2, None);
        assert_eq!(
            layout.select_lines,
            vec![SelectLine {
                role: SelectRole::Cs1,
                gpio: 13
            }]
        );

        // data_pin_gpios: 2364's 8 data lines (physical pins
        // 9,10,11,13,14,15,16,17) resolve via Fire24A's socket_pin_map to
        // GPIOs 16,17,18,19,20,21,22,23 respectively.
        assert_eq!(layout.data_pin_gpios, vec![16, 17, 18, 19, 20, 21, 22, 23]);
    }

    #[test]
    fn fire24a_2316_single_cs2_cs3_ignored() {
        let cs_config = CsConfig::new(
            Some(CsLogic::ActiveLow),
            Some(CsLogic::Ignore),
            Some(CsLogic::Ignore),
        );

        let layout = derive_cs_data_layout(
            Board::Fire24A,
            ChipSetType::Single,
            &[ChipType::Chip2316],
            &cs_config,
            None,
        )
        .expect("layout derivation should succeed");

        assert_eq!(layout.gpio_base, 0);
        assert_eq!(layout.base_cs_pin, 13);
        assert_eq!(layout.num_cs_pins, 1);
        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(layout.alg_cs2, None);
        assert_eq!(
            layout.select_lines,
            vec![SelectLine {
                role: SelectRole::Cs1,
                gpio: 13
            }]
        );
    }

    /// Fire24A, single 2316, CS2 active (CS3 ignored).
    ///
    /// CS1=GPIO13, CS2=GPIO15. Gap at GPIO14 (= 2316's A10) ->
    /// `AlgCs1`, `cs_ignore_index = Some(1)` (middle of the 3-wide range).
    #[test]
    fn fire24a_2316_single_cs2_active() {
        let cs_config = CsConfig::new(
            Some(CsLogic::ActiveLow),
            Some(CsLogic::ActiveLow),
            Some(CsLogic::Ignore),
        );

        let layout = derive_cs_data_layout(
            Board::Fire24A,
            ChipSetType::Single,
            &[ChipType::Chip2316],
            &cs_config,
            None,
        )
        .expect("layout derivation should succeed (AlgCs1)");

        assert_eq!(layout.gpio_base, 0);
        assert_eq!(layout.base_cs_pin, 13);
        assert_eq!(layout.num_cs_pins, 3);
        assert_eq!(layout.cs_ignore_index, Some(1));
        assert_eq!(layout.alg_cs2, None);
        assert_eq!(
            layout.select_lines,
            vec![
                SelectLine {
                    role: SelectRole::Cs1,
                    gpio: 13
                },
                SelectLine {
                    role: SelectRole::Cs2,
                    gpio: 15
                },
            ]
        );
    }

    /// 23QL384 without addr_pin_gpios returns MissingAddrPinGpios.
    #[test]
    fn missing_addr_pin_gpios_for_alg_cs2_chip_errors() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None);

        let result = derive_cs_data_layout(
            Board::Fire28A,
            ChipSetType::Single,
            &[ChipType::Chip23QL384],
            &cs_config,
            None,
        );

        assert!(matches!(
            result,
            Err(LayoutError::MissingAddrPinGpios { .. })
        ));
    }

    /// 23QL384 with resolved addr_pin_gpios: verify alg_cs2 is populated
    /// with the correct qualifier config. A14 and A15 (indices 14, 15 into
    /// address_pins()) must both be high to deselect the chip.
    ///
    /// The exact expected values depend on the Fire28A board's GPIO mapping
    /// for the 23QL384's A14/A15 pins; fill in once that mapping is known.
    #[test]
    fn fire28a_23ql384_single_alg_cs2_populated() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None);

        // Derive addr_layout first to get the resolved addr_pin_gpios.
        // (derive_addr_layout is tested separately; we call it here only
        // to get the resolved GPIO slice for the CS2 qualifier lookup.)
        let addr_layout = super::super::addr_layout::derive_addr_layout(
            Board::Fire28A,
            ChipSetType::Single,
            &[ChipType::Chip23QL384],
            BitModes::BitMode8,
        )
        .expect("addr layout derivation should succeed");

        let layout = derive_cs_data_layout(
            Board::Fire28A,
            ChipSetType::Single,
            &[ChipType::Chip23QL384],
            &cs_config,
            Some(&addr_layout),
        )
        .expect("layout derivation should succeed");

        let cs2 = layout.alg_cs2.expect("23QL384 must have alg_cs2");

        // A14 and A15 are contiguous, so num_qualifier_pins == 2 and
        // qualifier_inactive_pattern == 0b11 (both high = deselected).
        assert_eq!(cs2.num_qualifier_pins, 2);
        assert_eq!(cs2.qualifier_inactive_pattern, 0b11);

        // base_qualifier_pin must be within the PIO window.
        assert!(cs2.base_qualifier_pin < 32);
    }
}
