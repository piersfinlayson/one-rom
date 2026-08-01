// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! GPIO input override configuration (point I, override portion).
//!
//! Two distinct concerns are handled here, both contributing to
//! `OneromAlgConfig::gpio_override_config`:
//!
//! ## CS-line polarity overrides
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
//!
//! ## X-pin address-bus inversion (Banked sets)
//!
//! For Banked sets, X1/X2 are jumper inputs that appear as bits in the
//! address-read PIO window. When `board.x_jumper_pull() == 0`, fitting a
//! jumper drives the GPIO low (0), but the ROM table was built with 0 = "no
//! jumper / default bank". Without correction, bank 0 is served when the
//! jumper IS fitted and bank 1 when it is not — the opposite of the expected
//! convention.
//!
//! `GpioOverInvert` on the X1/X2 address GPIOs corrects this: fitting a
//! jumper still drives the pin low physically, but the PIO sees it as 1,
//! so bank 1 is selected. Bank 0 remains the "no jumper" default on all
//! boards regardless of `x_jumper_pull` direction.
//!
//! When `x_jumper_pull() != 0` (jumper pulls high), fitting a jumper drives
//! the GPIO high (1) and no inversion is needed.
//!
//! ## Unused address-window pins (`GpioOverLow`)
//!
//! The address-read PIO reads a contiguous GPIO window and uses it
//! directly as the ROM-table index. Some GPIOs in that window aren't real
//! address lines for this ROM type - genuine gaps in the span,
//! `MIN_ADDR_PINS` padding, or (the motivating case) GPIOs mapped to
//! socket pins that are NC on the emulated chip. The doubled ROM image
//! already serves correct data for either value of such a bit, but a host
//! that ties an emulated NC pin to a 1 would drive that GPIO high and index
//! into the duplicated upper half. `build_unused_addr_overrides` pins each
//! such GPIO's input to 0 (`GpioOverLow`), making the served address
//! independent of whatever the host does with those pins.
//!
//! A GPIO is only forced if it is used for *nothing* else - never an
//! address line, X pin, CS/select line, commoned line, data line or
//! `/BYTE` pin. The entries are therefore disjoint from the CS-polarity,
//! `/BYTE`-invert and X-pin-inversion overrides above: no GPIO receives
//! two override entries.
//!
//! Note the distinction between *commoned* and *truly-ignored* control lines
//! in a Multi set. A commoned line is driven active on every read and is a
//! real member of the CS-detect range, so it counts as "used" and is never
//! forced low. A truly-ignored line (`Ignore` on chip0 as well as the
//! secondaries) carries no meaning: `derive_cs_data_layout` keeps it out of
//! the CS range wherever the geometry allows, so it is *not* "used" here and
//! is forced low if it falls inside the address window. This is the case that
//! motivated the split - see `multi_cs_config::derive_multi_cs_config`.

use alloc::collections::BTreeSet;
use alloc::vec::Vec;

use onerom_config::hw::Board;
use onerom_metadata::{GpioOverride, OneromAlgDataConfig};

use crate::image::{ChipSetType, CsConfig, CsLogic};

use super::addr_layout::AddrLayout;
use super::cs_data_layout::{CsDataLayout, SelectRole};

