// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom firmware`.

use std::str::FromStr;

use crate::args::{CommandTrait, program::ProgramArgs};
use clap::builder::{PossibleValue, TypedValueParser};
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;
use onerom_app::BoardSize;
use onerom_cli::pin::{Pin, parse_reserve_pin};

/// Value parser for `--size`, driven by [`BoardSize::supported_values`].
///
/// A size added to [`BoardSize`] is accepted here and appears in `--help`
/// with its description, without a CLI change.
#[derive(Clone)]
struct BoardSizeParser;

impl TypedValueParser for BoardSizeParser {
    type Value = BoardSize;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        // The refusal's source is BoardSizeError, as it is with
        // `value_parser = BoardSize::from_str`.
        BoardSize::from_str.parse_ref(cmd, arg, value)
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(BoardSize::supported_values().iter().map(|size| {
            PossibleValue::new(size.name()).help(size.description())
        })))
    }
}

#[derive(Debug, Args)]
pub struct FirmwareArgs {
    #[command(subcommand)]
    pub command: FirmwareCommands,
}

impl CommandTrait for FirmwareArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }

    fn uses_device(&self) -> bool {
        self.command.uses_device()
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
    ///   onerom firmware build --config c64.json --board fire-24-e --out firmware.bin
    ///
    ///   onerom firmware build --board fire-24-e \
    ///       --slot file=kernal.bin,type=2364,cs1=active-low \
    ///       --out firmware.bin
    Build(FirmwareBuildArgs),

    /// Inspect the contents of a One ROM firmware binary.
    ///
    /// Displays the firmware version, board type, MCU, and details of any
    /// embedded ROM images and metadata.
    ///
    /// Example:
    ///
    ///   onerom firmware inspect --firmware firmware.bin
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
    /// For a board, displays each chip type it can emulate with the flash each
    /// one uses, or with --all, every chip type grouped by pin count.
    ///
    /// Examples:
    ///
    ///   onerom firmware chips --board fire-24-e
    ///
    ///   onerom firmware chips --board fire-24-e --chip-type 2364
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
        long = "config",
        short='j',
        visible_aliases = ["config-file", "config-json", "json"],
        value_name = "FILE",
        conflicts_with_all = ["slot", "config_name", "config_description", "save_config", "no_config"]
    )]
    pub config_file: Option<String>,

    /// ROM slot specification. May be repeated for multiple slots.
    ///
    /// Format: file=<path_or_url>,type=<romtype>[,cs1=<logic>][,cs2=<logic>][,cs3=<logic>][,size-handling=<handling>][,format=<binary|ihex>][,load-address=<addr>][,cpu-freq=<freq>][,cpu-vreg=<voltage>][,led=<bool>][,force-16-bit=<bool>][,standby=<bool>]
    ///
    /// CS logic values: active-low (or 0), active-high (or 1), ignore.  The
    /// snake_case config spellings are also accepted.
    ///
    /// Required CS lines depend on chip type (e.g. 2332 requires cs1 and cs2).
    ///
    /// Size handling values: none, duplicate (or dup), truncate (or trunc), pad.
    ///
    /// Format values: binary (default), ihex (Intel HEX). load-address is only
    /// valid with format=ihex and gives the Intel HEX address mapping to byte 0
    /// of the ROM, as a decimal or 0x-/$-prefixed hex value (e.g. $E000).
    ///
    /// CPU frequency: e.g. 150, 150mhz, 150MHz. Values above 150MHz require
    /// confirmation (suppressed with --yes). Sets overclock automatically.
    ///
    /// Vreg voltage: e.g. 1.1, 1.10, 1.10v, 1.10V. Values above 1.10V require
    /// confirmation (suppressed with --yes). Must be a supported voltage level.
    ///
    /// Boolean values (led, force-16-bit, standby): on/off, true/false, 1/0.
    /// force-16-bit is only valid on 40-pin boards.
    ///
    /// standby=on boots One ROM into standby mode when this slot is selected.
    /// In standby mode One ROM doesn't serve the ROM. With standby=on, file is
    /// optional. Requires firmware v0.8.0 or later.
    ///
    /// Examples:
    ///
    ///   --slot file=kernal.bin,type=2364,cs1=active-low
    ///
    ///   --slot file=chargen.bin,type=2332,cs1=active-low,cs2=active-high
    ///
    ///   --slot file=https://example.com/basic.bin,type=2716
    ///
    ///   --slot file=small.bin,type=2364,cs1=active-low,size-handling=duplicate
    ///
    ///   --slot file=kernal.bin,type=2364,cs1=active-low,cpu-freq=200MHz,cpu-vreg=1.2V
    ///
    ///   --slot file=char.bin,type=2332,cs1=active-low,cs2=active-high,led=off
    ///
    ///   --slot file=amiga.bin,type=27C400,force-16-bit=true
    ///
    ///   --slot file=kernal.hex,type=2364,cs1=active-low,format=ihex
    ///
    ///   --slot file=kernal.hex,type=2364,cs1=active-low,format=ihex,load-address=$E000
    ///
    ///   --slot file=undersized.bin,type=2732,size=pad
    ///
    ///   --slot file=oversized.bin,type=2732,size=trunc
    ///
    ///   --slot file=halfsized.bin,type=2732,size=dup
    ///
    ///   --slot file=amiga.bin,type=27C400,transform=swap_bytes
    ///
    ///   --slot file=rom32.bin,type=27C010,transform=deinterleave:1/2/2+swap_bytes
    ///
    /// Mutually exclusive with --config and --no-config.
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
    /// May be combined with --config: the plugins are inserted ahead of
    /// the config's ROM slots (shifting them up). It is an error if the config
    /// already defines a plugin of its own.
    ///
    /// Forms:
    ///   --plugin usb                       latest compatible version by name
    ///   --plugin system/usb                with explicit type
    ///   --plugin usb,version=0.1.0         pinned version
    ///   --plugin file=path/to/plugin.bin   local or remote file
    ///   --plugin file=https://example.com/plugin.bin
    ///
    #[arg(long, value_name = "SPEC")]
    pub plugin: Vec<String>,

    /// Name for the generated ROM configuration.
    ///
    /// Mutually exclusive with --config.
    #[arg(long, value_name = "NAME", conflicts_with = "config_file")]
    pub config_name: Option<String>,

    /// Description for the generated ROM configuration. Defaults to
    /// "Created by the One ROM CLI" if not specified.
    ///
    /// Mutually exclusive with --config.
    #[arg(long, value_name = "DESC", visible_aliases=["desc", "description"], conflicts_with = "config_file")]
    pub config_description: Option<String>,

    /// Save the generated ROM configuration to a JSON file.
    ///
    /// Only valid with --slot or --no-config. Mutually exclusive with
    /// --config.
    #[arg(long, value_name = "FILE", conflicts_with = "config_file")]
    pub save_config: Option<String>,

    /// Target board type (e.g. fire-24-e). Required when not inferrable
    /// from a connected One ROM.
    #[arg(long, short, value_name = "BOARD")]
    pub board: Option<String>,

    /// Target board size
    #[arg(long, value_name = "SIZE", value_parser = BoardSizeParser, default_value = "M")]
    pub size: BoardSize,

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

    /// Continue despite non-fatal problems, reporting each as a warning.
    #[arg(long, short)]
    pub force: bool,

    /// Confirm building a firmware with no ROM configuration.
    ///
    /// Only valid with --config-name and/or --config-description.
    /// Mutually exclusive with --config and --slot.
    #[arg(
        long,
        conflicts_with_all = ["config_file", "slot", "instance_name", "serial_override", "logging", "disable_swd", "turbo_boot", "reserve_pin"]
    )]
    pub no_config: bool,

    /// Provide this One ROM with a name
    #[arg(long, visible_aliases = ["name", "instance_name", "onerom", "onerom-name", "one-rom", "one-rom-name"], value_name = "NAME", conflicts_with_all = ["no_config"])]
    pub instance_name: Option<String>,

    /// Give this One ROM a custom USB serial number, in place of the RP2350
    /// chip ID it would otherwise report.
    ///
    /// Used by the USB plugin while One ROM is running. A stopped One ROM is on
    /// the bootrom's USB stack and continues to report the chip ID.
    #[arg(long, visible_aliases = ["serial_override"], value_name = "SERIAL", conflicts_with_all = ["no_config"])]
    pub serial_override: Option<String>,

    /// Enable logging on this One ROM firmware
    #[arg(long, visible_aliases = ["boot-logging", "boot_logging"], default_missing_value = "true", num_args = 0..=1, conflicts_with_all = ["no_config"])]
    pub logging: Option<bool>,

    /// Shut SWD down before ROM serving starts, to stop debug port SRAM
    /// accesses stealing cycles from the serving DMAs.  SWD stays up for the
    /// whole of boot (including boot logging), then goes off until the next
    /// reset.  Not a debug lockout - BOOTSEL/PICOBOOT are unaffected
    #[arg(long, visible_aliases = ["swd-disable", "swd_disable"], default_missing_value = "true", num_args = 0..=1, conflicts_with_all = ["no_config"])]
    pub disable_swd: Option<bool>,

    /// Enable turbo boot - starts ROM serving faster by not reading the image
    /// select jumpers, so the first non-plugin slot is always the one served.
    /// More than one non-plugin slot is refused unless --force is given.
    #[arg(long, visible_aliases = ["turbo_boot"], default_missing_value = "true", num_args = 0..=1, conflicts_with_all = ["no_config"])]
    pub turbo_boot: Option<bool>,

    /// Reserve a pin for another use, for example a pin connected to a host's
    /// reset line. Repeat for each reserved pin.
    ///
    /// Requires firmware v0.8.0 or later.
    ///
    /// Example: --reserve-pin sel_c --reserve-pin x1
    #[arg(long, visible_aliases = ["reserved-pin", "reserved_pins"], value_name = "PIN", value_parser = parse_reserve_pin, conflicts_with_all = ["no_config"])]
    pub reserve_pin: Vec<Pin>,
}

