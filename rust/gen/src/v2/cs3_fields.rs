// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The `ALG_CS_3` field descriptor: one byte per chip select field, in the
//! order the serving state machine reads them.
//!
//! The firmware takes one sample of the CS window and shifts fields off it,
//! testing each.  A byte is `op << 6 | width`, where width is in bits and op
//! is an [`onerom_metadata::OneromAlgCs3Op`].  The slot is served when every
//! `COMMON` field is active and either one `GROUP` field or one bit of an
//! `ANY` field is.
//!
//! Read order is pin order, and [`Cs3Descriptor::high_gpio_first`] says which
//! end of the window it starts from.  Only the first field tested can send the
//! state machine straight back to polling without evaluating the rest, so the
//! direction is chosen to put the field most often inactive first — the
//! selects, which are inactive whenever the bus is not addressing this slot,
//! ahead of the commoned lines, which on a board that ties one active are
//! asserted whatever the bus is doing.

use alloc::vec::Vec;

use super::cs_data_layout::CsDataLayout;

/// The most fields a descriptor may hold.
///
/// Deliberately smaller than the geometry can produce.  Every arrangement we
/// serve needs two or three, and the cost of the limit being too low is a
/// host-side constant, against a device that boots into limp mode if it is too
/// high.  [`ConfigOverrides`](crate::ConfigOverrides) lifts it.
pub const MAX_CS3_FIELDS: usize = 3;

/// What `ALG_CS_3` does with one field.  Mirrors `onerom_alg_cs3_op_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cs3Op {
    Skip = 0,
    Common = 1,
    Group = 2,
    Any = 3,
}

/// A derived `ALG_CS_3` descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cs3Descriptor {
    /// One byte per field, in read order.
    pub fields: Vec<u8>,
    /// Read order: `false` starts at the lowest GPIO in the CS window.
    pub high_gpio_first: bool,
}

/// Why a chip set has no `ALG_CS_3` descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cs3Error {
    /// More fields than [`MAX_CS3_FIELDS`].
    TooManyFields { fields: usize, max: usize },
}

/// The role a single GPIO in the CS window plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Select,
    Common,
    Gap,
}

impl Cs3Descriptor {
    /// Derive the descriptor for a chip set's CS window.
    ///
    /// `allow_over_max` accepts a descriptor longer than [`MAX_CS3_FIELDS`],
    /// for the caller that has been told to.
    pub fn derive(layout: &CsDataLayout, allow_over_max: bool) -> Result<Self, Cs3Error> {
        let roles = window_roles(layout);

        // Both directions are valid, so build each and take the one whose
        // first field is a select.  Where neither or both start with one the
        // ascending order wins, since it needs no padding instruction.
        let ascending = coalesce(&roles);
        let descending = {
            let mut reversed = roles.clone();
            reversed.reverse();
            coalesce(&reversed)
        };
        let prefer_descending = !starts_with_select(&ascending) && starts_with_select(&descending);
        let fields = if prefer_descending {
            descending
        } else {
            ascending
        };

        if fields.len() > MAX_CS3_FIELDS && !allow_over_max {
            return Err(Cs3Error::TooManyFields {
                fields: fields.len(),
                max: MAX_CS3_FIELDS,
            });
        }

        Ok(Self {
            fields: fields.iter().map(|(op, width)| encode(*op, *width)).collect(),
            high_gpio_first: prefer_descending,
        })
    }
}

/// Pack one field byte.  Width is in bits and never exceeds the CS window,
/// which is far below the 6 bits the encoding gives it.
fn encode(op: Cs3Op, width: u8) -> u8 {
    ((op as u8) << 6) | (width & 0x3F)
}

fn starts_with_select(fields: &[(Cs3Op, u8)]) -> bool {
    matches!(fields.first(), Some((Cs3Op::Any | Cs3Op::Group, _)))
}

/// The role of every GPIO in the CS window, lowest first.
fn window_roles(layout: &CsDataLayout) -> Vec<Role> {
    let base = layout.gpio_base + layout.base_cs_pin;
    (0..layout.num_cs_pins)
        .map(|i| {
            let gpio = base + i;
            if layout.select_lines.iter().any(|l| l.gpio == gpio) {
                Role::Select
            } else if layout.commoned_lines.iter().any(|l| l.gpio == gpio) {
                Role::Common
            } else {
                Role::Gap
            }
        })
        .collect()
}

