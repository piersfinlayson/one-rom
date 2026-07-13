// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use log::{debug, trace};
use std::io::Write;

use onerom_config::chip::{CHIP_TYPE_NAMES_PLUGINS, chip_type_names_for_pins};
use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::Board;
use onerom_config::mcu::Variant;
use onerom_fw::net::{Release, Releases, fetch_license_async};
use onerom_fw::{assemble_firmware, get_rom_files_async, read_rom_config, validate_sizes};
use onerom_fw_parser::{ParsedDevice, Parser, readers::MemoryReader};
use onerom_gen::{Builder, FIRMWARE_SIZE, License};

use crate::args;
use crate::utils::{resolve_board, resolve_firmware_output};
use onerom_cli::plugin::{PluginSpec, ResolvedPlugin, resolve_plugins};
use onerom_cli::slot::{
    ConfirmationsRequired, GlobalConfig, check_slot_confirmations, parse_slots, save_config,
    slots_to_config_json,
};
use onerom_cli::{Error, Options};

// ------------------------------- Config resolution -------------------------------

/// Resolve a ROM configuration to a JSON string from any of the three sources:
/// a config file path, a list of slot specs, or an empty config (--no-config).
///
/// `board` is required when `slots` is non-empty, for chip type validation.
/// This is shared between `firmware build` and `program`.
pub fn resolve_config_json(
    config_file: Option<&str>,
    slots: &[String],
    no_config: bool,
    board: &Board,
    global_config: Option<&GlobalConfig>,
    plugins: &[ResolvedPlugin],
    allow_unsupported_chip_type: bool,
) -> Result<String, Error> {
    if let Some(path) = config_file {
        // --config-file is mutually exclusive with --plugin at the args level,
        // so plugins is always empty here.
        let json = read_rom_config(path)?;
        if let Some(overrides) = global_config {
            apply_global_overrides(json, overrides)
        } else {
            Ok(json)
        }
    } else if no_config || slots.is_empty() {
        slots_to_config_json(plugins, &[], global_config)
    } else {
        let parsed = parse_slots(slots, board, allow_unsupported_chip_type)?;
        slots_to_config_json(plugins, &parsed, global_config)
    }
}

fn apply_global_overrides(json: String, global_config: &GlobalConfig) -> Result<String, Error> {
    let mut value: serde_json::Value = serde_json::from_str(&json)
        .map_err(|e| Error::Other(format!("Failed to parse config JSON: {e}")))?;

    let obj = value
        .as_object_mut()
        .ok_or_else(|| Error::Other("Config JSON root is not an object".to_string()))?;

    if let Some(v) = &global_config.config_name {
        obj.insert("name".to_string(), v.clone().into());
    }
    if let Some(v) = &global_config.config_description {
        obj.insert("description".to_string(), v.clone().into());
    }
    if let Some(v) = &global_config.instance_name {
        obj.insert("instance_name".to_string(), v.clone().into());
    }
    if let Some(v) = &global_config.serial_override {
        obj.insert("serial_override".to_string(), v.clone().into());
    }
    if let Some(v) = global_config.boot_logging {
        obj.insert("boot_logging".to_string(), v.into());
    }
    if let Some(v) = global_config.disable_swd {
        obj.insert("swd_enabled".to_string(), (!v).into());
    }
    if let Some(v) = global_config.turbo_boot {
        obj.insert("turbo_boot".to_string(), v.into());
    }

    serde_json::to_string(&value)
        .map_err(|e| Error::Other(format!("Failed to re-serialize config JSON: {e}")))
}

// ------------------------------- Firmware parsing and sizing -------------------------------

