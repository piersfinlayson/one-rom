// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Plugin management commands.

use onerom_cli::plugin::{
    PluginRelease, PluginReleasesManifest, PluginType, fetch_plugin_releases,
    fetch_plugins_manifest,
};
use onerom_cli::{Error, Options};
use onerom_config::fw::FirmwareVersion;

use crate::args::plugin::PluginArgs;

/// Handle the `onerom plugin` command.
///
/// Lists available plugins from the release manifest. By default shows only
/// the latest version of each plugin. With `--all-versions`, shows all
/// versions. With `--type`, filters to system or user plugins only.
///
/// Compatibility with a specific firmware version can be checked by passing
/// `--fw-version` or connecting a device — incompatible releases are flagged
/// with a warning.
pub async fn cmd_plugin(options: &Options, args: &PluginArgs) -> Result<(), Error> {
    // Parse firmware version filter if provided, or infer from connected device
    let fw_version = resolve_fw_version(options, args)?;

    // Fetch top-level manifest to get the list of plugins
    let manifest = fetch_plugins_manifest().await?;

    if manifest.plugins.is_empty() {
        println!("No plugins available.");
        return Ok(());
    }

    // Filter by type if requested
    let plugins: Vec<_> = manifest
        .plugins
        .iter()
        .filter(|p| args.r#type.is_none_or(|t| p.plugin_type == t))
        .collect();

    if plugins.is_empty() {
        println!("No plugins found matching the specified type.");
        return Ok(());
    }

    // Print latest firmware version for reference if known
    if options.verbose {
        if let Some(fw) = &fw_version {
            println!("Firmware version: {}", fw);
        } else {
            println!("Connect a device or use --fw-version to check compatibility.");
        }
    }

    println!("Available plugins ({}):", plugins.len());
    for entry in plugins {
        // Fetch per-plugin releases manifest
        let releases = match fetch_plugin_releases(entry.plugin_type, &entry.name).await {
            Ok(r) => r,
            Err(e) => {
                let short_type = entry.plugin_type.short();
                println!(
                    "  {}/{}: failed to fetch releases: {e}",
                    short_type, entry.name
                );
                continue;
            }
        };

        println!("---");

        print_plugin(
            options,
            &releases,
            entry.plugin_type,
            &entry.name,
            &fw_version,
            args.all_versions,
        );
    }

    Ok(())
}

/// Print a single plugin's information.
fn print_plugin(
    options: &Options,
    releases: &PluginReleasesManifest,
    plugin_type: PluginType,
    name: &str,
    fw_version: &Option<FirmwareVersion>,
    all_versions: bool,
) {
    println!("{}/{name} - {}", plugin_type.short(), releases.display_name);
    println!("  {}", releases.description);

    if releases.releases.is_empty() {
        println!("  No releases available.");
        return;
    }

    let to_show: Vec<&PluginRelease> = if all_versions {
        releases.releases.iter().collect()
    } else {
        // Show only latest
        releases.releases.iter().take(1).collect()
    };

    for release in to_show {
        print_release(options, release, fw_version);
    }
}

/// Print a single release entry with compatibility information.
fn print_release(options: &Options, release: &PluginRelease, fw_version: &Option<FirmwareVersion>) {
    let compat = match fw_version {
        Some(fw) if !release.compatible_with_firmware(fw) => {
            " - incompatible with selected firmware"
        }
        _ => "",
    };
    let min_fw = if options.verbose {
        format!(
            " - requires One ROM firmware >= v{}",
            release.min_fw_version
        )
    } else {
        "".to_string()
    };
    println!("    v{}{min_fw}{compat}", release.version);
}

/// Resolve the firmware version to check compatibility against.
///
/// Uses `--fw-version` if provided, otherwise infers from the connected
/// device if one is attached. Returns `None` if neither is available —
/// in that case, compatibility is not checked and min_fw_version is shown
/// for reference only.
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
