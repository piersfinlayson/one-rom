// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM CLI library.
//!
//! Implements the logic behind each CLI command. The binary in main.rs
//! handles argument parsing and output formatting; this library owns
//! everything in between.

use clap::ValueEnum;

pub mod device;
pub mod error;
pub mod fetch;
pub mod picobootx;
pub mod plugin;
pub mod scan;
pub mod slot;
pub mod usb;

pub use device::{Device, DeviceState};
pub use error::Error;
pub use fetch::CliFetch;

pub const LIVE_ROM_BASE: u32 = 0x9000_0000;
pub const LIVE_ROM_MAX_OFFSET: u32 = 0x0008_0000;

#[derive(ValueEnum, Clone, Default, Debug)]
pub enum LogLevel {
    #[default]
    Warn,
    Info,
    Debug,
    Trace,
}

pub struct Options {
    pub verbose: bool,
    pub log_level: LogLevel,
    pub yes: bool,
    pub unrecognised: bool,
    pub device: Option<Device>,
    pub vid_pid: Vec<(u16, u16)>,
}