#[allow(clippy::collapsible_if)]
pub async fn verify_assembled_firmware(
    options: &Options,
    data: &[u8],
    force: bool,
    expected_board: Option<Board>,
) -> Result<(), Error> {
    let info = parse_firmware(data).await?;

    if let (Some(expected), Some(actual)) = (expected_board, info.get_board()) {
        if actual != expected {
            if force {
                eprintln!(
                    "Warning: firmware board type '{}' does not match expected '{}' (continuing due to --force)",
                    actual.name(),
                    expected.name()
                );
            } else {
                return Err(Error::BoardMismatch(
                    expected.name().to_string(),
                    actual.name().to_string(),
                ));
            }
        } else if options.verbose {
            println!("Board match confirmed: {}", expected.name());
        }
    }

    if !info.parse_errors().is_empty() {
        let detail = info
            .parse_errors()
            .iter()
            .map(|e| format!("  {e}"))
            .collect::<Vec<_>>()
            .join("\n");
        if force {
            eprintln!("Warning: assembled firmware has parse errors (continuing due to --force):");
            eprintln!("{detail}");
        } else {
            return Err(Error::FirmwareValidation(detail));
        }
    } else if options.verbose {
        if let Some(version) = info.version() {
            println!(
                "Assembled firmware version {} parsed successfully with no errors",
                version
            );
        }
    }
    Ok(())
}

pub async fn parse_firmware(data: &[u8]) -> Result<ParsedDevice, Error> {
    // The hardcoded base address looks odd here, as the STM32's base flash
    // address, but when using a memory reader, onerom-fw-parser will just figure
    // it out for itself based on what it finds in the image.
    let mut reader = MemoryReader::new(data.to_vec(), 0x0800_0000);
    let mut parser = Parser::new(&mut reader);
    Ok(parser.parse_device().await)
}

fn check_firmware_size(options: &Options, data: &[u8]) -> Result<(), Error> {
    if options.verbose {
        println!("Firmware size {} bytes", data.len());
    }
    if data.len() > FIRMWARE_SIZE {
        return Err(Error::BaseFirmwareTooLarge(data.len(), FIRMWARE_SIZE));
    }
    Ok(())
}

// ------------------------------- Release resolution -------------------------------

fn resolve_release<'a>(
    releases: &'a Releases,
    version: &Option<String>,
) -> Result<&'a Release, Error> {
    if let Some(version) = version {
        releases
            .release_from_string(version)
            .ok_or_else(|| Error::VersionNotFound(version.clone(), releases.releases_str()))
    } else {
        releases
            .release_from_string(releases.latest())
            .ok_or(Error::NoLatestRelease)
    }
}

// ------------------------------- Firmware acquisition -------------------------------

pub async fn acquire_firmware(
    options: &Options,
    firmware_path: &Option<String>,
    version_arg: &Option<String>,
    board: &Board,
    mcu: &Variant,
) -> Result<(Vec<u8>, FirmwareVersion, String), Error> {
    if let Some(firmware) = firmware_path {
        acquire_local_firmware(options, firmware).await
    } else {
        acquire_release_firmware(options, version_arg, board, mcu).await
    }
}

async fn acquire_local_firmware(
    options: &Options,
    firmware: &str,
) -> Result<(Vec<u8>, FirmwareVersion, String), Error> {
    if options.verbose {
        println!("Using local firmware: {firmware}");
    }
    let data = std::fs::read(firmware).map_err(|e| Error::io(firmware, e))?;
    check_firmware_size(options, &data)?;
    let info = parse_firmware(&data).await?;
    let version = info
        .version()
        .ok_or_else(|| Error::Other("Could not determine firmware version".to_string()))?;
    let version_str = format!("{}", version);
    if options.verbose {
        println!("Detected firmware version: {version_str}");
    }
    Ok((data, version, version_str))
}

async fn acquire_release_firmware(
    options: &Options,
    version_arg: &Option<String>,
    board: &Board,
    mcu: &Variant,
) -> Result<(Vec<u8>, FirmwareVersion, String), Error> {
    if options.verbose {
        println!("Checking available firmware versions...");
    }
    let releases = Releases::from_network_async().await?;
    let release = resolve_release(&releases, version_arg)?;
    let version = release.firmware_version()?;
    let version_str = release.version.clone();
    if options.verbose {
        println!(
            "Downloading firmware v{version_str} for {}...",
            board.name()
        );
    }
    let data = releases
        .download_firmware_async(&version, board, mcu)
        .await?;
    check_firmware_size(options, &data)?;
    Ok((data, version, version_str))
}

