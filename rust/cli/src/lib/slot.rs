// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Slot string parsing and ROM configuration JSON generation.
//!
//! Handles parsing of `--slot file=...,type=...,cs1=...` arguments and
//! converting them into a One ROM JSON configuration suitable for the builder.

use crate::Error;
use crate::plugin::{ResolvedPlugin, plugin_to_chip_set_config};
use onerom_config::chip::{CHIP_TYPE_NAMES_PLUGINS, ChipFunction, ChipType, ControlLineType};
use onerom_config::hw::Board;
use onerom_gen::{
    ChipConfig, ChipSetConfig, ChipSetType, ChipTypeSpec, Config, CsLogic, FileFormat, FireConfig,
    FireCpuFreq, FireVreg, FirmwareConfig, LedConfig, LoadAddress, SizeHandling, Transform,
    parse_transform_list, requires_half_select_cs1,
};

const DEFAULT_CONFIG_DESCRIPTION: &str = "Created by the One ROM CLI";

pub struct GlobalConfig {
    pub config_name: Option<String>,
    pub config_description: Option<String>,
    pub instance_name: Option<String>,
    pub serial_override: Option<String>,
    pub boot_logging: Option<bool>,
    pub disable_swd: Option<bool>,
    pub turbo_boot: Option<bool>,
}

/// The result of checking whether any slot specifications require user
/// confirmation before proceeding.
pub struct ConfirmationsRequired {
    /// True if any slot has a CPU frequency above the stock threshold.
    pub cpu_freq: bool,
    /// True if any slot has a vreg above the stock threshold.
    pub vreg: bool,
    /// Names of chip types that the board does not support but were permitted
    /// via `--allow-unsupported-chip-type`. Empty unless the override was used.
    pub unsupported_chip_types: Vec<String>,
}

/// Check whether any slot specifications require user confirmation.
///
/// The caller should inspect the returned flags and prompt the user
/// accordingly before proceeding to build the firmware. The `--yes`
/// flag suppresses both prompts.
pub fn check_confirmations(slots: &[SlotSpec]) -> ConfirmationsRequired {
    ConfirmationsRequired {
        cpu_freq: slots.iter().any(|s| {
            s.cpu_freq
                .map(|f| f > FireCpuFreq::stock_value())
                .unwrap_or(false)
        }),
        vreg: slots.iter().any(|s| {
            s.vreg
                .as_ref()
                .map(|v| *v > FireVreg::stock_value())
                .unwrap_or(false)
        }),
        unsupported_chip_types: Vec::new(),
    }
}

/// Parse slot strings and check whether any require user confirmation.
///
/// Slots are parsed purely for validation and confirmation checking.
/// The caller should prompt as needed before proceeding.
pub fn check_slot_confirmations(
    slots: &[String],
    board: &Board,
    allow_unsupported_chip_type: bool,
) -> Result<ConfirmationsRequired, Error> {
    let parsed = parse_slots(slots, board, allow_unsupported_chip_type)?;
    let mut confirmations = check_confirmations(&parsed);
    // A parsed slot with an unsupported chip type can only be present because
    // the override was set (parse_slot errors otherwise), so collect these for
    // the caller to warn about.
    confirmations.unsupported_chip_types = parsed
        .iter()
        .filter(|s| !board.allows_chip_type(s.chip_type.resolved()))
        .map(|s| s.chip_type.resolved().name().to_string())
        .collect();
    Ok(confirmations)
}

// Handle tilde expansion for file paths in slot specifications, since these
// are passed directly to the builder as-is and won't be expanded by the
// shell.
fn expand_tilde(path: &str) -> std::borrow::Cow<'_, str> {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        format!("{}/{}", home.to_string_lossy(), rest).into()
    } else {
        path.into()
    }
}