/// The `CsLogic` a select line is configured as, for override comparison.
///
/// Used only for chip[0]'s own control lines. `X1`/`X2` are handled
/// separately in `build_cs_overrides` using the respective secondary
/// chip's `cs1_logic`, because in a Multi set with mixed polarities those
/// lines carry a different chip's CS and must not inherit chip[0]'s logic.
///
/// - `Cs1`/`HalfSelect` read `cs_config.cs1_logic()`. For `HalfSelect`,
///   `derive_cs_data_layout` pre-validates that `cs1_logic` is
///   `Some(ActiveLow|ActiveHigh)`, so the `unwrap` is safe.
/// - `Cs2`/`Cs3`/`Cs4` read the corresponding logic; `select_phys_pins` only
///   includes them when `Some(ActiveLow|ActiveHigh)`, so `unwrap_or` here
///   is just a defensive fallback, not expected to be hit.
/// - `Ce`/`Oe` (27xx-style fixed lines) are always active-low.
///
/// A CS line's polarity may be fixed by the silicon rather than chosen by the
/// user (e.g. the HM7641's active-high CS3/CS4). That is resolved into
/// `CsConfig` by `CsConfig::from_chip_type`, so it needs no special handling
/// here - a fixed line reads exactly like a configured one.
fn cs_logic_for_role(role: SelectRole, cs_config: &CsConfig) -> CsLogic {
    match role {
        SelectRole::Cs1 | SelectRole::X1 | SelectRole::X2 | SelectRole::HalfSelect => {
            cs_config.cs1_logic().unwrap()
        }
        SelectRole::Cs2 => cs_config.cs2_logic().unwrap_or(CsLogic::ActiveLow),
        SelectRole::Cs3 => cs_config.cs3_logic().unwrap_or(CsLogic::ActiveLow),
        SelectRole::Cs4 => cs_config.cs4_logic().unwrap_or(CsLogic::ActiveLow),
        SelectRole::Ce => CsLogic::ActiveLow,
        SelectRole::Oe => CsLogic::ActiveLow,
    }
}

/// The CS polarity the PIO requires, for this set type.
fn required_cs_logic(set_type: ChipSetType) -> CsLogic {
    match set_type {
        ChipSetType::Single | ChipSetType::Banked => CsLogic::ActiveLow,
        ChipSetType::Multi => CsLogic::ActiveHigh,
    }
}

pub fn encode_override(gpio: u8, ov: GpioOverride) -> u8 {
    ((ov as u8) << 6) | (gpio & 0x3F)
}

/// Build the `GpioOverInvert` entries (point I) needed to make every select
/// line *and every commoned line* in `layout` conform to the PIO's required CS
/// polarity for `set_type`. Commoned lines sit in the CS-detect range but are
/// not select lines; they are driven active by chip0, so their polarity is read
/// from `cs_config`. Without normalising them, an active-low commoned line
/// reads high at idle in a Multi set and spuriously fires the CS gate. The
/// `cs_ignore_index` gap (if any) isn't in either list, so is untouched.
///
/// `HalfSelect` lines (excess address pins acting as half-selects for
/// oversized ROMs, e.g. 27C080's A19) are treated identically to `Cs1`:
/// their polarity is `cs1_logic`, and an override is emitted if that
/// doesn't match the required convention for this set type.
///
/// `secondary_cs_configs` carries `chips[1]`'s config at index 0 and
/// `chips[2]`'s at index 1 (Multi sets only; empty for Single/Banked).
/// X1/X2 carry the *per-chip select line* of the respective secondary chip -
/// which is not necessarily CS1 (it can be CS2/CS3/CE/OE). Their override
/// decisions therefore read that chip's logic for the per-chip-select role
/// (`select_lines[0].role`), not a hardcoded `cs1_logic`: reading CS1 for a
/// CS2-primary set gives the wrong polarity and mis-inverts the X pins.
pub fn build_cs_overrides(
    layout: &CsDataLayout,
    set_type: ChipSetType,
    cs_config: &CsConfig,
    secondary_cs_configs: &[CsConfig],
) -> Vec<u8> {
    let required = required_cs_logic(set_type);

    // The per-chip select (select_lines[0] in a Multi set) is the line
    // fly-leaded to X1/X2, so it determines which of each secondary chip's
    // control lines those X pins carry. Not necessarily CS1.
    let per_chip_role = layout
        .select_lines
        .first()
        .map(|l| l.role)
        .unwrap_or(SelectRole::Cs1);

    layout
        .select_lines
        .iter()
        .filter_map(|line| {
            let configured = match line.role {
                SelectRole::X1 => secondary_cs_configs
                    .first()
                    .map(|c| cs_logic_for_role(per_chip_role, c))
                    .unwrap_or(CsLogic::ActiveLow),
                SelectRole::X2 => secondary_cs_configs
                    .get(1)
                    .map(|c| cs_logic_for_role(per_chip_role, c))
                    .unwrap_or(CsLogic::ActiveLow),
                // Every other role takes its polarity from the set's primary
                // chip config.  HalfSelect is deliberately here: its active
                // state follows cs1_logic, so it goes down the same path as
                // Cs1 (see SelectRole::HalfSelect).
                SelectRole::Cs1
                | SelectRole::Cs2
                | SelectRole::Cs3
                | SelectRole::Cs4
                | SelectRole::Ce
                | SelectRole::Oe
                | SelectRole::HalfSelect => cs_logic_for_role(line.role, cs_config),
            };
            if configured != required {
                Some(encode_override(line.gpio, GpioOverride::GpioOverInvert))
            } else {
                None
            }
        })
        // Commoned lines are driven active by chip0, so their polarity comes
        // from `cs_config`. Normalise them to the required convention exactly
        // as for select lines.
        .chain(layout.commoned_lines.iter().filter_map(|line| {
            if cs_logic_for_role(line.role, cs_config) != required {
                Some(encode_override(line.gpio, GpioOverride::GpioOverInvert))
            } else {
                None
            }
        }))
        .collect()
}