// ------------------------------- ROM image building -------------------------------

/// Build a ROM image from a JSON configuration string.
///
/// Takes the config as an already-resolved JSON string (not a file path).
/// Use [`resolve_config_json`] to obtain the JSON from any config source.
pub async fn build_rom_image(
    options: &Options,
    config_json: &str,
    version: FirmwareVersion,
    board: Board,
    mcu: Variant,
) -> Result<(FirmwareProperties, Option<Vec<u8>>, Option<Vec<u8>>, String), Error> {
    let mut builder =
        Builder::from_json(version, mcu.family(), config_json).map_err(onerom_fw::Error::parse)?;

    for license in builder.licenses() {
        accept_license(options, &license).await?;
        builder
            .accept_license(&license)
            .map_err(onerom_fw::Error::license)?;
    }

    get_rom_files_async(&mut builder).await?;

    let fw_props = FirmwareProperties::new(version, board, mcu, ServeAlg::default(), true)?;
    let (metadata, image_data) = builder.build(fw_props).map_err(onerom_fw::Error::build)?;

    let metadata = if metadata.is_empty() {
        None
    } else {
        Some(metadata)
    };
    let image_data = if image_data.is_empty() {
        None
    } else {
        Some(image_data)
    };
    let desc = builder.description();

    Ok((fw_props, metadata, image_data, desc))
}

// ------------------------------- firmware build command -------------------------------

fn check_build_args(
    _options: &Options,
    args: &args::firmware::FirmwareBuildArgs,
) -> Result<(), Error> {
    if !args.no_config && args.config_file.is_none() && args.slot.is_empty() {
        return Err(Error::InvalidArgument(
            "build".to_string(),
            "Either --config-file or --slot must be specified unless --no-config is set"
                .to_string(),
        ));
    }
    if args.no_config && (!args.slot.is_empty() || args.config_file.is_some()) {
        return Err(Error::InvalidArgument(
            "build".to_string(),
            "--no-config cannot be used with --slot or --config-file".to_string(),
        ));
    }
    Ok(())
}

pub async fn cmd_build(
    options: &Options,
    args: &args::firmware::FirmwareBuildArgs,
) -> Result<(), Error> {
    check_build_args(options, args)?;

    let board = resolve_board(options, &args.board)?.ok_or(Error::NoBoardOrDevice)?;
    let mcu = Variant::RP2350;

    if !args.slot.is_empty() {
        let confirmations =
            check_slot_confirmations(&args.slot, &board, args.allow_unsupported_chip_type)?;
        confirm_slot_overrides(options, &confirmations).await?;
    }

    let (firmware_data, version, version_str) =
        acquire_firmware(options, &args.base_firmware, &args.version, &board, &mcu).await?;

    let plugins = resolve_plugins(
        &parse_plugin_specs(&args.plugin)?,
        &version,
        &onerom_cli::CliFetch,
    )
    .await?;
    if options.verbose {
        for plugin in &plugins {
            println!(
                "Resolved plugin: {}/{} v{} ({})",
                plugin.plugin_type.short(),
                plugin.name,
                plugin.version,
                plugin.file(),
            );
        }
    }

    let global_config = if args.no_config {
        None
    } else {
        Some(GlobalConfig {
            config_name: args.config_name.clone(),
            config_description: args.config_description.clone(),
            instance_name: args.instance_name.clone(),
            serial_override: args.serial_override.clone(),
            boot_logging: args.logging,
            disable_swd: args.disable_swd,
            turbo_boot: args.turbo_boot,
        })
    };
    let config_json = resolve_config_json(
        args.config_file.as_deref(),
        &args.slot,
        args.no_config,
        &board,
        global_config.as_ref(),
        &plugins,
        args.allow_unsupported_chip_type,
    )?;

    if let Some(path) = &args.save_config {
        save_config(path, &config_json)?;
        if options.verbose {
            println!("Saved ROM configuration to {path}");
        }
    }

    let (fw_props, metadata, image_data, desc) =
        build_rom_image(options, &config_json, version, board, mcu).await?;

    validate_sizes(&fw_props, &firmware_data, &metadata, &image_data)?;

    let assembled = assemble_firmware(firmware_data, metadata, image_data)?;
    let size = assembled.len();
    verify_assembled_firmware(options, &assembled, args.force, Some(board)).await?;

    let out = resolve_firmware_output(
        &args.output,
        &args.path,
        &board,
        Some(&version_str),
        args.config_file.as_deref(),
    );
    std::fs::write(&out, &assembled).map_err(|e| Error::io(&out, e))?;

    if options.verbose {
        println!("Wrote {size} bytes to {out}");
        if !desc.is_empty() {
            println!("---\n{desc}");
        }
    } else {
        if let Some(path) = &args.save_config {
            println!("Firmware configuration written to {path}");
        }
        println!("Firmware written to {out}");
    }

    Ok(())
}

