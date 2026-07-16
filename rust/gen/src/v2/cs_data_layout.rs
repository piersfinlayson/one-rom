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
use super::slot_context::SlotContext;
use crate::image::{ChipSetType, CsConfig, CsLogic};

use super::multi_cs_config::{ControlLineKind, control_line_logic};

/// Which "select" role a GPIO plays in a chip set's CS-detect range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectRole {
    Cs1,
    Cs2,
    Cs3,
    Cs4,
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

/// Map a `ControlLineKind` to its equivalent `SelectRole` for use in
/// `select_lines`. Covers only chip control lines; X1/X2/HalfSelect
/// are not produced by `ControlLineKind`.
fn control_line_kind_to_select_role(kind: ControlLineKind) -> SelectRole {
    match kind {
        ControlLineKind::Ce => SelectRole::Ce,
        ControlLineKind::Oe => SelectRole::Oe,
        ControlLineKind::Cs1 => SelectRole::Cs1,
        ControlLineKind::Cs2 => SelectRole::Cs2,
        ControlLineKind::Cs3 => SelectRole::Cs3,
        ControlLineKind::Cs4 => SelectRole::Cs4,
    }
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

    /// Commoned lines (Multi sets only): control lines driven active on chip0
    /// and shared across the set. Physically present in the CS-detect span
    /// (folded into `num_cs_pins`) but not discriminating, so absent from
    /// `select_lines`. Carried here so `build_cs_overrides` can normalise their
    /// polarity to the required convention exactly as it does for select lines.
    /// Without it, an active-low commoned line reads high at idle and mis-
    /// fires the CS gate. Empty for Single/Banked.
    pub commoned_lines: Vec<SelectLine>,

    /// `ALG_CS_2` qualifier config for chips with
    /// `deselect_when_address_all_high()` (e.g. 23QL384). `None` for all
    /// other chip types.
    pub alg_cs2: Option<AlgCs2Config>,
}

