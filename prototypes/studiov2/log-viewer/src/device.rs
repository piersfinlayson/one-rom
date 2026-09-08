//! A fake device command shell, standing in for a serial console.
//!
//! Nothing here knows about iced.  Every entry point returns
//! `Result<_, Error>`, and the UI layer maps the result into its own
//! message type.  The two stages — opening a session and running a command —
//! are separate so that the UI can sequence them with `Task::and_then` rather
//! than hand-rolling a state machine that drives itself through the message
//! loop.

use std::sync::Arc;
use std::time::Duration;

use crate::error::Error;

/// A connection to the fake device.
#[derive(Debug, Clone)]
pub struct Session {
    /// The device's reported firmware version.
    pub firmware: String,
    /// The device's reported serial number.
    pub serial: String,
}

impl Session {
    /// The session a console opens against the selected device.
    ///
    /// No device selected is still a session, against a nameless one: the
    /// console is a prototype of a device shell and its own reason to exist
    /// does not depend on the shell having found hardware.
    pub fn for_device(device: Option<&studiov2_shared::Device>) -> Self {
        match device {
            Some(device) => Self {
                firmware: device.firmware.clone(),
                serial: device.serial.clone(),
            },
            None => Self {
                firmware: "unknown".to_owned(),
                serial: "no device".to_owned(),
            },
        }
    }
}

/// What the UI should do after a command, beyond printing its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Nothing beyond printing.
    None,
    /// Clear the scrollback.
    Clear,
    /// Close the session.
    Disconnect,
    /// Print this many further lines of device chatter.
    Flood(usize),
}

/// A device's answer to one command.
#[derive(Debug, Clone)]
pub struct Reply {
    /// The lines to print, oldest first.
    pub lines: Vec<Arc<str>>,
    /// What the UI should do besides printing.
    pub effect: Effect,
}

impl Reply {
    /// A reply that is just output.
    fn lines(lines: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            lines: lines.into_iter().map(Arc::from).collect(),
            effect: Effect::None,
        }
    }
}

/// The commands the fake device knows, for `help` and for the error message.
const COMMANDS: &[(&str, &str)] = &[
    ("help", "list commands"),
    ("version", "firmware and board identity"),
    ("status", "serving state and slot summary"),
    ("scan", "enumerate attached One ROM devices"),
    ("echo <text>", "echo the rest of the line back"),
    ("flood <n>", "emit n lines of device chatter"),
    ("clear", "clear the scrollback"),
    ("disconnect", "close the session"),
];

/// Opens a session with the device.
///
/// Fails with [`Error::Disconnected`] when the console has been
/// disconnected, which is what makes the `and_then` chain in the UI a real
/// two-stage sequence rather than decoration.
pub async fn open_session(connected: bool, session: Session) -> Result<Session, Error> {
    // A serial device does not answer instantly.
    tokio::time::sleep(Duration::from_millis(8)).await;

    if !connected {
        return Err(Error::Disconnected);
    }

    Ok(session)
}

/// Runs one command line against an open session.
pub async fn execute(session: Session, line: String) -> Result<Reply, Error> {
    tokio::time::sleep(Duration::from_millis(12)).await;

    let trimmed = line.trim();
    let (command, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (trimmed, ""),
    };

    match command {
        "help" => {
            let mut lines = vec![Arc::from("commands:")];
            for (name, description) in COMMANDS {
                lines.push(Arc::from(format!("  {name:<12}  {description}")));
            }
            Ok(Reply {
                lines,
                effect: Effect::None,
            })
        }

        "version" => Ok(Reply {
            lines: vec![
                Arc::from(format!("One ROM Fire, firmware {}", session.firmware)),
                Arc::from(format!("serial {}", session.serial)),
                Arc::from("RP2350A, 200 MHz, 2 MB flash"),
            ],
            effect: Effect::None,
        }),

        "status" => Ok(Reply::lines([
            "state       Running",
            "slots       1 of 8 used",
            "slot 0      23128, cs1=active-low cs2=active-low cs3=active-high",
            "serving     5 cycles after CS assertion",
            "led         solid green",
        ])),

        "scan" => Ok(Reply::lines([
            "BOARD       FW      STATE    SERIAL",
            "fire-28-a   0.7.2   Running  ORFA-0027-3F1C",
            "fire-24-a   0.7.1   Stopped  ORFA-0019-B204",
        ])),

        "echo" => {
            if rest.is_empty() {
                return Err(Error::BadArguments {
                    command: command.to_owned(),
                    detail: "expected some text to echo".to_owned(),
                });
            }
            Ok(Reply {
                lines: vec![Arc::from(rest)],
                effect: Effect::None,
            })
        }

        "flood" => {
            let count: usize = rest.parse().map_err(|_| Error::BadArguments {
                command: command.to_owned(),
                detail: format!("expected a line count, got {rest:?}"),
            })?;
            Ok(Reply {
                lines: vec![Arc::from(format!("flooding {count} lines"))],
                effect: Effect::Flood(count),
            })
        }

        "clear" => Ok(Reply {
            lines: Vec::new(),
            effect: Effect::Clear,
        }),

        "disconnect" => Ok(Reply {
            lines: vec![Arc::from("session closed")],
            effect: Effect::Disconnect,
        }),

        "" => Ok(Reply {
            lines: Vec::new(),
            effect: Effect::None,
        }),

        other => Err(Error::UnknownCommand(other.to_owned())),
    }
}

/// The banner a freshly connected session prints.
pub fn banner(session: &Session) -> Vec<Arc<str>> {
    vec![
        Arc::from(format!(
            "connected to One ROM Fire, firmware {} ({})",
            session.firmware, session.serial
        )),
        Arc::from("type `help` for commands"),
    ]
}