/// Build `GpioOverInvert` entries for the X1/X2 address-bus GPIOs of a
/// Banked set when `board.x_jumper_pull() == 0`.
///
/// When the jumper pulls low on close, fitting it drives the GPIO to 0.
/// The address PIO reads that 0 and indexes into the ROM table with that
/// bit clear — so bank 0 is served when the jumper IS fitted and bank 1
/// when it is not. This is the opposite of the expected convention
/// (fitting a jumper = selecting a non-default bank).
///
/// Inverting the GPIO input corrects this: the PIO sees 1 when the jumper
/// is fitted (selecting bank 1) and 0 when it is not (bank 0 = default),
/// making the bank assignment consistent across boards regardless of
/// `x_jumper_pull` direction.
///
/// Mirrors `build_gpio_pull_config` in chip-count gating: 1 entry for
/// 2-chip (X1 only), 2 entries for 3/4-chip (X1 and X2). Returns an empty
/// `Vec` for Single/Multi sets and for Banked sets where
/// `x_jumper_pull != 0` (no inversion needed there).
pub fn build_gpio_x_overrides(
    layout: &AddrLayout,
    set_type: ChipSetType,
    num_chips: usize,
    board: Board,
) -> Vec<u8> {
    if !matches!(set_type, ChipSetType::Banked) {
        return Vec::new();
    }

    // Inversion only needed when the jumper pulls low: fitting drives the
    // GPIO to 0, but the ROM table expects 1 to mean "this bank selected".
    if board.x_jumper_pull() != 0 {
        return Vec::new();
    }

    let mut params = Vec::with_capacity(2);

    if let Some(x1_gpio) = layout.x1_gpio {
        params.push(encode_override(x1_gpio, GpioOverride::GpioOverInvert));
    }

    #[allow(clippy::collapsible_if)]
    if num_chips >= 3 {
        if let Some(x2_gpio) = layout.x2_gpio {
            params.push(encode_override(x2_gpio, GpioOverride::GpioOverInvert));
        }
    }

    params
}