// ------------------------------- License acceptance -------------------------------

pub async fn accept_license(options: &Options, license: &License) -> Result<(), Error> {
    let text = fetch_license_async(&license.url).await?;

    println!("License required:");
    println!("---");
    println!("{text}");
    println!("---");

    if options.yes {
        println!("Auto-accepted (--yes)");
        return Ok(());
    }

    print!("Do you accept this license? (y/N): ");
    std::io::stdout()
        .flush()
        .map_err(|e| Error::Other(e.to_string()))?;

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| Error::Other(e.to_string()))?;

    match input.trim().to_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => Err(Error::LicenseNotAccepted),
    }
}

/// Prompt the user for confirmation if any slot overrides require it.
///
/// CPU frequencies above 150MHz and vreg voltages above 1.10V each require
/// separate confirmation. Both are suppressed by `--yes`. Any chip types
/// permitted via `--allow-unsupported-chip-type` are also surfaced here as a
/// warning (not suppressed, since it is informational rather than a prompt).
pub async fn confirm_slot_overrides(
    options: &Options,
    confirmations: &ConfirmationsRequired,
) -> Result<(), Error> {
    for chip_type in &confirmations.unsupported_chip_types {
        eprintln!(
            "Warning: chip type {chip_type} is not supported by this board - proceeding anyway (--allow-unsupported-chip-type)"
        );
    }

    if confirmations.cpu_freq {
        if options.yes {
            println!("Auto-accepted above-stock CPU frequency (--yes)");
        } else {
            print!("One or more slots specify a CPU frequency above 150MHz. Continue? (y/N): ");
            std::io::stdout()
                .flush()
                .map_err(|e| Error::Other(e.to_string()))?;
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| Error::Other(e.to_string()))?;
            if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
                return Err(Error::AboveStockNotAccepted("CPU Frequency".to_string()));
            }
        }
    }

    if confirmations.vreg {
        if options.yes {
            println!("Auto-accepted above-stock vreg voltage (--yes)");
        } else {
            print!("One or more slots specify a vreg above 1.10V. Continue? (y/N): ");
            std::io::stdout()
                .flush()
                .map_err(|e| Error::Other(e.to_string()))?;
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| Error::Other(e.to_string()))?;
            if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
                return Err(Error::AboveStockNotAccepted("CPU VReg".to_string()));
            }
        }
    }

    Ok(())
}

// ------------------------------- firmware inspect command -------------------------------

pub async fn cmd_inspect(
    options: &Options,
    args: &args::firmware::FirmwareInspectArgs,
) -> Result<(), Error> {
    let data = if let Some(file) = &args.firmware {
        inspect_local_firmware(options, file)?
    } else {
        inspect_release_firmware(options, args).await?
    };

    if options.verbose {
        println!("Firmware size: {} bytes", data.len());
    }

    let info = parse_firmware(&data).await?;
    print_firmware_info(options, &info)
}

