// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom inspect`.

use crate::args::CommandTrait;
use crate::utils::{parse_u8, parse_u32};
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;
use onerom_cli::pin::{Pin, parse_pin};

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

    /// Show what every One ROM GPIO is, and what it is doing.
    ///
    /// One row per MCU GPIO: everything the GPIO is - its signal under the ROM
    /// currently being served, the board peripheral it drives, the header pad
    /// it surfaces on - plus its direction and level, whether it is
    /// 5V-tolerant, and what One ROM itself is using it for.
    ///
    /// Only GPIOs connected to something are listed; --all adds the rest.
    /// --verbose adds a legend explaining where each column comes from.
    ///
    /// The device reports only a coarse category - free, read by serving,
    /// driven by serving, or a system pin - along with the level and
    /// direction. Every name in the table comes from this CLI's board and chip
    /// metadata, not from the device.
    ///
    /// Examples:
    ///
    ///   onerom inspect gpio
    ///
    ///   onerom inspect gpio --all
    ///
    ///   onerom inspect gpio --pin gpio9
    ///
    ///   onerom inspect gpio --pin x1
    Gpio(InspectGpioArgs),

    /// Draw the connected One ROM's pin (jumper / programming) header as ASCII.
    ///
    /// Shows the 2xN header along the board's top edge, pad by pad, with the
    /// MCU GPIO behind each image-select and X pad and — on RP2350 (Fire)
    /// boards — whether that GPIO is 5V-tolerant or 3.3V-only (an ADC pin). The
    /// board is inferred from the connected device, or taken from --board.
    ///
    /// Examples:
    ///
    ///   onerom inspect header
    ///
    ///   onerom inspect header --board fire-24-f
    Header(InspectHeaderArgs),

    /// Draw the connected One ROM's ROM socket pinout as ASCII.
    ///
    /// Without --chip-type each socket pin is labelled with the GPIO(s) behind it;
    /// with --chip-type <chip> the pins show that ROM's functions (address / data /
    /// chip-select / …), and --gpio overlays both. The board is inferred from
    /// the connected device, or taken from --board.
    ///
    /// Examples:
    ///
    ///   onerom inspect socket
    ///
    ///   onerom inspect socket --chip-type 2364 --gpio
    Socket(InspectSocketArgs),
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
    #[arg(long, value_name = "INDEX", value_parser = parse_u8)]
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
    /// Show only this pin: an MCU GPIO written gpio<N>, or a header pad name
    /// (sel_a..sel_e, x1, x2).
    ///
    /// A bare number is rejected - see 'onerom inspect header' for the GPIO
    /// behind each header pad.
    #[arg(long, value_name = "PIN", value_parser = parse_pin)]
    pub pin: Option<Pin>,

    /// Board type, overriding what the connected One ROM reports.
    ///
    /// Only needed to resolve a header pad name on a One ROM whose board type
    /// this build does not recognise. A GPIO named as gpio<N> needs no board.
    #[arg(long, short, value_name = "BOARD")]
    pub board: Option<String>,

    /// Also show GPIOs with no function at all.
    ///
    /// By default only GPIOs connected to something - a ROM socket signal, a
    /// board peripheral or a header pad - are listed. On a 48-GPIO board a
    /// quarter of them are connected to nothing, and listing them buries the
    /// rest.
    #[arg(long, short, conflicts_with = "pin")]
    pub all: bool,
}

impl CommandTrait for InspectGpioArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct InspectHeaderArgs {
    /// Board type, overriding what the connected One ROM reports.
    ///
    /// Only needed on a One ROM whose board type this build does not
    /// recognise. To draw a board by name with no One ROM connected, use
    /// 'onerom board header --board <board>'.
    #[arg(long, short, value_name = "BOARD")]
    pub board: Option<String>,
}

impl CommandTrait for InspectHeaderArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct InspectSocketArgs {
    /// Board type, overriding what the connected One ROM reports.
    ///
    /// Only needed on a One ROM whose board type this build does not
    /// recognise. To draw a board by name with no One ROM connected, use
    /// 'onerom board socket --board <board>'.
    #[arg(long, short, value_name = "BOARD")]
    pub board: Option<String>,

    /// Show ROM pin functions for this chip type (e.g. 2364) instead of GPIOs.
    #[arg(long, short = 'c', value_name = "CHIP")]
    pub chip_type: Option<String>,

    /// Overlay the GPIO(s) behind each pin onto the --chip-type function view.
    #[arg(long, requires = "chip_type")]
    pub gpio: bool,
}

impl CommandTrait for InspectSocketArgs {
    fn requires_device(&self) -> bool {
        true
    }
}