/// Parsed and validated slot specification from a `--slot` argument.
#[derive(Debug)]
pub struct SlotSpec {
    pub file: Option<String>,
    pub label: Option<String>,
    pub chip_type: ChipTypeSpec,
    pub cs1: Option<CsLogic>,
    pub cs2: Option<CsLogic>,
    pub cs3: Option<CsLogic>,
    pub cs4: Option<CsLogic>,
    size_handling: Option<SizeHandling>,
    pub cpu_freq: Option<FireCpuFreq>,
    pub vreg: Option<FireVreg>,
    pub led: Option<bool>,
    pub force_16bit: Option<bool>,
    pub format: Option<FileFormat>,
    pub load_address: Option<LoadAddress>,
    pub transform: Vec<Transform>,
}

/// Parse a CS logic value, accepting active_low/0 and active_high/1.
fn parse_cs_logic(slot: &str, key: &str, value: &str) -> Result<CsLogic, Error> {
    match value {
        "active_low" | "0" => Ok(CsLogic::ActiveLow),
        "active_high" | "1" => Ok(CsLogic::ActiveHigh),
        other => Err(Error::InvalidArgument(
            "--slot".to_string(),
            format!(
                "Invalid CS logic '{other}': expected {key}=active_low|active_high|0|1\n   --slot '{slot}'"
            ),
        )),
    }
}

