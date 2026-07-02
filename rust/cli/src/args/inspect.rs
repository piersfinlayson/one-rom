// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom inspect`.

use crate::args::CommandTrait;
use crate::utils::{parse_u8, parse_u32};
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;

#[derive(Debug, Args)]
pub struct InspectArgs {
    #[command(subcommand)]
    pub command: InspectCommands,
}

impl CommandTrait for InspectArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum InspectCommands {
    /// Display identity and configuration information for a One ROM.
    ///
    /// Shows the device's serial number, user-assigned name, board type,
    /// MCU, firmware version, and hardware revision.
    ///
    /// Example:
    ///   onerom inspect info
    ///
    ///   onerom --serial 1234abcd inspect info
    Info(InspectInfoArgs),

    /// Display runtime telemetry from a One ROM (not yet supported).
    ///
    /// Shows access counts, timing statistics, and other runtime metrics
    /// collected by the device firmware.
    ///
    /// Example:
    ///   onerom inspect telemetry
    Telemetry(InspectTelemetryArgs),

    /// List the ROM image slots (formerly sets) stored on a One ROM.
    ///
    /// Displays the index, ROM type, size, and description of each
    /// configured image slot, and indicates which slot is currently active.
    ///
    /// Example:
    ///
    ///   onerom inspect slots
    Slots(InspectSlotsArgs),

    /// Read and display the ROM image currently loaded in a slot (not yet supported).
    ///
    /// Displays or saves the ROM image data from the specified slot.
    /// If no slot is specified, reads the image currently being served.
    ///
    /// Examples:
    ///
    ///   onerom inspect image --slot 2
    ///
    ///   onerom inspect image --slot 2 --output kernal-backup.bin
    Image(InspectImageArgs),

    /// Read data from One ROM's SRAM or the live ROM image.
    ///
    /// Peek provides read access to device memory. Use `inspect peek memory`
    /// for SRAM reads and `inspect peek live` for reads from the ROM image
    /// currently being served.
    ///
    /// Examples:
    ///
    ///   onerom inspect peek memory --address 0x20000000 --length 128
    ///
    ///   onerom inspect peek live --address 0x100 --length 64
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Peek(InspectPeekArgs),

    /// Read the current state of the One ROM GPIO pins (not yet supported).
    ///
    /// Displays the direction and logic level of each exposed GPIO pin.
    ///
    /// Example:
    ///
    ///   onerom inspect gpio
    Gpio(InspectGpioArgs),
}

#[derive(Debug, Args)]
pub struct InspectInfoArgs {}

impl CommandTrait for InspectInfoArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct InspectTelemetryArgs {
    /// Output telemetry in JSON format.
    #[arg(long)]
    pub json: bool,
}

impl CommandTrait for InspectTelemetryArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct InspectSlotsArgs {}

impl CommandTrait for InspectSlotsArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct InspectImageArgs {
    /// Slot index to read. Reads the currently active slot if omitted.
    #[arg(long, short='l', value_name = "INDEX", value_parser = parse_u8)]
    pub slot: Option<u8>,

    /// Save the image data to this file.
    #[arg(long, short, visible_alias = "out", value_name = "FILE")]
    pub output: Option<String>,
}

impl CommandTrait for InspectImageArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct InspectPeekArgs {
    #[command(subcommand)]
    pub command: InspectPeekCommands,
}

impl CommandTrait for InspectPeekArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum InspectPeekCommands {
    /// Read and display the live ROM image.
    ///
    /// Can be used to read what byte One ROM will serve if queried for a
    /// particular address. This is a live read of the currently active image.
    ///
    /// The address is a logical ROM offset starting from 0, not a physical
    /// memory address. The device must be in the running state.
    ///
    /// Example:
    ///   onerom inspect peek live --address 0x100 --length 64
    ///   onerom inspect peek live --address 0 --length 8192 --output rom-image.bin
    Live(InspectPeekLiveArgs),

    /// Read and display One ROM's SRAM contents.
    ///
    /// Can be used to read the SRAM from a One ROM. Note that when
    /// used on a device in the "Stopped" state, SRAM will not contain
    /// meaningful information.
    ///
    /// Most addresses that can be queried via the PICOBOOT protocol can be
    /// queried. When in "Stopped" state, flash reads must be performed
    /// aligned to flash page boundaries.
    ///
    /// Example:
    ///   onerom inspect peek memory --address 0x20000000 --length 128
    ///   onerom inspect peek memory --address 0x10000000 --length 8192 --output flash-start.bin
    Memory(InspectPeekMemoryArgs),
}

#[derive(Debug, Args)]
pub struct InspectPeekLiveArgs {
    /// Read from the ROM image at this logical address, starting from 0.
    ///
    /// Accepts decimal and hexadecimal (0x prefix) formats.
    #[arg(long, short, value_name = "ADDRESS", visible_alias = "addr", value_parser = parse_u32, default_value = "0")]
    pub address: u32,

    /// Read this many bytes of data from the ROM image.
    ///
    /// Accepts decimal and hexadecimal (0x prefix) formats.
    ///
    /// If not specified the command reads from the --address to the end of
    /// the live ROM image
    #[arg(long, short, visible_aliases = ["len", "size"], value_name = "LENGTH", value_parser = parse_u32)]
    pub length: Option<u32>,

    /// Save the image data to this file.
    #[arg(long, short, visible_alias = "out", value_name = "FILE")]
    pub output: Option<String>,
}

impl CommandTrait for InspectPeekLiveArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct InspectPeekMemoryArgs {
    /// Read from this address.
    ///
    /// Accepts decimal and hexadecimal (0x prefix) formats.
    #[arg(long, short, visible_alias = "addr", value_name = "ADDRESS", value_parser = parse_u32)]
    pub address: u32,

    /// Read this many bytes of data.
    ///
    /// Accepts decimal and hexadecimal (0x prefix) formats.
    #[arg(long, short, visible_aliases = ["len", "size"], value_name = "LENGTH", value_parser = parse_u32)]
    pub length: u32,

    /// Save the data to this file.
    #[arg(long, short, visible_alias = "out", value_name = "FILE")]
    pub output: Option<String>,
}

impl CommandTrait for InspectPeekMemoryArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct InspectGpioArgs {
    /// Show only this specific pin.
    #[arg(long, value_name = "PIN")]
    pub pin: Option<u8>,
}

impl CommandTrait for InspectGpioArgs {
    fn requires_device(&self) -> bool {
        true
    }
}
