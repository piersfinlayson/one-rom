// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! CLI argument definitions for the One ROM command-line interface.
//!
//! The top-level structure is:
//!   onerom scan                  - Discover connected One ROMs
//!   onerom firmware <subcommand> - Firmware binary management
//!   onerom program               - Build and flash firmware to a One ROM
//!   onerom inspect <subcommand>  - Read-only One ROM state and information
//!   onerom control <subcommand>  - Transient One ROM actions
//!   onerom update <subcommand>   - Persistent One ROM modifications
//!   onerom image <subcommand>    - ROM image file manipulation
//!
//! The --serial option is global and can be specified at any level to select
//! a specific One ROM when multiple are connected.

// rustdoc doesn't like a bunch of the doc comments, which are used for clap
// argument documentation and included in the binary.  Suppress the rustdoc
// warnings.
#![allow(rustdoc::broken_intra_doc_links)]
#![allow(rustdoc::invalid_html_tags)]
#![allow(rustdoc::bare_urls)]

pub mod control;
pub mod firmware;
pub mod image;
pub mod inspect;
pub mod plugin;
pub mod program;
pub mod scan;
pub mod update;

use clap::{Parser, Subcommand};
use enum_dispatch::enum_dispatch;
use log::debug;
use onerom_cli::LogLevel;

use crate::utils::parse_u16_hex_only;
use onerom_cli::{Error, Options};

use control::{
    ControlArgs, ControlCommands, ControlEraseArgs, ControlGpioArgs, ControlLedArgs,
    ControlLedBeaconArgs, ControlLedCommands, ControlLedFlameArgs, ControlLedOffArgs,
    ControlLedOnArgs, ControlPokeArgs, ControlPokeCommands, ControlPokeLiveArgs,
    ControlPokeMemoryArgs, ControlRebootArgs, ControlResetArgs, ControlSelectArgs,
};
use firmware::{
    FirmwareArgs, FirmwareBuildArgs, FirmwareChipsArgs, FirmwareCommands, FirmwareDownloadArgs,
    FirmwareInspectArgs, FirmwareReleasesArgs,
};
use image::{ImageArgs, ImageCommands, ImageSwapBytesArgs};
use inspect::{
    InspectArgs, InspectCommands, InspectGpioArgs, InspectImageArgs, InspectInfoArgs,
    InspectPeekArgs, InspectPeekCommands, InspectPeekLiveArgs, InspectPeekMemoryArgs,
    InspectSlotsArgs, InspectTelemetryArgs,
};
use plugin::PluginArgs;
use program::ProgramArgs;
use scan::ScanArgs;
use update::{UpdateArgs, UpdateCommands, UpdateCommitArgs, UpdateOtpArgs, UpdateSlotArgs};

#[enum_dispatch]
pub trait CommandTrait {
    fn requires_device(&self) -> bool;
}

/// Command line interface for One ROM - the most flexible retrom ROM replacement.
///
/// https://onerom.org/
///
/// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
///
/// Manage One ROMs, firmware, and ROM configurations. Run `onerom help
/// <command>` for detailed information on any subcommand.
///
/// Use `onerom scan` to discover connected One ROMs before running One ROM
/// commands.
///
/// Most CLI commands take either --board to identify the One ROM's board
/// type, or --serial to infer it directly from the specified One ROM.  If only
/// a single One ROM is connected and it can be identifed automatically,
/// neither --board or --serial are needed.  --board can be supplied to override
/// the current connected One ROM's board type.
///
/// Unprogrammed and unrecognised (e.g. bricked) One ROMs can be managed by
/// using the --unrecognised flag and supplying --board.
#[derive(Debug, Parser)]
#[command(name = "onerom", version = concat!("v", env!("CARGO_PKG_VERSION")), about, long_about)]
pub struct Cli {
    /// Select a specific One ROM by serial number.
    ///
    /// Required when multiple One ROMs are connected.
    ///
    /// Accepts * and ? wildcards for partial matching.
    ///
    /// If omitted and exactly one One ROM is connected, that One ROM is
    /// used automatically.
    #[arg(global = true, long, short, value_name = "DEVICE")]
    pub serial: Option<String>,

    /// USB vendor/product ID pair (hex, e.g. 1234:abcd).
    ///
    /// Used to detect One ROMs using non-standard USB vendor/product IDs.  If
    /// specified, only those VID/PID pairs specified will be matched.
    ///
    /// Specify multiple pairs by specifying the --vid-pid argument multiple
    /// times.
    ///
    /// Use in conjunction with --unrecognised to manage One ROMs that do not
    /// have a known One ROM firmware signature, such as unprogrammed or
    /// bricked One ROMs.
    #[arg(global = true, long, short='i', visible_alias="id", value_name = "VID:PID", value_parser = parse_vid_pid, action = clap::ArgAction::Append)]
    pub vid_pid: Vec<(u16, u16)>,

