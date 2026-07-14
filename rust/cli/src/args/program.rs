// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom program`.

use crate::args::CommandTrait;
use clap::Args;
use onerom_cli::usb::RebootArgs;

// See Commands::Program in args/mod.rs for the top-level documentation of
// this command and examples.
#[derive(Debug, Args)]
pub struct ProgramArgs {
    /// ROM configuration JSON file. Mutually exclusive with --slot,
    /// --config-name, --config-description, --save-config, --no-config,
    /// and --firmware.
    #[arg(
        long,
        short='j',
        visible_aliases = ["config-json", "config", "json"],
        value_name = "FILE",
        conflicts_with_all = ["slot", "config_name", "config_description", "save_config", "no_config", "firmware"]
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
    /// Mutually exclusive with --config-file, --no-config, and --firmware.
    #[arg(
        long,
        value_name = "SPEC",
        visible_alias = "rom",
        conflicts_with_all = ["config_file", "no_config", "firmware"]
    )]
    pub slot: Vec<String>,

    /// Plugin specification. May be repeated for multiple plugins.
    ///
    /// A maximum of one system plugin and one user plugin is supported.
    ///
    /// A user plugin requires a system plugin.
    ///
    /// System plugins are always placed in slot 0, user plugins in slot 1.
    ///
    /// Mutually exclusive with --config-file.
    ///
    /// Forms:
    ///
    ///   --plugin usb                       latest compatible version by name
    ///
    ///   --plugin system/usb                with explicit type
    ///
    ///   --plugin usb,version=0.1.0         pinned version
    ///
    ///   --plugin file=path/to/plugin.bin   local or remote file
    ///
    ///   --plugin file=https://example.com/plugin.bin
    ///
    #[arg(long, value_name = "SPEC", conflicts_with_all = ["config_file", "firmware"])]
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

    /// Flash a pre-built complete firmware binary directly.
    ///
    /// Mutually exclusive with --config-file, --slot, --base-firmware,
    /// and --plugin because a pre-built firmware already contains all of the
    /// ROMs, plugins, etc.  Also conflicts with --version.
    #[arg(
        long,
        value_name = "FILE",
        visible_alias = "fw",
        conflicts_with_all = ["config_file", "slot", "base_firmware", "version", "plugin"]
    )]
    pub firmware: Option<String>,

    /// Use a local minimal firmware binary instead of downloading from the
    /// release server.
    ///
    /// When used with --slot, the ROM images are built into this firmware.
    /// When used alone, requires --no-config to confirm flashing without
    /// ROM images.
    ///
    /// Must be built with EXCLUDE_METADATA=1 and ROM_CONFIGS= in order to
    /// be suitable.
    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["firmware", "version"]
    )]
    pub base_firmware: Option<String>,

    /// Confirm flashing a base firmware with no ROM configuration.
    ///
    /// Only valid with --config-name and/or --config-description.
    /// Mutually exclusive with --config-file, --slot, and --firmware.
    #[arg(
        long,
        conflicts_with_all = ["config_file", "slot", "firmware", "instance_name", "serial_override", "logging", "disable_swd", "turbo_boot"]
    )]
    pub no_config: bool,

    /// Target board type (e.g. fire-24-e). Inferred from connected device
    /// if not specified.
    #[arg(long, short, value_name = "BOARD")]
    pub board: Option<String>,

    /// Firmware version to build against. Defaults to the latest release.
    #[arg(
        long,
        value_name = "VERSION",
        conflicts_with_all = ["firmware", "base_firmware"]
    )]
    pub version: Option<String>,

    /// Write the built firmware to this file in addition to flashing it.
    #[arg(long, short, visible_alias = "out", value_name = "FILE")]
    pub output: Option<String>,

    /// After flashing, reboot the device into stopped mode instead of running.
    #[arg(long, short = 'p', conflicts_with = "running")]
    pub stopped: bool,

    /// After flashing, reboot the device into running mode (the default).
    #[arg(long, short = 'r', conflicts_with = "stopped")]
    pub running: bool,

    /// Do not reboot the device after flashing.
    #[arg(long, conflicts_with = "stopped")]
    pub no_reboot: bool,

    /// Verify flash contents after programming by reading back. (Not yet supported.)
    #[arg(long)]
    pub verify: bool,

    /// Continue even if the assembled firmware has parse errors.
    #[arg(long, short)]
    pub force: bool,

    /// Allow a --slot chip type that is not in the target board's supported set.
    ///
    /// Bypasses only the supported-chip-type check. Board electrical
    /// constraints (e.g. the 40-pin requirement for force_16bit) still apply.
    #[arg(long, visible_alias = "allow-unsupported-chip")]
    pub allow_unsupported_chip_type: bool,

    /// Mount mass storage device when rebooting into stopped mode.
    #[arg(long, short = 'm')]
    pub msd: bool,

    /// Don't pause after final reboot for the device to re-enumerate.
    #[arg(long, conflicts_with = "no_reboot")]
    pub fast: bool,

    /// Program multiple devices, with a pause for user confirmation between
    /// each one.
    ///
    /// Note that each board will be progammed with the same firmware config,
    /// including board config, as the first board.
    #[arg(long, visible_aliases = ["multiple", "multi"])]
    pub batch: bool,

    /// After programming, automatically run `onerom scan --slots` to output
    /// the contents of the programmed One ROM.
    #[arg(long, conflicts_with = "fast")]
    pub scan_slots: bool,

    /// Provide this One ROM with a name
    #[arg(long, visible_aliases = ["instance-name", "instance_name", "onerom", "onerom-name", "one-rom", "one-rom-name"], value_name = "NAME", conflicts_with_all = ["no_config"])]
    pub instance_name: Option<String>,

    // Provide a serial number to override this device's actual serial number
    #[arg(long, visible_aliases = ["serial-override", "serial_override"], value_name = "NEW SERIAL", conflicts_with_all = ["no_config"])]
    pub serial_override: Option<String>,

    /// Enable logging on this One ROM firmware
    #[arg(long, visible_aliases = ["boot-logging", "boot_logging"], default_missing_value = "true", num_args = 0..=1, conflicts_with_all = ["no_config"])]
    pub logging: Option<bool>,

    /// Enable/disable SWD debugging on this One ROM firmware
    #[arg(long, visible_aliases = ["swd-disable", "swd_disable"], default_missing_value = "true", num_args = 0..=1, conflicts_with_all = ["no_config"])]
    pub disable_swd: Option<bool>,

    /// Enable turbo boot - starts ROM serving faster, but only supports a
    /// single programmed slot
    #[arg(long, visible_aliases = ["turbo-boot", "turbo_boot"], default_missing_value = "true", num_args = 0..=1, conflicts_with_all = ["no_config"])]
    pub turbo_boot: Option<bool>,
}

impl CommandTrait for ProgramArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

impl From<&ProgramArgs> for RebootArgs {
    fn from(args: &ProgramArgs) -> Self {
        if args.no_reboot {
            RebootArgs::none()
        } else if args.stopped {
            RebootArgs::stopped(args.msd, args.fast)
        } else {
            RebootArgs::running(args.fast, false)
        }
    }
}
