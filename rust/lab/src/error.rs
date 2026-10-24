// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - Error handling

use core::fmt;

#[derive(Debug, Copy, Clone)]
pub enum Error {
    /// Invalid or unrecognised address value
    Address,
    /// Buffer too small for the requested operation
    Buffer,
    /// Unrecognised or unsupported board name
    InvalidBoard,
    /// One ROM Lab only supported Fire boards
    NonFireBoard,
    /// Unrecognised or unsupported chip type
    InvalidChip,
    /// The chip type isn't one the board supports
    ChipNotOnBoard,
    /// A socket pin the chip uses has no GPIO on the board
    NoGpio(u8),
    /// Unrecognised output format specifier
    InvalidFormat,
    /// Board must be configured before this command can run
    BoardNotSet,
    /// Chip type must be set before this command can run
    #[allow(unused)]
    ChipNotSet,
    /// USB host disconnected
    UsbDisconnected,
    /// USB TX channel full; the message was dropped
    #[allow(unused)]
    UsbFull,
    /// Input line exceeded the maximum allowed length
    #[allow(unused)]
    LineTooLong,
    /// Command or argument syntax error
    Syntax,
    /// Command was cancelled by the user (Ctrl-C at a required prompt)
    Cancelled,
    /// An invalid control line polarity was entered
    InvalidCsPolarity,
    /// A control line's polarity is unset or set to auto-detect
    CsPolarityNotSet,
    /// An RBCP step failed
    Rbcp(crate::rbcp::Failure),
    /// The chip is larger than a fast read supports
    FastReadChipSize,
    /// The board's pins for this chip can't be driven by a fast read
    FastReadPins,
    /// The board's data pins for this chip can't be driven as a square wave
    DrivePins,
    /// A drive's data lines include one the chip doesn't have
    DriveLines(usize),
    /// A host profile breaks a limit
    HostProfile(onerom_lab_bus::ProfileError),
    /// The board's pins for this chip can't be driven by the host bus
    HostPins,
    /// The host bus didn't return a sample in time
    HostTimeout,
    /// A host bus machine ran out of words mid-burst
    HostStall,
}

impl From<crate::usb::Error> for Error {
    fn from(err: crate::usb::Error) -> Error {
        match err {
            crate::usb::Error::Disconnected => Error::UsbDisconnected,
            crate::usb::Error::Full => Error::UsbFull,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Address => write!(f, "invalid address"),
            Self::Buffer => write!(f, "buffer too small"),
            Self::InvalidBoard => write!(f, "unrecognised board type"),
            Self::NonFireBoard => write!(f, "only Fire boards are supported"),
            Self::InvalidChip => write!(f, "unrecognised chip type"),
            Self::ChipNotOnBoard => write!(
                f,
                "chip type isn't supported on this board - use 'l' to list supported chips"
            ),
            Self::NoGpio(pin) => write!(f, "socket pin {pin} has no GPIO on this board"),
            Self::InvalidFormat => write!(f, "unrecognised output format"),
            Self::BoardNotSet => write!(f, "board not set — use 'B:<name>' to configure it"),
            Self::ChipNotSet => write!(f, "chip type not set"),
            Self::UsbDisconnected => write!(f, "USB host disconnected"),
            Self::UsbFull => write!(f, "USB TX channel full"),
            Self::LineTooLong => write!(f, "input line too long"),
            Self::Syntax => write!(f, "syntax error"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::InvalidCsPolarity => write!(f, "invalid control line polarity"),
            Self::CsPolarityNotSet => {
                write!(f, "control line polarity not set - use 'c' to set it")
            }
            Self::Rbcp((step, e)) => write!(f, "RBCP {step}: {e}"),
            Self::FastReadChipSize => write!(
                f,
                "a fast read supports chips up to {}KB",
                crate::fast_read::MAX_BYTES / 1024
            ),
            Self::FastReadPins => {
                write!(f, "a fast read doesn't support this chip on this board")
            }
            Self::DrivePins => {
                write!(f, "data lines can't be driven for this chip on this board")
            }
            Self::DriveLines(n) => write!(f, "this chip has {n} data lines"),
            Self::HostProfile(e) => write!(f, "{}", crate::host::profile_error(*e)),
            Self::HostPins => {
                write!(f, "the host bus doesn't support this chip on this board")
            }
            Self::HostTimeout => write!(f, "host bus read timed out"),
            Self::HostStall => write!(f, "host bus stalled"),
        }
    }
}
