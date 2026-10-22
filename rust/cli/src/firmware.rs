// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use log::{debug, trace};
use std::io::Write;

use onerom_config::chip::{CHIP_TYPE_NAMES_PLUGINS, ChipType, chip_type_names_for_pins};
use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::{Board, BoardSize};
use onerom_config::mcu::RP235X_BASE_FLASH;
use onerom_config::mcu::Variant;
use onerom_config::pin::ReservedPins;
use onerom_fw::get_rom_files_async;
use onerom_fw::net::{Release, Releases, fetch_license_async};
use onerom_fw_parser::readers::MemoryReader;
use onerom_fw_parser::{ParseError, ParsedDevice, SlotKind};
use onerom_gen::ChipSetType;
use onerom_gen::compat::{
    ChipCompat, check_chip_set_on_board, default_cs_config, format_size, supported_chips,
};
use onerom_gen::{
    Builder, Config, ConfigOverrides, Error as GenError, FIRMWARE_SIZE, FlashChips, License,
    second_chip_supported, supports_board_size,
};
use onerom_lab_parser::LabParser;
use onerom_metadata::{ONEROM_INFO_METADATA_OFFSET, ONEROM_INFO_OFFSET};

use crate::args;
use crate::args::hardware::supported_size;
use crate::utils::{check_fire_board, resolve_board, resolve_firmware_output};
use onerom_app::{CheckedImage, ImageProblem, LocalFetch, PluginError, assemble_image};
use onerom_cli::error::image_problem_warning;
use onerom_cli::image::parse_firmware;
use onerom_cli::plugin::{
    Plugin, PluginNote, PluginSpec, PluginType, ResolvedPlugin, check_config_plugins, parse_plugin,
    resolve_config_plugins, resolve_plugin, validate_resolved_plugin_types,
};
use onerom_cli::slot::{
    ConfirmationsRequired, GlobalConfig, check_slot_chip_types, check_slot_confirmations,
    defines_plugin, inject_plugins_into_config, parse_slots, save_config, saved_config_json,
    slots_to_config_json,
};
use onerom_cli::{Error, Options};

// ------------------------------- Config resolution -------------------------------

/// The config file at `source`, a path or http(s) URL.
pub async fn fetch_config_json(source: &str) -> Result<String, Error> {
    let data = onerom_cli::CliFetch.fetch(source).await?;
    Ok(String::from_utf8(data).map_err(|e| {
        onerom_fw::Error::read(
            source.to_string(),
            std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        )
    })?)
}

