// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Plugin management commands.

use onerom_cli::plugin::{Catalogue, Plugin, Release};
use onerom_cli::{CliFetch, Error, Options};
use onerom_config::fw::FirmwareVersion;

use crate::args::plugin::PluginArgs;

/// Handle the `onerom plugin` command.
///
/// Lists available plugins from the release manifest. By default shows only
/// the latest version of each plugin; with `--all-versions`, all versions.
/// With `--type`, filters to system or user plugins only.
///
/// Compatibility with a specific firmware version can be checked by passing
/// `--fw-version` or connecting a device - incompatible releases are flagged
/// (they are shown, not hidden).
pub async fn cmd_plugin(options: &Options, args: &PluginArgs) -> Result<(), Error> {
    // Parse firmware version filter if provided, or infer from a connected device.
    let fw_version = resolve_fw_version(options, args)?;

    // Fetch the catalogue, then load every plugin's releases tolerantly: a
    // plugin whose releases cannot be fetched keeps empty releases and is
    // reported below, rather than aborting the whole listing.
    let mut catalogue = Catalogue::fetch(&CliFetch).await?;
    let failures = catalogue.load_all_releases_resilient(&CliFetch).await;

    // Filter by type if requested.
    let plugins: Vec<&Plugin> = catalogue
        .plugins()
        .iter()
        .filter(|p| args.r#type.is_none_or(|t| p.plugin_type == t))
        .collect();

    if plugins.is_empty() {
        if catalogue.plugins().is_empty() {
            println!("No plugins available.");
        } else {
            println!("No plugins found matching the specified type.");
        }
        return Ok(());
    }

    if options.verbose {
        match &fw_version {
            Some(fw) => println!("Firmware version: {fw}"),
            None => println!("Connect a device or use --fw-version to check compatibility."),
        }
    }

    println!("Available plugins ({}):", plugins.len());
    for plugin in plugins {
        println!("---");
        print_plugin(options, plugin, &fw_version, args.all_versions);
    }

    // Report any plugins whose releases could not be fetched.
    for (name, error) in &failures {
        println!("---");
        println!("  {name}: failed to fetch releases: {error}");
    }

    Ok(())
}

/// Print a single plugin's information.
fn print_plugin(
    options: &Options,
    plugin: &Plugin,
    fw_version: &Option<FirmwareVersion>,
    all_versions: bool,
) {
    let display = plugin.display_name.as_deref().unwrap_or(&plugin.name);
    println!("{}/{} - {display}", plugin.plugin_type.short(), plugin.name);
    if let Some(desc) = plugin.description.as_deref() {
        println!("  {desc}");
    }

    if plugin.releases.is_empty() {
        println!("  No releases available.");
        return;
    }

    let to_show: Vec<&Release> = if all_versions {
        plugin.releases.iter().collect()
    } else {
        plugin.releases.iter().take(1).collect()
    };

    for release in to_show {
        print_release(options, release, fw_version);
    }
}

/// Print a single release entry with compatibility information.
fn print_release(options: &Options, release: &Release, fw_version: &Option<FirmwareVersion>) {
    let compat = match fw_version {
        Some(fw) if !release.is_compatible(fw) => " - incompatible with selected firmware",
        _ => "",
    };
    let min_fw = if options.verbose {
        format!(
            " - requires One ROM firmware >= v{}",
            release.min_fw_version
        )
    } else {
        String::new()
    };
    println!("    v{}{min_fw}{compat}", release.version);
}

/// Resolve the firmware version to check compatibility against.
///
/// Uses `--fw-version` if provided, otherwise infers from the connected device
/// if one is attached. Returns `None` if neither is available - in that case
/// compatibility is not checked and `min_fw_version` is shown for reference.
fn resolve_fw_version(
    options: &Options,
    args: &PluginArgs,
) -> Result<Option<FirmwareVersion>, Error> {
    if let Some(v) = &args.fw_version {
        return FirmwareVersion::try_from_str(v).map(Some).map_err(|_| {
            Error::InvalidArgument(
                "--fw-version".to_string(),
                format!("Expected format major.minor.patch (e.g. 0.6.6)\n    --fw-version '{v}'"),
            )
        });
    }

    if let Some(device) = &options.device
        && let Some(onerom) = &device.onerom
    {
        return Ok(onerom.version());
    }

    Ok(None)
}
