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
    Cs4,
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
            Self::Cs4 => "cs4",
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
        "cs4" => Some(ControlLineKind::Cs4),
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

    /// Lines commoned across all chips: driven active on chip0 and shared
    /// across the set, so asserted during any valid read. Physically present
    /// in the CS-detect span and part of the "any select active" gate, but
    /// not used to discriminate between chips. Identified by being `Ignore`
    /// on the secondaries yet *active* on chip0.
    pub commoned_lines: Vec<ControlLineKind>,

    /// Lines that are truly ignored: `Ignore` on both the secondaries and
    /// chip0. They carry no meaning for the set — excluded from the CS range
    /// where geometry allows, and forced low in the address window (with
    /// ROM-image duplication as backstop) where they cannot be.
    pub ignored_lines: Vec<ControlLineKind>,
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
        "cs4" => cs_config.cs4_logic().unwrap_or(CsLogic::ActiveLow),
        _ => CsLogic::ActiveLow,
    }
}

/// Derive `MultiChipCsConfig` from the chip set's `CsConfig`s.
///
/// For single-control-line chips (e.g. 2364 with only CS1), returns the one
/// line as `per_chip_select` with no commoned or ignored lines.
///
/// For N-line chips (N > 1): the secondaries (`chips[1+]`) have exactly one
/// active line — the per-chip select, fly-leaded to X1/X2 — and `Ignore` the
/// other N-1 (validated by `check_cs_v2`). The secondary can't say what those
/// N-1 lines *are*, so chip0's polarity disambiguates each:
///
/// * active on chip0 → commoned (a real, always-active shared select)
/// * `Ignore` on chip0 → truly ignored (a don't-care line)
///
/// `check_cs_v2` guarantees all `chips[1+]` agree and that `chips[0]` does not
/// ignore its per-chip select.
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
            ignored_lines: Vec::new(),
        };
    }

    // The secondary's single active line is the per-chip select; the rest are
    // Ignore there. chip0's own polarity then splits those into commoned
    // (active on chip0) vs truly ignored (Ignore on chip0).
    let secondary_config = chips[1].cs_config();
    let chip0_config = chips[0].cs_config();

    let mut per_chip_select = None;
    let mut commoned_lines = Vec::new();
    let mut ignored_lines = Vec::new();

    for line in &named {
        let kind = name_to_control_line_kind(line.name).expect("filtered to known names above");
        if control_line_logic(line.name, secondary_config) != CsLogic::Ignore {
            per_chip_select = Some(kind);
        } else if control_line_logic(line.name, chip0_config) == CsLogic::Ignore {
            ignored_lines.push(kind);
        } else {
            commoned_lines.push(kind);
        }
    }

    MultiChipCsConfig {
        per_chip_select: per_chip_select.expect(
            "Multi set must have exactly one active select line \
                     (validated by check_cs_v2)",
        ),
        commoned_lines,
        ignored_lines,
    }
}