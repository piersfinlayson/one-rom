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
