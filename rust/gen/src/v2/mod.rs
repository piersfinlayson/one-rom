// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Known open TODOs:
//
// Multi sets with heterogeneous chip types: chip0's address-line layout is
// assumed representative of the whole set — flagged, not validated against
// real hardware.
//
// Add force_8_bit including possibly new algdata to not read byte, just decide
// which byte to apply - and have the appropriate addr read delay.
//
// For force_8_bit 27c200 right now it'll use 512KB as A17 then A-1 is included.
// But in future, a new data alg that doesn't test /byte but does test A-1 and
// output correct byte, would save 256KB.
//
// Allow chip types to specify 8 bit or 16 bit only modes.  (8 bit only is
// obvious and easy)
//
// Removal of gpio_x * causes pioplugin compile problems

pub(crate) mod addr_layout;
pub(crate) mod alg_config;
mod alg_cs;
pub(crate) mod alg_preference;
pub(crate) mod cs_data_layout;
mod cs_overrides;
pub(crate) mod firmware_config;
mod gpio_pull_config;
mod gpio_window;
pub(crate) mod hardware_info;
pub(crate) mod multi_cs_config;
pub(crate) mod rom_image;
pub(crate) mod rom_info;
pub(crate) mod rom_slot;
pub(crate) mod slot_context;

pub mod builder;
pub use builder::*;

use onerom_metadata::{METADATA_SIZE, SerializeError};

use crate::Error;

impl From<SerializeError> for Error {
    fn from(err: SerializeError) -> Self {
        match err {
            SerializeError::Overflow => Error::MetadataOverflow {
                size: METADATA_SIZE,
            },
            SerializeError::CountOverflow { field } => Error::InvalidConfig {
                error: alloc::format!(
                    "Too many entries for metadata field '{field}' (maximum 255)"
                ),
            },
        }
    }
}
