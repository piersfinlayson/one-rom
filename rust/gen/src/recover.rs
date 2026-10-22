// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Recovering the config an image was built from, out of its metadata.
//!
//! The recovered config builds an identical image, with each chip's bytes as
//! its file. The image doesn't record the config's name, descriptions, notes,
//! categories or licences, so the recovered config doesn't have them.
//!
//! Each chip has:
//!
//! - an empty `file`, for the caller to fill in
//! - its recorded filename as its `label`, so the rebuilt metadata records the
//!   same filename
//! - no `size_handling`, `transform`, `location`, `extract` or `format`. The
//!   bytes are already transformed, so these would apply twice.
//!
//! # Chip selects
//!
//! An image stores a slot's chip-select settings only as its chip-select
//! window and GPIO input overrides. The slot builder runs for every
//! chip-select setting the chip types allow, and the first setting whose
//! window and overrides match the image's is kept. A setting that passes
//! config validation without `allow_cs_ignore` comes before one that requires
//! it. A slot no setting matches is reported, and its chips' chip-select
//! settings are left unset. This happens where the onerom-gen that built the
//! image encoded chip selects differently.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::hw::Board;
use onerom_config::pin::{Pin, ReservedPins};
use onerom_metadata::{
    FireVreg as MetadataFireVreg, MaybeKnown, OneromAlgAddrConfig, OneromAlgConfig,
    OneromAlgCsConfig, OneromAlgDataConfig, OneromAlgDmaConfig, OneromFirmwareOverrides,
    OneromMetadataHeader, OneromRomInfo, OneromRomSlot, OverrideState, RomSlotType,
    addr_mon_min_cs_act_to_ns,
};

use crate::builder::check_cs_v2;
use crate::v2::rom_slot::build_rom_slot;
use crate::{
    Chip, ChipConfig, ChipSetConfig, ChipSetType, ChipTypeSpec, Config, CsConfig, CsLogic,
    DebugConfig, FireConfig, FireCpuFreq, FireVreg, FirmwareConfig, LedConfig, UnscrambleError,
    requires_half_select_cs1, unscramble_slot,
};

/// A config recovered from an image, with each chip's bytes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RecoveredConfig {
    /// The config. Every chip's `file` is empty.
    pub config: Config,

    /// Each chip's bytes, indexed as `config.chip_sets[set].chips[chip]`.
    pub data: Vec<ChipData>,

    /// Settings the config couldn't recover, in slot order.
    pub problems: Vec<RecoveryProblem>,
}

/// Part of an image's config that couldn't be recovered. The rest of the
/// config is recovered.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecoveryProblem {
    /// No chip-select setting builds the slot's chip-select window and
    /// overrides. The slot's chips don't have chip-select settings.
    ChipSelect {
        /// The slot's chip set, counting plugins.
        set: usize,
    },

    /// A firmware override this build can't express, which the recovered
    /// config leaves out.
    Override {
        /// The slot's chip set, counting plugins.
        set: usize,
        /// The override's config name.
        name: &'static str,
    },
}

impl fmt::Display for RecoveryProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChipSelect { set } => {
                write!(f, "Chip set {set}'s chip-select settings weren't recovered")
            }
            Self::Override { set, name } => {
                write!(f, "Chip set {set}'s {name} override wasn't recovered")
            }
        }
    }
}

/// A slot of an image, numbered as in `onerom inspect slots`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SlotName {
    /// A ROM slot, numbered from 0 without plugins.
    Rom(usize),
    /// The system plugin's slot.
    SystemPlugin,
    /// The user plugin's slot.
    UserPlugin,
    /// The PIO plugin's slot.
    PioPlugin,
}

impl SlotName {
    /// Slot `index` of `slots`, counting plugins.
    pub fn of(slots: &[OneromRomSlot], index: usize) -> Self {
        match slots.get(index).and_then(plugin_slot) {
            Some(name) => name,
            None => Self::Rom(
                slots
                    .iter()
                    .take(index)
                    .filter(|slot| plugin_slot(slot).is_none())
                    .count(),
            ),
        }
    }
}