/// Physical pins (with roles) for chip_type's select line(s) for
/// Single/Banked sets. Each control line is checked independently:
///
/// - CE: included if not `CsLogic::Ignore` (from `CeOeExplicit`) or if
///   `CsConfig::CeOe` (always active).
/// - OE: same as CE.
/// - CS1/CS2/CS3: included if configured `ActiveLow`/`ActiveHigh`. Each
///   is checked independently — there is no chain dependency between them,
///   so any combination of active/ignore is valid (subject to the
///   permission rules enforced by `check_cs_v2`).
fn select_phys_pins(
    board: Board,
    chip_type: ChipType,
    cs_config: &CsConfig,
) -> Result<Vec<(SelectRole, u8)>, LayoutError> {
    let lines = chip_type.control_lines();
    let mut pins = vec![];

    let active = |l: CsLogic| matches!(l, CsLogic::ActiveLow | CsLogic::ActiveHigh);

    for line in lines {
        match line.name {
            "ce" => {
                let logic = control_line_logic("ce", cs_config);
                if active(logic) {
                    pins.push((SelectRole::Ce, line.pin));
                }
            }
            "oe" => {
                let logic = control_line_logic("oe", cs_config);
                if active(logic) {
                    pins.push((SelectRole::Oe, line.pin));
                }
            }
            "cs1" => {
                let logic = control_line_logic("cs1", cs_config);
                if active(logic) {
                    pins.push((SelectRole::Cs1, line.pin));
                }
            }
            "cs2" => {
                let logic = control_line_logic("cs2", cs_config);
                if active(logic) {
                    pins.push((SelectRole::Cs2, line.pin));
                }
            }
            "cs3" => {
                let logic = control_line_logic("cs3", cs_config);
                if active(logic) {
                    pins.push((SelectRole::Cs3, line.pin));
                }
            }
            "cs4" => {
                let logic = control_line_logic("cs4", cs_config);
                if active(logic) {
                    pins.push((SelectRole::Cs4, line.pin));
                }
            }
            _ => {}
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
    pin_offset: i16,
) -> Result<&'static [u8], LayoutError> {
    let socket_pin = phys_pin as i16 + pin_offset;
    if socket_pin < 1 || socket_pin > board.chip_pins() as i16 {
        // Data/CS pins cannot be fly-leaded: if one overhangs the socket
        // the chip/board combination is not supported.
        return Err(LayoutError::UnmappedPin {
            board,
            chip_type,
            phys_pin,
        });
    }
    let gpios = board.gpios_for_socket_pin(socket_pin as u8);
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
///
/// `ctx.multi_cs_config` must be `Some` for `ChipSetType::Multi`, `None`
/// for all other set types. Derive it from the chip set's chips via
/// `derive_multi_cs_config` before calling this function.
///
/// `ctx.pin_offset` is applied to every chip physical pin before the
/// board socket-pin lookup, to handle chips smaller than the board socket.
pub fn derive_cs_data_layout(
    ctx: &SlotContext,
    addr_layout: Option<&AddrLayout>,
) -> Result<CsDataLayout, LayoutError> {
    let board = ctx.board;
    let set_type = ctx.set_type;
    let chip_types = &ctx.chip_types;
    let cs_config = &ctx.cs_config;
    let multi_cs_config = ctx.multi_cs_config.as_ref();
    let pin_offset = ctx.pin_offset;

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
        data_candidates.push(gpios_for_pin(board, chip0, phys_pin, pin_offset)?);
    }
    let num_data_pins = chip0.data_pins().len() as u8;

    // select_roles[i] / select_candidates[i]: per-chip discriminating lines.
    // commoned_candidates: GPIOs of lines commoned across all chips (Multi
    // sets only). Not added to select_lines, but folded into the contiguity
    // check so that CE/OE/X pins all form a contiguous span even when the
    // commoned line sits between the per-chip select and X1/X2.
    let mut select_roles: Vec<SelectRole> = Vec::new();
    let mut select_candidates: Vec<&'static [u8]> = Vec::new();
    let mut commoned_candidates: Vec<&'static [u8]> = Vec::new();
    // Roles parallel to commoned_candidates, so the resolved commoned GPIOs can
    // be paired back into SelectLine entries for polarity normalisation.
    let mut commoned_roles: Vec<SelectRole> = Vec::new();
    // Truly-ignored control lines' resolved GPIOs (all bond options). NOT part
    // of the CS span - resolved only so that an ignored line landing in a
    // select-span gap (interior, not edge) can be reported precisely as an
    // InteriorIgnoredLine rather than a generic NonContiguousSelect.
    let mut ignored_gpios: BTreeSet<u8> = BTreeSet::new();

    match set_type {
        ChipSetType::Single | ChipSetType::Banked => {
            for (role, phys_pin) in select_phys_pins(board, chip0, cs_config)? {
                select_roles.push(role);
                select_candidates.push(gpios_for_pin(board, chip0, phys_pin, pin_offset)?);
            }
        }
        ChipSetType::Multi => {
            let mcc = multi_cs_config
                .expect("derive_cs_data_layout called for Multi set without MultiChipCsConfig");

            // per_chip_select: chip0's fly-leaded line → becomes a select_line.
            let per_chip_line = chip0
                .control_lines()
                .iter()
                .find(|l| l.name == mcc.per_chip_select.name())
                .ok_or(LayoutError::NoSelectLine {
                    board,
                    chip_type: chip0,
                })?;
            select_roles.push(control_line_kind_to_select_role(mcc.per_chip_select));
            select_candidates.push(gpios_for_pin(board, chip0, per_chip_line.pin, pin_offset)?);

            // commoned lines: physically present, always active. Their GPIOs
            // are included in the span (filling any gap between per_chip_select
            // and X pins) but not in select_lines.
            for &commoned_kind in &mcc.commoned_lines {
                let commoned_line = chip0
                    .control_lines()
                    .iter()
                    .find(|l| l.name == commoned_kind.name())
                    .ok_or(LayoutError::NoSelectLine {
                        board,
                        chip_type: chip0,
                    })?;
                commoned_roles.push(control_line_kind_to_select_role(commoned_kind));
                commoned_candidates.push(gpios_for_pin(
                    board,
                    chip0,
                    commoned_line.pin,
                    pin_offset,
                )?);
            }

            // Resolve truly-ignored lines' GPIOs for the interior-gap check.
            // They are deliberately NOT added to the CS span (that is the whole
            // point of the commoned/ignored split): an edge-positioned ignored
            // line drops out of num_cs_pins and is forced low as an
            // address-window gap. Only an *interior* ignored line - one whose
            // GPIO sits between the select lines - is unservable, and is
            // reported as InteriorIgnoredLine below.
            for &ignored_kind in &mcc.ignored_lines {
                let ignored_line = chip0
                    .control_lines()
                    .iter()
                    .find(|l| l.name == ignored_kind.name())
                    .ok_or(LayoutError::NoSelectLine {
                        board,
                        chip_type: chip0,
                    })?;
                for &gpio in gpios_for_pin(board, chip0, ignored_line.pin, pin_offset)? {
                    ignored_gpios.insert(gpio);
                }
            }

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
    let select_end = select_start + select_candidates.len();
    let mut all_candidates = data_candidates;
    all_candidates.extend_from_slice(&select_candidates);
    all_candidates.extend_from_slice(&commoned_candidates);

    let two_option_slots: Vec<usize> = all_candidates
        .iter()
        .enumerate()
        .filter(|(_, opts)| opts.len() > 1)
        .map(|(i, _)| i)
        .collect();
    let num_combos: u32 = 1 << two_option_slots.len();

    let mut best: Option<(CsDataLayout, (CsAlgPreference, u32))> = None;
    let mut last_noncontig_select: Option<Vec<u8>> = None;
    // GPIO of a truly-ignored line found sitting in a select-span gap.
    let mut interior_ignored: Option<u8> = None;

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
        let select_resolved = &resolved[select_start..select_end];
        let commoned_resolved = &resolved[select_end..];
        let mut select_gpios: BTreeSet<u8> = select_resolved.iter().copied().collect();
        // Excess address pin GPIOs (already resolved by derive_addr_layout,
        // fixed across all combos) are folded in as additional CS select
        // pins. The contiguity/gap check and algorithm selection naturally
        // account for them alongside CE/OE.
        for &gpio in excess_addr_pin_gpios {
            select_gpios.insert(gpio);
        }
        // Commoned line GPIOs (Multi sets only) fill any physical gap between
        // the per-chip select and X1/X2. They are always present in hardware
        // (driven active by the board), so including them here makes the
        // span contiguous without adding them to select_lines.
        for &gpio in commoned_resolved {
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
            // Non-contiguous select span. If the gap is occupied by a
            // truly-ignored control line, that is the specific (and currently
            // unsupported) cause - record it so the final error names it,
            // rather than falling back to a generic NonContiguousSelect.
            if interior_ignored.is_none() {
                interior_ignored = (sel_min..=sel_max)
                    .find(|g| !select_gpios.contains(g) && ignored_gpios.contains(g));
            }
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

            let commoned_lines: Vec<SelectLine> = commoned_roles
                .iter()
                .zip(commoned_resolved.iter())
                .map(|(&role, &gpio)| SelectLine { role, gpio })
                .collect();

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
                    commoned_lines,
                    alg_cs2,
                },
                score,
            ));
        }
    }

    best.map(|(layout, _)| layout).ok_or({
        if let Some(gpio) = interior_ignored {
            LayoutError::InteriorIgnoredLine {
                board,
                chip_type: chip0,
                gpio,
            }
        } else if let Some(gpios) = last_noncontig_select {
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

    use super::super::multi_cs_config::MultiChipCsConfig;
    use super::super::slot_context::SlotContext;

    fn ctx_single(board: Board, chip_type: ChipType, cs_config: CsConfig) -> SlotContext {
        SlotContext {
            board,
            set_type: ChipSetType::Single,
            chip_types: alloc::vec![chip_type],
            cs_config,
            bit_mode: BitModes::BitMode8,
            pin_offset: 0,
            force_16_bit: false,
            multi_cs_config: None,
        }
    }

    fn ctx_multi(
        board: Board,
        chip_types: Vec<ChipType>,
        cs_config: CsConfig,
        mcc: MultiChipCsConfig,
    ) -> SlotContext {
        SlotContext {
            board,
            set_type: ChipSetType::Multi,
            chip_types,
            cs_config,
            bit_mode: BitModes::BitMode8,
            pin_offset: 0,
            force_16_bit: false,
            multi_cs_config: Some(mcc),
        }
    }

    #[test]
    fn fire24a_2364_single() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let ctx = ctx_single(Board::Fire24A, ChipType::Chip2364, cs_config);

        let layout = derive_cs_data_layout(&ctx, None).expect("layout derivation should succeed");

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
            None,
        );
        let ctx = ctx_single(Board::Fire24A, ChipType::Chip2316, cs_config);

        let layout = derive_cs_data_layout(&ctx, None).expect("layout derivation should succeed");

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
            None,
        );
        let ctx = ctx_single(Board::Fire24A, ChipType::Chip2316, cs_config);

        let layout =
            derive_cs_data_layout(&ctx, None).expect("layout derivation should succeed (AlgCs1)");

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
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let ctx = ctx_single(Board::Fire28A, ChipType::Chip23QL384, cs_config);

        let result = derive_cs_data_layout(&ctx, None);

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
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let ctx = ctx_single(Board::Fire28A, ChipType::Chip23QL384, cs_config);

        // Derive addr_layout first to get the resolved addr_pin_gpios.
        // (derive_addr_layout is tested separately; we call it here only
        // to get the resolved GPIO slice for the CS2 qualifier lookup.)
        let addr_layout = super::super::addr_layout::derive_addr_layout(&ctx)
            .expect("addr layout derivation should succeed");

        let layout = derive_cs_data_layout(&ctx, Some(&addr_layout))
            .expect("layout derivation should succeed");

        let cs2 = layout.alg_cs2.expect("23QL384 must have alg_cs2");

        // A14 and A15 are contiguous, so num_qualifier_pins == 2 and
        // qualifier_inactive_pattern == 0b11 (both high = deselected).
        assert_eq!(cs2.num_qualifier_pins, 2);
        assert_eq!(cs2.qualifier_inactive_pattern, 0b11);

        // base_qualifier_pin must be within the PIO window.
        assert!(cs2.base_qualifier_pin < 32);
    }

    /// Fire24A, 2-chip Multi set with 2364 (CS1). On Fire24A, CS1 is at
    /// GPIO 13 and X1 is at GPIO 9 — not contiguous — so Multi sets are not
    /// supported on this board for 2364 chips.
    #[test]
    fn fire24a_2364_multi_2chip_cs1_primary() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Cs1,
            commoned_lines: alloc::vec![],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire24A,
            alloc::vec![ChipType::Chip2364, ChipType::Chip2364],
            cs_config,
            mcc,
        );

        let result = derive_cs_data_layout(&ctx, None);

        assert!(
            matches!(result, Err(LayoutError::NonContiguousSelect { .. })),
            "Fire24A Multi 2364: CS1 (GPIO 13) and X1 (GPIO 9) are not contiguous"
        );
    }

    /// Fire24A, 2-chip Multi set with 2732, CeOeExplicit { ce: ActiveLow,
    /// oe: Ignore }. On Fire24A, CE is at GPIO 15 (pin 18) and X1 is at
    /// GPIO 9 — not contiguous.
    #[test]
    fn fire24a_2732_multi_2chip_ce_primary() {
        let cs_config = CsConfig::new_with_ce_oe(Some(CsLogic::ActiveLow), Some(CsLogic::Ignore));
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Ce,
            commoned_lines: alloc::vec![ControlLineKind::Oe],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire24A,
            alloc::vec![ChipType::Chip2732, ChipType::Chip2732],
            cs_config,
            mcc,
        );

        let result = derive_cs_data_layout(&ctx, None);

        assert!(
            matches!(result, Err(LayoutError::NonContiguousSelect { .. })),
            "Fire24A Multi 2732 CE-primary: CE (GPIO 15) and X1 (GPIO 9) are not contiguous"
        );
    }

    /// Fire24A, 2-chip Multi set with 2732, CeOeExplicit { ce: Ignore,
    /// oe: ActiveLow }. On Fire24A, OE is at GPIO 13 (pin 20) and X1 is at
    /// GPIO 9 — not contiguous.
    #[test]
    fn fire24a_2732_multi_2chip_oe_primary() {
        let cs_config = CsConfig::new_with_ce_oe(Some(CsLogic::Ignore), Some(CsLogic::ActiveLow));
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Oe,
            commoned_lines: alloc::vec![ControlLineKind::Ce],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire24A,
            alloc::vec![ChipType::Chip2732, ChipType::Chip2732],
            cs_config,
            mcc,
        );

        let result = derive_cs_data_layout(&ctx, None);

        assert!(
            matches!(result, Err(LayoutError::NonContiguousSelect { .. })),
            "Fire24A Multi 2732 OE-primary: OE (GPIO 13) and X1 (GPIO 9) are not contiguous"
        );
    }

    // =========================================================================
    // Fire24E Multi tests
    //
    // Fire24E has X1=GPIO9, X2=GPIO8. CE=GPIO11, OE=GPIO10 for 2732.
    // CS1=GPIO10 for 2364. For 2732, both CE-primary and OE-primary work:
    // the commoned line fills any gap, making CE+OE+X contiguous {8,9,10,11}.
    //   - 2364 CS1 = GPIO 10: {9,10} or {8,9,10} ✓
    //   - 2732 OE-primary: {OE@10, X1@9}, commoned CE@11 → {9,10,11} ✓
    //   - 2732 CE-primary: {CE@11, X1@9}, commoned OE@10 → {9,10,11} ✓
    // =========================================================================

    /// Fire24E, 2-chip Multi set with 2364 (CS1).
    /// CS1 at GPIO 10, X1 at GPIO 9 — contiguous, AlgCs0.
    #[test]
    fn fire24e_2364_multi_2chip_cs1_primary() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Cs1,
            commoned_lines: alloc::vec![],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire24E,
            alloc::vec![ChipType::Chip2364, ChipType::Chip2364],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None)
            .expect("Fire24E Multi 2364 2-chip layout derivation should succeed");

        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines[0],
            SelectLine {
                role: SelectRole::Cs1,
                gpio: 10
            }
        );
        assert_eq!(
            layout.select_lines[1],
            SelectLine {
                role: SelectRole::X1,
                gpio: 9
            }
        );
    }

    /// Fire24E, 3-chip Multi set with 2364 (CS1).
    /// CS1 at GPIO 10, X1 at GPIO 9, X2 at GPIO 8 — all contiguous, AlgCs0.
    #[test]
    fn fire24e_2364_multi_3chip_cs1_primary() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Cs1,
            commoned_lines: alloc::vec![],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire24E,
            alloc::vec![ChipType::Chip2364, ChipType::Chip2364, ChipType::Chip2364],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None)
            .expect("Fire24E Multi 2364 3-chip layout derivation should succeed");

        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines[0],
            SelectLine {
                role: SelectRole::Cs1,
                gpio: 10
            }
        );
        assert_eq!(
            layout.select_lines[1],
            SelectLine {
                role: SelectRole::X1,
                gpio: 9
            }
        );
        assert_eq!(
            layout.select_lines[2],
            SelectLine {
                role: SelectRole::X2,
                gpio: 8
            }
        );
    }

    /// Fire24E, 2-chip Multi set with 2732, CeOeExplicit { ce: Ignore,
    /// oe: ActiveLow }. OE at GPIO 10 (pin 20), X1 at GPIO 9 — contiguous.
    #[test]
    fn fire24e_2732_multi_2chip_oe_primary() {
        let cs_config = CsConfig::new_with_ce_oe(Some(CsLogic::Ignore), Some(CsLogic::ActiveLow));
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Oe,
            commoned_lines: alloc::vec![ControlLineKind::Ce],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire24E,
            alloc::vec![ChipType::Chip2732, ChipType::Chip2732],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None)
            .expect("Fire24E Multi 2732 OE-primary layout derivation should succeed");

        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines[0],
            SelectLine {
                role: SelectRole::Oe,
                gpio: 10
            }
        );
        assert_eq!(
            layout.select_lines[1],
            SelectLine {
                role: SelectRole::X1,
                gpio: 9
            }
        );
    }

    /// Fire24E, 2-chip Multi set with 2732, CeOeExplicit { ce: ActiveLow,
    /// oe: Ignore }. CE at GPIO 11 (pin 18), X1 at GPIO 9. OE (GPIO 10,
    /// commoned) fills the gap, making {9,10,11} contiguous.
    #[test]
    fn fire24e_2732_multi_2chip_ce_primary() {
        let cs_config = CsConfig::new_with_ce_oe(Some(CsLogic::ActiveLow), Some(CsLogic::Ignore));
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Ce,
            commoned_lines: alloc::vec![ControlLineKind::Oe],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire24E,
            alloc::vec![ChipType::Chip2732, ChipType::Chip2732],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None).expect(
            "Fire24E Multi 2732 CE-primary: OE (GPIO 10) fills gap between CE (11) and X1 (9)",
        );

        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines[0],
            SelectLine {
                role: SelectRole::Ce,
                gpio: 11
            }
        );
        assert_eq!(
            layout.select_lines[1],
            SelectLine {
                role: SelectRole::X1,
                gpio: 9
            }
        );
    }

    // =========================================================================
    // Fire28C Multi tests
    //
    // Fire28C has X1=[9,28] (dual-bonded), X2=[8,29] (dual-bonded).
    // CE=GPIO10, OE=GPIO11 for 27-series; CS1=GPIO10 for 23-series.
    // X1 resolves to GPIO 9 (the contiguous candidate over GPIO 28).
    //
    // For 27-series, CE and OE are both physically present (contiguous with
    // X1/X2), so both CE-primary and OE-primary work — the commoned line
    // fills any gap:
    //   - CE-primary: {CE@10, X1@9}, commoned OE@11 → {9,10,11} ✓
    //   - OE-primary: {OE@11, X1@9}, commoned CE@10 → {9,10,11} ✓
    // =========================================================================

    /// Fire28C, 2-chip Multi set with 23128 (CS1 at GPIO 10).
    /// X1 resolves to GPIO 9 (not 28) as the contiguous candidate.
    #[test]
    fn fire28c_23128_multi_2chip_cs1_primary() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Cs1,
            commoned_lines: alloc::vec![ControlLineKind::Cs2, ControlLineKind::Cs3],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire28C,
            alloc::vec![ChipType::Chip23128, ChipType::Chip23128],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None)
            .expect("Fire28C Multi 23128 2-chip layout derivation should succeed");

        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines[0],
            SelectLine {
                role: SelectRole::Cs1,
                gpio: 10
            }
        );
        assert_eq!(
            layout.select_lines[1],
            SelectLine {
                role: SelectRole::X1,
                gpio: 9
            }
        );
    }

    /// Fire28C, 3-chip Multi set with 23128 (CS1 at GPIO 10).
    /// CS1=10, X1=9, X2=8 — all contiguous.
    #[test]
    fn fire28c_23128_multi_3chip_cs1_primary() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Cs1,
            commoned_lines: alloc::vec![ControlLineKind::Cs2, ControlLineKind::Cs3],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire28C,
            alloc::vec![
                ChipType::Chip23128,
                ChipType::Chip23128,
                ChipType::Chip23128
            ],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None)
            .expect("Fire28C Multi 23128 3-chip layout derivation should succeed");

        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines[0],
            SelectLine {
                role: SelectRole::Cs1,
                gpio: 10
            }
        );
        assert_eq!(
            layout.select_lines[1],
            SelectLine {
                role: SelectRole::X1,
                gpio: 9
            }
        );
        assert_eq!(
            layout.select_lines[2],
            SelectLine {
                role: SelectRole::X2,
                gpio: 8
            }
        );
    }

    /// Fire28C, 2-chip Multi set with 27128, CeOeExplicit { ce: ActiveLow,
    /// oe: Ignore }. CE at GPIO 10 (pin 20), X1 resolves to GPIO 9.
    #[test]
    fn fire28c_27128_multi_2chip_ce_primary() {
        let cs_config = CsConfig::new_with_ce_oe(Some(CsLogic::ActiveLow), Some(CsLogic::Ignore));
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Ce,
            commoned_lines: alloc::vec![ControlLineKind::Oe],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire28C,
            alloc::vec![ChipType::Chip27128, ChipType::Chip27128],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None)
            .expect("Fire28C Multi 27128 CE-primary 2-chip layout derivation should succeed");

        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines[0],
            SelectLine {
                role: SelectRole::Ce,
                gpio: 10
            }
        );
        assert_eq!(
            layout.select_lines[1],
            SelectLine {
                role: SelectRole::X1,
                gpio: 9
            }
        );
    }

    /// Fire28C, 3-chip Multi set with 27128, CeOeExplicit { ce: ActiveLow,
    /// oe: Ignore }. CE=10, X1=9, X2=8 — all contiguous.
    #[test]
    fn fire28c_27128_multi_3chip_ce_primary() {
        let cs_config = CsConfig::new_with_ce_oe(Some(CsLogic::ActiveLow), Some(CsLogic::Ignore));
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Ce,
            commoned_lines: alloc::vec![ControlLineKind::Oe],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire28C,
            alloc::vec![
                ChipType::Chip27128,
                ChipType::Chip27128,
                ChipType::Chip27128
            ],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None)
            .expect("Fire28C Multi 27128 CE-primary 3-chip layout derivation should succeed");

        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines[0],
            SelectLine {
                role: SelectRole::Ce,
                gpio: 10
            }
        );
        assert_eq!(
            layout.select_lines[1],
            SelectLine {
                role: SelectRole::X1,
                gpio: 9
            }
        );
        assert_eq!(
            layout.select_lines[2],
            SelectLine {
                role: SelectRole::X2,
                gpio: 8
            }
        );
    }

    /// Fire28C, 2-chip Multi set with 27128, CeOeExplicit { ce: Ignore,
    /// oe: ActiveLow }. OE at GPIO 11 (pin 22), X1 at GPIO 9. CE (GPIO 10,
    /// commoned) fills the gap, making {9,10,11} contiguous.
    #[test]
    fn fire28c_27128_multi_2chip_oe_primary() {
        let cs_config = CsConfig::new_with_ce_oe(Some(CsLogic::Ignore), Some(CsLogic::ActiveLow));
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Oe,
            commoned_lines: alloc::vec![ControlLineKind::Ce],
            ignored_lines: alloc::vec![],
        };
        let ctx = ctx_multi(
            Board::Fire28C,
            alloc::vec![ChipType::Chip27128, ChipType::Chip27128],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None).expect(
            "Fire28C Multi 27128 OE-primary: CE (GPIO 10) fills gap between OE (11) and X1 (9)",
        );

        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines[0],
            SelectLine {
                role: SelectRole::Oe,
                gpio: 11
            }
        );
        assert_eq!(
            layout.select_lines[1],
            SelectLine {
                role: SelectRole::X1,
                gpio: 9
            }
        );
    }

    // =========================================================================
    // Fire24F Multi tests — truly-ignored (not commoned) control lines
    //
    // Fire24F has X1=GPIO9, X2=GPIO8, and for the 2316 CS1=GPIO10, CS2=GPIO11,
    // CS3=GPIO12. In a Multi set CS1 is the per-chip select; CS2/CS3 are
    // `Ignore` on chip0 as well as the secondaries, so they are truly ignored
    // rather than commoned. Unlike commoned lines, ignored lines are NOT
    // folded into the CS-detect span: the range is just the real selects, and
    // CS2/CS3 are left to be forced low as address-window gaps by
    // `cs_overrides`. This is the issue266 layout.
    // =========================================================================

    /// Fire24F, 2-chip Multi 2316: per-chip select CS1@10 + X1@9. CS2/CS3
    /// (GPIO 11/12) are ignored, so the range is {9,10} (num_cs_pins=2), not
    /// {9..12}.
    #[test]
    fn fire24f_2316_multi_2chip_cs2_cs3_ignored() {
        let cs_config = CsConfig::new(
            Some(CsLogic::ActiveLow),
            Some(CsLogic::Ignore),
            Some(CsLogic::Ignore),
            None
        );
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Cs1,
            commoned_lines: alloc::vec![],
            ignored_lines: alloc::vec![ControlLineKind::Cs2, ControlLineKind::Cs3],
        };
        let ctx = ctx_multi(
            Board::Fire24F,
            alloc::vec![ChipType::Chip2316, ChipType::Chip2316],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None)
            .expect("Fire24F Multi 2316 2-chip layout derivation should succeed");

        assert_eq!(layout.gpio_base, 0);
        assert_eq!(layout.base_cs_pin, 9);
        // Ignored CS2/CS3 are excluded from the span: 2 real selects, not 4.
        assert_eq!(layout.num_cs_pins, 2);
        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines,
            vec![
                SelectLine {
                    role: SelectRole::Cs1,
                    gpio: 10
                },
                SelectLine {
                    role: SelectRole::X1,
                    gpio: 9
                },
            ]
        );
    }

    /// Fire24F, 3-chip Multi 2316: per-chip select CS1@10 + X1@9 + X2@8. As
    /// above, CS2/CS3 (GPIO 11/12) are ignored and excluded from the span, so
    /// num_cs_pins=3 ({8,9,10}) rather than 5 ({8..12}).
    #[test]
    fn fire24f_2316_multi_3chip_cs2_cs3_ignored() {
        let cs_config = CsConfig::new(
            Some(CsLogic::ActiveLow),
            Some(CsLogic::Ignore),
            Some(CsLogic::Ignore),
            None
        );
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Cs1,
            commoned_lines: alloc::vec![],
            ignored_lines: alloc::vec![ControlLineKind::Cs2, ControlLineKind::Cs3],
        };
        let ctx = ctx_multi(
            Board::Fire24F,
            alloc::vec![ChipType::Chip2316, ChipType::Chip2316, ChipType::Chip2316],
            cs_config,
            mcc,
        );

        let layout = derive_cs_data_layout(&ctx, None)
            .expect("Fire24F Multi 2316 3-chip layout derivation should succeed");

        assert_eq!(layout.gpio_base, 0);
        assert_eq!(layout.base_cs_pin, 8);
        // {X2@8, X1@9, CS1@10}: the two ignored lines above (11/12) are not
        // part of the CS range.
        assert_eq!(layout.num_cs_pins, 3);
        assert_eq!(layout.cs_ignore_index, None);
        assert_eq!(
            layout.select_lines,
            vec![
                SelectLine {
                    role: SelectRole::Cs1,
                    gpio: 10
                },
                SelectLine {
                    role: SelectRole::X1,
                    gpio: 9
                },
                SelectLine {
                    role: SelectRole::X2,
                    gpio: 8
                },
            ]
        );
    }

    /// Fire24F, 3-chip Multi 2316 with an *interior* ignored line. Contrived
    /// via a hand-built mcc so CS3 is the per-chip select and CS1/CS2 are
    /// ignored: on Fire24F CS3=GPIO12, X1=GPIO9, X2=GPIO8 give selects
    /// {8,9,12}, and the ignored CS1@10 / CS2@11 fall in the gap. An ignored
    /// line interior to the select span is not servable (the CS span cannot be
    /// made contiguous without reading it), and must surface as a specific
    /// InteriorIgnoredLine rather than a generic NonContiguousSelect.
    ///
    /// The reported GPIO is the lowest gap position occupied by an ignored
    /// line: CS1 at GPIO 10.
    #[test]
    fn fire24f_2316_multi_interior_ignored_errors() {
        let cs_config = CsConfig::new(
            Some(CsLogic::Ignore),
            Some(CsLogic::Ignore),
            Some(CsLogic::ActiveLow),
            None
        );
        let mcc = MultiChipCsConfig {
            per_chip_select: ControlLineKind::Cs3,
            commoned_lines: alloc::vec![],
            ignored_lines: alloc::vec![ControlLineKind::Cs1, ControlLineKind::Cs2],
        };
        let ctx = ctx_multi(
            Board::Fire24F,
            alloc::vec![ChipType::Chip2316, ChipType::Chip2316, ChipType::Chip2316],
            cs_config,
            mcc,
        );

        let result = derive_cs_data_layout(&ctx, None);

        assert!(
            matches!(
                result,
                Err(LayoutError::InteriorIgnoredLine { gpio: 10, .. })
            ),
            "interior ignored CS1@10 must produce InteriorIgnoredLine, got {result:?}"
        );
    }
}