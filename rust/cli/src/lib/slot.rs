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
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::{Board, Model};
use onerom_gen::MIN_SUPPORTED_FIRMWARE_VERSION_V2;
use onerom_gen::compat::{check_chip_set_on_board, default_cs_config, supported_chips};
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
    }
}

/// Parse slot strings and check whether any require user confirmation.
///
/// Slots are parsed purely for validation and confirmation checking.
/// The caller should prompt as needed before proceeding.
pub fn check_slot_confirmations(
    slots: &[String],
    board: &Board,
) -> Result<ConfirmationsRequired, Error> {
    Ok(check_confirmations(&parse_slots(slots, board)?))
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

/// Parse a CS logic value.
///
/// Delegates to [`CsLogic::try_from_str`], so `--slot` accepts exactly what a
/// config file's chip-select field does - including `ignore`, which a 2332 or
/// 2316 needs for a line One ROM does not monitor and which was previously
/// expressible only in a config file. Whether `ignore` is legal for the chip in
/// hand is settled downstream, by `validate_cs_lines` and `allow_cs_ignore`.
fn parse_cs_logic(slot: &str, key: &str, value: &str) -> Result<CsLogic, Error> {
    CsLogic::try_from_str(value).ok_or_else(|| {
        let supported = CsLogic::supported_values()
            .iter()
            .map(|v| v.name())
            .collect::<Vec<_>>()
            .join("|");
        Error::InvalidArgument(
            "--slot".to_string(),
            format!(
                "Invalid CS logic '{value}': expected {key}={supported}|0|1\n   --slot '{slot}'"
            ),
        )
    })
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
                "Invalid size-handling '{value}'\n    --slot '{slot}'\n  Supported values: {supported_variants}"
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

/// The slot keys advertised when an unrecognised one is given.
///
/// Spelled kebab-case, matching the CLI's own convention for argument names.
/// The parser also accepts the snake_case spelling of every key, so a config
/// file's JSON key can be pasted straight into a `--slot` without translation;
/// those are not listed here because listing every accepted spelling would bury
/// the keys themselves. `size` is the exception: it is a genuine short form
/// rather than a re-spelling, and is the one most people reach for.
const SLOT_KEYS: &[&str] = &[
    "file",
    "label",
    "type",
    "cs1",
    "cs2",
    "cs3",
    "cs4",
    "size-handling",
    "size",
    "cpu-freq",
    "cpu-vreg",
    "led",
    "force-16-bit",
    "format",
    "load-address",
    "transform",
];

/// Parse a single `--slot` string into a [`SlotSpec`], validating against the given board.
fn parse_slot(slot: &str, board: &Board) -> Result<SlotSpec, Error> {
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
            "size-handling" | "size_handling" | "size" => {
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
        let supported = candidate_chip_names_for_board(board);
        Error::UnsupportedChipType(chip_type_str.clone(), supported)
    })?;

    // Whether the board can *serve* this chip type depends on which firmware
    // is being built for, which is not known at parse time. See
    // [`check_slot_chip_types`], applied once the version is resolved.

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
            format!("force-16-bit is only valid on 40-pin boards\n    --slot '{slot}'"),
        ));
    }

    // A load address only makes sense for an Intel HEX image.
    if load_address.is_some() && format != Some(FileFormat::IntelHex) {
        return Err(Error::InvalidArgument(
            "--slot".to_string(),
            format!("load-address is only valid with format=ihex\n    --slot '{slot}'"),
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

/// The chip types `board` can emulate, in the order [`supported_chips`] ranks
/// them: native first, then overhang, then fly-lead.
///
/// Wider than [`Board::supported_chip_type_names`], which covers only the
/// board's own pin count. This is the set `--slot` accepts and the set
/// `firmware chips --board` lists, so the gate and the listing cannot disagree.
pub fn emulatable_chip_names(board: &Board) -> Vec<&'static str> {
    supported_chips(*board, ChipSetType::Single, 1)
        .iter()
        .map(|e| e.alias)
        .collect()
}

/// Every chip type name `board` might take, under either builder, for the
/// "that is not a chip type" error.
///
/// A name that resolves to no chip type at all is a typo, and the useful hint
/// is everything the board could plausibly accept. Which of them the *target*
/// firmware can actually serve is settled by [`check_slot_chip_types`], which
/// is version-aware and names the right subset when it rejects one.
fn candidate_chip_names_for_board(board: &Board) -> String {
    let mut names: Vec<&'static str> = board.supported_chip_type_names().to_vec();
    names.extend(board.extra_chip_types().iter().map(|c| c.name()));
    if board.model() == Model::Fire {
        names.extend(emulatable_chip_names(board));
    }
    names.extend_from_slice(CHIP_TYPE_NAMES_PLUGINS);
    names.sort_unstable();
    names.dedup();
    names.join(", ")
}

/// The chip type names `board` accepts in a `--slot` built for `version`,
/// including plugins, as a comma-separated list for error messages.
///
/// Follows the same V1/V2 split as [`check_slot_chip_types`], so the list a
/// rejection offers is the list that rejection was made against.
pub fn supported_chip_names_for_board(board: &Board, version: &FirmwareVersion) -> String {
    let mut names = if serves_v2(version) {
        emulatable_chip_names(board)
    } else {
        let mut v1: Vec<&'static str> = board.supported_chip_type_names().to_vec();
        // `supported_chip_type_names` is per pin count, so the board's extra
        // types - the whole point of the V1 gate - are not in it.
        v1.extend(board.extra_chip_types().iter().map(|c| c.name()));
        v1.sort_unstable();
        v1.dedup();
        v1
    };
    names.extend_from_slice(CHIP_TYPE_NAMES_PLUGINS);
    names.join(", ")
}

/// Whether `version` is served by the V2 builder.
fn serves_v2(version: &FirmwareVersion) -> bool {
    *version >= MIN_SUPPORTED_FIRMWARE_VERSION_V2
}

/// Reject any slot whose chip type the target firmware cannot serve on `board`.
///
/// The two builders decide this differently, so the gate has to know which one
/// will run:
///
/// - **V2** (0.7.0 and later) derives the address and CS/data layouts, which
///   admits every overhang and fly-lead combination `docs/COMPATIBILITY.md`
///   documents, and refuses chip types no v2 firmware serves. A `--slot`
///   becomes a one-chip Single set, so this is the same derivation the build
///   will run. `default_cs_config` stands in for the slot's own polarities:
///   every configurable line must have been supplied ([`parse_slots`]) and the
///   fixed ones come from the silicon, so the two differ only in polarity -
///   which places a line on the same GPIO either way.
/// - **V1** (pre-0.7.0) serves a fixed set of chip types per board, so
///   `Board::allows_chip_type` is the gate, exactly as `build_v1` applies it.
///
/// Plugins have no ROM layout to derive and are validated by the builder.
pub fn check_slot_chip_types(
    slots: &[SlotSpec],
    board: &Board,
    version: &FirmwareVersion,
) -> Result<(), Error> {
    // V2 firmware exists only for Fire (RP2350) boards, so its layout
    // derivation would reject every chip type on an Ice board and offer an
    // empty list of alternatives. The build and program commands refuse an Ice
    // board long before this point; say so here too rather than let a direct
    // caller of this library see that nonsense.
    if serves_v2(version) && board.model() != Model::Fire {
        return Err(Error::IceBoardUnsupported(board.name().to_string()));
    }

    for slot in slots {
        let chip_type = slot.chip_type.resolved();
        if chip_type.is_plugin() {
            continue;
        }
        let servable = if serves_v2(version) {
            check_chip_set_on_board(
                *board,
                chip_type,
                ChipSetType::Single,
                1,
                default_cs_config(chip_type),
            )
            .is_ok()
        } else {
            board.allows_chip_type(chip_type)
        };
        if !servable {
            return Err(Error::UnsupportedBoardChipType(
                chip_type.name().to_string(),
                chip_type.aliases().join(", "),
                supported_chip_names_for_board(board, version),
            ));
        }
    }

    Ok(())
}

/// Parse all `--slot` strings against a resolved board, returning a vec of
/// [`SlotSpec`] or the first error.
///
/// Validates each slot's syntax, chip type name, CS lines and board electrical
/// constraints. Whether the target firmware can *serve* those chip types is a
/// separate, version-dependent question - see [`check_slot_chip_types`].
pub fn parse_slots(slots: &[String], board: &Board) -> Result<Vec<SlotSpec>, Error> {
    slots.iter().map(|s| parse_slot(s, board)).collect()
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

    /// The last firmware the V1 builder serves, and the first the V2 builder
    /// does. Both gates are exercised against a real released version.
    const V1: FirmwareVersion = FirmwareVersion::new(0, 6, 14, 0);
    const V2: FirmwareVersion = MIN_SUPPORTED_FIRMWARE_VERSION_V2;

    /// Run the chip type gate over one slot spec, as the build path does.
    fn gate(board: &str, slot: &str, version: &FirmwareVersion) -> Result<(), Error> {
        let board = Board::try_from_str(board).unwrap();
        let parsed = parse_slots(&[slot.to_string()], &board).expect("slot parses");
        check_slot_chip_types(&parsed, &board, version)
    }

    /// V2 gates on the serving layout, not the board's pin count: a 28-pin chip
    /// reached by a fly-lead to X1 is servable on a 24-pin board, exactly as
    /// `docs/COMPATIBILITY.md` lists it. V1 has no such layout and refuses it -
    /// `fire-24-*` carries no `extra_chip_types` at all - which is why the gate
    /// cannot be applied without knowing the target firmware.
    #[test]
    fn a_fly_lead_chip_is_servable_only_under_v2() {
        assert!(gate("fire-24-a", "file=rom.bin,type=2764", &V2).is_ok());
        assert!(gate("fire-24-a", "file=rom.bin,type=2764", &V1).is_err());
    }

    /// Likewise for overhang. The 28-pin boards' `extra_chip_types` name five
    /// 24-pin types, so V1 serves exactly those; V2 places every 24-pin type
    /// the board can reach, and `28C16` is outside that list.
    #[test]
    fn an_overhang_chip_outside_the_extras_list_is_servable_only_under_v2() {
        assert!(gate("fire-28-a", "file=rom.bin,type=28C16", &V2).is_ok());
        assert!(gate("fire-28-a", "file=rom.bin,type=28C16", &V1).is_err());
    }

    /// And the other way round: a 24-pin SRAM is in V1's per-board set but no
    /// V2 firmware serves it yet, so building one against 0.6.x must keep
    /// working while 0.7.x refuses it up front.
    ///
    /// This flips on its own the day `Chip6116` joins `SUPPORTED_CHIP_TYPES_V2`
    /// - the expectation is read from that list rather than written down here.
    #[test]
    fn sram_is_servable_under_v1_and_tracks_the_v2_chip_list() {
        assert!(gate("fire-24-a", "type=6116", &V1).is_ok());
        assert_eq!(
            gate("fire-24-a", "type=6116", &V2).is_ok(),
            onerom_gen::SUPPORTED_CHIP_TYPES_V2.contains(&ChipType::Chip6116),
        );
    }

    /// A chip no 24-pin board can reach is refused by either builder, and the
    /// error names what that builder does serve - so the alternatives offered
    /// are ones that would actually have worked.
    #[test]
    fn an_unservable_chip_is_refused_with_the_right_alternatives() {
        let err = gate("fire-24-a", "file=rom.bin,type=27C400", &V2).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("cannot serve chip types 27C400"), "{msg}");
        // V2 offers the fly-lead types alongside the native ones.
        assert!(msg.contains("2364"), "{msg}");
        assert!(msg.contains("2764"), "{msg}");

        // V1 offers only what V1 serves, so no fly-lead type appears.
        let msg = format!(
            "{}",
            gate("fire-24-a", "file=rom.bin,type=27C400", &V1).unwrap_err()
        );
        assert!(msg.contains("2364"), "{msg}");
        assert!(!msg.contains("2764"), "{msg}");
    }

    /// Plugins have no ROM layout to derive, so neither gate may eat them - the
    /// builder validates plugin slots itself.
    #[test]
    fn a_plugin_chip_type_passes_both_gates() {
        for version in [&V1, &V2] {
            assert!(gate("fire-24-a", "file=plugin.bin,type=SystemPlugin", version).is_ok());
        }
    }

    /// V2 firmware exists only for Fire boards, so its layout has nothing to
    /// say about an Ice one: the answer is the same "not supported" the build
    /// command gives up front, not a chip-type error listing no alternatives.
    /// V1 firmware is what an Ice board runs, so that gate still applies.
    #[test]
    fn an_ice_board_is_refused_by_the_v2_gate_but_served_by_v1() {
        let err = gate("ice-24-e", "file=rom.bin,type=2364,cs1=active_low", &V2).unwrap_err();
        assert!(matches!(err, Error::IceBoardUnsupported(_)), "{err}");
        assert!(gate("ice-24-e", "file=rom.bin,type=2364,cs1=active_low", &V1).is_ok());
    }

    #[test]
    fn slot_parses_ihex_format_and_load_address() {
        let board = Board::try_from_str("fire-24-e").unwrap();
        let slot = parse_slot(
            "file=rom.hex,type=2364,cs1=active_low,format=ihex,load_address=$E000",
            &board,
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
        let board = Board::try_from_str("fire-24-e").unwrap();
        let slot = parse_slot("file=rom.bin,type=2364,cs1=active_low", &board).unwrap();
        assert_eq!(slot.format, None);
        assert_eq!(slot.load_address, None);
        assert_eq!(slot_to_chip_config(&slot).format, FileFormat::Binary);
    }

    /// Both spellings of a chip-select value reach the same variant.
    ///
    /// The config file writes `active_low`, the CLI's own convention is
    /// `active-low`, and a user moving between the two should not have to
    /// translate. Asserting the parsed variant rather than just success, so a
    /// spelling that silently landed on the wrong polarity would fail.
    #[test]
    fn slot_accepts_both_spellings_of_a_cs_value() {
        let board = Board::try_from_str("fire-24-e").unwrap();
        for (spelling, expected) in [
            ("active_low", CsLogic::ActiveLow),
            ("active-low", CsLogic::ActiveLow),
            ("ACTIVE-LOW", CsLogic::ActiveLow),
            ("0", CsLogic::ActiveLow),
            ("active_high", CsLogic::ActiveHigh),
            ("active-high", CsLogic::ActiveHigh),
            ("1", CsLogic::ActiveHigh),
        ] {
            let slot = parse_slot(&format!("file=rom.bin,type=2364,cs1={spelling}"), &board)
                .unwrap_or_else(|e| panic!("cs1={spelling} rejected: {e}"));
            assert_eq!(slot.cs1, Some(expected), "cs1={spelling}");
        }
    }

    /// `ignore` parses on the command line, as it always has in a config file.
    ///
    /// It is not a polarity - it says One ROM does not monitor the line - so
    /// whether a given chip may use it is settled downstream by
    /// `allow_cs_ignore`. What matters here is that the *parser* no longer
    /// rejects it as an invalid value, which made the config-file-only.
    #[test]
    fn slot_accepts_the_ignore_cs_value() {
        let board = Board::try_from_str("fire-24-e").unwrap();
        let slot = parse_slot("file=rom.bin,type=2332,cs1=active-low,cs2=ignore", &board)
            .expect("cs2=ignore should parse");
        assert_eq!(slot.cs2, Some(CsLogic::Ignore));
    }

    /// A bad value is still refused, and the message lists what is accepted.
    #[test]
    fn slot_rejects_an_unknown_cs_value_and_names_the_alternatives() {
        let board = Board::try_from_str("fire-24-e").unwrap();
        let err = parse_slot("file=rom.bin,type=2364,cs1=sideways", &board).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("sideways"), "{msg}");
        for value in CsLogic::supported_values() {
            assert!(
                msg.contains(value.name()),
                "{} missing from: {msg}",
                value.name()
            );
        }
    }

    #[test]
    fn slot_parses_a_single_transform() {
        let board = Board::try_from_str("fire-24-e").unwrap();
        let slot = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,transform=swap_bytes",
            &board,
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
        let board = Board::try_from_str("fire-24-e").unwrap();
        for key in ["transform", "trans"] {
            let slot = parse_slot(
                &format!("file=rom.bin,type=2364,cs1=active_low,{key}=swap_bytes"),
                &board,
            )
            .unwrap_or_else(|e| panic!("slot key '{key}' rejected: {e}"));
            assert_eq!(slot.transform, vec![Transform::SwapBytes]);
        }
    }

    #[test]
    fn slot_parses_a_transform_list_in_order() {
        let board = Board::try_from_str("fire-24-e").unwrap();
        let slot = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,transform=deinterleave:1/2/2+swap_bytes",
            &board,
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
        let board = Board::try_from_str("fire-24-e").unwrap();
        let slot = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,transform=deinterleave:0/4",
            &board,
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
        let board = Board::try_from_str("fire-24-e").unwrap();
        let slot = parse_slot("file=rom.bin,type=2364,cs1=active_low", &board).unwrap();
        assert!(slot.transform.is_empty());
        assert!(slot_to_chip_config(&slot).transform.is_empty());
    }

    #[test]
    fn slot_rejects_a_bad_transform() {
        let board = Board::try_from_str("fire-24-e").unwrap();
        let err = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,transform=nonsense",
            &board,
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
        let board = Board::try_from_str("fire-24-e").unwrap();
        let err = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,transform=swap_bytes,transform=swap_bytes",
            &board,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("Duplicate slot key 'transform'"));
    }

    #[test]
    fn slot_load_address_requires_ihex() {
        let board = Board::try_from_str("fire-24-e").unwrap();
        let err = parse_slot(
            "file=rom.bin,type=2364,cs1=active_low,load_address=0x100",
            &board,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("load-address is only valid with format=ihex"));
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
