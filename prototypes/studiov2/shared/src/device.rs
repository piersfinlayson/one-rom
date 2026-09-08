// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The device the user is working with.
//!
//! Deliberately three strings.  A richer type would have to name a
//! `onerom_config::hw::Board`, and that would put `onerom-config` — and
//! through it `onerom-gen` — into the dependencies of every screen, including
//! the log pane, which has no interest in either.  A screen that needs the
//! real `Board` resolves the code itself.

/// A One ROM the app is connected to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// The device's serial, as `onerom scan` prints it.
    pub serial: String,
    /// The board code, e.g. `fire-28-d`.
    pub board: String,
    /// The firmware version the device is running.
    pub firmware: String,
}

impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.serial, self.board)
    }
}

/// The devices on the bus.
///
/// FAKED: there is no USB here.  Enumeration is `onerom_cli`'s job and it is
/// out of scope for a layout experiment, so the shell offers this pair and a
/// standalone screen takes the first.
pub fn attached() -> Vec<Device> {
    vec![
        Device {
            serial: "ORFA-0027-3F1C".to_owned(),
            board: "fire-28-d".to_owned(),
            firmware: "0.7.2".to_owned(),
        },
        Device {
            serial: "ORFA-0031-A840".to_owned(),
            board: "fire-24-a".to_owned(),
            firmware: "0.7.1".to_owned(),
        },
    ]
}