/// The name of `slot` where it's a plugin's. `None` for a ROM slot.
fn plugin_slot(slot: &OneromRomSlot) -> Option<SlotName> {
    match slot.slot_type {
        MaybeKnown::Known(RomSlotType::RomSlotTypePluginSystem) => Some(SlotName::SystemPlugin),
        MaybeKnown::Known(RomSlotType::RomSlotTypePluginUser) => Some(SlotName::UserPlugin),
        MaybeKnown::Known(RomSlotType::RomSlotTypePluginPio) => Some(SlotName::PioPlugin),
        MaybeKnown::Known(
            RomSlotType::RomSlotTypeSingleRom
            | RomSlotType::RomSlotTypeBankedRom
            | RomSlotType::RomSlotTypeMultiRom
            | RomSlotType::RomSlotTypeSingleRam,
        )
        | MaybeKnown::Unknown(_) => None,
    }
}

impl fmt::Display for SlotName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rom(slot) => write!(f, "Slot {slot}"),
            Self::SystemPlugin => f.write_str("The system plugin"),
            Self::UserPlugin => f.write_str("The user plugin"),
            Self::PioPlugin => f.write_str("The PIO plugin"),
        }
    }
}

/// Why an image's config can't be recovered.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecoverError {
    /// The metadata's board isn't recognised.
    UnknownBoard {
        /// The board name the metadata records.
        name: String,
    },

    /// The number of slots' data passed isn't the number of slots.
    SlotCount {
        /// The metadata's slots.
        expected: usize,
        /// The data passed.
        actual: usize,
    },

    /// A slot with an image whose data wasn't passed.
    MissingData {
        /// The slot.
        slot: SlotName,
    },

    /// The metadata's chip type isn't recognised.
    UnknownChipType {
        /// The slot.
        slot: SlotName,
        /// The chip type the metadata records.
        name: String,
    },

    /// A slot's data doesn't unscramble.
    Unscramble {
        /// The slot.
        slot: SlotName,
        /// Why.
        error: UnscrambleError,
    },

    /// A value recovery reads is unrecognised. Newer firmware writes these.
    UnknownValue {
        /// The slot.
        slot: SlotName,
        /// The field, by its config name where it has one.
        field: &'static str,
    },
}

impl fmt::Display for RecoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownBoard { name } => write!(f, "The board '{name}' isn't recognised"),
            Self::SlotCount { expected, actual } => {
                write!(
                    f,
                    "Data for {actual} slots was read, but there are {expected}"
                )
            }
            Self::MissingData { slot } => write!(f, "{slot}'s data wasn't read"),
            Self::UnknownChipType { slot, name } => {
                write!(f, "{slot}'s chip type '{name}' isn't recognised")
            }
            Self::Unscramble { slot, error } => write!(f, "{slot}: {error}"),
            Self::UnknownValue { slot, field } => {
                write!(f, "{slot}'s {field} value isn't recognised")
            }
        }
    }
}

/// Recovers the config `header` was built from.
///
/// `slots` is each slot's stored data, in slot order, read from where the slot
/// points. `None` for a slot without an image.
pub fn recover_config(
    header: &OneromMetadataHeader,
    slots: &[Option<Vec<u8>>],
) -> Result<RecoveredConfig, RecoverError> {
    check_known(header)?;
    let board =
        Board::try_from_str(&header.hw.hw_rev).ok_or_else(|| RecoverError::UnknownBoard {
            name: header.hw.hw_rev.clone(),
        })?;
    if slots.len() != header.rom_slots.len() {
        return Err(RecoverError::SlotCount {
            expected: header.rom_slots.len(),
            actual: slots.len(),
        });
    }

    let mut chip_sets = Vec::with_capacity(slots.len());
    let mut data = Vec::with_capacity(slots.len());
    let mut problems = Vec::new();
    for (set, (slot, stored)) in header.rom_slots.iter().zip(slots).enumerate() {
        let (chip_set, chips) = recover_slot(board, set, header, slot, stored, &mut problems)?;
        chip_sets.push(chip_set);
        data.push(chips);
    }

    let mut config = Config::new(String::new(), chip_sets);
    config.instance_name = header.fw.name.clone();
    config.serial_override = header.fw.serial_number.clone();
    config.boot_logging = header.boot_logging != 0;
    config.swd_enabled = header.swd_enabled != 0;
    config.turbo_boot = header.turbo_boot != 0;
    config.addr_mon_min_cs_act_ns = addr_mon_min_cs_act_to_ns(header.addr_mon_min_cs_act);
    config.reserved_pins =
        ReservedPins::from_bits(header.reserved_sel_pins, header.reserved_x_pins)
            .pins()
            .map(Pin::Header)
            .collect();

    Ok(RecoveredConfig {
        config,
        data,
        problems,
    })
}