    /// Allow management of unrecognised and unprogrammed One ROMs.
    ///
    /// This is a global flag that can be used with any command to allow
    /// this tool to manage RP2350-based One ROMs that do not have a known One
    /// ROM firmware signature, such as unprogrammed or bricked One ROMs.
    ///
    /// Note that even unrecognised One ROMs must expose a valid picoboot USB
    /// interface to be detected and managed by this tool.
    ///
    /// Use with caution as this allows programming of any non-One ROM RP2350
    /// boards that are attached.
    ///
    /// Use in conjunction with --vid-pid to manage One ROMs that have
    /// unexpected USB vendor and/or product IDs.
    #[arg(global = true, visible_alias = "unrecognized", long, short)]
    pub unrecognised: bool,

    /// Auto-confirm all prompts with "yes".
    ///
    /// This is a global flag that can be used with any command to
    /// automatically answer "yes" to all prompts, allowing for non-interactive
    /// use.
    ///
    /// Use with caution, as it may lead to unintended consequences if used
    /// without fully understanding the implications of the command being
    /// run.
    #[arg(global = true, long, short)]
    pub yes: bool,

    /// Enable verbose output.
    #[arg(global = true, long, short)]
    pub verbose: bool,

    /// Set logging level.
    #[arg(global = true, long, value_enum, default_value_t = LogLevel::Warn)]
    pub log_level: LogLevel,

    #[command(subcommand)]
    pub command: Commands,
}

fn parse_vid_pid(s: &str) -> Result<(u16, u16), String> {
    let (vid, pid) = s
        .split_once(':')
        .ok_or_else(|| format!("expected VID:PID, got '{s}'"))?;
    let vid = parse_u16_hex_only(vid).map_err(|e| format!("invalid VID '{vid}': {e}"))?;
    let pid = parse_u16_hex_only(pid).map_err(|e| format!("invalid PID '{pid}': {e}"))?;
    Ok((vid, pid))
}

fn check_vid_pid_unique(vid_pid_list: &[(u16, u16)]) -> Result<(), Error> {
    let mut seen = std::collections::HashSet::new();
    for (vid, pid) in vid_pid_list {
        if !seen.insert((*vid, *pid)) {
            return Err(Error::InvalidArgument(
                "global".to_string(),
                format!("Duplicate VID:PID pair '{:04x}:{:04x}'", vid, pid),
            ));
        }
    }
    Ok(())
}