impl CommandTrait for FirmwareBuildArgs {
    fn requires_device(&self) -> bool {
        false
    }

    fn uses_device(&self) -> bool {
        self.board.is_none()
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

    fn uses_device(&self) -> bool {
        self.firmware.is_none() && self.board.is_none()
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

    fn uses_device(&self) -> bool {
        !self.all && self.board.is_none()
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

    fn uses_device(&self) -> bool {
        self.board.is_none()
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

    /// Show just this chip type's flash usage on the board.
    #[arg(long, short = 'c', value_name = "CHIP", conflicts_with = "all")]
    pub chip_type: Option<String>,
}

impl CommandTrait for FirmwareChipsArgs {
    fn requires_device(&self) -> bool {
        false
    }

    fn uses_device(&self) -> bool {
        !self.all && self.board.is_none()
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use clap::Parser;
    use clap::error::ErrorKind;
    use onerom_app::BoardSizeError;

    use super::*;
    use crate::args::{Cli, Commands};

    /// `onerom firmware build --board fire-40-a` and `options`, parsed.
    fn build(options: &[&str]) -> Result<FirmwareBuildArgs, clap::Error> {
        let words = ["onerom", "firmware", "build", "--board", "fire-40-a"];
        let cli = Cli::try_parse_from(words.iter().chain(options))?;
        let Commands::Firmware(firmware) = cli.command else {
            panic!("not firmware");
        };
        let FirmwareCommands::Build(args) = firmware.command else {
            panic!("not firmware build");
        };
        Ok(args)
    }

    #[test]
    fn size_parses_every_size_in_either_case() {
        for &size in BoardSize::supported_values() {
            for text in [size.name().to_lowercase(), size.name().to_uppercase()] {
                assert_eq!(build(&["--size", &text]).unwrap().size, size, "{text}");
            }
        }
    }

    #[test]
    fn size_defaults_to_m() {
        assert_eq!(build(&[]).unwrap().size, BoardSize::M);
    }

    #[test]
    fn size_refuses_a_size_it_doesnt_know() {
        for text in ["XL", "Q", ""] {
            let error = build(&["--size", text]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{text:?}");
            let reason = error
                .source()
                .and_then(|e| e.downcast_ref::<BoardSizeError>());
            assert_eq!(reason, Some(&BoardSizeError::Unknown), "{text:?}");
        }
    }

    /// `--help` lists each size with its description.
    #[test]
    fn size_help_lists_every_size() {
        let values: Vec<PossibleValue> = BoardSizeParser.possible_values().unwrap().collect();
        let sizes = BoardSize::supported_values();
        assert_eq!(values.len(), sizes.len());
        for (value, size) in values.iter().zip(sizes) {
            assert_eq!(value.get_name(), size.name());
            assert_eq!(
                value.get_help().map(ToString::to_string).as_deref(),
                Some(size.description())
            );
        }
    }
}