fn inspect_local_firmware(options: &Options, file: &str) -> Result<Vec<u8>, Error> {
    if options.verbose {
        println!("Inspecting local firmware: {file}");
    }
    std::fs::read(file).map_err(|e| Error::io(file, e))
}

async fn inspect_release_firmware(
    options: &Options,
    args: &args::firmware::FirmwareInspectArgs,
) -> Result<Vec<u8>, Error> {
    let board = resolve_board(options, &args.board)?.ok_or(Error::NoBoardOrDevice)?;
    let mcu = Variant::RP2350;
    let releases = Releases::from_network_async().await?;
    let release = resolve_release(&releases, &args.version)?;
    let version = release.firmware_version()?;
    if options.verbose {
        println!(
            "Downloading firmware v{} for {}...",
            release.version,
            board.name()
        );
    }
    releases
        .download_firmware_async(&version, &board, &mcu)
        .await
        .map_err(Error::from)
}

fn print_firmware_info(options: &Options, info: &ParsedDevice) -> Result<(), Error> {
    if !info.parse_errors().is_empty() {
        eprintln!("Warning: firmware parsed with errors:");
        for error in info.parse_errors() {
            eprintln!("  {error}");
        }
        eprintln!();
    }

    match info {
        ParsedDevice::Original(sdrr) => print_original_firmware_info(options, sdrr),
        ParsedDevice::Schema(onerom) => print_schema_firmware_info(options, onerom),
    }
}

fn print_original_firmware_info(
    options: &Options,
    sdrr: &onerom_fw_parser::Sdrr,
) -> Result<(), Error> {
    let Some(info) = sdrr.flash.as_ref() else {
        println!("(no flash information available)");
        return Ok(());
    };

    if options.verbose {
        let json = serde_json::to_string_pretty(info).map_err(|e| Error::Other(e.to_string()))?;
        println!("---");
        println!("{json}");
    } else {
        println!("Version:  {}", info.version);
        if let Some(hw_rev) = &info.hw_rev {
            println!("Hardware: {hw_rev}");
        }
        println!("MCU:      {:?}", info.stm_line);
        println!("Slots: {}", info.rom_set_count);
        for (i, set) in info.rom_sets.iter().enumerate() {
            println!("  Slot {i}: {} ROM(s), {} bytes", set.rom_count, set.size);
            for (j, rom) in set.roms.iter().enumerate() {
                let name = rom.filename.as_deref().unwrap_or("<unnamed>");
                println!("    ROM {j}: {} {name}", rom.rom_type);
            }
        }
    }
    Ok(())
}

fn print_schema_firmware_info(
    options: &Options,
    onerom: &onerom_fw_parser::OneRom,
) -> Result<(), Error> {
    let Some(info) = onerom.info() else {
        println!("(no firmware information available)");
        return Ok(());
    };

    let board = onerom
        .metadata()
        .and_then(|m| Board::try_from_str(m.hw.hw_rev.as_str()));
    let board_name = board.map_or("unknown".to_string(), |b| b.name().to_string());
    if options.verbose {
        println!(
            "Version:  {}.{}.{}",
            info.major_version, info.minor_version, info.patch_version
        );
        println!("Build:    {}", info.build_number);
        println!("Format:   Schema (v0.7.0+)");
        println!("Board:    {board_name}");
        if let Some(metadata) = onerom.metadata() {
            println!("Slots: {}", metadata.rom_slot_count);
            for (i, slot) in metadata.rom_slots.iter().enumerate() {
                println!("  Slot {i}: {} ROM(s)", slot.rom_count);
            }
        }
    } else {
        println!(
            "Version:  {}.{}.{}",
            info.major_version, info.minor_version, info.patch_version
        );
        println!("Board:    {board_name}");
    }
    Ok(())
}

// ------------------------------- firmware releases command -------------------------------