/// Fails with [`RecoverError::UnknownValue`] where a value
/// [`recover_config`] reads from `header` is unrecognised.
pub fn check_known(header: &OneromMetadataHeader) -> Result<(), RecoverError> {
    for (index, slot) in header.rom_slots.iter().enumerate() {
        if let Some(field) = unknown_field(slot) {
            return Err(RecoverError::UnknownValue {
                slot: SlotName::of(&header.rom_slots, index),
                field,
            });
        }
    }
    Ok(())
}

/// The first field recovery reads from `slot` with an unrecognised value.
fn unknown_field(slot: &OneromRomSlot) -> Option<&'static str> {
    if !slot.slot_type.is_known() {
        return Some("slot_type");
    }
    if plugin_slot(slot).is_none()
        && let Some(alg) = &slot.alg
    {
        if matches!(alg.alg_cs, OneromAlgCsConfig::Unknown { .. }) {
            return Some("alg_cs");
        }
        if matches!(alg.alg_addr, OneromAlgAddrConfig::Unknown { .. }) {
            return Some("alg_addr");
        }
        if matches!(alg.alg_data, OneromAlgDataConfig::Unknown { .. }) {
            return Some("alg_data");
        }
        match &alg.alg_dma {
            OneromAlgDmaConfig::AlgDma0 { bit_mode, .. } if bit_mode.is_known() => {}
            OneromAlgDmaConfig::AlgDma0 { .. } => return Some("bit_mode"),
            OneromAlgDmaConfig::Unknown { .. } => return Some("alg_dma"),
        }
    }
    let overrides = slot.firmware_overrides.as_ref()?;
    if overrides.vreg().is_some_and(|vreg| !vreg.is_known()) {
        return Some("vreg");
    }
    if overrides
        .standby()
        .is_some_and(|standby| !standby.is_known())
    {
        return Some("standby");
    }
    None
}

/// Each chip's bytes in one chip set, `None` for a chip without an image.
pub type ChipData = Vec<Option<Vec<u8>>>;

/// One slot's chip set and each chip's bytes.
fn recover_slot(
    board: Board,
    set: usize,
    header: &OneromMetadataHeader,
    slot: &OneromRomSlot,
    stored: &Option<Vec<u8>>,
    problems: &mut Vec<RecoveryProblem>,
) -> Result<(ChipSetConfig, ChipData), RecoverError> {
    let name = SlotName::of(&header.rom_slots, set);
    let specs: Vec<ChipTypeSpec> = slot
        .roms
        .iter()
        .map(|rom| chip_type_spec(rom).ok_or_else(|| unknown_chip_type(name, rom)))
        .collect::<Result<_, _>>()?;
    let set_type = match slot.slot_type {
        MaybeKnown::Known(RomSlotType::RomSlotTypeBankedRom) => ChipSetType::Banked,
        MaybeKnown::Known(RomSlotType::RomSlotTypeMultiRom) => ChipSetType::Multi,
        MaybeKnown::Known(
            RomSlotType::RomSlotTypeSingleRom
            | RomSlotType::RomSlotTypeSingleRam
            | RomSlotType::RomSlotTypePluginSystem
            | RomSlotType::RomSlotTypePluginUser
            | RomSlotType::RomSlotTypePluginPio,
        ) => ChipSetType::Single,
        MaybeKnown::Unknown(raw) => {
            return Err(RecoverError::Unscramble {
                slot: name,
                error: UnscrambleError::UnknownSlotType(raw),
            });
        }
    };

    let chips_data: ChipData = match (slot.data.is_null(), stored) {
        (true, _) => vec![None; slot.roms.len()],
        (false, None) => return Err(RecoverError::MissingData { slot: name }),
        (false, Some(stored)) => unscramble_slot(&header.hw, slot, stored)
            .map_err(|error| RecoverError::Unscramble { slot: name, error })?
            .into_iter()
            .map(Some)
            .collect(),
    };

    let mut overrides = slot
        .firmware_overrides
        .as_ref()
        .map(|overrides| firmware_config(set, overrides, problems));

    let is_plugin = specs.iter().any(|spec| spec.resolved().is_plugin());
    let selects = match &slot.alg {
        Some(alg) if !is_plugin => {
            let force_16_bit: &[bool] =
                if overrides.is_some() && specs[0].resolved().supports_bit_mode(16) {
                    &[false, true]
                } else {
                    &[false]
                };
            search_chip_selects(board, set_type, &specs, force_16_bit, alg)
        }
        Some(_) | None => None,
    };
    let selects = match selects {
        Some((selects, force_16_bit)) => {
            if force_16_bit && let Some(overrides) = overrides.as_mut() {
                overrides
                    .fire
                    .get_or_insert_with(FireConfig::default)
                    .force_16_bit = true;
            }
            Some(selects)
        }
        None if is_plugin => None,
        None => {
            problems.push(RecoveryProblem::ChipSelect { set });
            None
        }
    };

    let chips = specs
        .into_iter()
        .zip(&slot.roms)
        .zip(&chips_data)
        .enumerate()
        .map(|(index, ((spec, rom), data))| {
            let mut chip = ChipConfig::new(String::new(), spec);
            if data.is_some() {
                chip.label = Some(rom.filename.clone().unwrap_or_default());
            }
            if let Some(select) = selects.as_ref().map(|selects| selects[index]) {
                select.apply(&mut chip);
            }
            chip
        })
        .collect();
    let mut chip_set = ChipSetConfig::new(set_type, chips);
    chip_set.firmware_overrides = overrides;
    Ok((chip_set, chips_data))
}

