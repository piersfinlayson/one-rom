// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM configuration crate
//!
//! This crate contains objects and functions for working with One ROM
//! configurations:
//! - Supported ROM types (e.g. 2364, 27128) and their specifications
//! - One ROM Hardware revisions and their properties
//! - One ROM runtime image configuration
//!
//! It is primarily intended for use by the One ROM firmware tooling, but may
//! be useful in other embedded or WASM projects related to One ROM - for
//! example, Airfrog and the One ROM website.
//!
//! This crate is designed to be `no_std` compatible, but alloc is used by
//! the runtime image configuration code.
//!
//! The code supporting ROM types and One ROM hardware revisions is generated
//! automatically by `build.rs` from JSON configuration files. This ensures
//! that the data is always up to date and consistent, and avoids the need for
//! runtime parsing or allocations.

#![no_std]

pub mod fw;
pub mod hw;
//pub mod image;
pub mod chip;
pub mod mcu;
pub mod pin_map;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Error {
    InvalidMcuVariant { variant: mcu::Variant },
    InvalidFirmwareVersion,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::InvalidMcuVariant { variant } => {
                write!(f, "Invalid MCU variant: {}", variant)
            }
            Error::InvalidFirmwareVersion => {
                write!(f, "Invalid firmware version")
            }
        }
    }
}

pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    Fire,
    Ice,
}

impl Model {
    pub fn mcu_family(&self) -> mcu::Family {
        match self {
            Model::Fire => mcu::Family::Rp2350,
            Model::Ice => mcu::Family::Stm32f4,
        }
    }
}