/// Coalesce adjacent GPIOs of the same role into fields.
///
/// A run of adjacent selects is one `ANY` field: each is a different chip's
/// own single select line, and any one of them being active means that chip is
/// selected.  `GROUP` — a chip whose select is more than one line — is not
/// reachable from a config today, because `check_cs_v2` requires a multi set's
/// secondaries to have exactly one active control line.
fn coalesce(roles: &[Role]) -> Vec<(Cs3Op, u8)> {
    let mut out: Vec<(Cs3Op, u8)> = Vec::new();
    for role in roles {
        let op = match role {
            Role::Select => Cs3Op::Any,
            Role::Common => Cs3Op::Common,
            Role::Gap => Cs3Op::Skip,
        };
        match out.last_mut() {
            Some((last_op, width)) if *last_op == op => *width += 1,
            _ => out.push((op, 1)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::cs_data_layout::{SelectLine, SelectRole};

    /// A layout with the given window and lines, enough for `derive`.
    fn layout(base_cs_pin: u8, num_cs_pins: u8, selects: &[u8], commoned: &[u8]) -> CsDataLayout {
        CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7],
            base_cs_pin,
            num_cs_pins,
            cs_ignore_index: None,
            select_lines: selects
                .iter()
                .map(|&gpio| SelectLine {
                    role: SelectRole::Cs1,
                    gpio,
                })
                .collect(),
            commoned_lines: commoned
                .iter()
                .map(|&gpio| SelectLine {
                    role: SelectRole::Cs2,
                    gpio,
                })
                .collect(),
            alg_cs2: None,
        }
    }

    /// Three 2316s on fire-24-c: X2, X1 and CS1 at GPIO 8-10 are three chips'
    /// own selects, CS2 at 11 is commoned.  Selects are lowest, so ascending
    /// order already tests them first and no padding instruction is needed.
    #[test]
    fn three_2316s_any_then_common() {
        let d = Cs3Descriptor::derive(&layout(8, 4, &[8, 9, 10], &[11]), false).unwrap();
        assert_eq!(d.fields, alloc::vec![0xC3, 0x41]);
        assert!(!d.high_gpio_first);
    }

    /// Two 2332s: X1 and CS1 at GPIO 9-10, the chip's own A11 at 11 with
    /// nothing to say, CS2 commoned at 12.  The gap becomes a SKIP rather than
    /// splitting the window.
    #[test]
    fn two_2332s_skip_the_address_line() {
        let d = Cs3Descriptor::derive(&layout(9, 4, &[9, 10], &[12]), false).unwrap();
        assert_eq!(d.fields, alloc::vec![0xC2, 0x01, 0x41]);
        assert!(!d.high_gpio_first);
    }

    /// Three 2364s: one select each, nothing commoned.  A single ANY field
    /// spanning the whole window is what lets the firmware emit its direct
    /// form, which is the program these slots already run.
    #[test]
    fn three_2364s_are_one_field() {
        let d = Cs3Descriptor::derive(&layout(8, 3, &[8, 9, 10], &[]), false).unwrap();
        assert_eq!(d.fields, alloc::vec![0xC3]);
    }

    /// A commoned line below the selects: reading from the high end puts the
    /// selects first, so the poll loop can bail without testing the commoned
    /// line on every pass.
    #[test]
    fn commoned_below_selects_reads_from_the_top() {
        let d = Cs3Descriptor::derive(&layout(8, 4, &[9, 10, 11], &[8]), false).unwrap();
        assert_eq!(d.fields, alloc::vec![0xC3, 0x41]);
        assert!(d.high_gpio_first);
    }

    /// `24-multi-2316.json` set 3: chip 0's per-chip select is CS2, and CS1 is
    /// commoned.  On fire-24-c that puts the commoned line at GPIO 10 between
    /// X1 at 9 and CS2 at 11, so the selects cannot coalesce into one field.
    #[test]
    fn commoned_line_between_selects_needs_two_select_fields() {
        let d = Cs3Descriptor::derive(&layout(8, 4, &[8, 9, 11], &[10]), false).unwrap();
        assert_eq!(d.fields, alloc::vec![0xC2, 0x41, 0xC1]);
    }

    /// The cap refuses a longer descriptor, and names both figures.
    #[test]
    fn too_many_fields_is_refused() {
        let l = layout(8, 7, &[8, 10, 12], &[14]);
        assert_eq!(
            Cs3Descriptor::derive(&l, false),
            Err(Cs3Error::TooManyFields { fields: 7, max: 3 })
        );
        assert_eq!(Cs3Descriptor::derive(&l, true).unwrap().fields.len(), 7);
    }
}