/// The chip type a ROM records, resolved as a config's `type` is, with its
/// spelling kept.
fn chip_type_spec(rom: &OneromRomInfo) -> Option<ChipTypeSpec> {
    serde_json::from_value(serde_json::Value::String(rom.rom_type.clone()))
        .ok()
        .or_else(|| ChipType::try_from_rbcp_u8(rom.rbcp_rom_type).map(ChipTypeSpec::from))
}

fn unknown_chip_type(slot: SlotName, rom: &OneromRomInfo) -> RecoverError {
    RecoverError::UnknownChipType {
        slot,
        name: rom.rom_type.clone(),
    }
}

/// The per-slot firmware overrides `overrides` records, other than
/// `force_16_bit`, which the slot's serving algorithms record.
fn firmware_config(
    set: usize,
    overrides: &OneromFirmwareOverrides,
    problems: &mut Vec<RecoveryProblem>,
) -> FirmwareConfig {
    let mut problem = |name| problems.push(RecoveryProblem::Override { set, name });
    let mut fire = FireConfig::default();
    if let Some(freq) = overrides.cpu_freq() {
        match freq {
            FireCpuFreq::NONE => fire.cpu_freq = Some(FireCpuFreq::none()),
            FireCpuFreq::STOCK => fire.cpu_freq = Some(FireCpuFreq::stock()),
            mhz => match FireCpuFreq::mhz(mhz) {
                Ok(freq) => fire.cpu_freq = Some(freq),
                Err(_) => problem("cpu_freq"),
            },
        }
    }
    fire.overclock = overrides.overclock_enabled();
    if let Some(vreg) = overrides.vreg() {
        match vreg {
            MaybeKnown::Known(MetadataFireVreg::FireVregNone) => {}
            MaybeKnown::Known(vreg) => match FireVreg::try_from(vreg as u8) {
                Ok(vreg) => fire.vreg = Some(vreg),
                Err(_) => problem("vreg"),
            },
            MaybeKnown::Unknown(_) => problem("vreg"),
        }
    }
    match overrides.standby() {
        None => {}
        Some(MaybeKnown::Known(OverrideState::OverrideStateOn)) => fire.standby = Some(true),
        Some(MaybeKnown::Known(OverrideState::OverrideStateOff)) => fire.standby = Some(false),
        Some(MaybeKnown::Known(OverrideState::OverrideStateNotSet) | MaybeKnown::Unknown(_)) => {
            problem("standby")
        }
    }
    fire.addr_mon_min_cs_act_ns = overrides.addr_mon_min_cs_act_ns();

    let mut config = FirmwareConfig::default();
    if fire != FireConfig::default() {
        config.fire = Some(fire);
    }
    config.led = overrides.led_enabled().map(|enabled| LedConfig { enabled });
    config.swd = overrides
        .swd_enabled()
        .map(|swd_enabled| DebugConfig { swd_enabled });
    config
}