// Use the SizeHandling deserialization to validate the value and get a
// normalized string.
fn parse_size_handling(slot: &str, _key: &str, value: &str) -> Result<SizeHandling, Error> {
    serde_json::from_str::<SizeHandling>(&format!("\"{value}\"")).map_err(|_| {
        let supported_variants = SizeHandling::supported_values()
            .iter()
            .map(|v| {
                serde_json::to_string(v)
                    .unwrap()
                    .trim_matches('"')
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(", ");
        Error::InvalidArgument(
            "--slot".to_string(),
            format!(
                "Invalid size_handling '{value}'\n    --slot '{slot}'\n  Supported values: {supported_variants}"
            ),
        )
    })
}

fn parse_format(slot: &str, value: &str) -> Result<FileFormat, Error> {
    FileFormat::try_from_str(value).ok_or_else(|| {
        Error::InvalidArgument(
            "--slot".to_string(),
            format!(
                "Invalid format '{value}'\n    --slot '{slot}'\n  Supported values: binary, ihex"
            ),
        )
    })
}

fn parse_load_address(slot: &str, value: &str) -> Result<LoadAddress, Error> {
    LoadAddress::parse_str(value).map_err(|e| {
        Error::InvalidArgument("--slot".to_string(), format!("{e}\n    --slot '{slot}'"))
    })
}

/// Parse a `+`-separated list of image transforms, e.g.
/// `deinterleave:1/2/2+swap_bytes`.
fn parse_transform(slot: &str, value: &str) -> Result<Vec<Transform>, Error> {
    parse_transform_list(value).map_err(|e| {
        Error::InvalidArgument("--slot".to_string(), format!("{e}\n    --slot '{slot}'"))
    })
}

fn parse_bool(slot: &str, key: &str, value: &str) -> Result<bool, Error> {
    match value.to_lowercase().as_str() {
        "true" | "on" | "1" => Ok(true),
        "false" | "off" | "0" => Ok(false),
        other => Err(Error::InvalidArgument(
            "--slot".to_string(),
            format!(
                "Invalid boolean '{other}': expected {key}=true|false|on|off|1|0\n    --slot '{slot}'"
            ),
        )),
    }
}

fn parse_cpu_freq(slot: &str, key: &str, value: &str) -> Result<FireCpuFreq, Error> {
    let digits = if value.to_lowercase().ends_with("mhz") {
        &value[..value.len() - 3]
    } else {
        value
    };
    let mhz = digits.parse::<u16>().map_err(|_| {
        Error::InvalidArgument(
            "--slot".to_string(),
            format!("Invalid CPU frequency '{value}': expected formats {key}=150|150MHz\n    --slot '{slot}'"),
        )
    })?;
    FireCpuFreq::mhz(mhz).map_err(|_| {
        Error::InvalidArgument(
            "--slot".to_string(),
            format!(
                "CPU frequency {mhz}MHz out of range ({}-{}MHz)\n    --slot '{slot}'",
                FireCpuFreq::MIN_MHZ,
                FireCpuFreq::MAX_MHZ,
            ),
        )
    })
}

fn parse_vreg(slot: &str, key: &str, value: &str) -> Result<FireVreg, Error> {
    let stripped = if value.ends_with('v') || value.ends_with('V') {
        &value[..value.len() - 1]
    } else {
        value
    };
    let canonical = match stripped.split_once('.') {
        Some((int, frac)) => {
            let padded = format!("{frac:0<2}");
            if padded.len() > 2 {
                return Err(Error::InvalidArgument(
                    "--slot".to_string(),
                    format!(
                        "Invalid VReg '{value}': too many decimal places, max 2\n    --slot '{slot}'"
                    ),
                ));
            }
            format!("{int}.{padded}V")
        }
        None => {
            return Err(Error::InvalidArgument(
                "--slot".to_string(),
                format!(
                    "Invalid VReg '{value}': expected format {key}=1.1|1.10|1.10V\n    --slot '{slot}'"
                ),
            ));
        }
    };
    serde_json::from_str::<FireVreg>(&format!("\"{canonical}\"")).map_err(|_| {
        let levels = FireVreg::supported_levels()
            .iter()
            .map(|v| {
                serde_json::to_string(v)
                    .unwrap()
                    .trim_matches('"')
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(", ");
        Error::InvalidArgument(
            "--slot".to_string(),
            format!(
                "Unsupported VReg '{value}'\n    --slot '{slot}'\n  Supported levels: {levels}"
            ),
        )
    })
}

const SLOT_KEYS: &[&str] = &[
    "file",
    "label",
    "type",
    "cs1",
    "cs2",
    "cs3",
    "cs4",
    "size_handling",
    "size",
    "cpu-freq",
    "cpu-vreg",
    "led",
    "force_16bit",
    "format",
    "load_address",
    "transform",
];

/// Parse a single `--slot` string into a [`SlotSpec`], validating against the given board.
///
/// When `allow_unsupported_chip_type` is set, a chip type outside the board's
/// supported set is permitted rather than rejected; the caller is expected to
/// warn (see [`ConfirmationsRequired::unsupported_chip_types`]). Board
/// electrical constraints (e.g. the 40-pin requirement for `force_16bit`) are
/// unaffected.
fn parse_slot(
    slot: &str,
    board: &Board,
    allow_unsupported_chip_type: bool,
) -> Result<SlotSpec, Error> {
    let mut file = None;
    let mut label = None;
    let mut chip_type_str = None;
    let mut cs1 = None;
    let mut cs2 = None;
    let mut cs3 = None;
    let mut cs4 = None;
    let mut size_handling = None;
    let mut cpu_freq = None;
    let mut vreg = None;
    let mut led = None;
    let mut force_16bit = None;
    let mut format = None;
    let mut load_address = None;
    let mut transform = Vec::new();

    //
    // Parse
    //
    let mut seen = std::collections::HashSet::new();
    for part in slot.split(',') {
        let (key, value) = part.split_once('=').ok_or_else(|| {
            Error::InvalidArgument("--slot".to_string(), format!("Slot key '{part}' is missing a value - expected '{part}=<value>'\n    --slot '{slot}'"))
        })?;
        let key = key.trim();
        if !seen.insert(key) {
            return Err(Error::InvalidArgument(
                "--slot".to_string(),
                format!("Duplicate slot key '{key}' found.\n    --slot '{slot}'"),
            ));
        }
        match key {
            "file" | "path" | "url" => file = Some(expand_tilde(value).into_owned()),
            "label" | "name" => label = Some(value.to_string()),
            "type" | "rom-type" | "rom_type" | "chip_type" | "chip-type" => {
                chip_type_str = Some(value.to_string())
            }
            "cs1" => cs1 = Some(parse_cs_logic(slot, key, value)?),
            "cs2" => cs2 = Some(parse_cs_logic(slot, key, value)?),
            "cs3" => cs3 = Some(parse_cs_logic(slot, key, value)?),
            "cs4" => cs4 = Some(parse_cs_logic(slot, key, value)?),
            "size_handling" | "size" => {
                size_handling = Some(parse_size_handling(slot, key, value)?)
            }
            "cpu" | "freq" | "frequency" | "cpu-freq" | "cpu_freq" | "cpu_frequency"
            | "cpu-frequency" => cpu_freq = Some(parse_cpu_freq(slot, key, value)?),
            "vreg" | "cpu-vreg" | "cpu_vreg" => vreg = Some(parse_vreg(slot, key, value)?),
            "led" | "status_led" | "status-led" => led = Some(parse_bool(slot, key, value)?),
            "16bit" | "force_16bit" | "force_16_bit" | "force-16bit" | "force-16-bit" => {
                force_16bit = Some(parse_bool(slot, key, value)?)
            }
            "format" => format = Some(parse_format(slot, value)?),
            "load_address" | "load-address" | "load_addr" => {
                load_address = Some(parse_load_address(slot, value)?)
            }
            "transform" | "trans" => transform = parse_transform(slot, value)?,
            other => {
                let supported_keys = SLOT_KEYS.join(", ");
                return Err(Error::InvalidArgument(
                    "--slot".to_string(),
                    format!(
                        "Unrecognised slot key '{other}'\n    --slot '{slot}'\n  Supported keys: {supported_keys}"
                    ),
                ));
            }
        }
    }

    //
    // Validate
    //
    let chip_type_str = chip_type_str.ok_or_else(|| {
        Error::InvalidArgument(
            "--slot".to_string(),
            format!("slot missing 'type' key\n    --slot '{slot}'"),
        )
    })?;
    let chip_type = ChipType::try_from_str(&chip_type_str).ok_or_else(|| {
        let supported = supported_chip_names_for_board(board);
        Error::UnsupportedChipType(chip_type_str.clone(), supported)
    })?;

    if !allow_unsupported_chip_type && !board.allows_chip_type(chip_type) {
        let supported = supported_chip_names_for_board(board);
        return Err(Error::UnsupportedBoardChipType(
            chip_type.name().to_string(),
            chip_type.aliases().join(", "),
            supported,
        ));
    }

    if chip_type.chip_function() != ChipFunction::Ram && file.is_none() {
        return Err(Error::InvalidArgument(
            "--slot".to_string(),
            format!("Missing 'file' key for ROM chip.\n    --slot '{slot}'"),
        ));
    }

    validate_cs_lines(slot, &chip_type, cs1, cs2, cs3, cs4)?;

    if force_16bit.is_some() && board.chip_pins() != 40 {
        return Err(Error::InvalidArgument(
            "--slot".to_string(),
            format!("force_16bit is only valid on 40-pin boards\n    --slot '{slot}'"),
        ));
    }

    // A load address only makes sense for an Intel HEX image.
    if load_address.is_some() && format != Some(FileFormat::IntelHex) {
        return Err(Error::InvalidArgument(
            "--slot".to_string(),
            format!("load_address is only valid with format=ihex\n    --slot '{slot}'"),
        ));
    }

    Ok(SlotSpec {
        file,
        label,
        // Preserve the user's exact spelling (e.g. `27SF512`) alongside the
        // resolved type, so it survives verbatim into the generated metadata.
        chip_type: ChipTypeSpec::new(chip_type_str, chip_type),
        cs1,
        cs2,
        cs3,
        cs4,
        size_handling,
        cpu_freq,
        vreg,
        led,
        force_16bit,
        format,
        load_address,
        transform,
    })
}

/// Validate the CS lines supplied against the chip type's control lines.
///
/// - A `Configurable` line's polarity is mask-programmed at manufacture, so
///   the user must state it.
/// - A fixed line's polarity is set by the silicon, so the user must not
///   state it. `ignore` is not a polarity - it says this One ROM does not
///   monitor the line - so it stays permitted here and is policed by
///   `check_cs_v2`'s `allow_cs_ignore` rules.
/// - A line the chip type does not have must not be specified, except `cs1`
///   on a chip needing a half-select (see `requires_half_select_cs1`), where
///   `cs1` names the excess top address line rather than a pin. `check_cs_v2`
///   requires it there.
fn validate_cs_lines(
    slot: &str,
    chip_type: &ChipType,
    cs1: Option<CsLogic>,
    cs2: Option<CsLogic>,
    cs3: Option<CsLogic>,
    cs4: Option<CsLogic>,
) -> Result<(), Error> {
    let cs_values = [("cs1", cs1), ("cs2", cs2), ("cs3", cs3), ("cs4", cs4)];

    for line in chip_type.control_lines() {
        let supplied = cs_values
            .iter()
            .find(|(name, _)| *name == line.name)
            .and_then(|(_, v)| *v);

        match line.line_type {
            ControlLineType::Configurable if supplied.is_none() => {
                return Err(Error::InvalidArgument(
                    "--slot".to_string(),
                    format!(
                        "Chip type {} requires {} to be specified\n    --slot '{slot}'",
                        chip_type.name(),
                        line.name
                    ),
                ));
            }
            ControlLineType::FixedActiveLow | ControlLineType::FixedActiveHigh if matches!(supplied, Some(logic) if logic != CsLogic::Ignore) =>
            {
                return Err(Error::InvalidArgument(
                    "--slot".to_string(),
                    format!(
                        "Chip type {} has fixed {} {}, do not specify it\n    --slot '{slot}'",
                        chip_type.name(),
                        if line.line_type == ControlLineType::FixedActiveHigh {
                            "active-high"
                        } else {
                            "active-low"
                        },
                        line.name
                    ),
                ));
            }
            // Everything the guards above did not catch: a Configurable line
            // that was supplied, and a fixed line that was either omitted or
            // explicitly ignored.  All valid.
            ControlLineType::Configurable
            | ControlLineType::FixedActiveLow
            | ControlLineType::FixedActiveHigh => {}
        }
    }

    for (cs_name, user) in &cs_values {
        // On an oversized chip, cs1 names the excess top address line acting
        // as a half-select, not a control line - its absence from
        // control_lines() is expected, and check_cs_v2 requires it.
        if *cs_name == "cs1" && requires_half_select_cs1(chip_type) {
            continue;
        }
        if user.is_some() && !chip_type.control_lines().iter().any(|l| l.name == *cs_name) {
            return Err(Error::InvalidArgument(
                "--slot".to_string(),
                format!(
                    "Chip type {} has no {} line\n    --slot '{slot}'",
                    chip_type.name(),
                    cs_name
                ),
            ));
        }
    }

    Ok(())
}

/// Build a human-readable sorted list of chip type names supported by a board,
/// including plugins.
pub fn supported_chip_names_for_board(board: &Board) -> String {
    let mut names: Vec<&str> = board.supported_chip_type_names().to_vec();
    names.extend_from_slice(CHIP_TYPE_NAMES_PLUGINS);
    names.sort_unstable();
    names.join(", ")
}

/// Parse all `--slot` strings against a resolved board, returning a vec of
/// [`SlotSpec`] or the first error.
pub fn parse_slots(
    slots: &[String],
    board: &Board,
    allow_unsupported_chip_type: bool,
) -> Result<Vec<SlotSpec>, Error> {
    slots
        .iter()
        .map(|s| parse_slot(s, board, allow_unsupported_chip_type))
        .collect()
}

fn slot_to_chip_config(slot: &SlotSpec) -> ChipConfig {
    let mut chip = ChipConfig::new(
        slot.file.clone().unwrap_or_default(),
        slot.chip_type.clone(),
    );
    chip.cs1 = slot.cs1;
    chip.cs2 = slot.cs2;
    chip.cs3 = slot.cs3;
    chip.cs4 = slot.cs4;
    chip.size_handling = slot.size_handling.clone().unwrap_or_default();
    chip.label = slot.label.clone();
    chip.format = slot.format.unwrap_or_default();
    chip.load_address = slot.load_address.unwrap_or_default();
    chip.transform = slot.transform.clone();
    chip
}

fn slot_to_firmware_overrides(slot: &SlotSpec) -> Option<FirmwareConfig> {
    let has_fire = slot.cpu_freq.is_some() || slot.vreg.is_some() || slot.force_16bit.is_some();
    let has_led = slot.led.is_some();

    if !has_fire && !has_led {
        return None;
    }

    let fire = has_fire.then(|| FireConfig {
        cpu_freq: slot.cpu_freq,
        overclock: slot.cpu_freq.map(|f| f > FireCpuFreq::stock_value()),
        vreg: slot.vreg.clone(),
        force_16_bit: slot.force_16bit.unwrap_or(false),
        ..Default::default()
    });

    Some(FirmwareConfig {
        ice: None,
        fire,
        led: slot.led.map(|enabled| LedConfig { enabled }),
        swd: None,
        serve_alg_params: None,
    })
}

/// Generate a One ROM JSON configuration string from resolved plugins and
/// slot specs.
///
/// Plugin chip_sets are inserted first (system plugin at index 0, user plugin
/// at index 1).  ROM slot
/// chip_sets follow from index 0 or 2 onwards depending on how many plugins
/// are present.
pub fn slots_to_config_json(
    plugins: &[ResolvedPlugin],
    slots: &[SlotSpec],
    global_config: Option<&GlobalConfig>,
) -> Result<String, Error> {
    // Ensure system plugins alway come first
    let mut sorted_plugins: Vec<&ResolvedPlugin> = plugins.iter().collect();
    sorted_plugins.sort_by_key(|p| p.plugin_type.slot_index());

    let mut chip_sets: Vec<ChipSetConfig> = sorted_plugins
        .iter()
        .map(|p| plugin_to_chip_set_config(&p.file(), p.plugin_type, p.size))
        .collect::<Result<Vec<_>, _>>()?;

    for slot in slots {
        let mut chip_set = ChipSetConfig::new(ChipSetType::Single, vec![slot_to_chip_config(slot)]);
        chip_set.firmware_overrides = slot_to_firmware_overrides(slot);
        chip_sets.push(chip_set);
    }

    let description = global_config
        .and_then(|c| c.config_description.clone())
        .unwrap_or(DEFAULT_CONFIG_DESCRIPTION.to_string());
    let mut config = Config::new(description, chip_sets);
    config.name = global_config.and_then(|c| c.config_name.clone());
    config.instance_name = global_config.and_then(|c| c.instance_name.clone());
    config.serial_override = global_config.and_then(|c| c.serial_override.clone());
    config.boot_logging = global_config.is_some_and(|c| c.boot_logging.unwrap_or(false));
    config.swd_enabled = !global_config.is_some_and(|c| c.disable_swd.unwrap_or(false));
    config.turbo_boot = global_config.is_some_and(|c| c.turbo_boot.unwrap_or(false));

    serde_json::to_string_pretty(&config).map_err(|e| Error::Other(e.to_string()))
}

/// Inject resolved plugins into a user-provided config JSON string.
///
/// The plugins are prepended to the config's `chip_sets` so a system plugin
/// lands in slot 0 and a user plugin in slot 1 — the placement the firmware
/// builder requires — with the config's existing ROM slots shifting up
/// accordingly.
///
/// Returns the JSON unchanged if `plugins` is empty. Returns an error if the
/// config already defines a plugin of its own, since merging command-line
/// plugins with config-defined plugins is ambiguous: remove the plugin from the
/// config, or drop `--plugin`.
pub fn inject_plugins_into_config(
    json: String,
    plugins: &[ResolvedPlugin],
) -> Result<String, Error> {
    if plugins.is_empty() {
        return Ok(json);
    }

    let mut config: Config = serde_json::from_str(&json)
        .map_err(|e| Error::Other(format!("Failed to parse config JSON: {e}")))?;

    if config
        .chip_sets
        .iter()
        .flat_map(|cs| cs.chips.iter())
        .any(|c| c.chip_type.resolved().is_plugin())
    {
        return Err(Error::Other(
            "The provided config file already defines a plugin; remove it from \
             the config, or drop --plugin."
                .to_string(),
        ));
    }

    // Ensure system plugins come before user plugins (slot 0 then slot 1).
    let mut sorted_plugins: Vec<&ResolvedPlugin> = plugins.iter().collect();
    sorted_plugins.sort_by_key(|p| p.plugin_type.slot_index());

    let mut chip_sets: Vec<ChipSetConfig> = sorted_plugins
        .iter()
        .map(|p| plugin_to_chip_set_config(&p.file(), p.plugin_type, p.size))
        .collect::<Result<Vec<_>, _>>()?;

    // Prepend the plugin slots ahead of the config's existing ROM slots.
    chip_sets.append(&mut config.chip_sets);
    config.chip_sets = chip_sets;

    serde_json::to_string_pretty(&config).map_err(|e| Error::Other(e.to_string()))
}

/// Save a config JSON string to a file.
pub fn save_config(path: &str, json: &str) -> Result<(), Error> {
    std::fs::write(path, json).map_err(|e| Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{PluginType, PluginVersion, ResolvedSource};

    /// A single-ROM config using the canonical `chip_sets`/`chips` keys.
    const ROM_ONLY: &str = r#"{
        "version": 1,
        "name": "t",
        "description": "d",
        "chip_sets": [
            { "type": "single", "chips": [
                { "file": "http://x/rom.bin", "type": "23128",
                  "cs1": "active_low", "cs2": "active_low", "cs3": "active_high" }
            ] }
        ]
    }"#;

    fn plugin(plugin_type: PluginType) -> ResolvedPlugin {
        ResolvedPlugin {
            plugin_type,
            name: "p".to_string(),
            version: PluginVersion::new(0, 1, 0, 0),
            size: 1024,
            source: ResolvedSource::File {
                path: "/tmp/p.bin".to_string(),
            },
        }
    }

    fn chip_types(json: &str) -> Vec<ChipType> {
        let config: Config = serde_json::from_str(json).expect("valid config");
        config
            .chip_sets
            .iter()
            .flat_map(|cs| cs.chips.iter())
            .map(|c| c.chip_type.resolved())
            .collect()
    }

    #[test]
    fn slot_parses_ihex_format_and_load_address() {
        let board = Board::try_from_str("24-e").unwrap();
        let slot = parse_slot(
            "file=rom.hex,type=2364,cs1=active_low,format=ihex,load_address=$E000",
            &board,
            false,
        )
        .unwrap();
        assert_eq!(slot.format, Some(FileFormat::IntelHex));
        assert_eq!(slot.load_address, Some(LoadAddress(0xE000)));

        // The parsed spec carries the values through to the ChipConfig.
        let chip = slot_to_chip_config(&slot);
        assert_eq!(chip.format, FileFormat::IntelHex);
        assert_eq!(chip.load_address, LoadAddress(0xE000));
    }

    #[test]
    fn slot_defaults_to_binary_format() {
        let board = Board::try_from_str("24-e").unwrap();
        let slot = parse_slot("file=rom.bin,type=2364,cs1=active_low", &board, false).unwrap();
        assert_eq!(slot.format, None);
        assert_eq!(slot.load_address, None);
        assert_eq!(slot_to_chip_config(&slot).format, FileFormat::Binary);
    }

    #[test]
    fn slot_parses_a_single_transform() {
        let board = Board::try_from_str("24-e").unwrap();
        let slot = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,transform=swap_bytes",
            &board,
            false,
        )
        .unwrap();
        assert_eq!(slot.transform, vec![Transform::SwapBytes]);
        assert_eq!(
            slot_to_chip_config(&slot).transform,
            vec![Transform::SwapBytes]
        );
    }

    #[test]
    fn slot_accepts_the_trans_key_alias() {
        let board = Board::try_from_str("24-e").unwrap();
        for key in ["transform", "trans"] {
            let slot = parse_slot(
                &format!("file=rom.bin,type=2364,cs1=active_low,{key}=swap_bytes"),
                &board,
                false,
            )
            .unwrap_or_else(|e| panic!("slot key '{key}' rejected: {e}"));
            assert_eq!(slot.transform, vec![Transform::SwapBytes]);
        }
    }

    #[test]
    fn slot_parses_a_transform_list_in_order() {
        let board = Board::try_from_str("24-e").unwrap();
        let slot = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,transform=deinterleave:1/2/2+swap_bytes",
            &board,
            false,
        )
        .unwrap();
        assert_eq!(
            slot.transform,
            vec![
                Transform::Deinterleave {
                    offset: 1,
                    stride: 2,
                    bytes: 2
                },
                Transform::SwapBytes,
            ]
        );
    }

    #[test]
    fn slot_transform_unit_defaults_to_one() {
        let board = Board::try_from_str("24-e").unwrap();
        let slot = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,transform=deinterleave:0/4",
            &board,
            false,
        )
        .unwrap();
        assert_eq!(
            slot.transform,
            vec![Transform::Deinterleave {
                offset: 0,
                stride: 4,
                bytes: 1
            }]
        );
    }

    #[test]
    fn slot_defaults_to_no_transform() {
        let board = Board::try_from_str("24-e").unwrap();
        let slot = parse_slot("file=rom.bin,type=2364,cs1=active_low", &board, false).unwrap();
        assert!(slot.transform.is_empty());
        assert!(slot_to_chip_config(&slot).transform.is_empty());
    }

    #[test]
    fn slot_rejects_a_bad_transform() {
        let board = Board::try_from_str("24-e").unwrap();
        let err = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,transform=nonsense",
            &board,
            false,
        )
        .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("unknown transform 'nonsense'"), "{msg}");
        // The error lists what is accepted, and echoes the offending slot.
        assert!(msg.contains("swap_bytes"), "{msg}");
        assert!(msg.contains("--slot 'file=rom.bin"), "{msg}");
    }

    #[test]
    fn slot_rejects_a_duplicate_transform_key() {
        let board = Board::try_from_str("24-e").unwrap();
        let err = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,transform=swap_bytes,transform=swap_bytes",
            &board,
            false,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("Duplicate slot key 'transform'"));
    }

    #[test]
    fn slot_load_address_requires_ihex() {
        let board = Board::try_from_str("24-e").unwrap();
        let err = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,load_address=0x100",
            &board,
            false,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("load_address is only valid with format=ihex"));
    }

    #[test]
    fn empty_plugins_is_passthrough() {
        let out = inject_plugins_into_config(ROM_ONLY.to_string(), &[]).unwrap();
        assert_eq!(out, ROM_ONLY);
    }

    #[test]
    fn plugins_are_prepended_system_then_user() {
        // Supply the plugins out of order to prove sorting, not input order,
        // decides placement: system must precede user, both ahead of the ROM.
        let out = inject_plugins_into_config(
            ROM_ONLY.to_string(),
            &[plugin(PluginType::User), plugin(PluginType::System)],
        )
        .unwrap();
        assert_eq!(
            chip_types(&out),
            vec![
                ChipType::SystemPlugin,
                ChipType::UserPlugin,
                ChipType::Chip23128,
            ]
        );
    }

    #[test]
    fn system_only_prepends_before_rom() {
        let out = inject_plugins_into_config(ROM_ONLY.to_string(), &[plugin(PluginType::System)])
            .unwrap();
        assert_eq!(
            chip_types(&out),
            vec![ChipType::SystemPlugin, ChipType::Chip23128]
        );
    }

    #[test]
    fn config_already_defining_a_plugin_is_rejected() {
        let with_plugin = r#"{
            "version": 1,
            "name": "t",
            "description": "d",
            "chip_sets": [
                { "type": "single", "chips": [
                    { "file": "http://x/usb.bin", "type": "system_plugin" } ] },
                { "type": "single", "chips": [
                    { "file": "http://x/rom.bin", "type": "23128",
                      "cs1": "active_low", "cs2": "active_low", "cs3": "active_high" } ] }
            ]
        }"#;
        let err =
            inject_plugins_into_config(with_plugin.to_string(), &[plugin(PluginType::System)])
                .unwrap_err();
        assert!(err.to_string().contains("already defines a plugin"));
    }
}
