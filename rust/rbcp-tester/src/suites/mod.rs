// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The scenario catalogue, grouped into suites.

use crate::Suite;

pub mod conformance;
pub mod integration;

/// Every suite, in the order they run.
pub static SUITES: &[Suite] = &[
    Suite {
        name: "conformance",
        blurb: "does the device obey the RBCP specification?",
        scenarios: conformance::SCENARIOS,
    },
    Suite {
        name: "integration",
        blurb: "do realistic application flows work end to end?",
        scenarios: integration::SCENARIOS,
    },
];

/// API identifiers to withhold from the plugin before running `scenario`.
///
/// A plugin degrades where a call its minimum firmware version does not
/// guarantee is missing, and those branches are unreachable against the
/// emulator, which implements the whole API.  Withholding is decided before
/// the plugin starts — that is when it resolves its pointers — so it cannot be
/// a scenario's own first act, and this table is where the scenarios that need
/// it say so.
pub fn withheld_api(scenario: &str) -> &'static [u32] {
    conformance::aux::WITHHELD_API
        .iter()
        .chain(conformance::led::WITHHELD_API)
        .find(|(name, _)| *name == scenario)
        .map(|(_, ids)| *ids)
        .unwrap_or(&[])
}