/// The control lines a config sets, in [`ChipSelect`]'s `lines` order.
const LINES: [&str; 6] = ["cs1", "cs2", "cs3", "cs4", "ce", "oe"];

/// One chip's chip-select settings, as a config sets them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChipSelect {
    lines: [Option<CsLogic>; 6],
    allow_cs_ignore: bool,
}

impl ChipSelect {
    fn apply(&self, chip: &mut ChipConfig) {
        let [cs1, cs2, cs3, cs4, ce, oe] = self.lines;
        chip.cs1 = cs1;
        chip.cs2 = cs2;
        chip.cs3 = cs3;
        chip.cs4 = cs4;
        chip.ce = ce;
        chip.oe = oe;
        chip.allow_cs_ignore = self.allow_cs_ignore;
    }

    fn ignores(&self) -> bool {
        self.lines.contains(&Some(CsLogic::Ignore))
    }

    /// The chip's resolved chip selects, or `None` where a config can't set
    /// these lines.
    fn cs_config(&self, chip_type: ChipType) -> Option<CsConfig> {
        let [cs1, cs2, cs3, cs4, ce, oe] = self.lines;
        // CsConfig::from_chip_type panics where another CS line is set and
        // CS1 isn't.
        let set = |line: Option<CsLogic>, name: &str| line.is_some() || is_fixed(chip_type, name);
        let others = set(cs2, "cs2") || set(cs3, "cs3") || set(cs4, "cs4");
        if ce.is_none() && oe.is_none() && others && !set(cs1, "cs1") {
            return None;
        }
        Some(CsConfig::from_chip_type(
            &chip_type, cs1, cs2, cs3, cs4, ce, oe,
        ))
    }
}

fn is_fixed(chip_type: ChipType, name: &str) -> bool {
    chip_type
        .control_lines()
        .iter()
        .any(|line| line.name == name && line.line_type.is_fixed())
}

/// The values a config can set on one control line of `chip_type`, `Ignore`
/// last.
fn line_values(chip_type: ChipType, name: &str) -> &'static [Option<CsLogic>] {
    const CONFIGURABLE: &[Option<CsLogic>] = &[
        Some(CsLogic::ActiveLow),
        Some(CsLogic::ActiveHigh),
        Some(CsLogic::Ignore),
    ];
    const HALF_SELECT: &[Option<CsLogic>] = &[Some(CsLogic::ActiveLow), Some(CsLogic::ActiveHigh)];
    const FIXED: &[Option<CsLogic>] = &[None, Some(CsLogic::Ignore)];
    match chip_type
        .control_lines()
        .iter()
        .find(|line| line.name == name)
    {
        Some(line) if line.line_type == ControlLineType::Configurable => CONFIGURABLE,
        Some(_) => FIXED,
        None if name == "cs1" && requires_half_select_cs1(&chip_type) => HALF_SELECT,
        None => &[None],
    }
}

/// Every chip-select setting of a chip that isn't a multi set's secondary.
fn primary_selects(chip_type: ChipType) -> Vec<ChipSelect> {
    let mut selects = vec![ChipSelect {
        lines: [None; 6],
        allow_cs_ignore: false,
    }];
    for (index, name) in LINES.iter().enumerate() {
        let values = line_values(chip_type, name);
        selects = selects
            .iter()
            .flat_map(|select| {
                values.iter().map(move |&value| {
                    let mut select = *select;
                    select.lines[index] = value;
                    select
                })
            })
            .collect();
    }
    selects
}

