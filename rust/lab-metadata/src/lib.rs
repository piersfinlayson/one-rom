// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Lab's metadata structures, generated from
//! [`metadata_schema.toml`](../../metadata_schema.toml).
//!
//! Lab fills `onerom_info_t` at the flash anchor, the same 64 bytes One ROM
//! does, and `onerom-metadata` owns that structure. What this crate describes
//! is what Lab's own `metadata` and `runtime` pointers reach.
//!
//! `firmware_type` in `onerom_info_t` is what says they lead here. A host
//! reads it before following either pointer.

#![no_std]

extern crate alloc;

use alloc::string::String;

use onerom_config::fw::FirmwareVersion;

pub use onerom_metadata::{DeviceMemoryView, MaybeKnown, ParseError, Pointer, SerializeError};

include!(concat!(env!("OUT_DIR"), "/metadata_generated.rs"));
include!(concat!(env!("OUT_DIR"), "/serialize_generated.rs"));
include!(concat!(env!("OUT_DIR"), "/device_generated.rs"));
