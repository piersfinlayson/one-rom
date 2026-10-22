// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom update` and `onerom clone`.

use crate::args::CommandTrait;
use crate::utils::parse_u8;
use clap::{Args, Subcommand};
use enum_dispatch::enum_dispatch;

#[derive(Debug, Args)]
pub struct UpdateArgs {
    #[command(subcommand)]
    pub command: UpdateCommands,
}

impl CommandTrait for UpdateArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }

    fn uses_device(&self) -> bool {
        self.command.uses_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum UpdateCommands {
    /// Change a One ROM's firmware version, keeping its configuration and ROM
    /// images.
    ///
    /// Every One ROM is built from a combination of configuration and ROM
    /// images. This command retrieves the config and ROM images used to build
    /// a One ROM's firmware and programs the One ROM with another firmware
    /// version built from them.
    ///
    /// Each published plugin is updated to its latest compatible release.
    ///
    /// Examples:
    ///
    ///   onerom update firmware
    ///
    ///   onerom update firmware --version 0.8.1
    Firmware(UpdateFirmwareArgs),

    /// Replace a ROM image in a slot, keeping the configuration and the other
    /// ROM images.
    ///
    /// Every One ROM is built from a combination of configuration and ROM
    /// images. This command retrieves the config and ROM images used to build
    /// a One ROM's firmware, replaces one ROM image and programs the One ROM
    /// with firmware built from them.
    ///
    /// The new ROM image has the same chip type and chip-select settings as the
    /// one it's replacing.
    ///
    /// Examples:
    ///
    ///   onerom update slot --slot 2 --image kernal.bin
    ///
    ///   onerom update slot --slot 3 --chip 1 --image char.bin
    Slot(UpdateSlotArgs),

    /// Add or replace a plugin keeping the rest of the configuration.
    ///
    /// Every One ROM is built from a combination of configuration and ROM
    /// images. Plugins are listed in the config. This command retrieves the
    /// config and ROM images used to build a One ROM's firmware, adds the
    /// plugin to the config and programs the One ROM with firmware built from
    /// them.
    ///
    /// A user plugin requires a system plugin.
    ///
    /// Examples:
    ///
    ///   onerom update plugin --plugin usb
    ///
    ///   onerom update plugin --plugin host-control
    Plugin(UpdatePluginArgs),

    /// Commit the live ROM image to flash (not yet supported).
    ///
    /// The live ROM image is written to its slot in flash so it survives power
    /// cycles.
    ///
    /// Examples:
    ///
    ///   onerom update commit
    ///
    ///   onerom update commit --slot 2
    Commit(UpdateCommitArgs),
}

/// Options for every command that rebuilds a One ROM.
#[derive(Debug, Args)]
pub struct RebuildArgs {
    /// Continue despite non-fatal problems. Each problem is printed as a
    /// warning.
    #[arg(long, short)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct UpdateFirmwareArgs {
    /// Firmware version to change to. Defaults to the latest release.
    #[arg(long, value_name = "VERSION", conflicts_with = "base_firmware")]
    pub version: Option<String>,

    /// Local base firmware to build with instead of downloading a release.
    ///
    /// Must be built with EXCLUDE_METADATA=1 and ROM_CONFIGS=.
    #[arg(long, value_name = "FILE")]
    pub base_firmware: Option<String>,

    #[command(flatten)]
    pub rebuild: RebuildArgs,
}

impl CommandTrait for UpdateFirmwareArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct UpdateSlotArgs {
    /// Slot being updated. For slot numbering see
    /// 'onerom inspect slots'.
    #[arg(long, value_name = "INDEX", value_parser = parse_u8)]
    pub slot: u8,

    /// Chip to replace in a slot with multiple chips. Chips are numbered
    /// from 0.
    #[arg(long, value_name = "INDEX", value_parser = parse_u8)]
    pub chip: Option<u8>,

    /// ROM image file or http(s) URL.
    #[arg(long, value_name = "FILE")]
    pub image: String,

    #[command(flatten)]
    pub rebuild: RebuildArgs,
}

impl CommandTrait for UpdateSlotArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct UpdatePluginArgs {
    /// Plugin to add.
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
    #[arg(long, value_name = "SPEC")]
    pub plugin: String,

    #[command(flatten)]
    pub rebuild: RebuildArgs,
}

impl CommandTrait for UpdatePluginArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct UpdateCommitArgs {
    /// Slot to commit. Defaults to the active slot.
    #[arg(long, value_name = "INDEX")]
    pub slot: Option<u8>,
}

impl CommandTrait for UpdateCommitArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct CloneArgs {
    /// Source One ROM to copy from. Can be the serial number of the One ROM to
    /// copy or a firmware file.
    #[arg(long, value_name = "SOURCE")]
    pub from: String,

    /// Serial number of the destination One ROM.
    #[arg(long, value_name = "DEVICE")]
    pub to: String,

    #[command(flatten)]
    pub rebuild: RebuildArgs,
}

impl CommandTrait for CloneArgs {
    // clone's One ROMs are selected by --from and --to.
    fn requires_device(&self) -> bool {
        false
    }

    fn uses_device(&self) -> bool {
        false
    }
}
