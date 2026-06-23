// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Multi-chip set CS configuration: identifies which chip control line is
//! the per-chip select (fly-leaded to X pins for chips[1+]) and which are
//! commoned (always active, physically present but not used for
//! discrimination).
//!
//! Kept in its own module so both `addr_layout` and `cs_data_layout` can
//! import `MultiChipCsConfig` without creating a circular dependency.

use alloc::vec::Vec;

use crate::image::{Chip, CsConfig, CsLogic};

/// A chip control line, identified by its role on the chip. Used in
/// `MultiChipCsConfig` to avoid a dependency on `cs_data_layout::SelectRole`
/// (which also covers X1/X2/HalfSelect roles not applicable here).
///
/// `ControlLineKind::name()` gives the string used in
/// `ChipType::control_lines()`, allowing GPIO lookups for either the
/// address-layout span or the CS-data-layout select lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlLineKind {
    Ce,
    Oe,
    Cs1,
    Cs2,
    Cs3,
}

impl ControlLineKind {
    /// The control line name as used in `ChipType::control_lines()`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Ce => "ce",
            Self::Oe => "oe",
            Self::Cs1 => "cs1",
            Self::Cs2 => "cs2",
            Self::Cs3 => "cs3",
        }
    }
}

fn name_to_control_line_kind(name: &str) -> Option<ControlLineKind> {
    match name {
        "ce" => Some(ControlLineKind::Ce),
        "oe" => Some(ControlLineKind::Oe),
        "cs1" => Some(ControlLineKind::Cs1),
        "cs2" => Some(ControlLineKind::Cs2),
        "cs3" => Some(ControlLineKind::Cs3),
        _ => None,
    }
}

/// Internal config for Multi sets, derived from all chips' `CsConfig`s
/// by `derive_multi_cs_config`.
///
/// Captures which control line is the per-chip select (fly-leaded to X
/// pins for chips[1+]) and which are commoned (always active across all
/// chips — present in the GPIO span for physical contiguity but not used
/// to discriminate between chips).
///
/// For single-control-line chips (e.g. 2364 with only CS1), `commoned_lines`
/// is empty: the one line is always the per-chip select.
///
/// Example — 3 x 27512, OE commoned, CE fly-leaded:
///   `per_chip_select: Ce, commoned_lines: [Oe]`
///
/// Example — 3 x 23128, CS1 fly-leaded, CS2+CS3 commoned:
///   `per_chip_select: Cs1, commoned_lines: [Cs2, Cs3]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiChipCsConfig {
    /// The control line used as the per-chip select: fly-leaded to X1/X2
    /// for chips[1+]. Chip0's instance of this line is its primary CS.
    pub per_chip_select: ControlLineKind,

    /// Control lines commoned across all chips in the set: physically
    /// present and always driven active, contributing to the contiguous
    /// GPIO span but not used to discriminate between chips.
    pub commoned_lines: Vec<ControlLineKind>,
}

/// Get the `CsLogic` for a named control line from a `CsConfig`.
///
/// For CE/OE chips reads from `CeOe`/`CeOeExplicit`; for configurable
/// chips reads cs1/cs2/cs3 logic. Lines not applicable to the config
/// return `ActiveLow` (the unconditional active default).
pub(crate) fn control_line_logic(name: &str, cs_config: &CsConfig) -> CsLogic {
    match name {
        "ce" => match cs_config {
            CsConfig::CeOe => CsLogic::ActiveLow,
            CsConfig::CeOeExplicit { ce, .. } => *ce,
            CsConfig::ChipSelect { .. } => CsLogic::ActiveLow,
        },
        "oe" => match cs_config {
            CsConfig::CeOe => CsLogic::ActiveLow,
            CsConfig::CeOeExplicit { oe, .. } => *oe,
            CsConfig::ChipSelect { .. } => CsLogic::ActiveLow,
        },
        "cs1" => cs_config.cs1_logic().unwrap_or(CsLogic::ActiveLow),
        "cs2" => cs_config.cs2_logic().unwrap_or(CsLogic::ActiveLow),
        "cs3" => cs_config.cs3_logic().unwrap_or(CsLogic::ActiveLow),
        _ => CsLogic::ActiveLow,
    }
}

/// Derive `MultiChipCsConfig` from the chip set's `CsConfig`s.
///
/// For single-control-line chips (e.g. 2364 with only CS1), returns the
/// one line as `per_chip_select` with no commoned lines.
///
/// For N-line chips (N > 1), reads chips[1]'s `CsConfig` to determine
/// which single line is active (per-chip select) and which N-1 are
/// `Ignore` (commoned). `check_cs_v2` has already validated that all
/// chips[1+] agree and that chip[0] does not ignore its per-chip select.
pub fn derive_multi_cs_config(chips: &[Chip]) -> MultiChipCsConfig {
    let chip0_type = *chips[0].chip_type();
    let control_lines = chip0_type.control_lines();

    // Only consider the chip's actual control lines (ce/oe/cs1/cs2/cs3).
    let named: Vec<_> = control_lines
        .iter()
        .filter(|l| name_to_control_line_kind(l.name).is_some())
        .collect();

    // Single-control-line chip: that line is always the per-chip select.
    if named.len() == 1 {
        let kind = name_to_control_line_kind(named[0].name).expect("filtered to known names above");
        return MultiChipCsConfig {
            per_chip_select: kind,
            commoned_lines: Vec::new(),
        };
    }

    // For N > 1 control lines: chips[1]'s CsConfig determines which line
    // is active (per_chip_select) and which are Ignore (commoned).
    // check_cs_v2 guarantees exactly 1 active line — derivation only.
    let ref_config = chips[1].cs_config();

    let mut per_chip_select = None;
    let mut commoned_lines = Vec::new();

    for line in &named {
        let kind = name_to_control_line_kind(line.name).expect("filtered to known names above");
        let logic = control_line_logic(line.name, ref_config);
        if logic == CsLogic::Ignore {
            commoned_lines.push(kind);
        } else {
            per_chip_select = Some(kind);
        }
    }

    MultiChipCsConfig {
        per_chip_select: per_chip_select.expect(
            "Multi set must have exactly one active select line \
                     (validated by check_cs_v2)",
        ),
        commoned_lines,
    }
}
