// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The scenario catalogue, grouped into suites.

use crate::Suite;

pub mod gpio;
pub mod led;
pub mod log;
pub mod picobootx;

/// Every suite, in the order they run.
pub static SUITES: &[Suite] = &[
    Suite {
        name: "picobootx",
        blurb: "does One ROM's picoboot extension behave?",
        scenarios: picobootx::SCENARIOS,
    },
    Suite {
        name: "gpio",
        blurb: "are bounded GPIO holds honoured?",
        scenarios: gpio::SCENARIOS,
    },
    Suite {
        name: "led",
        blurb: "does the status LED do as it is told?",
        scenarios: led::SCENARIOS,
    },
    Suite {
        name: "log",
        blurb: "does the CDC port carry the device's log?",
        scenarios: log::SCENARIOS,
    },
];
