// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The scenario catalogue, grouped into suites.

use crate::Suite;

pub mod conformance;
pub mod extensions;
pub mod host;
pub mod integration;

/// Every suite, in the order they run.
pub static SUITES: &[Suite] = &[
    Suite {
        name: "conformance",
        blurb: "does the device obey the RBCP specification?",
        scenarios: conformance::SCENARIOS,
    },
    Suite {
        name: "extensions",
        blurb: "does the device obey host-control's EXTENSIONS.md?",
        scenarios: extensions::SCENARIOS,
    },
    Suite {
        name: "host",
        blurb: "does onerom-rbcp-host work against the device?",
        scenarios: host::SCENARIOS,
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
        .chain(conformance::nv_storage::WITHHELD_API)
        .chain(conformance::read_group::WITHHELD_API)
        .find(|(name, _)| *name == scenario)
        .map(|(_, ids)| *ids)
        .unwrap_or(&[])
}

/// The array elements the shim's metadata stand-in returns for `scenario`, or
/// none where the scenario runs against the firmware's metadata alone.
///
/// Decided before the plugin starts, as withholding is.
pub fn metadata_stand_in(scenario: &str) -> &'static [u32] {
    extensions::device_info::STAND_IN
        .iter()
        .find(|(name, _)| *name == scenario)
        .map(|(_, elems)| *elems)
        .unwrap_or(&[])
}