/// Resolve a ROM configuration to a JSON string from any of the three sources:
/// a config file's JSON, a list of slot specs, or an empty config
/// (--no-config).
///
/// `board` and `version` are required when `slots` is non-empty, for chip type
/// validation: which chip types a board can serve depends on which builder the
/// target firmware uses. A config file is validated by the builder itself, so
/// neither is consulted on that path. Shared between `firmware build` and
/// `program`.
pub fn resolve_config_json(
    config: Option<String>,
    slots: &[String],
    no_config: bool,
    board: &Board,
    version: &FirmwareVersion,
    global_config: Option<&GlobalConfig>,
    plugins: &[ResolvedPlugin],
) -> Result<String, Error> {
    if let Some(json) = config {
        // A config file supplies the ROM slots; any --plugin entries are
        // injected ahead of them (erroring if the config defines its own).
        let json = inject_plugins_into_config(json, plugins)?;
        if let Some(overrides) = global_config {
            apply_global_overrides(json, overrides)
        } else {
            Ok(json)
        }
    } else if no_config || slots.is_empty() {
        slots_to_config_json(plugins, &[], global_config)
    } else {
        let parsed = parse_slots(slots, board)?;
        check_slot_chip_types(&parsed, board, version)?;
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
    if !global_config.reserved_pins.is_empty() {
        let pins = serde_json::to_value(&global_config.reserved_pins)
            .map_err(|e| Error::Other(format!("Failed to serialize reserved pins: {e}")))?;
        obj.insert("reserved_pins".to_string(), pins);
    }
    if let Some(v) = global_config.addr_mon_min_cs_act_ns {
        obj.insert("addr_mon_min_cs_act_ns".to_string(), v.into());
    }

    serde_json::to_string(&value)
        .map_err(|e| Error::Other(format!("Failed to re-serialize config JSON: {e}")))
}

// ------------------------------- Firmware parsing and sizing -------------------------------

/// Returns the image and its parse, or fails on the first problem `checked`
/// found. With `force` each problem prints as a warning instead. `board` is the
/// board the image was checked for.
pub fn accept_image(
    options: &Options,
    checked: CheckedImage,
    force: bool,
    board: Option<Board>,
) -> Result<(Vec<u8>, ParsedDevice), Error> {
    let CheckedImage {
        image,
        device,
        problems,
        ..
    } = checked;
    if options.verbose
        && let (Some(expected), Some(actual)) = (board, device.get_board())
        && actual == expected
    {
        println!("Board match confirmed: {}", expected.name());
    }

    // A clean parse is reported between the board check and the image file
    // check, the order check_image runs them in.
    let mut parsed = options.verbose && device.parse_errors().is_empty();
    for problem in problems {
        if parsed && !matches!(problem, ImageProblem::BoardMismatch { .. }) {
            print_parsed(&device);
            parsed = false;
        }
        if !force {
            return Err(problem.into());
        }
        eprintln!("{}", image_problem_warning(&problem));
    }
    if parsed {
        print_parsed(&device);
    }
    Ok((image, device))
}

fn print_parsed(device: &ParsedDevice) {
    if let Some(version) = device.version() {
        println!("Assembled firmware version {version} parsed successfully with no errors");
    }
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

/// Release `version` of the firmware for `board`, or the latest release where
/// `version` is `None`.
pub(crate) fn resolve_release<'a>(
    releases: &'a Releases,
    version: &Option<String>,
    board: &Board,
    mcu: &Variant,
) -> Result<&'a Release, Error> {
    if let Some(version) = version {
        let Some(release) = releases.release_from_string(version) else {
            let available: Vec<String> = releases
                .hw_releases(board, mcu)
                .into_iter()
                .map(|release| release.version)
                .collect();
            return Err(Error::VersionNotFound(
                version.clone(),
                available.join(", "),
            ));
        };
        if !release.supports_hw(board, mcu) {
            return Err(Error::ReleaseNotForBoard {
                version: release.version.clone(),
                board: board.name().to_string(),
            });
        }
        Ok(release)
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
    let info = parse_firmware(&data).await;
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
    let release = resolve_release(&releases, version_arg, board, mcu)?;
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
///
/// `size` is the size of board the image is for.
///
/// `force` accepts the config checks that are refused by default, reporting
/// each one that fires as a warning instead.
///
/// The config's plugins are fetched through `fetch`.
#[allow(clippy::too_many_arguments)]
pub async fn build_rom_image<F: LocalFetch<Error = onerom_fw::Error>>(
    options: &Options,
    config_json: &str,
    version: FirmwareVersion,
    board: Board,
    mcu: Variant,
    size: BoardSize,
    force: bool,
    fetch: &F,
    before_fetch: impl FnOnce(&Config) -> Result<(), Error>,
) -> Result<(FirmwareProperties, Vec<u8>, Vec<u8>, String), Error> {
    let overrides = ConfigOverrides::default().allow_turbo_boot_multi_slot(force);

    let (mut builder, warnings) =
        Builder::from_json_with_overrides(version, mcu.family(), config_json, &overrides).map_err(
            |e| {
                // Give the one check --force covers a message naming it;
                // onerom-gen knows nothing about the CLI's flags.
                if matches!(e, GenError::TurboBootMultiSlot { .. }) {
                    Error::TurboBootMultiSlot(e)
                } else {
                    Error::Build(onerom_fw::Error::parse(e))
                }
            },
        )?;

    for warning in warnings {
        eprintln!("Warning: {warning}\n  Continuing due to --force.");
    }

    // Checked before anything is fetched.
    let reserved = builder
        .config()
        .reserved_pins_on(board)
        .map_err(|e| Error::Build(onerom_fw::Error::parse(e)))?;
    if let Some(warning) = unselectable_slots(builder.config(), board, reserved) {
        eprintln!("Warning: {warning}");
    }

    for license in builder.licenses() {
        accept_license(options, &license).await?;
        builder
            .accept_license(&license)
            .map_err(onerom_fw::Error::license)?;
    }

    // Before any ROM image or plugin referred to by name is downloaded.
    before_fetch(builder.config())?;

    let plugins = resolve_config_plugins(&mut builder, &version, fetch).await?;
    if options.verbose {
        print_resolved_plugins(&plugins);
    }

    get_rom_files_async(&mut builder).await?;

    // Before the image is assembled, so a 16-bit ROM supplied the wrong way
    // round is reported before the user waits for a build.
    onerom_cli::byte_order::report_slots(&builder, options.verbose);

    // A plugin referred to by URL hasn't been checked against the manifest.
    // The others have, and checking them again is harmless.
    report_plugin_checks(
        options,
        check_config_plugins(&builder, &version, fetch).await?,
    );

    let fw_props = FirmwareProperties::new(version, board, mcu, ServeAlg::default(), true)?
        .with_board_size(size);
    let (metadata, roms) = builder.build(fw_props).map_err(|e| {
        // `firmware build` adds advice to this one.
        if matches!(e, GenError::SlotDoesNotFit { .. }) {
            Error::SlotDoesNotFit {
                error: onerom_fw::Error::build(e),
                advise_second_chip: false,
            }
        } else {
            Error::Build(onerom_fw::Error::build(e))
        }
    })?;

    let desc = builder
        .description_for_board(board)
        .map_err(onerom_fw::Error::build)?;

    Ok((fw_props, metadata, roms, desc))
}

/// The warning for ROM slots that can't be selected with the image select
/// jumpers.
///
/// Slots are numbered from 0 excluding plugins, as in `inspect slots`. Turbo
/// boot doesn't read the jumpers.
pub(crate) fn unselectable_slots(
    config: &Config,
    board: Board,
    reserved: ReservedPins,
) -> Option<String> {
    if config.turbo_boot {
        return None;
    }
    let slots = config
        .chip_sets
        .iter()
        .filter(|set| !set.chips.iter().any(|c| c.chip_type.resolved().is_plugin()))
        .count();
    let pins: Vec<String> = reserved
        .select_pins_read(&board)
        .map(|pin| pin.silkscreen().to_string())
        .collect();
    let combinations = 1usize << pins.len();
    if slots <= combinations {
        return None;
    }

    let (first, last) = (combinations, slots - 1);
    let which = match last - first {
        0 => format!("slot {first} cannot"),
        1 => format!("slots {first} and {last} cannot"),
        _ => format!("slots {first} to {last} cannot"),
    };
    // Every Fire board has image select pins so an empty list means all are
    // reserved.
    let why = match pins.as_slice() {
        [] => "Every image select pin is reserved.".to_string(),
        [only] => format!("{only} provides 2 combinations for {slots} slots."),
        [rest @ .., last] => format!(
            "{} and {last} provide {combinations} combinations for {slots} slots.",
            rest.join(", ")
        ),
    };
    Some(format!("{which} be selected by jumpers. {why}"))
}

fn print_resolved_plugins(plugins: &[ResolvedPlugin]) {
    for plugin in plugins {
        println!(
            "Resolved plugin: {}/{} v{} ({})",
            plugin.plugin_type.short(),
            plugin.name,
            plugin.version,
            plugin.file(),
        );
    }
}

/// Report the non-fatal outcomes of checking the plugins a config named.
///
/// A skipped check is always reported, whatever the verbosity: the published
/// compatibility window is the only thing standing between a stale plugin and a
/// device that hard faults on boot, so a build that could not consult it must
/// say so.  A check that ran, and a plugin there was nothing to check, are
/// detail for --verbose.
fn report_plugin_checks(options: &Options, notes: Vec<PluginNote<onerom_fw::Error>>) {
    for note in notes {
        match note {
            PluginNote::Checked { name, version } => {
                if options.verbose {
                    println!("Plugin '{name}' v{version} is compatible with firmware");
                }
            }
            PluginNote::Unofficial { source } => {
                if options.verbose {
                    println!(
                        "Plugin {source} is not an official One ROM plugin - no published compatibility to check"
                    );
                }
            }
            PluginNote::Unchecked { source, error } => {
                eprintln!(
                    "Warning: could not check plugin {source} for firmware compatibility\n  {error}"
                );
            }
        }
    }
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

/// `--size` as `size` for `board`. A size with a second flash chip needs a
/// board that supports external flash.
pub(crate) fn build_size(board: Board, size: BoardSize) -> Result<BoardSize, Error> {
    supported_size(board, size)
        .map_err(|e| Error::InvalidArgument("--size".to_string(), e.to_string()))
}

/// `error` with advice to use `--size`, where a chip set didn't fit a `size`
/// build without a second flash chip, and `board` and firmware `version` both
/// support a size with one.
fn advise_second_chip(
    error: Error,
    board: Board,
    mcu: Variant,
    size: BoardSize,
    version: FirmwareVersion,
) -> Error {
    let Error::SlotDoesNotFit { error, .. } = error else {
        return error;
    };
    let second_chip = FlashChips::new(mcu, size).second().is_some();
    Error::SlotDoesNotFit {
        error,
        advise_second_chip: !second_chip && second_chip_supported(board, version),
    }
}

pub async fn cmd_build(
    options: &Options,
    args: &args::firmware::FirmwareBuildArgs,
) -> Result<(), Error> {
    check_build_args(options, args)?;

    let board = resolve_board(options, &args.board)?.ok_or(Error::NoBoardOrDevice)?;
    check_fire_board(&board)?;
    let size = build_size(board, args.size)?;
    let mcu = Variant::RP2350;

    if !args.slot.is_empty() {
        let confirmations = check_slot_confirmations(&args.slot, &board)?;
        confirm_slot_overrides(options, &confirmations).await?;
    }

    let (firmware_data, version, version_str) =
        acquire_firmware(options, &args.base_firmware, &args.version, &board, &mcu).await?;
    if !supports_board_size(version, size) {
        return Err(Error::InvalidArgument(
            "--size".to_string(),
            format!("Firmware {version} doesn't support --size other than M"),
        ));
    }

    let config = match &args.config_file {
        Some(source) => Some(fetch_config_json(source).await?),
        None => None,
    };
    let (specs, plugins) = resolve_build_plugins(
        &args.plugin,
        args.no_plugin,
        config.as_deref(),
        &version,
        &onerom_cli::CliFetch,
    )
    .await?;
    if options.verbose {
        print_resolved_plugins(&plugins);
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
            reserved_pins: args.reserve_pin.clone(),
            addr_mon_min_cs_act_ns: args.addr_mon_min_cs_act_ns,
        })
    };
    let config_json = resolve_config_json(
        config,
        &args.slot,
        args.no_config,
        &board,
        &version,
        global_config.as_ref(),
        &plugins,
    )?;

    if let Some(path) = &args.save_config {
        save_config(path, &saved_config_json(&config_json, &specs, &plugins)?)?;
        if options.verbose {
            println!("Saved ROM configuration to {path}");
        }
    }

    let (fw_props, metadata, roms, desc) = build_rom_image(
        options,
        &config_json,
        version,
        board,
        mcu,
        size,
        args.force,
        &onerom_cli::CliFetch,
        |_| Ok(()),
    )
    .await
    .map_err(|e| advise_second_chip(e, board, mcu, size, version))?;

    let checked = assemble_image(&fw_props, firmware_data, metadata, roms).await?;
    let (assembled, _) = accept_image(options, checked, args.force, Some(board))?;
    let size = assembled.len();

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
/// separate confirmation. Both are suppressed by `--yes`.
pub async fn confirm_slot_overrides(
    options: &Options,
    confirmations: &ConfirmationsRequired,
) -> Result<(), Error> {
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

    let info = parse_firmware(&data).await;
    print_firmware_info(options, &info, &data).await
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
    check_fire_board(&board)?;
    let mcu = Variant::RP2350;
    let releases = Releases::from_network_async().await?;
    let release = resolve_release(&releases, &args.version, &board, &mcu)?;
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

async fn print_firmware_info(
    options: &Options,
    info: &ParsedDevice,
    data: &[u8],
) -> Result<(), Error> {
    let errors = reported_parse_errors(info, data);
    if !errors.is_empty() {
        eprintln!("Warning: firmware parsed with errors:");
        for error in errors {
            eprintln!("  {error}");
        }
        eprintln!();
    }

    for line in firmware_summary(options.verbose, info, data).await? {
        println!("{line}");
    }
    Ok(())
}

/// The parse errors `firmware inspect` prints a warning for. A base firmware
/// doesn't contain metadata, so failing to read it isn't one of them.
fn reported_parse_errors<'a>(info: &'a ParsedDevice, data: &[u8]) -> Vec<&'a ParseError> {
    let base = is_base_firmware(info, data);
    info.parse_errors()
        .iter()
        .filter(|error| !(base && error.field == "metadata"))
        .collect()
}

/// Whether `data`, parsed as `info`, is v0.7.0+ base firmware: a file ending
/// before the metadata address in its header. `firmware build` puts the
/// metadata at that address, so a built image always extends past it.
fn is_base_firmware(info: &ParsedDevice, data: &[u8]) -> bool {
    if !matches!(info, ParsedDevice::Schema(_)) {
        return false;
    }
    let at = ONEROM_INFO_OFFSET as usize + ONEROM_INFO_METADATA_OFFSET;
    let Some(pointer) = data.get(at..at + 4) else {
        return false;
    };
    let metadata = u32::from_le_bytes(pointer.try_into().unwrap());
    metadata
        .checked_sub(RP235X_BASE_FLASH)
        .is_some_and(|offset| offset as usize >= data.len())
}

/// The lines `firmware inspect` prints for `data`, parsed as `info`.
async fn firmware_summary(
    verbose: bool,
    info: &ParsedDevice,
    data: &[u8],
) -> Result<Vec<String>, Error> {
    match info {
        ParsedDevice::Original(sdrr) => original_summary(verbose, sdrr),
        ParsedDevice::Schema(onerom) => Ok(schema_summary(
            verbose,
            info,
            onerom,
            is_base_firmware(info, data),
        )),
        ParsedDevice::Lab => lab_summary(verbose, data).await,
        _ => Ok(vec!["(firmware this build can't show)".to_string()]),
    }
}

/// A One ROM Lab image's summary.
async fn lab_summary(verbose: bool, data: &[u8]) -> Result<Vec<String>, Error> {
    let mut reader = MemoryReader::new(data.to_vec(), RP235X_BASE_FLASH);
    let lab = LabParser::new(&mut reader)
        .parse()
        .await
        .map_err(Error::Other)?;
    let info = &lab.info;
    // A Lab built without a board baked in is a valid image.  "(not set)" is
    // Lab's own words for it.
    let board = match &lab.metadata {
        Ok(metadata) => metadata.hw.hw_rev.as_deref().unwrap_or("(not set)"),
        Err(_) => "unknown",
    };

    let mut lines = vec![
        "Firmware: One ROM Lab".to_string(),
        format!(
            "Version:  {}.{}.{}",
            info.major_version, info.minor_version, info.patch_version
        ),
    ];
    if verbose {
        lines.push(format!("Build:    {}", info.build_number));
    }
    lines.push(format!("Board:    {board}"));
    Ok(lines)
}

/// A pre-0.7.0 image's summary, or with `verbose` its parsed header as JSON.
fn original_summary(verbose: bool, sdrr: &onerom_fw_parser::Sdrr) -> Result<Vec<String>, Error> {
    let Some(info) = sdrr.flash.as_ref() else {
        return Ok(vec!["(no flash information available)".to_string()]);
    };

    if verbose {
        let json = serde_json::to_string_pretty(info).map_err(|e| Error::Other(e.to_string()))?;
        let mut lines = vec!["---".to_string()];
        lines.extend(json.lines().map(str::to_string));
        return Ok(lines);
    }

    let mut lines = vec![format!("Version:  {}", info.version)];
    if let Some(hw_rev) = &info.hw_rev {
        lines.push(format!("Board:    {hw_rev}"));
    }
    lines.push(format!("MCU:      {}", info.stm_line));
    lines.push(format!("Slots: {}", info.rom_set_count));
    for (i, set) in info.rom_sets.iter().enumerate() {
        lines.push(format!(
            "  Slot {i}: {} ROM(s), {} bytes",
            set.rom_count, set.size
        ));
        for (j, rom) in set.roms.iter().enumerate() {
            let name = rom.filename.as_deref().unwrap_or("<unnamed>");
            lines.push(format!("    Chip {j}: {} {name}", rom.rom_type));
        }
    }
    Ok(lines)
}

/// A v0.7.0+ image's summary. `base` is whether it's a base firmware.
///
/// Plugins are listed separately from ROM slots, and ROM slots are numbered
/// from 0 with plugins excluded, the same way [`crate::inspect`] numbers a
/// connected device's slots.  A plugin is named by the image source recorded
/// in the firmware, with no manifest lookup - there is no device here, and the
/// binary already carries the name.
fn schema_summary(
    verbose: bool,
    parsed: &ParsedDevice,
    onerom: &onerom_fw_parser::OneRom,
    base: bool,
) -> Vec<String> {
    let Some(info) = onerom.info() else {
        return vec!["(no firmware information available)".to_string()];
    };

    let mut lines = vec![format!(
        "Version:  {}.{}.{}",
        info.major_version, info.minor_version, info.patch_version
    )];
    if verbose {
        lines.push(format!("Build:    {}", info.build_number));
        lines.push("Format:   Schema (v0.7.0+)".to_string());
    }

    let Some(metadata) = onerom.metadata() else {
        // A base firmware runs on any board. Otherwise the metadata failed to
        // read, and a warning has been printed.
        let board = if base {
            "any (base firmware)"
        } else {
            "unknown"
        };
        lines.push(format!("Board:    {board}"));
        return lines;
    };

    let board = Board::try_from_str(metadata.hw.hw_rev.as_str())
        .map_or("unknown".to_string(), |b| b.name().to_string());
    lines.push(format!("Board:    {board}"));
    lines.push(format!("MCU:      {}", metadata.hw.rp235x));

    let mut plugins = Vec::new();
    let mut rom_slots = Vec::new();
    for slot in parsed.slots() {
        match slot.kind {
            SlotKind::Plugin => plugins.push(
                slot.roms()
                    .next()
                    .and_then(|r| r.filename)
                    .unwrap_or("unknown"),
            ),
            SlotKind::Rom => rom_slots.push(slot),
        }
    }
    if !plugins.is_empty() {
        lines.push("Plugins:".to_string());
        lines.extend(plugins.iter().map(|plugin| format!("  {plugin}")));
    }
    lines.push(format!("Slots: {}", rom_slots.len()));
    if let Some(line) = crate::inspect::reserved_pins_line(parsed) {
        lines.push(format!("  {line}"));
    }
    if let Some(line) = crate::inspect::addr_mon_min_cs_act_line(parsed) {
        lines.push(format!("  {line}"));
    }
    for slot in &rom_slots {
        // A ROM slot always has a user_index.
        let user_index = slot.user_index.unwrap_or(0);
        let size = metadata.rom_slots[slot.slot_index].size;
        lines.push(format!(
            "  Slot {user_index}: {} ROM(s), {size} bytes",
            slot.roms().count()
        ));
        for (j, rom) in slot.roms().enumerate() {
            let name = rom.filename.unwrap_or("<unnamed>");
            lines.push(format!("    Chip {j}: {} {name}", rom.rom_type));
        }
    }
    lines
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

    match (board, args.chip_type.as_deref()) {
        // A single chip's flash usage is a per-board figure, so --chip-type
        // needs a board - given, or inferred from a connected One ROM.
        (None, Some(_)) => return Err(Error::NoBoardOrDevice),
        (Some(board), Some(chip_type)) => print_chip_on_board(&board, chip_type)?,
        (Some(board), None) => print_chips_for_board(&board),
        (None, None) => print_all_chips(),
    }

    Ok(())
}

fn print_plugin_chips() {
    println!("Supported plugin types:");
    let names_str = CHIP_TYPE_NAMES_PLUGINS.join(", ");
    println!("  {names_str}");
}

/// Heading for a run of chips that fit `board` the same way, matching the
/// section headings in `docs/COMPATIBILITY.md`.
fn chip_group_heading(board: &Board, entry: &ChipCompat) -> String {
    let chip_pins = entry.chip_type.chip_pins();
    if entry.result.is_native() {
        format!("{}-pin chips (native)", board.chip_pins())
    } else if entry.result.requires_fly_leads() {
        format!("{chip_pins}-pin chips (with fly-leads)")
    } else {
        format!("{chip_pins}-pin chips (with overhang)")
    }
}

/// The chip types `board` can emulate, as a comma-separated list for error
/// messages. Plugins are excluded: they are listed separately by
/// [`print_plugin_chips`], and `chips --chip-type` has no size to report for
/// one.
fn emulatable_chip_names(board: &Board) -> String {
    onerom_cli::slot::emulatable_chip_names(board).join(", ")
}

/// List a board's chip types by name only, without image sizes.
///
/// Used for boards `onerom_gen::compat` cannot size - it derives the v2
/// (Fire/RP2350) serving layout, so an Ice/STM32 board has no per-chip figure to
/// report and falls back to this.
fn print_chip_names_for_board(board: &Board) {
    let names = board.supported_chip_type_names();
    if names.is_empty() {
        println!("  (none)");
    } else {
        println!("  {}", names.join(", "));
    }
}

fn print_chips_for_board(board: &Board) {
    let entries = supported_chips(*board, ChipSetType::Single, 1);
    println!(
        "Supported chip types for {} ({}):",
        board.name(),
        board.description()
    );

    if entries.is_empty() {
        print_chip_names_for_board(board);
        print_plugin_chips();
        return;
    }

    // Size the columns across every group, so they line up down the whole
    // listing rather than shifting between sections.
    let width = |header: &str, longest: usize| longest.max(header.len());
    let chip_w = width(
        "Chip",
        entries.iter().map(|e| e.alias.len()).max().unwrap_or(0),
    );
    let rom_w = width(
        "ROM size",
        entries
            .iter()
            .map(|e| format_size(e.rom_size_bytes).len())
            .max()
            .unwrap_or(0),
    );
    let image_w = width(
        "Image size",
        entries
            .iter()
            .map(|e| format_size(e.result.slot_size_bytes).len())
            .max()
            .unwrap_or(0),
    );

    // `supported_chips` orders the entries so chips that fit the same way are
    // consecutive, so a change of pin offset starts a new section.
    let mut group: Option<i16> = None;
    for entry in &entries {
        if group != Some(entry.result.pin_offset) {
            group = Some(entry.result.pin_offset);
            println!();
            println!("  {}", chip_group_heading(board, entry));
            println!(
                "    {:chip_w$}  {:>rom_w$}  {:>image_w$}  Fit",
                "Chip", "ROM size", "Image size"
            );
        }
        println!(
            "    {:chip_w$}  {:>rom_w$}  {:>image_w$}  {}",
            entry.alias,
            format_size(entry.rom_size_bytes),
            format_size(entry.result.slot_size_bytes),
            entry.result.fit_description(),
        );
    }

    println!();
    println!(
        "  Image size is the flash One ROM uses to emulate the chip, which may exceed \
         the chip's own ROM size.  See docs/COMPATIBILITY.md."
    );

    // Chip types of this board's own pin count that it cannot serve - either
    // because no firmware serves them yet (the SRAM types, at the time of
    // writing) or because this particular board's layout cannot place them.
    // They have no image size, so they cannot appear in the table above, but
    // naming them beats leaving a recognised type unaccounted for.
    let unservable: Vec<&str> = board
        .supported_chip_type_names()
        .iter()
        .copied()
        .filter(|name| !entries.iter().any(|e| e.alias == *name))
        .collect();
    if !unservable.is_empty() {
        println!(
            "\n  Recognised but not servable on this board: {}",
            unservable.join(", ")
        );
    }

    println!();
    print_plugin_chips();
}

/// Print one chip type's flash usage on `board`.
fn print_chip_on_board(board: &Board, name: &str) -> Result<(), Error> {
    let unsupported = || Error::UnsupportedChipType(name.to_string(), emulatable_chip_names(board));

    let chip_type = ChipType::try_from_str(name).ok_or_else(unsupported)?;
    let result = check_chip_set_on_board(
        *board,
        chip_type,
        ChipSetType::Single,
        1,
        default_cs_config(chip_type),
    )
    .map_err(|_| unsupported())?;

    println!("{name} on {} ({}):", board.name(), board.description());
    println!(
        "  ROM size    {}",
        format_size(chip_type.size_bytes() as u32)
    );
    println!("  Image size  {}", format_size(result.slot_size_bytes));
    println!("  Fit         {}", result.fit_description());

    Ok(())
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
    check_fire_board(&board)?;
    let mcu = Variant::RP2350;

    let releases = Releases::from_network_async().await?;
    let release = resolve_release(&releases, &args.version, &board, &mcu)?;
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

/// The system plugin an image is built with where a system plugin isn't
/// specified.
const DEFAULT_PLUGIN: &str = "usb";

/// Resolves the `--plugin` values `raw` for firmware `version`, and adds the
/// USB plugin when a system plugin isn't specified. Returns the specs and the
/// plugins in the same order.
///
/// The USB plugin isn't added with `no_plugin`, when `config` defines a plugin
/// or doesn't parse, or when it doesn't have a version that supports `version`.
pub(crate) async fn resolve_build_plugins<F: LocalFetch<Error = onerom_fw::Error>>(
    raw: &[String],
    no_plugin: bool,
    config: Option<&str>,
    version: &FirmwareVersion,
    fetch: &F,
) -> Result<(Vec<PluginSpec>, Vec<ResolvedPlugin>), Error> {
    // A config that doesn't parse doesn't get the default plugin, and the
    // build reports it.
    let config = config.map(serde_json::from_str::<Config>);
    let config_has_plugin = matches!(&config, Some(Ok(config)) if defines_plugin(config));
    let config_parses = !matches!(config, Some(Err(_)));
    if no_plugin && config_has_plugin {
        return Err(Error::InvalidArgument(
            "--no-plugin".to_string(),
            "The config file defines a plugin. Remove it from the config or drop --no-plugin."
                .to_string(),
        ));
    }

    // Each spec is resolved alone, as a user plugin without a system plugin is
    // only an error where the default isn't added.
    let mut specs = raw
        .iter()
        .map(|spec| parse_plugin(spec))
        .collect::<Result<Vec<_>, _>>()?;
    let mut plugins = Vec::with_capacity(specs.len() + 1);
    for spec in &specs {
        let plugin = resolve_plugin(spec, version, fetch)
            .await
            .map_err(|e| plugin_error(std::slice::from_ref(spec), e))?;
        plugins.push(plugin);
    }

    let has_system = plugins.iter().any(|p| p.plugin_type == PluginType::System);
    if !no_plugin && !config_has_plugin && config_parses && !has_system {
        let spec = PluginSpec::Named {
            name: DEFAULT_PLUGIN.to_string(),
            plugin_type: Some(PluginType::System),
            version: None,
        };
        match resolve_plugin(&spec, version, fetch).await {
            Ok(plugin) => {
                specs.insert(0, spec);
                plugins.insert(0, plugin);
            }
            Err(onerom_app::Error::Plugin(
                PluginError::Incompatible {
                    newest_compatible: None,
                    ..
                }
                | PluginError::IncompatibleNewer {
                    newest_compatible: None,
                    ..
                },
            )) => println!(
                "No version of plugin '{DEFAULT_PLUGIN}' supports the selected firmware version {version}.\n  \
                 Building without a plugin."
            ),
            Err(e) => {
                let error = plugin_error(std::slice::from_ref(&spec), e);
                return Err(Error::Other(format!(
                    "{error}\n  Use --no-plugin to build without plugins."
                )));
            }
        }
    }

    let types: Vec<PluginType> = plugins.iter().map(|p| p.plugin_type).collect();
    validate_resolved_plugin_types(&types)?;
    Ok((specs, plugins))
}

/// The CLI error for `error`, from resolving the plugins in `specs`.
///
/// A plugin with a type isn't looked up in the plugins manifest, so an
/// unpublished one fails with a 404 for its releases.json.
pub(crate) fn plugin_error(
    specs: &[PluginSpec],
    error: onerom_app::Error<onerom_fw::Error>,
) -> Error {
    if let onerom_app::Error::Fetch {
        source,
        error: onerom_fw::Error::Http { status: 404, .. },
    } = &error
    {
        let missing = specs.iter().find_map(|spec| match spec {
            PluginSpec::Named {
                name,
                plugin_type: Some(plugin_type),
                ..
            } => {
                let plugin = Plugin {
                    name: name.clone(),
                    plugin_type: *plugin_type,
                    display_name: None,
                    description: None,
                    releases: Vec::new(),
                };
                (plugin.releases_manifest_url() == *source).then(|| Error::PluginNotPublished {
                    plugin_type: *plugin_type,
                    name: name.clone(),
                })
            }
            PluginSpec::Named {
                plugin_type: None, ..
            }
            | PluginSpec::File { .. } => None,
        });
        if let Some(missing) = missing {
            return missing;
        }
    }
    error.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_board::{
        Files, IMAGE_27C400, NoFetch, base_firmware, holds, image_addr_mon, image_file, in_turn,
        move_slot, original_image, published_plugin, shows,
    };
    use onerom_app::check_image;
    use onerom_metadata::FLASH_CS1_BASE_ADDR;

    /// A plugin with a type whose releases.json isn't found isn't published.
    /// A plugin without a type that isn't in the plugins manifest isn't
    /// published either.
    #[test]
    fn a_plugin_that_isnt_published_is_reported() {
        let specs = onerom_cli::plugin::parse_plugins(&["usb".to_string(), "user/usb".to_string()])
            .unwrap();
        let url = "https://images.onerom.org/plugins/user/usb/releases.json";
        let not_found = onerom_app::Error::Fetch {
            source: url.to_string(),
            error: onerom_fw::Error::Http {
                url: url.to_string(),
                status: 404,
            },
        };
        let e = plugin_error(&specs, not_found);
        assert!(
            matches!(
                &e,
                Error::PluginNotPublished { plugin_type: onerom_cli::plugin::PluginType::User, name }
                    if name == "usb"
            ),
            "{e:?}"
        );

        let not_found = onerom_cli::plugin::PluginError::NotFound("nosuch".to_string());
        let e = plugin_error(&specs, onerom_app::Error::Plugin(not_found));
        assert!(
            matches!(&e, Error::PluginNotFound(name) if name == "nosuch"),
            "{e:?}"
        );
    }

    fn manifest() -> Releases {
        let board =
            |name: &str| format!(r#"{{ "name": "{name}", "mcus": [{{ "name": "rp2350" }}] }}"#);
        let json = format!(
            r#"{{ "version": 1, "latest": "0.8.0", "releases": [
                {{ "version": "0.8.0", "boards": [{}, {}] }},
                {{ "version": "0.5.10", "boards": [{}] }}
            ] }}"#,
            board("fire-24-a"),
            board("fire-32-b"),
            board("fire-24-a"),
        );
        Releases::from_json(&json).unwrap()
    }

    /// A version the manifest doesn't have fails with the board's releases.
    /// One it has only for other boards fails identifying the version and
    /// board.
    #[test]
    fn a_release_is_resolved_for_its_board() {
        let releases = manifest();
        let board = Board::Fire32B;
        let resolve = |version: &str| {
            resolve_release(
                &releases,
                &Some(version.to_string()),
                &board,
                &Variant::RP2350,
            )
        };

        assert_eq!(resolve("0.8.0").unwrap().version, "0.8.0");
        let e = resolve("9.9.9").unwrap_err();
        assert!(
            matches!(&e, Error::VersionNotFound(version, list) if version == "9.9.9" && list == "0.8.0"),
            "{e:?}"
        );
        let e = resolve("0.5.10").unwrap_err();
        assert!(
            matches!(&e, Error::ReleaseNotForBoard { version, board } if version == "0.5.10" && board == "fire-32-b"),
            "{e:?}"
        );
    }

    /// `chips --board` lists every chip type the board can emulate, which is
    /// wider than `Board::supported_chip_type_names()` - that covers only the
    /// board's own pin count, omitting the overhang and fly-lead combinations
    /// `docs/COMPATIBILITY.md` documents. The error message for an unknown
    /// `--chip-type` must offer the same wider list the listing shows.
    #[test]
    fn emulatable_names_include_cross_size_chips() {
        let board = Board::try_from_str("fire-24-f").unwrap();
        let names = emulatable_chip_names(&board);

        // Native 24-pin type, in both lists.
        assert!(names.contains("2364"));
        // 28-pin type, reachable only with a fly-lead - listed here, but not by
        // the board's own pin-count list.
        assert!(names.contains("2764"));
        assert!(!board.supported_chip_type_names().contains(&"2764"));
        // A chip this board cannot serve at all appears in neither.
        assert!(!names.contains("27C400"));
    }

    /// A chip type the board cannot serve has no image size to report, so the
    /// query fails rather than printing a figure for an unservable combination.
    #[test]
    fn single_chip_query_rejects_unservable_chips() {
        let board = Board::try_from_str("fire-24-f").unwrap();
        assert!(print_chip_on_board(&board, "2364").is_ok());
        assert!(print_chip_on_board(&board, "27C400").is_err());
        assert!(print_chip_on_board(&board, "not-a-chip").is_err());
    }

    fn options() -> Options {
        Options {
            verbose: false,
            log_level: onerom_cli::LogLevel::Warn,
            yes: false,
            unrecognised: false,
            device: None,
            vid_pid: Vec::new(),
        }
    }

    /// The data address of each of `image`'s slots.
    fn slot_addresses(image: &ParsedDevice) -> Vec<Option<u32>> {
        let ParsedDevice::Schema(onerom) = image else {
            panic!("not a schema image");
        };
        let metadata = onerom.metadata().expect("metadata");
        metadata
            .rom_slots
            .iter()
            .map(|slot| slot.data.addr())
            .collect()
    }

    const FIRST_CHIP: usize = 2 * 1024 * 1024;

    #[tokio::test]
    async fn an_image_on_the_first_chip_is_read_from_the_first_chip() {
        let file = image_file(BoardSize::M, 3).await;
        assert!(file.len() <= FIRST_CHIP);
        let image = parse_firmware(&file).await;
        assert!(
            image.parse_errors().is_empty(),
            "{:?}",
            image.parse_errors()
        );
        assert_eq!(image.get_board(), Some(Board::Fire40A));
        let addrs = slot_addresses(&image);
        assert_eq!(addrs.len(), 3);
        assert!(
            addrs.iter().all(|addr| addr.unwrap() < 0x1020_0000),
            "{addrs:x?}"
        );
    }

    /// A slot on the second chip is read from the file past the first chip's
    /// length.
    #[tokio::test]
    async fn an_image_using_the_second_chip_is_read_from_both_chips() {
        let file = image_file(BoardSize::L, 4).await;
        assert_eq!(file.len(), FIRST_CHIP + IMAGE_27C400);
        let checked = check_image(file, Some(Board::Fire40A)).await;
        let (_, image) = accept_image(&options(), checked, false, Some(Board::Fire40A)).unwrap();
        assert!(
            image.parse_errors().is_empty(),
            "{:?}",
            image.parse_errors()
        );
        assert_eq!(slot_addresses(&image)[3], Some(FLASH_CS1_BASE_ADDR));
    }

    /// A file shorter than its slots, one with a slot on neither chip, and one
    /// longer than the first chip without a slot on the second, are refused
    /// unless forced.
    #[tokio::test]
    async fn an_image_file_that_doesnt_match_its_slots_needs_force() {
        use onerom_fw_parser::ImageFileError;
        let mut short = image_file(BoardSize::M, 3).await;
        short.truncate(short.len() - 4096);
        let damaged = move_slot(image_file(BoardSize::M, 3).await, 2, 0x1030_0000).await;
        let mut long = image_file(BoardSize::M, 3).await;
        long.resize(FIRST_CHIP + 4096, 0xFF);
        for (file, expected) in [
            (short, ImageFileError::TooShort { short_by: 4096 }),
            (
                damaged,
                ImageFileError::BadAddress {
                    slot: 2,
                    addr: 0x1030_0000,
                },
            ),
            (long, ImageFileError::TooLong { too_long_by: 4096 }),
        ] {
            let checked = check_image(file.clone(), None).await;
            let error = accept_image(&options(), checked, false, None).unwrap_err();
            assert!(
                matches!(&error, Error::ImageFile(e) if *e == expected),
                "{error:?}"
            );
            let checked = check_image(file, None).await;
            assert!(accept_image(&options(), checked, true, None).is_ok());
        }
    }

    /// `--size L` is refused for a board that doesn't support external flash.
    #[test]
    fn a_second_chip_needs_a_board_that_supports_external_flash() {
        let board = |name| Board::try_from_str(name).unwrap();
        assert_eq!(
            build_size(board("fire-40-a"), BoardSize::L).unwrap(),
            BoardSize::L
        );
        assert!(build_size(board("fire-24-f"), BoardSize::L).is_err());
        assert_eq!(
            build_size(board("fire-24-f"), BoardSize::M).unwrap(),
            BoardSize::M
        );
    }

    /// `firmware build` advises `--size` only for a build without a second
    /// flash chip, where the board and the firmware both support one.
    #[test]
    fn size_is_advised_only_where_it_helps() {
        let board = |name| Board::try_from_str(name).unwrap();
        let new = FirmwareVersion::new(0, 8, 0, 0);
        let old = FirmwareVersion::new(0, 7, 3, 0);
        let advised = |board, size, version| {
            let error = Error::SlotDoesNotFit {
                error: onerom_fw::Error::build(GenError::SlotDoesNotFit { slot: 3 }),
                advise_second_chip: false,
            };
            matches!(
                advise_second_chip(error, board, Variant::RP2350, size, version),
                Error::SlotDoesNotFit {
                    advise_second_chip: true,
                    ..
                }
            )
        };
        assert!(advised(board("fire-40-a"), BoardSize::M, new));
        assert!(!advised(board("fire-40-a"), BoardSize::L, new));
        assert!(!advised(board("fire-24-f"), BoardSize::M, new));
        assert!(!advised(board("fire-40-a"), BoardSize::M, old));
    }

    /// The refusal reports the image's board as the firmware's and the board
    /// asked for as the expected one.
    #[test]
    fn board_mismatch_says_which_board_is_which() {
        let expected = Board::try_from_str("fire-24-f").unwrap();
        let actual = Board::try_from_str("fire-28-a").unwrap();
        let msg = Error::from(ImageProblem::BoardMismatch {
            image: actual,
            expected,
        })
        .to_string();
        let text =
            "Firmware board type 'fire-28-a' does not match the expected board type 'fire-24-f'";
        assert!(msg.contains(text), "{msg}");
    }

    /// A built image's summary contains its board and MCU, then each slot's
    /// size followed by its chip type and image source.
    #[tokio::test]
    async fn a_built_images_summary_lists_its_mcu_and_roms() {
        let file = image_file(BoardSize::M, 2).await;
        let image = parse_firmware(&file).await;
        let lines = firmware_summary(false, &image, &file).await.unwrap();
        assert!(shows(&lines, &["fire-40-a"]), "{lines:#?}");
        assert!(shows(&lines, &["RP235xB"]), "{lines:#?}");
        let size = IMAGE_27C400.to_string();
        for n in 0..2 {
            let source = format!("{n}.bin");
            assert!(
                in_turn(&lines, &[&[&size], &["27C400", &source]]),
                "{lines:#?}"
            );
        }
    }

    /// Base firmware is told apart from a built image, from one cut off in its
    /// metadata and from firmware before v0.7.0. Only the cut-off image's
    /// parse errors are printed.
    #[tokio::test]
    async fn base_firmware_is_told_apart_from_a_damaged_image() {
        let built = image_file(BoardSize::M, 1).await;
        let mut cut_off = built.clone();
        cut_off.truncate(FIRMWARE_SIZE + 0x100);
        for (name, file, base, warned) in [
            ("base", base_firmware(8), true, false),
            ("built", built, false, false),
            ("cut off", cut_off, false, true),
            ("before v0.7.0", original_image(), false, false),
        ] {
            let image = parse_firmware(&file).await;
            assert_eq!(is_base_firmware(&image, &file), base, "{name}");
            let errors = reported_parse_errors(&image, &file);
            assert_eq!(!errors.is_empty(), warned, "{name}: {errors:?}");
        }
    }

    /// A summary of firmware from before v0.7.0 contains its board and its
    /// MCU's display text.
    #[tokio::test]
    async fn a_summary_from_before_0_7_0_contains_the_mcu() {
        let file = original_image();
        let image = parse_firmware(&file).await;
        let lines = firmware_summary(false, &image, &file).await.unwrap();
        assert!(shows(&lines, &["fire-24-e"]), "{lines:#?}");
        assert!(shows(&lines, &["RP2350"]), "{lines:#?}");
    }

    #[tokio::test]
    async fn a_summary_contains_the_global_addr_mon_min_cs_act_ns() {
        let file = image_addr_mon(Some(140), &[None]).await;
        let image = parse_firmware(&file).await;
        let lines = firmware_summary(false, &image, &file).await.unwrap();
        assert!(shows(&lines, &["140ns"]), "{lines:#?}");
    }

    // -- reserved_pins -----------------------------------------------------

    fn build_args(words: &[&str]) -> args::firmware::FirmwareBuildArgs {
        use clap::Parser;
        let cli = crate::args::Cli::try_parse_from(words).unwrap();
        let crate::args::Commands::Firmware(firmware) = cli.command else {
            panic!("not firmware");
        };
        let args::firmware::FirmwareCommands::Build(args) = firmware.command else {
            panic!("not firmware build");
        };
        args
    }

    #[test]
    fn reserve_pin_repeats_under_any_name() {
        use onerom_config::pin::{HeaderPin, Pin};
        let args = build_args(&[
            "onerom",
            "firmware",
            "build",
            "--board",
            "fire-24-f",
            "--slot",
            "file=a.bin,type=2364,cs1=active-low",
            "--reserve-pin",
            "SEL-C",
            "--reserved_pins",
            "gpio9",
            "--reserved-pin",
            "gpio10",
        ]);
        assert_eq!(
            args.reserve_pin,
            [
                Pin::Header(HeaderPin::Select(2)),
                Pin::Gpio(9),
                Pin::Gpio(10)
            ]
        );
    }

    /// An address line can't be reserved, and `--reserve-pin` conflicts with
    /// `--no-config`.
    #[test]
    fn reserve_pin_fails_where_it_cant_apply() {
        use clap::Parser;
        for words in [
            &[
                "onerom",
                "firmware",
                "build",
                "--board",
                "fire-24-f",
                "--slot",
                "file=a.bin,type=2364,cs1=active-low",
                "--reserve-pin",
                "a17",
            ][..],
            &[
                "onerom",
                "firmware",
                "build",
                "--board",
                "fire-24-f",
                "--no-config",
                "--reserve-pin",
                "sel_c",
            ][..],
        ] {
            assert!(
                crate::args::Cli::try_parse_from(words).is_err(),
                "{words:?}"
            );
        }
    }

    #[test]
    fn reserve_pin_replaces_the_configs_reserved_pins() {
        use onerom_config::pin::{HeaderPin, Pin};
        let json =
            r#"{ "version": 1, "description": "d", "reserved_pins": ["sel_a"], "chip_sets": [] }"#;
        let global = |reserved_pins: Vec<Pin>| GlobalConfig {
            config_name: None,
            config_description: None,
            instance_name: None,
            serial_override: None,
            boot_logging: None,
            disable_swd: None,
            turbo_boot: None,
            reserved_pins,
            addr_mon_min_cs_act_ns: None,
        };
        let reserved = |json: String| -> serde_json::Value {
            serde_json::from_str::<serde_json::Value>(&json).unwrap()["reserved_pins"].clone()
        };

        let replaced =
            apply_global_overrides(json.to_string(), &global(vec![Pin::Header(HeaderPin::X1)]))
                .unwrap();
        assert_eq!(reserved(replaced), serde_json::json!(["x1"]));
        let kept = apply_global_overrides(json.to_string(), &global(Vec::new())).unwrap();
        assert_eq!(reserved(kept), serde_json::json!(["sel_a"]));
    }

    #[test]
    fn slots_past_the_jumpers_are_named() {
        use crate::test_board::{config_2364, holds};
        use onerom_config::pin::{HeaderPin, ReservedPins};
        let config = |sets: usize| -> Config {
            let sets = vec!["single"; sets];
            serde_json::from_str(&config_2364(&sets, std::path::Path::new("."), &[])).unwrap()
        };
        let mut sel_c = ReservedPins::new();
        sel_c.insert(HeaderPin::Select(2));
        let mut sel_c_d = sel_c;
        sel_c_d.insert(HeaderPin::Select(3));

        // Four image select pins provide 16 combinations and three provide 8.
        assert_eq!(
            unselectable_slots(&config(16), Board::Fire24F, ReservedPins::new()),
            None
        );
        assert_eq!(unselectable_slots(&config(8), Board::Fire24F, sel_c), None);

        let one = unselectable_slots(&config(5), Board::Fire24F, sel_c_d).unwrap();
        assert!(holds(&one, &["4", "SEL_A", "SEL_B", "5"]), "{one}");
        assert!(!one.contains("SEL_C"), "{one}");

        let two = unselectable_slots(&config(6), Board::Fire24F, sel_c_d).unwrap();
        assert!(holds(&two, &["4", "5", "6"]), "{two}");

        let many = unselectable_slots(&config(10), Board::Fire24F, sel_c).unwrap();
        assert!(holds(&many, &["8", "9", "SEL_D", "10"]), "{many}");

        let mut turbo = config(10);
        turbo.turbo_boot = true;
        assert_eq!(unselectable_slots(&turbo, Board::Fire24F, sel_c), None);
    }

    #[tokio::test]
    async fn reserve_pin_reaches_the_image() {
        use crate::test_board::{IMAGE_2364, config_2364};
        use onerom_config::pin::ReservedPins;

        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.bin");
        let config = dir.path().join("sets.json");
        let out = dir.path().join("out.bin");
        std::fs::write(&base, base_firmware(8)).unwrap();
        std::fs::write(&config, config_2364(&["single"], dir.path(), &[])).unwrap();
        std::fs::write(dir.path().join("0.bin"), vec![0; IMAGE_2364]).unwrap();

        let args = build_args(&[
            "onerom",
            "firmware",
            "build",
            "--board",
            "fire-24-f",
            "--config",
            config.to_str().unwrap(),
            "--base-firmware",
            base.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--reserve-pin",
            "sel_d",
            "--reserve-pin",
            "gpio8",
            "--no-plugin",
        ]);
        cmd_build(&options(), &args).await.unwrap();

        let image = onerom_cli::image::parse_firmware(&std::fs::read(&out).unwrap()).await;
        assert_eq!(
            image.reserved_pins(),
            Some(ReservedPins::from_bits(0b1000, 0b10))
        );
    }

    // -- addr_mon_min_cs_act_ns --------------------------------------------

    #[test]
    fn addr_mon_min_cs_act_ns_parses_under_either_name() {
        use onerom_gen::MAX_ADDR_MON_MIN_CS_ACT_NS;
        for (name, ns) in [
            ("--addr-mon-min-cs-act-ns", 0),
            ("--addr_mon_min_cs_act_ns", MAX_ADDR_MON_MIN_CS_ACT_NS),
        ] {
            let value = ns.to_string();
            let args = build_args(&[
                "onerom",
                "firmware",
                "build",
                "--board",
                "fire-24-f",
                "--slot",
                "file=a.bin,type=2364,cs1=active-low",
                name,
                &value,
            ]);
            assert_eq!(args.addr_mon_min_cs_act_ns, Some(ns), "{name} {value}");
        }
    }

    /// A value above the maximum or with a unit fails, and the option
    /// conflicts with `--no-config`.
    #[test]
    fn addr_mon_min_cs_act_ns_fails_where_it_cant_apply() {
        use clap::Parser;
        use onerom_gen::MAX_ADDR_MON_MIN_CS_ACT_NS;
        let above = (MAX_ADDR_MON_MIN_CS_ACT_NS + 1).to_string();
        let line = |value: &str| {
            [
                "onerom",
                "firmware",
                "build",
                "--board",
                "fire-24-f",
                "--slot",
                "file=a.bin,type=2364,cs1=active-low",
                "--addr-mon-min-cs-act-ns",
            ]
            .into_iter()
            .chain([value])
            .map(str::to_string)
            .collect::<Vec<_>>()
        };
        let no_config = [
            "onerom",
            "firmware",
            "build",
            "--board",
            "fire-24-f",
            "--no-config",
            "--addr-mon-min-cs-act-ns",
            "140",
        ]
        .map(str::to_string)
        .to_vec();
        for words in [line(&above), line("140ns"), no_config] {
            assert!(
                crate::args::Cli::try_parse_from(&words).is_err(),
                "{words:?}"
            );
        }
    }

    #[test]
    fn addr_mon_min_cs_act_ns_replaces_the_configs_value() {
        let json = r#"{ "version": 1, "description": "d", "addr_mon_min_cs_act_ns": 140, "chip_sets": [] }"#;
        let global = |addr_mon_min_cs_act_ns: Option<u16>| GlobalConfig {
            config_name: None,
            config_description: None,
            instance_name: None,
            serial_override: None,
            boot_logging: None,
            disable_swd: None,
            turbo_boot: None,
            reserved_pins: Vec::new(),
            addr_mon_min_cs_act_ns,
        };
        let value = |json: String| -> serde_json::Value {
            serde_json::from_str::<serde_json::Value>(&json).unwrap()["addr_mon_min_cs_act_ns"]
                .clone()
        };

        let replaced = apply_global_overrides(json.to_string(), &global(Some(300))).unwrap();
        assert_eq!(value(replaced), serde_json::json!(300));
        let kept = apply_global_overrides(json.to_string(), &global(None)).unwrap();
        assert_eq!(value(kept), serde_json::json!(140));
    }

    /// The option and a per-slot value reach the image, in the metadata's
    /// units.
    #[tokio::test]
    async fn addr_mon_min_cs_act_ns_reaches_the_image() {
        use crate::test_board::{IMAGE_2364, base_firmware_patch};

        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.bin");
        let rom = dir.path().join("0.bin");
        let out = dir.path().join("out.bin");
        std::fs::write(&base, base_firmware_patch(8, 1)).unwrap();
        std::fs::write(&rom, vec![0; IMAGE_2364]).unwrap();
        let slot = format!(
            "file={},type=2364,cs1=active-low,addr-mon-min-cs-act-ns=300",
            rom.display()
        );

        let args = build_args(&[
            "onerom",
            "firmware",
            "build",
            "--board",
            "fire-24-f",
            "--slot",
            &slot,
            "--base-firmware",
            base.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--addr-mon-min-cs-act-ns",
            "140",
            "--no-plugin",
        ]);
        cmd_build(&options(), &args).await.unwrap();

        let image = onerom_cli::image::parse_firmware(&std::fs::read(&out).unwrap()).await;
        let metadata = image
            .as_schema()
            .and_then(|onerom| onerom.metadata())
            .unwrap();
        assert_eq!(metadata.addr_mon_min_cs_act, 14);
        let overrides = metadata.rom_slots[0].firmware_overrides.as_ref().unwrap();
        assert_eq!(overrides.addr_mon_min_cs_act, 30);
    }

    // -- default plugin ----------------------------------------------------

    const V0_8_0: FirmwareVersion = FirmwareVersion::new(0, 8, 0, 0);

    /// A config whose only chip set is a system plugin.
    const PLUGIN_CONFIG: &str = r#"{
        "version": 1,
        "name": "t",
        "description": "d",
        "chip_sets": [
            { "type": "single", "chips": [{ "file": "plugin.bin", "type": "system_plugin" }] }
        ]
    }"#;

    fn raw(specs: &[&str]) -> Vec<String> {
        specs.iter().map(|spec| spec.to_string()).collect()
    }

    fn names(plugins: &[ResolvedPlugin]) -> Vec<(PluginType, &str)> {
        plugins
            .iter()
            .map(|p| (p.plugin_type, p.name.as_str()))
            .collect()
    }

    #[tokio::test]
    async fn the_usb_plugin_is_added_where_no_plugin_is_specified() {
        let (_, served) = published_plugin(PluginType::System, "usb");
        let (specs, plugins) = resolve_build_plugins(&[], false, None, &V0_8_0, &served)
            .await
            .unwrap();
        assert_eq!(names(&plugins), [(PluginType::System, "usb")]);
        // A saved config refers to it by name.
        assert!(matches!(
            &specs[..],
            [PluginSpec::Named { version: None, .. }]
        ));
    }

    #[tokio::test]
    async fn the_usb_plugin_is_added_ahead_of_a_user_plugin() {
        let (_, Files(mut served)) = published_plugin(PluginType::System, "usb");
        served.extend(published_plugin(PluginType::User, "host-control").1.0);
        let (specs, plugins) = resolve_build_plugins(
            &raw(&["user/host-control"]),
            false,
            None,
            &V0_8_0,
            &Files(served),
        )
        .await
        .unwrap();
        assert_eq!(
            names(&plugins),
            [
                (PluginType::System, "usb"),
                (PluginType::User, "host-control")
            ]
        );
        assert_eq!(specs.len(), plugins.len());
    }

    /// Only `other` is published, so fetching the usb plugin would fail.
    #[tokio::test]
    async fn no_plugin_is_added_where_a_system_plugin_is_specified() {
        let (_, served) = published_plugin(PluginType::System, "other");
        let (_, plugins) =
            resolve_build_plugins(&raw(&["system/other"]), false, None, &V0_8_0, &served)
                .await
                .unwrap();
        assert_eq!(names(&plugins), [(PluginType::System, "other")]);
    }

    #[tokio::test]
    async fn no_plugin_is_added_where_the_config_defines_one() {
        let (_, plugins) =
            resolve_build_plugins(&[], false, Some(PLUGIN_CONFIG), &V0_8_0, &NoFetch)
                .await
                .unwrap();
        assert!(plugins.is_empty());
    }

    #[tokio::test]
    async fn no_plugin_leaves_out_the_usb_plugin() {
        let (_, plugins) = resolve_build_plugins(&[], true, None, &V0_8_0, &NoFetch)
            .await
            .unwrap();
        assert!(plugins.is_empty());
    }

    #[tokio::test]
    async fn no_plugin_with_a_config_that_defines_one_fails() {
        let result = resolve_build_plugins(&[], true, Some(PLUGIN_CONFIG), &V0_8_0, &NoFetch).await;
        assert!(
            matches!(&result, Err(Error::InvalidArgument(option, _)) if option == "--no-plugin"),
            "{result:?}"
        );
    }

    /// The published release requires firmware v0.7.0.
    #[tokio::test]
    async fn the_usb_plugin_is_left_out_where_no_version_supports_the_firmware() {
        let (_, served) = published_plugin(PluginType::System, "usb");
        let (specs, plugins) =
            resolve_build_plugins(&[], false, None, &FirmwareVersion::new(0, 6, 0, 0), &served)
                .await
                .unwrap();
        assert!(specs.is_empty() && plugins.is_empty());
    }

    /// The error includes the option that builds without the plugin.
    #[tokio::test]
    async fn a_usb_plugin_that_cant_be_fetched_fails() {
        let result = resolve_build_plugins(&[], false, None, &V0_8_0, &Files(Vec::new())).await;
        let Err(e) = result else {
            panic!("{result:?}");
        };
        assert!(holds(&e.to_string(), &["--no-plugin"]), "{e}");
    }

    // -- config URL --------------------------------------------------------

    /// Serves `body` with `status` to one request and returns the URL.
    fn serve_once(status: &'static str, body: String) -> String {
        use std::io::{BufRead, BufReader};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/config.json", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut request = BufReader::new(&stream);
            let mut line = String::new();
            while request.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            write!(
                &stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        url
    }

    #[tokio::test]
    async fn config_url_builds_the_same_image_as_a_file() {
        use crate::test_board::{IMAGE_2364, config_2364};

        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.bin");
        let config = dir.path().join("sets.json");
        let json = config_2364(&["single"], dir.path(), &[]);
        std::fs::write(&base, base_firmware(8)).unwrap();
        std::fs::write(&config, &json).unwrap();
        std::fs::write(dir.path().join("0.bin"), vec![0x5a; IMAGE_2364]).unwrap();

        let mut images = Vec::new();
        for source in [
            config.to_str().unwrap().to_string(),
            serve_once("200 OK", json),
        ] {
            let out = dir.path().join(format!("out-{}.bin", images.len()));
            let args = build_args(&[
                "onerom",
                "firmware",
                "build",
                "--board",
                "fire-24-f",
                "--config",
                &source,
                "--base-firmware",
                base.to_str().unwrap(),
                "--output",
                out.to_str().unwrap(),
                "--no-plugin",
            ]);
            cmd_build(&options(), &args).await.unwrap();
            images.push(std::fs::read(&out).unwrap());
        }
        assert_eq!(images[0], images[1]);
    }

    #[tokio::test]
    async fn config_url_http_error_fails() {
        let url = serve_once("404 Not Found", "Not Found".to_string());
        let result = fetch_config_json(&url).await;
        assert!(result.is_err(), "{result:?}");
    }
}