/// Build the `GpioOverLow` entries forcing every GPIO inside the
/// address-read PIO window that serves no purpose for this ROM type to
/// read 0. See the module-level "Unused address-window pins" section for
/// the rationale.
///
/// The address PIO reads `[addr_layout.gpio_base, addr_layout.gpio_base +
/// num_addr_pins)`. A GPIO in that window is forced low iff it is absent
/// from the "used" set - the union of every GPIO either layout references:
///
/// - address lines (`addr_pin_gpios`),
/// - X1/X2 *as resolved within the address window* (`addr_layout`'s own
///   fields - `Some` only for Banked/Multi, so a Single set's X-pin GPIOs
///   are correctly treated as unused and forced low),
/// - excess / half-select address pins,
/// - data lines,
/// - every select line, plus the full CS GPIO range. The range is what
///   captures Multi commoned lines and the `AlgCs1` `cs_ignore` gap pin -
///   both physically present and inside the CS span, but deliberately
///   absent from `select_lines`. Truly-ignored control lines are *not* in
///   this range (`derive_cs_data_layout` excludes them), so they are not
///   "used" and are forced low if they fall inside the address window,
/// - the `/BYTE` input pin (`AlgData1` only).
///
/// Entries are emitted in ascending GPIO order for deterministic output.
/// Because every entry produced by the other override builders targets a
/// GPIO in the used set, the result is disjoint from them.
pub fn build_unused_addr_overrides(
    addr_layout: &AddrLayout,
    cs_data_layout: &CsDataLayout,
    alg_data: &OneromAlgDataConfig,
) -> Vec<u8> {
    let mut used: BTreeSet<u8> = BTreeSet::new();

    // Address-side GPIOs: real address lines, the addr-window X pins
    // (Banked/Multi only), and any excess/half-select lines.
    used.extend(addr_layout.addr_pin_gpios.iter().copied());
    used.extend(addr_layout.x1_gpio);
    used.extend(addr_layout.x2_gpio);
    used.extend(addr_layout.excess_addr_pin_gpios.iter().copied());

    // Data lines and explicit select lines.
    used.extend(cs_data_layout.data_pin_gpios.iter().copied());
    used.extend(cs_data_layout.select_lines.iter().map(|line| line.gpio));

    // Full CS GPIO range: captures Multi commoned lines and the AlgCs1
    // cs_ignore gap, neither of which appears in select_lines. Truly-ignored
    // lines are excluded from this range upstream, so they fall through to
    // GpioOverLow below.
    let cs_base = cs_data_layout.gpio_base + cs_data_layout.base_cs_pin;
    used.extend(cs_base..cs_base + cs_data_layout.num_cs_pins);

    // /BYTE is an input the AlgData1 data PIO reads each access.
    if let OneromAlgDataConfig::AlgData1 { byte_pin, .. } = alg_data {
        used.insert(*byte_pin + cs_data_layout.gpio_base);
    }

    let window_start = addr_layout.gpio_base;
    let window_end = addr_layout.gpio_base + addr_layout.num_addr_pins;
    (window_start..window_end)
        .filter(|gpio| !used.contains(gpio))
        .map(|gpio| encode_override(gpio, GpioOverride::GpioOverLow))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::cs_data_layout::SelectLine;
    use super::*;

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
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        }
    }

    fn layout_with_commoned(
        select_lines: Vec<SelectLine>,
        commoned_lines: Vec<SelectLine>,
    ) -> CsDataLayout {
        CsDataLayout {
            gpio_base: 0,
            base_data_pin: 16,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![16, 17, 18, 19, 20, 21, 22, 23],
            base_cs_pin: 13,
            num_cs_pins: (select_lines.len() + commoned_lines.len()) as u8,
            cs_ignore_index: None,
            select_lines,
            commoned_lines,
            alg_cs2: None,
        }
    }

    // ========================================================================
    // build_cs_overrides tests
    // ========================================================================

    /// Single set, CS1 configured ActiveLow (matches required ActiveLow) ->
    /// no override.
    #[test]
    fn single_active_low_no_override() {
        let layout = layout_with(alloc::vec![SelectLine {
            role: SelectRole::Cs1,
            gpio: 13
        }]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);

        let overrides = build_cs_overrides(&layout, ChipSetType::Single, &cs_config, &[]);
        assert!(overrides.is_empty());
    }

    /// Single set, CS1 configured ActiveHigh (required ActiveLow) ->
    /// GpioOverInvert on GPIO13.
    #[test]
    fn single_active_high_inverted() {
        let layout = layout_with(alloc::vec![SelectLine {
            role: SelectRole::Cs1,
            gpio: 13
        }]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveHigh), None, None, None);

        let overrides = build_cs_overrides(&layout, ChipSetType::Single, &cs_config, &[]);
        assert_eq!(
            overrides,
            alloc::vec![encode_override(13, GpioOverride::GpioOverInvert)]
        );
    }

    /// Multi set, CS1 configured ActiveLow (required ActiveHigh for Multi)
    /// -> CS1, X1 and X2 all inverted.
    #[test]
    fn multi_active_low_all_inverted() {
        let layout = layout_with(alloc::vec![
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
        ]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);

        let overrides = build_cs_overrides(
            &layout,
            ChipSetType::Multi,
            &cs_config,
            &[
                CsConfig::new(Some(CsLogic::ActiveLow), None, None, None),
                CsConfig::new(Some(CsLogic::ActiveLow), None, None, None),
            ],
        );
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
            SelectLine {
                role: SelectRole::Cs1,
                gpio: 13
            },
            SelectLine {
                role: SelectRole::X1,
                gpio: 14
            },
        ]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveHigh), None, None, None);

        let overrides = build_cs_overrides(
            &layout,
            ChipSetType::Multi,
            &cs_config,
            &[CsConfig::new(Some(CsLogic::ActiveHigh), None, None, None)],
        );
        assert!(overrides.is_empty());
    }

    /// Multi set, mixed polarity: chip[0] CS1 active_low (needs inversion),
    /// chip[1] X1 active_high (matches required active_high, no inversion).
    /// Regression test for the bug where X1 used chip[0]'s polarity and
    /// was incorrectly inverted, causing chip[1] to never be served.
    #[test]
    fn multi_mixed_polarity_only_cs1_inverted() {
        let layout = layout_with(alloc::vec![
            SelectLine {
                role: SelectRole::Cs1,
                gpio: 13
            },
            SelectLine {
                role: SelectRole::X1,
                gpio: 14
            },
        ]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let secondary = &[CsConfig::new(Some(CsLogic::ActiveHigh), None, None, None)];

        let overrides = build_cs_overrides(&layout, ChipSetType::Multi, &cs_config, secondary);
        // Only CS1 inverted (active_low ≠ required active_high).
        // X1 not inverted (chip[1] active_high == required active_high).
        assert_eq!(
            overrides,
            alloc::vec![encode_override(13, GpioOverride::GpioOverInvert)]
        );
    }

    /// 27C080-shaped layout: CE (fixed active-low) + HalfSelect (A19, cs1
    /// active-high = serve upper half). For a Single set (required
    /// active-low), CE needs no override (already active-low) but A19
    /// does (active-high ≠ active-low).
    #[test]
    fn half_select_active_high_inverted_for_single() {
        let layout = layout_with(alloc::vec![
            SelectLine {
                role: SelectRole::Ce,
                gpio: 13
            },
            SelectLine {
                role: SelectRole::HalfSelect,
                gpio: 14
            },
        ]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveHigh), None, None, None);

        let overrides = build_cs_overrides(&layout, ChipSetType::Single, &cs_config, &[]);
        // CE is always active-low (matches required) -> no override.
        // HalfSelect is active-high (cs1_logic) != active-low (required) -> override.
        assert_eq!(
            overrides,
            alloc::vec![encode_override(14, GpioOverride::GpioOverInvert)]
        );
    }

    /// 27C080-shaped layout: cs1 active-low = serve lower half. CE and A19
    /// are both effectively active-low -> no overrides needed for Single.
    #[test]
    fn half_select_active_low_no_override_for_single() {
        let layout = layout_with(alloc::vec![
            SelectLine {
                role: SelectRole::Ce,
                gpio: 13
            },
            SelectLine {
                role: SelectRole::HalfSelect,
                gpio: 14
            },
        ]);
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);

        let overrides = build_cs_overrides(&layout, ChipSetType::Single, &cs_config, &[]);
        assert!(overrides.is_empty());
    }

    // ========================================================================
    // build_gpio_x_overrides tests
    // ========================================================================

    /// AddrLayout with X1/X2 set to illustrative GPIO values. Fire24A has
    /// x_jumper_pull() == 0 (jumper pulls low when fitted), which is the
    /// condition that requires inversion.
    fn addr_layout_with_x(x1: u8, x2: u8) -> AddrLayout {
        AddrLayout {
            gpio_base: 0,
            num_addr_pins: 16,
            x1_gpio: Some(x1),
            x2_gpio: Some(x2),
            addr_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12],
            excess_addr_pin_gpios: alloc::vec![],
        }
    }

    /// Single sets never need X-pin overrides.
    #[test]
    fn x_override_single_never() {
        let layout = addr_layout_with_x(14, 15);
        let overrides = build_gpio_x_overrides(&layout, ChipSetType::Single, 1, Board::Fire24A);
        assert!(overrides.is_empty());
    }

    /// Multi sets never need X-pin overrides (X1/X2 are driven CS outputs,
    /// not jumper inputs).
    #[test]
    fn x_override_multi_never() {
        let layout = addr_layout_with_x(14, 15);
        let overrides = build_gpio_x_overrides(&layout, ChipSetType::Multi, 3, Board::Fire24A);
        assert!(overrides.is_empty());
    }

    /// 2-chip Banked, Fire24A (x_jumper_pull=0): X1 gets GpioOverInvert;
    /// X2 is not included (2-chip sets only use X1).
    #[test]
    fn x_override_banked_2chip_x1_only() {
        let layout = addr_layout_with_x(14, 15);
        let overrides = build_gpio_x_overrides(&layout, ChipSetType::Banked, 2, Board::Fire24A);

        assert_eq!(
            overrides,
            alloc::vec![encode_override(14, GpioOverride::GpioOverInvert)]
        );
    }

    /// 3-chip Banked, Fire24A (x_jumper_pull=0): both X1 and X2 get
    /// GpioOverInvert. Bank index 3 (X1=1, X2=1) is the "no chip" slot, but
    /// both jumpers exist and both GPIOs need inversion so the address PIO
    /// reads them correctly.
    #[test]
    fn x_override_banked_3chip_x1_and_x2() {
        let layout = addr_layout_with_x(14, 15);
        let overrides = build_gpio_x_overrides(&layout, ChipSetType::Banked, 3, Board::Fire24A);

        assert_eq!(
            overrides,
            alloc::vec![
                encode_override(14, GpioOverride::GpioOverInvert),
                encode_override(15, GpioOverride::GpioOverInvert),
            ]
        );
    }

    /// 4-chip Banked, Fire24A (x_jumper_pull=0): both X1 and X2 get
    /// GpioOverInvert, same as 3-chip.
    #[test]
    fn x_override_banked_4chip_x1_and_x2() {
        let layout = addr_layout_with_x(14, 15);
        let overrides = build_gpio_x_overrides(&layout, ChipSetType::Banked, 4, Board::Fire24A);

        assert_eq!(
            overrides,
            alloc::vec![
                encode_override(14, GpioOverride::GpioOverInvert),
                encode_override(15, GpioOverride::GpioOverInvert),
            ]
        );
    }

    /// Verify encode_override produces the correct byte: top 2 bits =
    /// GpioOverride discriminant, bottom 6 bits = GPIO number.
    #[test]
    fn encode_override_invert_format() {
        // GpioOverInvert = 1, GPIO 14: (1 << 6) | 14 = 0x4E
        assert_eq!(
            encode_override(14, GpioOverride::GpioOverInvert),
            (1u8 << 6) | 14
        );
        // GpioOverNormal = 0, GPIO 14: (0 << 6) | 14 = 0x0E
        assert_eq!(encode_override(14, GpioOverride::GpioOverNormal), 14);
    }

    // ========================================================================
    // build_unused_addr_overrides tests
    // ========================================================================

    /// 8-bit `AlgData0` with data lines well outside the address window.
    fn alg_data0_high() -> OneromAlgDataConfig {
        OneromAlgDataConfig::AlgData0 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            base_data_pin: 16,
            word_size: 8,
        }
    }

    /// Fire24A 2364 CS/data layout: data on GPIO16-23, CS1 on GPIO13.
    fn fire24a_2364_cs_data() -> CsDataLayout {
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
                gpio: 13,
            }],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        }
    }

    /// Single Fire24A 2364: address window [0,16). Address lines occupy
    /// {0-7,10,11,12,14,15}, CS1 is GPIO13. GPIO8 (X2) and GPIO9 (X1) carry
    /// nothing on a Single set, so both are forced low.
    #[test]
    fn unused_single_forces_unused_x_pins() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 16,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12],
            excess_addr_pin_gpios: alloc::vec![],
        };

        let overrides =
            build_unused_addr_overrides(&addr_layout, &fire24a_2364_cs_data(), &alg_data0_high());

        assert_eq!(
            overrides,
            alloc::vec![
                encode_override(8, GpioOverride::GpioOverLow),
                encode_override(9, GpioOverride::GpioOverLow),
            ]
        );
    }

    /// 2-chip Banked Fire24A 2364: X1 (GPIO9) is the bank-select line, so
    /// it's "used" and not forced. X2 (GPIO8) is unused on a 2-chip set and
    /// is the only gap, so it's forced low.
    #[test]
    fn unused_banked_excludes_used_x1() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 16,
            x1_gpio: Some(9),
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12],
            excess_addr_pin_gpios: alloc::vec![],
        };

        let overrides =
            build_unused_addr_overrides(&addr_layout, &fire24a_2364_cs_data(), &alg_data0_high());

        assert_eq!(
            overrides,
            alloc::vec![encode_override(8, GpioOverride::GpioOverLow)]
        );
    }

    /// 3/4-chip Banked Fire24A 2364: both X1 (GPIO9) and X2 (GPIO8) are
    /// used. The window {0..15} is then fully occupied (addr + X1 + X2 +
    /// CS1), so nothing is forced.
    #[test]
    fn unused_full_window_emits_nothing() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 16,
            x1_gpio: Some(9),
            x2_gpio: Some(8),
            addr_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12],
            excess_addr_pin_gpios: alloc::vec![],
        };

        let overrides =
            build_unused_addr_overrides(&addr_layout, &fire24a_2364_cs_data(), &alg_data0_high());

        assert!(overrides.is_empty());
    }

    /// Synthetic layout exercising the `/BYTE` and full-CS-range exclusions
    /// directly. Window [0,8): address lines {0,1,2,3}; a select line on
    /// GPIO7 with a commoned line on GPIO6 (CS range {6,7}, only GPIO7 in
    /// `select_lines`); `/BYTE` on GPIO5. GPIO4 is the sole gap and is the
    /// only GPIO forced low - GPIO5 (byte), GPIO6 (commoned, via CS range)
    /// and GPIO7 (select line) are all excluded.
    #[test]
    fn unused_excludes_byte_pin_and_commoned_cs_range() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 8,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![0, 1, 2, 3],
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 24,
            num_data_pins: 4,
            data_pin_gpios: alloc::vec![24, 25, 26, 27],
            base_cs_pin: 6,
            num_cs_pins: 2, // CS range {6,7}: 7 = select line, 6 = commoned.
            cs_ignore_index: None,
            select_lines: alloc::vec![SelectLine {
                role: SelectRole::Ce,
                gpio: 7,
            }],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };
        let alg_data = OneromAlgDataConfig::AlgData1 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            base_data_pin: 24,
            word_size: 16,
            byte_pin: 5,
            a_minus_1_pin: 27,
        };

        let overrides = build_unused_addr_overrides(&addr_layout, &cs_data_layout, &alg_data);

        assert_eq!(
            overrides,
            alloc::vec![encode_override(4, GpioOverride::GpioOverLow)]
        );
    }

    /// Multi set with an active-low commoned line: it must be inverted just
    /// like an active-low select line, otherwise it reads high at idle and
    /// spuriously fires the CS gate. Regression for the commoned-not-normalised
    /// bug (bus violations on active-low commoned lines).
    #[test]
    fn multi_active_low_commoned_inverted() {
        let layout = layout_with_commoned(
            alloc::vec![
                SelectLine {
                    role: SelectRole::Cs1,
                    gpio: 13,
                },
                SelectLine {
                    role: SelectRole::X1,
                    gpio: 15,
                },
            ],
            alloc::vec![SelectLine {
                role: SelectRole::Cs2,
                gpio: 14,
            }],
        );
        // chip0: cs1 (select) and cs2 (commoned) both active_low.
        let cs_config = CsConfig::new(
            Some(CsLogic::ActiveLow),
            Some(CsLogic::ActiveLow),
            None,
            None,
        );
        let secondary = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);

        let overrides = build_cs_overrides(&layout, ChipSetType::Multi, &cs_config, &[secondary]);

        // Selects (CS1@13, X1@15) invert first, then the commoned CS2@14.
        assert_eq!(
            overrides,
            alloc::vec![
                encode_override(13, GpioOverride::GpioOverInvert),
                encode_override(15, GpioOverride::GpioOverInvert),
                encode_override(14, GpioOverride::GpioOverInvert),
            ]
        );
    }

    /// Multi set with an active-high commoned line: already matches the
    /// required active-high convention, so it needs no override - only the
    /// active-low select is inverted.
    #[test]
    fn multi_active_high_commoned_not_inverted() {
        let layout = layout_with_commoned(
            alloc::vec![
                SelectLine {
                    role: SelectRole::Cs1,
                    gpio: 13,
                },
                SelectLine {
                    role: SelectRole::X1,
                    gpio: 15,
                },
            ],
            alloc::vec![SelectLine {
                role: SelectRole::Cs2,
                gpio: 14,
            }],
        );
        // cs1 (select) active_low -> invert; cs2 (commoned) active_high -> not.
        let cs_config = CsConfig::new(
            Some(CsLogic::ActiveLow),
            Some(CsLogic::ActiveHigh),
            None,
            None,
        );
        let secondary = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);

        let overrides = build_cs_overrides(&layout, ChipSetType::Multi, &cs_config, &[secondary]);

        assert_eq!(
            overrides,
            alloc::vec![
                encode_override(13, GpioOverride::GpioOverInvert),
                encode_override(15, GpioOverride::GpioOverInvert),
            ]
        );
    }

    /// Multi set, CS2-primary and active-high: X1/X2 carry the secondaries'
    /// CS2, so their polarity must be read as CS2 (active-high == required) and
    /// NOT CS1 (ignored -> defaults active-low, which would wrongly invert the
    /// X pins). Regression for the X-pin-reads-cs1 bug that corrupted the CS
    /// gate and ROM-select index on active-high CS2-primary multi sets.
    #[test]
    fn multi_cs2_primary_active_high_x_pins_not_inverted() {
        let layout = layout_with(alloc::vec![
            SelectLine {
                role: SelectRole::Cs2,
                gpio: 14,
            },
            SelectLine {
                role: SelectRole::X1,
                gpio: 15,
            },
            SelectLine {
                role: SelectRole::X2,
                gpio: 16,
            },
        ]);
        // chip0 drives CS2 active-high (the per-chip select); CS1/CS3 ignored.
        let cs_config = CsConfig::new(
            Some(CsLogic::Ignore),
            Some(CsLogic::ActiveHigh),
            Some(CsLogic::Ignore),
            None,
        );

        let overrides = build_cs_overrides(
            &layout,
            ChipSetType::Multi,
            &cs_config,
            &[
                CsConfig::new(
                    Some(CsLogic::Ignore),
                    Some(CsLogic::ActiveHigh),
                    Some(CsLogic::Ignore),
                    None,
                ),
                CsConfig::new(
                    Some(CsLogic::Ignore),
                    Some(CsLogic::ActiveHigh),
                    Some(CsLogic::Ignore),
                    None,
                ),
            ],
        );

        // Everything already active-high == required: no inversions at all.
        assert!(
            overrides.is_empty(),
            "CS2-primary active-high multi must not invert X pins, got {overrides:?}"
        );
    }
}
