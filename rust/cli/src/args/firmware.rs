// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom firmware`.

use crate::args::{CommandTrait, program::ProgramArgs};
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;

#[derive(Debug, Args)]
pub struct FirmwareArgs {
    #[command(subcommand)]
    pub command: FirmwareCommands,
}

impl CommandTrait for FirmwareArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum FirmwareCommands {
    /// Build a One ROM firmware binary from a ROM configuration.
    ///
    /// Produces a flashable firmware binary for the specified board and MCU.
    /// ROM images and configuration are supplied either via a JSON config
    /// file or individual --slot arguments.
    ///
    /// Examples:
    ///
    ///   onerom firmware build --config-file c64.json --board fire-24-e --out firmware.bin
    ///
    ///   onerom firmware build --board fire-24-e \
    ///       --slot file=kernal.bin,type=2364,cs1=active_low \
    ///       --out firmware.bin
    Build(FirmwareBuildArgs),

    /// Inspect the contents of a One ROM firmware binary.
    ///
    /// Displays the firmware version, board type, MCU, and details of any
    /// embedded ROM images and metadata.
    ///
    /// Example:
    ///
    ///   onerom firmware inspect firmware.bin
    Inspect(FirmwareInspectArgs),

    /// List available One ROM firmware releases.
    ///
    /// Fetches the release manifest from the network and displays available
    /// firmware versions with their supported board types and MCUs.
    ///
    /// Example:
    ///
    ///   onerom firmware releases
    Releases(FirmwareReleasesArgs),

    /// Download a specific release of One ROM firmware.
    ///
    /// Downloads the base (ROM-less) firmware binary for the specified
    /// version, board, and MCU.
    ///
    /// Use `program` to build and flash a complete firmware with ROM images in one step.
    ///
    /// Use `firmware build` to build a complete firmware with ROM images
    /// from a config, but without flashing.
    ///
    /// Example:
    ///
    ///   onerom firmware download --version 0.6.5 --board fire-24-e --out firmware.bin
    Download(FirmwareDownloadArgs),

    /// List supported chip types.
    ///
    /// Displays the chip types supported by a specific board, or all chip types
    /// grouped by pin count.
    ///
    /// Examples:
    ///
    ///   onerom firmware chips --board fire-24-e
    ///
    ///   onerom firmware chips --all
    Chips(FirmwareChipsArgs),

    /// Build firmware and program One ROM in one step.
    ///
    /// This is an alias for `onerom program`.  Use `onerom program --help` for
    /// more details and examples.
    Program(ProgramArgs),
}

