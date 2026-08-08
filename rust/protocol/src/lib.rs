//! One ROM Protocol
//!
//! **Deprecated.**  This protocol served the original STM32F4-based One ROM
//! Lab.  The current One ROM Lab is Fire (RP2350) firmware driven interactively
//! over USB CDC, and does not use this crate.  It is kept because the approach
//! may return; do not build anything new on it.

// Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

#![no_std]
#![deprecated(
    note = "superseded - the current One ROM Lab is driven over USB CDC and does not use this protocol"
)]

extern crate alloc;

use onerom_database::Error as DbError;

pub mod lab;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Buffer too small for the data
    BufferTooSmall,
    /// Response was not as expected
    InvalidResponse,
    /// Invalid data received
    InvalidData,
    /// No ROM detected
    NoRom,
    /// ROM not recognised
    RomNotRecognised,
}

impl From<DbError> for Error {
    fn from(err: DbError) -> Self {
        match err {
            DbError::ParseError => Error::InvalidData,
        }
    }
}