/// Every chip-select setting of a multi set's secondary, which selects on
/// one line and ignores the rest.
fn secondary_selects(chip_type: ChipType) -> Vec<ChipSelect> {
    let has = |name: &str| {
        chip_type
            .control_lines()
            .iter()
            .any(|line| line.name == name)
    };
    let mut ignored = [None; 6];
    for (index, name) in LINES.iter().enumerate() {
        if has(name) {
            ignored[index] = Some(CsLogic::Ignore);
        }
    }

    let mut selects = Vec::new();
    for (index, name) in LINES.iter().enumerate() {
        if !has(name) {
            continue;
        }
        let active: &[Option<CsLogic>] = if is_fixed(chip_type, name) {
            &[None]
        } else {
            &[Some(CsLogic::ActiveLow), Some(CsLogic::ActiveHigh)]
        };
        for &value in active {
            let mut lines = ignored;
            lines[index] = value;
            selects.push(ChipSelect {
                lines,
                allow_cs_ignore: false,
            });
        }
    }
    selects
}

/// The first chip-select setting and `force_16_bit` value that build `alg`'s
/// chip-select window and overrides.
fn search_chip_selects(
    board: Board,
    set_type: ChipSetType,
    specs: &[ChipTypeSpec],
    force_16_bit: &[bool],
    alg: &OneromAlgConfig,
) -> Option<(Vec<ChipSelect>, bool)> {
    let recorded = ChipSelectSignature::of(alg);
    let primary = primary_selects(specs[0].resolved());
    let candidates: Vec<Vec<ChipSelect>> = match set_type {
        ChipSetType::Multi => {
            let mut candidates: Vec<Vec<ChipSelect>> =
                primary.into_iter().map(|select| vec![select]).collect();
            for spec in &specs[1..] {
                let secondaries = secondary_selects(spec.resolved());
                candidates = candidates
                    .iter()
                    .flat_map(|candidate| {
                        secondaries.iter().map(move |&secondary| {
                            let mut candidate = candidate.clone();
                            candidate.push(secondary);
                            candidate
                        })
                    })
                    .collect();
            }
            candidates
        }
        ChipSetType::Single | ChipSetType::Banked => primary
            .into_iter()
            .map(|select| vec![select; specs.len()])
            .collect(),
    };

    let mut needs_allow = None;
    for &force in force_16_bit {
        for candidate in &candidates {
            if !builds(board, set_type, specs, candidate, force, &recorded) {
                continue;
            }
            if validates(set_type, specs, candidate) {
                return Some((candidate.clone(), force));
            }
            if needs_allow.is_none() {
                let allowed: Vec<ChipSelect> = candidate
                    .iter()
                    .map(|select| ChipSelect {
                        allow_cs_ignore: select.ignores(),
                        ..*select
                    })
                    .collect();
                if validates(set_type, specs, &allowed) {
                    needs_allow = Some((allowed, force));
                }
            }
        }
    }
    needs_allow
}

/// Whether `selects` builds a slot with the chip-select window and overrides
/// `recorded`.
fn builds(
    board: Board,
    set_type: ChipSetType,
    specs: &[ChipTypeSpec],
    selects: &[ChipSelect],
    force_16_bit: bool,
    recorded: &ChipSelectSignature,
) -> bool {
    let chips: Option<Vec<Chip>> = specs
        .iter()
        .zip(selects)
        .enumerate()
        .map(|(index, (spec, select))| {
            let cs_config = select.cs_config(spec.resolved())?;
            Some(Chip::new(
                index,
                String::new(),
                None,
                spec.clone(),
                cs_config,
                None,
                None,
            ))
        })
        .collect();
    let Some(chips) = chips else {
        return false;
    };
    match build_rom_slot(board, set_type, &chips, 0, None, force_16_bit) {
        Ok((slot, ..)) => slot
            .alg
            .as_ref()
            .is_some_and(|alg| ChipSelectSignature::of(alg) == *recorded),
        Err(_) => false,
    }
}

/// Whether `selects` passes config validation.
fn validates(set_type: ChipSetType, specs: &[ChipTypeSpec], selects: &[ChipSelect]) -> bool {
    let chips = specs
        .iter()
        .zip(selects)
        .map(|(spec, select)| {
            let mut chip = ChipConfig::new(String::new(), spec.clone());
            select.apply(&mut chip);
            chip
        })
        .collect();
    let config = Config::new(String::new(), vec![ChipSetConfig::new(set_type, chips)]);
    check_cs_v2(&config).is_ok()
}