#[derive(Debug, Args)]
pub struct FirmwareBuildArgs {
    /// ROM configuration JSON file. Mutually exclusive with --slot,
    /// --config-name, --config-description, --save-config, and --no-config.
    #[arg(
        long,
        short='j',
        visible_aliases = ["config-json", "config", "json"],
        value_name = "FILE",
        conflicts_with_all = ["slot", "config_name", "config_description", "save_config", "no_config"]
    )]
    pub config_file: Option<String>,

    /// ROM slot specification. May be repeated for multiple slots.
    ///
    /// Format: file=<path_or_url>,type=<romtype>[,cs1=<logic>][,cs2=<logic>][,cs3=<logic>][,size_handling=<handling>][,cpu-freq=<freq>][,cpu-vreg=<voltage>][,led=<bool>][,force_16bit=<bool>]
    ///
    /// CS logic values: active_low (or 0), active_high (or 1).
    ///
    /// Required CS lines depend on chip type (e.g. 2332 requires cs1 and cs2).
    ///
    /// Size handling values: none, duplicate (or dup), truncate (or trunc), pad.
    ///
    /// CPU frequency: e.g. 150, 150mhz, 150MHz. Values above 150MHz require
    /// confirmation (suppressed with --yes). Sets overclock automatically.
    ///
    /// Vreg voltage: e.g. 1.1, 1.10, 1.10v, 1.10V. Values above 1.10V require
    /// confirmation (suppressed with --yes). Must be a supported voltage level.
    ///
    /// Boolean values (led, force_16bit): on/off, true/false, 1/0.
    /// force_16bit is only valid on 40-pin boards.
    ///
    /// Examples:
    ///
    ///   --slot file=kernal.bin,type=2364,cs1=active_low
    ///
    ///   --slot file=chargen.bin,type=2332,cs1=active_low,cs2=active_high
    ///
    ///   --slot file=https://example.com/basic.bin,type=2716
    ///
    ///   --slot file=small.bin,type=2364,cs1=active_low,size_handling=duplicate
    ///
    ///   --slot file=kernal.bin,type=2364,cs1=active_low,cpu-freq=200MHz,cpu-vreg=1.2V
    ///
    ///   --slot file=char.bin,type=2332,cs1=active_low,cs2=active_high,led=off
    ///
    ///   --slot file=amiga.bin,type=27C400,force_16bit=true
    ///
    ///   --slot file=undersized.bin,type=2732,size=pad
    ///
    ///   --slot file=oversized.bin,type=2732,size=trunc
    ///
    ///   --slot file=halfsized.bin,type=2732,size=dup
    ///
    /// Mutually exclusive with --config-file and --no-config.
    #[arg(
        long,
        value_name = "SPEC",
        visible_alias = "rom",
        conflicts_with_all = ["config_file", "no_config"]
    )]
    pub slot: Vec<String>,

    /// Plugin specification. May be repeated for multiple plugins.
    ///
    /// A maximum of one system plugin and one user plugin is supported.
    /// A user plugin requires a system plugin.
    /// System plugins are always placed in slot 0, user plugins in slot 1.
    ///
    /// Mutually exclusive with --config-file.
    ///
    /// Forms:
    ///   --plugin usb                       latest compatible version by name
    ///   --plugin system/usb                with explicit type
    ///   --plugin usb,version=0.1.0         pinned version
    ///   --plugin file=path/to/plugin.bin   local or remote file
    ///   --plugin file=https://example.com/plugin.bin
    ///
    #[arg(long, value_name = "SPEC", conflicts_with = "config_file")]
    pub plugin: Vec<String>,

    /// Name for the generated ROM configuration.
    ///
    /// Mutually exclusive with --config-file.
    #[arg(
        long,
        value_name = "NAME",
        visible_alias = "name",
        conflicts_with = "config_file"
    )]
    pub config_name: Option<String>,

    /// Description for the generated ROM configuration. Defaults to
    /// "Created by the One ROM CLI" if not specified.
    ///
    /// Mutually exclusive with --config-file.
    #[arg(long, value_name = "DESC", visible_aliases=["desc", "description"], conflicts_with = "config_file")]
    pub config_description: Option<String>,

    /// Save the generated ROM configuration to a JSON file.
    ///
    /// Only valid with --slot or --no-config. Mutually exclusive with
    /// --config-file.
    #[arg(long, value_name = "FILE", conflicts_with = "config_file")]
    pub save_config: Option<String>,

    /// Target board type (e.g. fire-24-e). Required when not inferrable
    /// from a connected One ROM.
    #[arg(long, short, value_name = "BOARD")]
    pub board: Option<String>,

    /// Firmware version to build against. Defaults to the latest release.
    #[arg(long, value_name = "VERSION")]
    pub version: Option<String>,

    /// Output file path. Defaults to onerom-<board>-<version>.bin.
    #[arg(
        long,
        short,
        visible_alias = "out",
        value_name = "FILE",
        conflicts_with = "path"
    )]
    pub output: Option<String>,

    /// Output directory. Uses the default filename within the given directory.
    #[arg(long, value_name = "DIR", conflicts_with = "output")]
    pub path: Option<String>,

    /// Use a local minimal firmware binary instead of downloading from the
    /// release server.
    ///
    /// This must be built with EXCLUDE_METADATA=1 and ROM_CONFIGS= in order to
    /// be suitable for then constructing a complete firmware image with this
    /// command.
    #[arg(long, value_name = "FILE", conflicts_with = "version")]
    pub base_firmware: Option<String>,

    /// Continue even if the assembled firmware has parse errors.
    #[arg(long, short)]
    pub force: bool,

    /// Allow a --slot chip type that is not in the target board's supported set.
    ///
    /// Bypasses only the supported-chip-type check. Board electrical
    /// constraints (e.g. the 40-pin requirement for force_16bit) still apply.
    #[arg(long, visible_alias = "allow-unsupported-chip")]
    pub allow_unsupported_chip_type: bool,

    /// Confirm building a firmware with no ROM configuration.
    ///
    /// Only valid with --config-name and/or --config-description.
    /// Mutually exclusive with --config-file and --slot.
    #[arg(
        long,
        conflicts_with_all = ["config_file", "slot", "instance_name", "serial_override", "logging", "disable_swd", "turbo_boot"]
    )]
    pub no_config: bool,

    /// Provide this One ROM with a name
    #[arg(long, visible_aliases = ["instance-name", "instance_name", "onerom", "onerom-name", "one-rom", "one-rom-name"], value_name = "NAME", conflicts_with_all = ["no_config"])]
    pub instance_name: Option<String>,

    // Provide a serial number to override this device's actual serial number
    #[arg(long, visible_aliases = ["serial-override", "serial_override"], value_name = "NEW SERIAL", conflicts_with_all = ["no_config"])]
    pub serial_override: Option<String>,

    /// Enable logging on this One ROM firmware
    #[arg(long, visible_aliases = ["boot-logging", "boot_logging"], default_missing_value = "true", num_args = 0..=1, conflicts_with_all = ["no_config"])]
    pub logging: Option<bool>,

    /// Disable SWD debugging on this One ROM firmware
    #[arg(long, visible_aliases = ["swd-disable", "swd_disabled"], default_missing_value = "true", num_args = 0..=1, conflicts_with_all = ["no_config"])]
    pub disable_swd: Option<bool>,

    /// Enable turbo boot - starts ROM serving faster, but only supports a
    /// single programmed slot
    #[arg(long, visible_aliases = ["turbo-boot", "turbo_boot"], default_missing_value = "true", num_args = 0..=1, conflicts_with_all = ["no_config"])]
    pub turbo_boot: Option<bool>,
}

