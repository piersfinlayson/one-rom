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
}

impl Into<Error> for crate::usb::Error {
    fn into(self) -> Error {
        match self {
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
            Self::InvalidFormat => write!(f, "unrecognised output format"),
            Self::BoardNotSet => write!(f, "board not set — use 'B:<name>' to configure it"),
            Self::ChipNotSet => write!(f, "chip type not set"),
            Self::UsbDisconnected => write!(f, "USB host disconnected"),
            Self::UsbFull => write!(f, "USB TX channel full"),
            Self::LineTooLong => write!(f, "input line too long"),
            Self::Syntax => write!(f, "syntax error"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::InvalidCsPolarity => write!(f, "invalid control line polarity"),
        }
    }
}
