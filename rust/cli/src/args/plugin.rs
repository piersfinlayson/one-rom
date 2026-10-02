// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom plugin`.

use crate::args::CommandTrait;
use clap::Args;
use onerom_cli::plugin::PluginType;

// args/plugin.rs
fn parse_plugin_type(s: &str) -> Result<PluginType, String> {
    PluginType::try_from_str(s)
        .ok_or_else(|| format!("invalid plugin type '{s}': expected 'system' or 'user'"))
}

/// List available One ROM plugins.
///
/// Fetches the plugin manifest from the network and displays available
/// plugins with their versions and minimum firmware version requirements.
///
/// Without a connected device or --fw-version, lists all plugins with
/// their minimum firmware version requirements shown for reference.
///
/// With a connected device or --fw-version, incompatible plugins are
/// flagged.
///
/// Examples:
///
///   onerom plugin
///
///   onerom plugin --all-versions
///
///   onerom plugin --type system
///
///   onerom plugin --fw-version 0.6.6
#[derive(Debug, Args)]
pub struct PluginArgs {
    /// Show all versions of each plugin, not just the latest.
    #[arg(long, short = 'a')]
    pub all_versions: bool,

    /// Filter by plugin type.
    #[arg(long, short, value_name = "TYPE", value_parser = parse_plugin_type)]
    pub r#type: Option<PluginType>,

    /// Firmware version to check compatibility against.
    ///
    /// If not specified and no device is connected, minimum firmware version
    /// requirements are shown for reference without compatibility filtering.
    #[arg(long, value_name = "VERSION")]
    pub fw_version: Option<String>,
}

impl CommandTrait for PluginArgs {
    fn requires_device(&self) -> bool {
        false
    }

    fn uses_device(&self) -> bool {
        self.fw_version.is_none()
    }
}