impl CommandTrait for FirmwareBuildArgs {
    fn requires_device(&self) -> bool {
        false
    }
}

#[derive(Debug, Args)]
pub struct FirmwareInspectArgs {
    /// Firmware binary file to inspect.
    #[arg(long, visible_aliases = [ "fw", "in", "input" ], value_name = "FILE")]
    pub firmware: Option<String>,

    /// Inspect release firmware for this board type.
    #[arg(long, short, value_name = "BOARD", conflicts_with = "firmware")]
    pub board: Option<String>,

    /// Firmware version to inspect. Defaults to latest.
    #[arg(long, value_name = "VERSION", conflicts_with = "firmware")]
    pub version: Option<String>,
}

impl CommandTrait for FirmwareInspectArgs {
    fn requires_device(&self) -> bool {
        false
    }
}

#[derive(Debug, Args)]
pub struct FirmwareReleasesArgs {
    /// Show only releases for this board type.
    #[arg(long, short, value_name = "BOARD")]
    pub board: Option<String>,

    /// Show all releases, even if a device is attached and detected
    #[arg(long, short, conflicts_with = "board")]
    pub all: bool,
}

impl CommandTrait for FirmwareReleasesArgs {
    fn requires_device(&self) -> bool {
        false
    }
}

#[derive(Debug, Args)]
pub struct FirmwareDownloadArgs {
    /// Firmware version to download (e.g. 0.6.5). Defaults to latest.
    #[arg(long, value_name = "VERSION")]
    pub version: Option<String>,

    /// Target board type (e.g. fire-24-e).
    ///
    /// Will be inferred from device if not included.
    #[arg(long, short, value_name = "BOARD")]
    pub board: Option<String>,

    /// Output file path. Defaults to onerom_<board>_<version>.bin.
    #[arg(
        long,
        short,
        visible_alias = "out",
        value_name = "FILE",
        conflicts_with = "path"
    )]
    pub output: Option<String>,

    /// Output directory. Uses the default filename within the given directory.
    #[arg(long, value_name = "DIR", conflicts_with = "output")]
    pub path: Option<String>,
}

impl CommandTrait for FirmwareDownloadArgs {
    fn requires_device(&self) -> bool {
        false
    }
}

#[derive(Debug, Args)]
pub struct FirmwareChipsArgs {
    /// Show supported chip types for this board type.
    #[arg(long, short, value_name = "BOARD", conflicts_with = "all")]
    pub board: Option<String>,

    /// Show all supported chip types grouped by pin count.
    #[arg(long, short, conflicts_with = "board")]
    pub all: bool,
}

impl CommandTrait for FirmwareChipsArgs {
    fn requires_device(&self) -> bool {
        false
    }
}
