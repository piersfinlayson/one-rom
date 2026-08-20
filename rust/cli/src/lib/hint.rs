// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The command lines the CLI's own messages tell a user to run.
//!
//! An error that says "use `onerom control pin --pin <PIN> --state low`" is
//! quoting another command's option names, and nothing about a string literal
//! keeps that quotation true: rename the option and the CLI carries on printing
//! a command line that no longer parses.
//!
//! So every such command line is declared here, once, and
//! [`ALL_HINTS`] plus the parameterised builders below are walked by a test that
//! parses each one through the real clap tree. A renamed or dropped option fails
//! the build instead of reaching a user.
//!
//! Values the user has to choose for themselves are written as `<UPPERCASE>`
//! placeholders, spelled as clap's own value names. The test substitutes a
//! parseable sample for each, and fails on a placeholder it does not know - so a
//! new one has to be declared rather than silently skipped. Where the command
//! knows the value the user just named - the pin they asked about - it is put in
//! rather than left as a placeholder, so the line can be pasted as it stands.

use crate::pin::Pin;

/// List connected One ROMs.
pub const SCAN: &str = "onerom scan";

/// List the plugins the release manifest offers.
pub const PLUGIN_LIST: &str = "onerom plugin";

/// List every published version of every plugin.
pub const PLUGIN_ALL_VERSIONS: &str = "onerom plugin --all-versions";

/// Show what One ROM is using each of its GPIOs for.
pub const INSPECT_GPIO: &str = "onerom inspect gpio";

/// Show which GPIO sits behind each header pad.
pub const INSPECT_HEADER: &str = "onerom inspect header";

/// Start a stopped One ROM.
pub const CONTROL_REBOOT_RUNNING: &str = "onerom control reboot --running";

/// Program a One ROM with a USB system plugin, so it has its own USB stack.
pub const PROGRAM_WITH_USB: &str = "onerom program --config <CONFIG> --plugin usb";

/// Every hint that takes no argument, for the test that parses them all.
pub const ALL_HINTS: &[&str] = &[
    SCAN,
    PLUGIN_LIST,
    PLUGIN_ALL_VERSIONS,
    INSPECT_GPIO,
    INSPECT_HEADER,
    CONTROL_REBOOT_RUNNING,
    PROGRAM_WITH_USB,
];

/// Draw a board's pin header or ROM socket by name, with no One ROM connected.
pub fn board_view(view: &str) -> String {
    format!("onerom board {view} --board <BOARD>")
}

/// Latch `pin` low and leave it there.
pub fn latch_pin_low(pin: Pin) -> String {
    format!("onerom control pin --pin {pin} --state low")
}

/// Drive `pin` low for a bounded time, overriding One ROM's refusal to give up
/// a pin it is using.
pub fn force_pin_low(pin: Pin) -> String {
    format!("onerom control pin --pin {pin} --state low --hold <MS> --force")
}