pub async fn cmd_releases(
    options: &Options,
    args: &args::firmware::FirmwareReleasesArgs,
) -> Result<(), Error> {
    let board = if args.all {
        trace!("Showing all releases (including those for attached device if present)");
        None
    } else {
        trace!("Resolving board to filter releases");
        resolve_board(options, &args.board)?
    };
    debug!("Resolved board for releases: {board:?}");

    let releases = Releases::from_network_async().await?;
    let filtered = filter_releases(&releases, board.as_ref());

    if filtered.is_empty() {
        println!("No releases found.");
        return Ok(());
    }

    print_releases(options, &releases, &filtered, board.as_ref())
}

fn filter_releases(releases: &Releases, board: Option<&Board>) -> Vec<Release> {
    if let Some(board) = board {
        releases
            .releases()
            .iter()
            .filter(|r| {
                r.boards
                    .iter()
                    .any(|b| b.name == board.name().to_ascii_lowercase())
            })
            .cloned()
            .collect()
    } else {
        releases.releases().clone()
    }
}

fn print_releases(
    options: &Options,
    releases: &Releases,
    filtered: &[Release],
    board: Option<&Board>,
) -> Result<(), Error> {
    if let Some(board) = board {
        println!("Available firmware releases for {}:", board.name());
    } else {
        println!("Available firmware releases:");
    }
    for r in filtered {
        let latest = if r.version == releases.latest() {
            " (latest)"
        } else {
            ""
        };
        println!("  v{}{latest}", r.version);
        if options.verbose {
            let boards = r
                .boards
                .iter()
                .map(|b| b.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(board) = board {
                let url = r.url(board, &Variant::RP2350)?;
                println!("    Location: {url}");
            }
            println!("    Supported boards: {boards}");
        }
    }
    Ok(())
}

// ------------------------------- firmware chips command -------------------------------

pub async fn cmd_chips(
    options: &Options,
    args: &args::firmware::FirmwareChipsArgs,
) -> Result<(), Error> {
    let board = if args.all {
        None
    } else {
        resolve_board(options, &args.board)?
    };

    if let Some(board) = board {
        print_chips_for_board(&board);
    } else {
        print_all_chips();
    }

    Ok(())
}

fn print_plugin_chips() {
    println!("Supported plugin types:");
    let names_str = CHIP_TYPE_NAMES_PLUGINS.join(", ");
    println!("  {names_str}");
}

fn print_chips_for_board(board: &Board) {
    println!("Supported chip types for {}:", board.name());
    let names = board.supported_chip_type_names();
    if names.is_empty() {
        println!("  (none)");
    } else {
        let names_str = names.join(", ");
        println!("  {names_str}");
    }
    print_plugin_chips();
}

fn print_all_chips() {
    for pins in [24u8, 28, 32, 40] {
        if let Some(names) = chip_type_names_for_pins(pins) {
            println!("Supported {pins}-pin chips:");
            let names_str = names.join(", ");
            println!("  {names_str}");
        }
    }
    print_plugin_chips();
}

// ------------------------------- firmware download command -------------------------------

pub async fn cmd_download(
    options: &Options,
    args: &args::firmware::FirmwareDownloadArgs,
) -> Result<(), Error> {
    let board = resolve_board(options, &args.board)?.ok_or(Error::NoBoardOrDevice)?;
    let mcu = Variant::RP2350;

    let releases = Releases::from_network_async().await?;
    let release = resolve_release(&releases, &args.version)?;
    let version = release.firmware_version()?;

    if options.verbose {
        println!(
            "Downloading firmware v{} for {}...",
            release.version,
            board.name()
        );
    }
    let data = releases
        .download_firmware_async(&version, &board, &mcu)
        .await?;
    check_firmware_size(options, &data)?;

    let out = resolve_firmware_output(
        &args.output,
        &args.path,
        &board,
        Some(&release.version),
        None,
    );
    std::fs::write(&out, &data).map_err(|e| Error::io(&out, e))?;

    if options.verbose {
        println!("Written {} bytes to {}", data.len(), out);
    } else {
        println!("Firmware downloaded to {out}");
    }

    Ok(())
}

fn parse_plugin_specs(raw: &[String]) -> Result<Vec<PluginSpec>, Error> {
    Ok(onerom_cli::plugin::parse_plugins(raw)?)
}
