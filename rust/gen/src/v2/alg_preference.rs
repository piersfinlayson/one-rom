// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Algorithm preference ordering for layout derivation.
//!
//! Each algorithm family has an ordered enum — `#[derive(Ord)]` gives
//! lexicographic comparison directly. Lower variant = simpler = preferred.
//!
//! [`CombinedAlgPreference`] is a tuple of all four families, used as the
//! primary sort key when scoring layout candidates. Because tuples derive
//! `Ord` lexicographically, CS preference is the primary key, then Addr,
//! Data, DMA, with GPIO spread as the final tiebreaker (carried separately
//! by the caller).
//!
//! Cross-family constraints (e.g. a future AlgAddr variant that cannot
//! support AlgData1's 6-cycle delay) are expressed by excluding invalid
//! combinations from the candidate set before comparison, rather than by
//! penalising the preference values themselves.
//!
//! [`cs_alg_preference`] infers CS preference from layout characteristics
//! during `derive_cs_data_layout`, before the full [`OneromAlgCsConfig`]
//! is built. The `From` impls on the enums cover the reverse direction
//! (scoring already-built configs, e.g. for post-derivation validation or
//! future joint-selection passes).

use onerom_metadata::{
    OneromAlgAddrConfig, OneromAlgCsConfig, OneromAlgDataConfig, OneromAlgDmaConfig,
};

use super::cs_data_layout::AlgCs2Config;

/// CS algorithm preference: lower = simpler = preferred.
///
/// - `AlgCs0`: contiguous CS range — simplest PIO implementation.
/// - `AlgCs1`: one gap in CS range — requires mask, otherwise identical.
/// - `AlgCs2`: enable + address-qualified — additional qualifier check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CsAlgPreference {
    AlgCs0 = 0,
    AlgCs1 = 1,
    AlgCs2 = 2,
}

/// Address algorithm preference: lower = simpler = preferred.
/// `AlgAddr0` is currently the only variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AddrAlgPreference {
    AlgAddr0 = 0,
}

/// Data algorithm preference: lower = simpler = preferred.
///
/// - `AlgData0`: direct data output (8-bit, or 16-bit with `force_16_bit`).
/// - `AlgData1`: 16-bit with `/BYTE` + A-1 read — more PIO cycles required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DataAlgPreference {
    AlgData0 = 0,
    AlgData1 = 1,
}

/// DMA algorithm preference: lower = simpler = preferred.
/// `AlgDma0` is currently the only variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DmaAlgPreference {
    AlgDma0 = 0,
}

/// Combined algorithm preference tuple, compared lexicographically.
///
/// CS is the primary key — the simplest CS algorithm that satisfies the
/// constraints always wins, regardless of the other families. Addr, Data,
/// and DMA follow in order. GPIO spread (a `u32`, carried separately by
/// the caller) is the final tiebreaker within an otherwise equal tuple.
///
/// Use as `(CombinedAlgPreference, u32)` as the sort key:
/// ```ignore
/// let score: (CombinedAlgPreference, u32) = (
///     (CsAlgPreference::AlgCs0, AddrAlgPreference::AlgAddr0,
///      DataAlgPreference::AlgData0, DmaAlgPreference::AlgDma0),
///     gpio_spread,
/// );
/// ```
pub type CombinedAlgPreference = (
    CsAlgPreference,
    AddrAlgPreference,
    DataAlgPreference,
    DmaAlgPreference,
);

/// Infer the CS algorithm preference from layout characteristics, for use
/// during `derive_cs_data_layout` before the full config is built.
///
/// `alg_cs2` takes priority since it represents a distinct algorithm
/// regardless of CS contiguity. `cs_ignore_index` distinguishes `AlgCs1`
/// from `AlgCs0`.
pub fn cs_alg_preference(
    cs_ignore_index: Option<u8>,
    alg_cs2: Option<&AlgCs2Config>,
) -> CsAlgPreference {
    if alg_cs2.is_some() {
        CsAlgPreference::AlgCs2
    } else if cs_ignore_index.is_some() {
        CsAlgPreference::AlgCs1
    } else {
        CsAlgPreference::AlgCs0
    }
}