/// The parts of a slot's algorithm that record its chips' chip-select settings.
#[derive(Debug, PartialEq, Eq)]
struct ChipSelectSignature {
    alg_cs: OneromAlgCsConfig,
    overrides: Vec<u8>,
}

impl ChipSelectSignature {
    fn of(alg: &OneromAlgConfig) -> Self {
        let mut overrides = alg
            .gpio_override_config
            .as_ref()
            .map(|config| config.params.clone())
            .unwrap_or_default();
        overrides.sort_unstable();
        Self {
            alg_cs: untimed(&alg.alg_cs),
            overrides,
        }
    }
}

/// `alg_cs` with its clock divider and delays zeroed.
fn untimed(alg_cs: &OneromAlgCsConfig) -> OneromAlgCsConfig {
    match *alg_cs {
        OneromAlgCsConfig::AlgCs0 {
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            serve_cs_low_0,
            byte_pin,
            first_rom_cs_base,
            first_rom_num_cs_pins,
            ..
        } => OneromAlgCsConfig::AlgCs0 {
            clkdiv_int: 0,
            clkdiv_frac: 0,
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            cs_active_delay: 0,
            cs_inactive_delay: 0,
            serve_cs_low_0,
            byte_pin,
            first_rom_cs_base,
            first_rom_num_cs_pins,
        },
        OneromAlgCsConfig::AlgCs1 {
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            cs_ignore_index,
            ..
        } => OneromAlgCsConfig::AlgCs1 {
            clkdiv_int: 0,
            clkdiv_frac: 0,
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            cs_active_delay: 0,
            cs_inactive_delay: 0,
            cs_ignore_index,
        },
        OneromAlgCsConfig::AlgCs2 {
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            base_qualifier_pin,
            num_qualifier_pins,
            qualifier_inactive_pattern,
            ..
        } => OneromAlgCsConfig::AlgCs2 {
            clkdiv_int: 0,
            clkdiv_frac: 0,
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            cs_active_delay: 0,
            cs_inactive_delay: 0,
            base_qualifier_pin,
            num_qualifier_pins,
            qualifier_inactive_pattern,
        },
        OneromAlgCsConfig::Unknown { .. } => alg_cs.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ROM slots are numbered from 0 without plugins, and a plugin's slot is
    /// identified by its type.
    #[test]
    fn slots_are_numbered_as_in_inspect_slots() {
        let slot = |slot_type| {
            let mut slot = OneromRomSlot::default();
            slot.slot_type = MaybeKnown::Known(slot_type);
            slot
        };
        let slots = [
            slot(RomSlotType::RomSlotTypePluginSystem),
            slot(RomSlotType::RomSlotTypePluginUser),
            slot(RomSlotType::RomSlotTypeSingleRom),
            slot(RomSlotType::RomSlotTypePluginPio),
            slot(RomSlotType::RomSlotTypeBankedRom),
        ];
        let names: Vec<SlotName> = (0..slots.len())
            .map(|index| SlotName::of(&slots, index))
            .collect();
        assert_eq!(
            names,
            [
                SlotName::SystemPlugin,
                SlotName::UserPlugin,
                SlotName::Rom(0),
                SlotName::PioPlugin,
                SlotName::Rom(1),
            ]
        );
    }

    /// A ROM slot whose values are all known.
    fn known_slot() -> OneromRomSlot {
        let mut alg = OneromAlgConfig::default();
        alg.alg_dma = OneromAlgDmaConfig::AlgDma0 {
            bit_mode: MaybeKnown::Known(onerom_metadata::BitModes::BitMode8),
            continuous: 0,
        };
        let mut slot = OneromRomSlot::default();
        slot.slot_type = MaybeKnown::Known(RomSlotType::RomSlotTypeSingleRom);
        slot.alg = Some(alg);
        slot.firmware_overrides = Some(OneromFirmwareOverrides::default());
        slot
    }

    /// The global and per-slot minimum CS-active times are recovered in
    /// nanoseconds where set, and left unset where not.
    #[test]
    fn addr_mon_min_cs_act_ns_is_recovered() {
        let recovered = |global: u8, slot_value: u8| {
            let mut slot = known_slot();
            slot.alg = None;
            slot.firmware_overrides
                .as_mut()
                .unwrap()
                .addr_mon_min_cs_act = slot_value;
            let mut header = OneromMetadataHeader::default();
            header.hw.hw_rev = Board::Fire24A.name().into();
            header.addr_mon_min_cs_act = global;
            header.rom_slots = vec![slot];
            let config = recover_config(&header, &[None]).unwrap().config;
            let slot_ns = config.chip_sets[0]
                .firmware_overrides
                .as_ref()
                .and_then(|overrides| overrides.fire.as_ref())
                .and_then(|fire| fire.addr_mon_min_cs_act_ns);
            (config.addr_mon_min_cs_act_ns, slot_ns)
        };
        assert_eq!(recovered(15, 30), (Some(150), Some(300)));
        let not_set = onerom_metadata::ADDR_MON_MIN_CS_ACT_NOT_SET;
        assert_eq!(recovered(not_set, not_set), (None, None));
    }

    /// Each unrecognised value recovery reads fails with its slot and field.
    #[test]
    fn an_unknown_value_is_reported() {
        let unknown = |change: fn(&mut OneromRomSlot)| {
            let mut slot = known_slot();
            change(&mut slot);
            slot
        };
        let cases: [(OneromRomSlot, &str); 7] = [
            (
                unknown(|slot| slot.slot_type = MaybeKnown::Unknown(99)),
                "slot_type",
            ),
            (
                unknown(|slot| {
                    slot.alg.as_mut().unwrap().alg_cs = OneromAlgCsConfig::Unknown {
                        alg: 99,
                        clkdiv_int: 0,
                        clkdiv_frac: 0,
                        gpio_base: 0,
                        base_cs_pin: 0,
                        num_cs_pins: 0,
                        base_data_pin: 0,
                        num_data_pins: 0,
                        cs_active_delay: 0,
                        cs_inactive_delay: 0,
                        params: Vec::new(),
                    }
                }),
                "alg_cs",
            ),
            (
                unknown(|slot| {
                    slot.alg.as_mut().unwrap().alg_addr = OneromAlgAddrConfig::Unknown {
                        alg: 99,
                        clkdiv_int: 0,
                        clkdiv_frac: 0,
                        gpio_base: 0,
                        num_delay_cycles: 0,
                        base_addr_pin: 0,
                        num_addr_pins: 0,
                        num_rom_table_bits: 0,
                        params: Vec::new(),
                    }
                }),
                "alg_addr",
            ),
            (
                unknown(|slot| {
                    slot.alg.as_mut().unwrap().alg_data = OneromAlgDataConfig::Unknown {
                        alg: 99,
                        clkdiv_int: 0,
                        clkdiv_frac: 0,
                        gpio_base: 0,
                        base_data_pin: 0,
                        word_size: 0,
                        params: Vec::new(),
                    }
                }),
                "alg_data",
            ),
            (
                unknown(|slot| {
                    slot.alg.as_mut().unwrap().alg_dma = OneromAlgDmaConfig::AlgDma0 {
                        bit_mode: MaybeKnown::Unknown(99),
                        continuous: 0,
                    }
                }),
                "bit_mode",
            ),
            (
                unknown(|slot| {
                    let overrides = slot.firmware_overrides.as_mut().unwrap();
                    // The vreg override is present.
                    overrides.override_present[0] = 1 << 4;
                    overrides.fire_vreg = MaybeKnown::Unknown(99);
                }),
                "vreg",
            ),
            (
                unknown(|slot| {
                    let overrides = slot.firmware_overrides.as_mut().unwrap();
                    overrides.override_states.standby = Some(MaybeKnown::Unknown(3));
                }),
                "standby",
            ),
        ];

        let mut header = OneromMetadataHeader::default();
        header.rom_slots = vec![known_slot()];
        assert_eq!(check_known(&header), Ok(()));
        for (slot, field) in cases {
            header.rom_slots = vec![known_slot(), slot];
            assert_eq!(
                check_known(&header),
                Err(RecoverError::UnknownValue {
                    slot: SlotName::Rom(1),
                    field
                })
            );
            assert_eq!(
                recover_config(&header, &[None, None]).err(),
                Some(RecoverError::UnknownValue {
                    slot: SlotName::Rom(1),
                    field
                })
            );
        }
    }
}