impl Cli {
    pub async fn try_into_options(&mut self) -> Result<Options, Error> {
        // Build the options struct first.
        let mut options = Options {
            log_level: self.log_level.clone(),
            verbose: self.verbose,
            yes: self.yes,
            unrecognised: self.unrecognised,
            device: None,
            vid_pid: self.vid_pid.clone(),
        };

        // Check for duplicate VID/PID pairs
        check_vid_pid_unique(&options.vid_pid)?;

        let requires_device = self.command.requires_device();

        // Check if command needs a device
        if let Some(device) = self.serial.as_ref()
            && !requires_device
        {
            debug!("Device {device} specified but not required, retrieving it anyway");
        }

        // If a serial was specified, select it and add it to the options,
        // unless handling a scan command (in which case we're scanning for
        // all devices that meet some criteria).
        if let Commands::Scan(scan) = &mut self.command {
            // Save off the serial for special handling in the scan case.
            scan.serial = self.serial.clone();
            return Ok(options);
        } else if let Some(serial) = self.serial.as_ref() {
            if options.verbose {
                println!("Scanning for device with serial '{}' ...", serial);
            }
            match onerom_cli::device::select_device(
                Some(serial),
                options.unrecognised,
                &options.vid_pid,
            )
            .await
            {
                Ok(device) => {
                    if options.verbose {
                        println!("Found device: {device}");
                    }
                    options.device = Some(device);
                }
                Err(e) => {
                    eprintln!("Error selecting device with serial '{}': {e}", serial);
                    std::process::exit(1);
                }
            }
        }

        // If no device was specified, attempt to detect one
        if options.device.is_none() {
            if options.verbose {
                println!("No device specified, scanning for connected devices ...");
            }
            match onerom_cli::device::select_device(None, options.unrecognised, &options.vid_pid)
                .await
            {
                Ok(device) => {
                    if options.verbose {
                        println!("Found device: {device}");
                    }
                    options.device = Some(device);
                }
                Err(Error::NoDevices) => {
                    // No devices found, this may or may not be an error depending on the command.
                    if requires_device {
                        debug!("No devices found.");
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(options)
    }
}

#[derive(Debug, clap::Args)]
pub struct BoardArgs {}

impl CommandTrait for BoardArgs {
    fn requires_device(&self) -> bool {
        false
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Discover and list connected One ROM.
    Scan(ScanArgs),

    /// Flash One ROM firmware to a connected One ROM.
    ///
    /// This is the primary workflow for most users. The board is
    /// inferred from the connected One ROM if not specified explicitly.
    ///
    /// This command either flashes a provided firmware binary, or builds on
    /// based on the configuration provided.
    ///
    /// With a single One ROM connected and a config file:
    ///
    ///   onerom program --config c64.json
    ///
    /// With multiple One ROMs connected, using a wildcard to select the target
    /// One ROM:
    ///
    ///   onerom program --serial '5*' --config c64.json
    ///
    /// With explicit ROM arguments instead of a config file:
    ///
    ///   onerom program --board fire-24-e \
    ///       --slot file=kernal.bin,type=2364,cs=active_low \
    ///       --slot file=basic.bin,type=2364,cs=active_low
    ///
    /// Using a local, pre-built firmware binary, containing the ROM metadata and
    /// images:
    ///
    ///   onerom program --firmware firmware.bin
    ///
    /// Using a local, pre-built minimal firmware, with no ROM metadata or images,
    /// and specifying the ROMs via arguments:
    ///
    ///   onerom program --firmware minimal.bin \
    ///       --slot file=kernal.bin,type=2364,cs=active_low \
    ///       --slot file=basic.bin,type=2364,cs=active_low
    ///
    /// To save the firmware to file, **as well** as programming the One ROM, use
    /// --out.
    ///
    ///   onerom program --config c64.json --out firmware.bin
    ///
    /// To generate a firmware binary without programming a One ROM, use the
    /// 'firmware' command.
    Program(ProgramArgs),

    /// Read-only inspection of a connected One ROM.
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Inspect(InspectArgs),

    /// Perform transient actions on a connected One ROM.
    ///
    /// These actions affect the One ROM's current state but do not persist
    /// across power cycles.
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Control(ControlArgs),

    /// Make persistent modifications to a connected One ROM.
    ///
    /// These operations write to the One ROM's flash memory and survive
    /// power cycles.
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Update(UpdateArgs),

    /// Manipulate ROM image files.
    ///
    /// File-based operations for preparing and transforming ROM binary images
    /// before programming. No device connection required.
    ///
    /// Example:
    ///
    ///   onerom image swap-bytes --input kick.bin --output kick-swapped.bin
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Image(ImageArgs),

    /// Read data from One ROM's live ROM image.
    ///
    /// Top-level alias for `inspect peek live`. See `onerom inspect peek live --help`
    /// for full documentation.
    ///
    /// Example:
    ///
    ///   onerom peek live --address 0x100 --length 64
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Peek(InspectPeekLiveArgs),

    /// Write data to One ROM's live ROM image.
    ///
    /// Top-level alias for `control poke live`. See `onerom control poke live --help`
    /// for full documentation.
    ///
    /// Example:
    ///
    ///   onerom poke live --address 0x100 --input patch.bin
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Poke(ControlPokeLiveArgs),

    /// Reboot a One ROM.
    ///
    /// Restarts a selected One ROM. The One ROM re-initialises and
    /// resumes serving ROM images after the reboot.
    ///
    /// By default, this command briefly pauses after a reboot to give the
    /// One ROM time to re-enumerate.
    ///
    /// Example:
    ///
    ///   onerom reboot
    Reboot(ControlRebootArgs),

    /// Build, inspect, and manage One ROM firmware binaries.
    ///
    /// Used to build complete One ROM firmware binaries from configuraton files,
    /// command line configuation, and also inspect firmware binaries.
    ///
    /// Use `program` to flash firmware to a One ROM - `program` can also
    /// build the firmware as part of the programming process.
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Firmware(FirmwareArgs),

    /// List available One ROM plugins.
    ///
    /// Displays plugins from the release manifest with version and minimum
    /// firmware version information.
    Plugin(PluginArgs),

    /// List supported chip types.
    ///
    /// Displays the chip types supported by a specific board, or all chip types
    /// grouped by pin count.
    ///
    /// Examples:
    ///
    ///   onerom chips --board fire-24-e
    ///
    ///   onerom chips --all
    Chips(FirmwareChipsArgs),

    /// List supported One ROM board types.
    /// 
    /// Displays a list of the supported One ROM board types.
    /// 
    /// Examples:
    /// 
    ///   onerom boards
    Boards(BoardArgs)
}