impl From<&OneromAlgCsConfig> for CsAlgPreference {
    fn from(alg: &OneromAlgCsConfig) -> Self {
        match alg {
            OneromAlgCsConfig::AlgCs0 { .. } => Self::AlgCs0,
            OneromAlgCsConfig::AlgCs1 { .. } => Self::AlgCs1,
            OneromAlgCsConfig::AlgCs2 { .. } => Self::AlgCs2,
        }
    }
}

impl From<&OneromAlgAddrConfig> for AddrAlgPreference {
    fn from(alg: &OneromAlgAddrConfig) -> Self {
        match alg {
            OneromAlgAddrConfig::AlgAddr0 { .. } => Self::AlgAddr0,
        }
    }
}

impl From<&OneromAlgDataConfig> for DataAlgPreference {
    fn from(alg: &OneromAlgDataConfig) -> Self {
        match alg {
            OneromAlgDataConfig::AlgData0 { .. } => Self::AlgData0,
            OneromAlgDataConfig::AlgData1 { .. } => Self::AlgData1,
        }
    }
}

impl From<&OneromAlgDmaConfig> for DmaAlgPreference {
    fn from(alg: &OneromAlgDmaConfig) -> Self {
        match alg {
            OneromAlgDmaConfig::AlgDma0 { .. } => Self::AlgDma0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preference ordering is correct: AlgCs0 < AlgCs1 < AlgCs2.
    #[test]
    fn cs_preference_ordering() {
        assert!(CsAlgPreference::AlgCs0 < CsAlgPreference::AlgCs1);
        assert!(CsAlgPreference::AlgCs1 < CsAlgPreference::AlgCs2);
    }

    /// Data preference ordering: AlgData0 < AlgData1.
    #[test]
    fn data_preference_ordering() {
        assert!(DataAlgPreference::AlgData0 < DataAlgPreference::AlgData1);
    }

    /// Combined tuple ordering: CS is primary key — AlgCs0 with worse
    /// Data beats AlgCs1 with better Data.
    #[test]
    fn combined_preference_cs_is_primary() {
        let better: CombinedAlgPreference = (
            CsAlgPreference::AlgCs0,
            AddrAlgPreference::AlgAddr0,
            DataAlgPreference::AlgData1, // worse Data
            DmaAlgPreference::AlgDma0,
        );
        let worse: CombinedAlgPreference = (
            CsAlgPreference::AlgCs1, // worse CS
            AddrAlgPreference::AlgAddr0,
            DataAlgPreference::AlgData0, // better Data
            DmaAlgPreference::AlgDma0,
        );
        assert!(better < worse);
    }

    /// GPIO spread breaks ties within an equal combined preference.
    #[test]
    fn gpio_spread_tiebreaker() {
        let pref: CombinedAlgPreference = (
            CsAlgPreference::AlgCs0,
            AddrAlgPreference::AlgAddr0,
            DataAlgPreference::AlgData0,
            DmaAlgPreference::AlgDma0,
        );
        let tight_spread: (CombinedAlgPreference, u32) = (pref, 10);
        let wide_spread: (CombinedAlgPreference, u32) = (pref, 20);
        assert!(tight_spread < wide_spread);
    }

    /// `cs_alg_preference` infers the correct variant from layout fields.
    #[test]
    fn cs_alg_preference_inference() {
        assert_eq!(cs_alg_preference(None, None), CsAlgPreference::AlgCs0);
        assert_eq!(cs_alg_preference(Some(1), None), CsAlgPreference::AlgCs1);
        assert_eq!(
            cs_alg_preference(
                None,
                Some(&AlgCs2Config {
                    base_qualifier_pin: 0,
                    num_qualifier_pins: 2,
                    qualifier_inactive_pattern: 0b11,
                })
            ),
            CsAlgPreference::AlgCs2
        );
        // alg_cs2 takes priority over cs_ignore_index.
        assert_eq!(
            cs_alg_preference(
                Some(1),
                Some(&AlgCs2Config {
                    base_qualifier_pin: 0,
                    num_qualifier_pins: 2,
                    qualifier_inactive_pattern: 0b11,
                })
            ),
            CsAlgPreference::AlgCs2
        );
    }
}
