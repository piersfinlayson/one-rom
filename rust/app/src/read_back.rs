// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Reading a One ROM or an image file back into a config and the bytes of
//! every ROM it serves.
//!
//! [`Builder`] builds from the result, so changing the slots, plugins,
//! firmware version or board is an ordinary build with the existing checks.
//! Firmware before [`MIN_SCHEMA_VERSION`] isn't read back.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use onerom_config::chip::ChipType;
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::{Board, BoardSize};
use onerom_config::mcu::{RP235X_BASE_FLASH, Variant};
use onerom_fw_parser::{
    OneRom, ParsedDevice, Parser, Reader, SlotKind, SlotReadError, image_file_reader,
};
use onerom_gen::{
    Builder, ChipSetConfig, Config, Error as GenError, FIRMWARE_SIZE, FileData, FlashChips,
    RecoverError, RecoveryProblem, UnscrambleError, check_known, recover_config, unscramble_slot,
};
use onerom_metadata::{FIRMWARE_RELEASE, MIN_SCHEMA_VERSION, OneromMetadataHeader};

use crate::plugin::official_plugin_from_source;
use crate::{PluginType, PluginVersion};

/// A One ROM, or an image file, read back into the config that builds it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReadBack {
    /// The firmware version.
    pub version: FirmwareVersion,
    /// The board.
    pub board: Board,
    /// The board's size. For a file, the smallest size the image fits.
    pub board_size: Option<BoardSize>,
    /// The config. A chip with bytes has a [`ReadBackFile`]'s name as its
    /// `file`, and a published plugin is listed by name until
    /// [`ReadBack::reuse_plugin_slots`].
    pub config: Config,
    /// Each chip's bytes, in config order.
    pub files: Vec<ReadBackFile>,
    /// The published plugins, each with the release the image contains.
    pub plugins: Vec<RecordedPlugin>,
    /// Settings the config couldn't recover.
    pub problems: Vec<RecoveryProblem>,
    /// The base firmware, the image's first [`FIRMWARE_SIZE`] bytes.
    pub base_firmware: Vec<u8>,
    /// Each published plugin's slot, for [`ReadBack::reuse_plugin_slots`].
    plugin_slots: Vec<PluginSlot>,
}

/// A published plugin's slot, as read back.
#[derive(Debug, Clone)]
struct PluginSlot {
    /// The config's chip set holding the plugin.
    set: usize,
    /// The plugin's recorded filename, its release's URL.
    source: String,
    /// The file the slot becomes.
    file: ReadBackFile,
}

/// One chip's bytes, as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReadBackFile {
    /// The name the chip's `file` contains, `slot<N>[-chip<M>][-<name>].bin`
    /// or for a plugin `<type>-plugin[-<name>].bin`.
    pub name: String,
    /// The chip's ROM slot, numbered from 0 without plugins. `None` for a
    /// plugin.
    pub slot: Option<usize>,
    /// The chip's index within its slot.
    pub chip: usize,
    /// The chip's bytes, as served.
    pub data: Vec<u8>,
}

/// A published plugin in a read back image.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecordedPlugin {
    /// The plugin's type.
    pub plugin_type: PluginType,
    /// The plugin's name.
    pub name: String,
    /// The release the image contains.
    pub version: PluginVersion,
}

/// Why a One ROM or an image file can't be read back.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReadBackError<E> {
    /// The image file doesn't contain One ROM firmware.
    FileNotOneRom,
    /// The One ROM's flash doesn't contain One ROM firmware.
    FlashNotOneRom,
    /// Its firmware is older than [`MIN_SCHEMA_VERSION`].
    TooOld {
        /// The firmware's version.
        version: FirmwareVersion,
    },
    /// Its firmware is newer than this build supports.
    FirmwareTooNew {
        /// The firmware's version.
        version: FirmwareVersion,
    },
    /// Its metadata is missing, as base firmware's is, or doesn't parse.
    Unparsed,
    /// A slot's data couldn't be read.
    Read(SlotReadError<E>),
    /// The base firmware couldn't be read.
    ReadFirmware {
        /// The reader's error.
        error: E,
    },
    /// The config couldn't be recovered.
    Recover(RecoverError),
    /// It doesn't have the requested ROM slot.
    NoSlot {
        /// The requested slot, numbered from 0 without plugins.
        slot: usize,
        /// The ROM slots it has.
        slots: usize,
    },
    /// The requested ROM slot doesn't have an image.
    NoImage {
        /// The slot, numbered from 0 without plugins.
        slot: usize,
    },
    /// The ROM slot's data doesn't unscramble.
    Unscramble {
        /// The slot, numbered from 0 without plugins.
        slot: usize,
        /// Why.
        error: UnscrambleError,
    },
}

impl<E: fmt::Debug> fmt::Display for ReadBackError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileNotOneRom => f.write_str("The file doesn't contain One ROM firmware"),
            Self::FlashNotOneRom => {
                f.write_str("The One ROM's flash doesn't contain One ROM firmware")
            }
            Self::TooOld { version } => write!(
                f,
                "Firmware v{version} isn't supported. The oldest supported version is v{MIN_SCHEMA_VERSION}"
            ),
            Self::FirmwareTooNew { version } => {
                write!(f, "Firmware v{version} is newer than this build supports")
            }
            Self::Unparsed => f.write_str("The firmware's metadata is missing or doesn't parse"),
            Self::Read(error) => write!(f, "{error}"),
            Self::ReadFirmware { error } => {
                write!(f, "The base firmware couldn't be read: {error:?}")
            }
            Self::Recover(error) => write!(f, "{error}"),
            Self::NoSlot { slot, slots: 0 } => {
                write!(
                    f,
                    "There isn't a ROM slot {slot}. There aren't any ROM slots"
                )
            }
            Self::NoSlot { slot, slots } => write!(
                f,
                "There isn't a ROM slot {slot}. The ROM slots are 0 to {}",
                slots - 1
            ),
            Self::NoImage { slot } => write!(f, "Slot {slot} doesn't have a ROM image"),
            Self::Unscramble { slot, error } => write!(f, "Slot {slot}: {error}"),
        }
    }
}

/// Reads back a One ROM `device` was parsed from, reading its base firmware
/// and slots through `reader`.
///
/// `board_size` is the device's size, which its runtime info or OTP records.
pub async fn read_back<R: Reader>(
    device: &ParsedDevice,
    reader: &mut R,
    board_size: Option<BoardSize>,
) -> Result<ReadBack, ReadBackError<R::Error>> {
    let (version, metadata) = readable(device, Source::OneRom)?;
    let mut base_firmware = vec![0; FIRMWARE_SIZE];
    reader
        .read(RP235X_BASE_FLASH, &mut base_firmware)
        .await
        .map_err(|error| ReadBackError::ReadFirmware { error })?;
    let slots = device
        .read_slots(reader)
        .await
        .map_err(ReadBackError::Read)?;
    from_slots(device, version, metadata, board_size, &slots, base_firmware)
}

/// Reads back an image file. Its board size is the smallest the image fits.
///
/// A file longer than the first flash chip contains the second chip's contents
/// after the first chip's.
pub async fn read_back_file(image: &[u8]) -> Result<ReadBack, ReadBackError<String>> {
    let mut reader = image_file_reader(image, FlashChips::first_for(Variant::RP2350));
    let device = Parser::new(&mut reader).parse_device().await;
    let (version, metadata) = readable(&device, Source::File)?;
    let slots = device
        .read_slots(&mut reader)
        .await
        .map_err(ReadBackError::Read)?;
    let base_firmware = image[..FIRMWARE_SIZE.min(image.len())].to_vec();
    from_slots(
        &device,
        version,
        metadata,
        device.min_board_size(),
        &slots,
        base_firmware,
    )
}

/// The ROMs in ROM slot `slot`, numbered from 0 without plugins, of the One
/// ROM `device` was parsed from, reading the slot through `reader`. Only that
/// slot is read.
pub async fn read_slot_roms<R: Reader>(
    device: &ParsedDevice,
    reader: &mut R,
    slot: usize,
) -> Result<Vec<Vec<u8>>, ReadBackError<R::Error>> {
    slot_roms(device, reader, slot, Source::OneRom).await
}

/// [`read_slot_roms`] for an image file.
pub async fn read_slot_roms_file(
    image: &[u8],
    slot: usize,
) -> Result<Vec<Vec<u8>>, ReadBackError<String>> {
    let mut reader = image_file_reader(image, FlashChips::first_for(Variant::RP2350));
    let device = Parser::new(&mut reader).parse_device().await;
    slot_roms(&device, &mut reader, slot, Source::File).await
}

/// [`read_slot_roms`] for `device`, parsed from `source`.
async fn slot_roms<R: Reader>(
    device: &ParsedDevice,
    reader: &mut R,
    slot: usize,
    source: Source,
) -> Result<Vec<Vec<u8>>, ReadBackError<R::Error>> {
    let (_, metadata) = readable(device, source)?;
    let rom_slots: Vec<usize> = device
        .slots()
        .filter(|view| view.kind == SlotKind::Rom)
        .map(|view| view.slot_index)
        .collect();
    let &index = rom_slots.get(slot).ok_or(ReadBackError::NoSlot {
        slot,
        slots: rom_slots.len(),
    })?;
    let data = device
        .read_slot(reader, index)
        .await
        .map_err(ReadBackError::Read)?
        .ok_or(ReadBackError::NoImage { slot })?;
    unscramble_slot(&metadata.hw, &metadata.rom_slots[index], &data)
        .map_err(|error| ReadBackError::Unscramble { slot, error })
}

/// Where a device was parsed from.
#[derive(Debug, Clone, Copy)]
enum Source {
    File,
    OneRom,
}

/// The device's firmware version and metadata, where it can be read back.
fn readable<E>(
    device: &ParsedDevice,
    source: Source,
) -> Result<(FirmwareVersion, &OneromMetadataHeader), ReadBackError<E>> {
    let schema = device.as_schema();
    if let Some(version) = schema.and_then(OneRom::newer_version) {
        return Err(ReadBackError::FirmwareTooNew { version });
    }
    let not_one_rom = || match source {
        Source::File => ReadBackError::FileNotOneRom,
        Source::OneRom => ReadBackError::FlashNotOneRom,
    };
    if !device.is_recognised() || matches!(device, ParsedDevice::Lab) {
        return Err(not_one_rom());
    }
    let version = device.version().ok_or_else(not_one_rom)?;
    if version < MIN_SCHEMA_VERSION {
        return Err(ReadBackError::TooOld { version });
    }
    let release = FirmwareVersion::new(version.major(), version.minor(), version.patch(), 0);
    if release > FIRMWARE_RELEASE
        || schema.is_some_and(|onerom| !onerom.newer_generations().is_empty())
    {
        return Err(ReadBackError::FirmwareTooNew { version });
    }
    let metadata = schema
        .and_then(OneRom::metadata)
        .ok_or(ReadBackError::Unparsed)?;
    check_known(metadata).map_err(|_| ReadBackError::FirmwareTooNew { version })?;
    Ok((version, metadata))
}

fn from_slots<E>(
    device: &ParsedDevice,
    version: FirmwareVersion,
    metadata: &OneromMetadataHeader,
    board_size: Option<BoardSize>,
    slots: &[Option<Vec<u8>>],
    base_firmware: Vec<u8>,
) -> Result<ReadBack, ReadBackError<E>> {
    let recovered = recover_config(metadata, slots).map_err(ReadBackError::Recover)?;
    let board = device.get_board().ok_or_else(|| {
        ReadBackError::Recover(RecoverError::UnknownBoard {
            name: metadata.hw.hw_rev.clone(),
        })
    })?;

    let mut config = recovered.config;
    let mut files = Vec::new();
    let mut plugins = Vec::new();
    let mut plugin_slots = Vec::new();
    let mut slot = 0;
    for (set, (chip_set, data)) in config.chip_sets.iter_mut().zip(recovered.data).enumerate() {
        let rom_slot = if is_plugin_set(chip_set) {
            None
        } else {
            slot += 1;
            Some(slot - 1)
        };
        let several = chip_set.chips.len() > 1;
        for (index, (chip, data)) in chip_set.chips.iter_mut().zip(data).enumerate() {
            let Some(data) = data else {
                continue;
            };
            let recorded = chip.label.clone().unwrap_or_default();
            if let Some(plugin) = published_plugin(chip.chip_type.resolved(), &recorded) {
                let prefix = plugin_prefix(chip.chip_type.resolved());
                plugin_slots.push(PluginSlot {
                    set,
                    source: recorded,
                    file: ReadBackFile {
                        name: format!("{prefix}-{}.bin", plugin.name),
                        slot: None,
                        chip: index,
                        data,
                    },
                });
                chip.plugin = Some(plugin.name.clone());
                chip.label = None;
                plugins.push(plugin);
                continue;
            }
            let prefix = match rom_slot {
                Some(slot) if several => format!("slot{slot}-chip{index}"),
                Some(slot) => format!("slot{slot}"),
                None => plugin_prefix(chip.chip_type.resolved()).to_string(),
            };
            let name = match file_stem(&recorded) {
                Some(stem) => format!("{prefix}-{stem}.bin"),
                None => format!("{prefix}.bin"),
            };
            chip.file = name.clone();
            files.push(ReadBackFile {
                name,
                slot: rom_slot,
                chip: index,
                data,
            });
        }
    }

    Ok(ReadBack {
        version,
        board,
        board_size,
        config,
        files,
        plugins,
        problems: recovered.problems,
        base_firmware,
        plugin_slots,
    })
}

fn is_plugin_set(chip_set: &ChipSetConfig) -> bool {
    chip_set
        .chips
        .iter()
        .any(|chip| chip.chip_type.resolved().is_plugin())
}

/// The published plugins in the image `device` was parsed from.
pub fn published_plugins(device: &ParsedDevice) -> Vec<RecordedPlugin> {
    let Some(metadata) = device.as_schema().and_then(|onerom| onerom.metadata()) else {
        return Vec::new();
    };
    device
        .slots()
        .filter(|view| view.kind == SlotKind::Plugin)
        .filter_map(|view| {
            let rom = metadata.rom_slots[view.slot_index].roms.first()?;
            let chip_type = ChipType::try_from_rbcp_u8(rom.rbcp_rom_type)?;
            published_plugin(chip_type, rom.filename.as_deref()?)
        })
        .collect()
}

/// The start of a plugin's file name.
fn plugin_prefix(chip_type: ChipType) -> &'static str {
    if chip_type == ChipType::SystemPlugin {
        "system-plugin"
    } else if chip_type == ChipType::UserPlugin {
        "user-plugin"
    } else {
        "pio-plugin"
    }
}

/// The published plugin a plugin chip's recorded source identifies.
fn published_plugin(chip_type: ChipType, source: &str) -> Option<RecordedPlugin> {
    let plugin_type = if chip_type == ChipType::SystemPlugin {
        PluginType::System
    } else if chip_type == ChipType::UserPlugin {
        PluginType::User
    } else {
        return None;
    };
    let (url_type, name, version) = official_plugin_from_source(source)?;
    (url_type == plugin_type).then_some(RecordedPlugin {
        plugin_type,
        name,
        version,
    })
}

/// The file name inside a recorded filename, without its extension and with
/// any character invalid in a file name replaced by `_`. `None` where that
/// leaves nothing.
///
/// A recorded filename is laid out by onerom-gen's `ChipConfig::filename`. The
/// name is the archive member's where there is one, or else the last part of
/// the file's path or URL.
fn file_stem(recorded: &str) -> Option<String> {
    let mut parts = recorded.split('|');
    let file = parts.next().unwrap_or_default();
    let source = parts
        .next()
        .filter(|part| !part.starts_with("start=") && !part.starts_with("transform="))
        .unwrap_or(file);
    let name = source.rsplit(['/', '\\']).next().unwrap_or_default();
    let stem = match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    };
    let stem: String = stem
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim_matches([' ', '.']);
    (!stem.is_empty()).then(|| stem.to_string())
}

impl ReadBack {
    /// The bytes of the file named `name`.
    pub fn file(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|file| file.name == name)
            .map(|file| file.data.as_slice())
    }

    /// The config's chip set holding ROM slot `slot`, numbered from 0 without
    /// plugins.
    pub fn chip_set_of_slot(&self, slot: usize) -> Option<usize> {
        self.config
            .chip_sets
            .iter()
            .enumerate()
            .filter(|(_, chip_set)| !is_plugin_set(chip_set))
            .nth(slot)
            .map(|(index, _)| index)
    }

    /// `problem`'s subject, for example `Slot 3's chip-select settings` or
    /// `The system plugin's vreg override`.
    pub fn problem_subject(&self, problem: &RecoveryProblem) -> String {
        match problem {
            RecoveryProblem::ChipSelect { set } => {
                format!("{}'s chip-select settings", self.place(*set))
            }
            RecoveryProblem::Override { set, name } => {
                format!("{}'s {name} override", self.place(*set))
            }
            _ => problem.to_string(),
        }
    }

    /// Chip set `set` as a user knows it: its ROM slot, or its plugin.
    fn place(&self, set: usize) -> String {
        if let Some(slot) =
            (0..self.config.chip_sets.len()).find(|&slot| self.chip_set_of_slot(slot) == Some(set))
        {
            return format!("Slot {slot}");
        }
        let chip_type = self.config.chip_sets[set]
            .chips
            .first()
            .map(|chip| chip.chip_type.resolved());
        if chip_type == Some(ChipType::SystemPlugin) {
            "The system plugin".to_string()
        } else if chip_type == Some(ChipType::UserPlugin) {
            "The user plugin".to_string()
        } else {
            "The PIO plugin".to_string()
        }
    }

    /// Lists each published plugin as the bytes read from its slot rather than
    /// by name, so a build retains the data the plugin stores there.
    ///
    /// Each slot becomes one of [`files`](Self::files). Its chip's label is
    /// the release's URL so the rebuilt image reads back with the plugin
    /// listed by name.
    pub fn reuse_plugin_slots(&mut self) {
        for PluginSlot { set, source, file } in core::mem::take(&mut self.plugin_slots) {
            let at = self.config.chip_sets[..set]
                .iter()
                .flat_map(|set| &set.chips)
                .chain(&self.config.chip_sets[set].chips[..file.chip])
                .filter(|chip| self.file(&chip.file).is_some())
                .count();
            let chip = &mut self.config.chip_sets[set].chips[file.chip];
            chip.plugin = None;
            chip.label = Some(source);
            chip.file = file.name.clone();
            self.files.insert(at, file);
        }
    }

    /// Adds each file `builder` requires that this read back contains.
    ///
    /// A published plugin isn't one of them until
    /// [`ReadBack::reuse_plugin_slots`]. `builder` must be built from this
    /// read back's config.
    pub fn add_files(&self, builder: &mut Builder) -> Result<(), GenError> {
        for spec in builder.file_specs() {
            if let Some(data) = self.file(&spec.source) {
                builder.add_file(FileData::new(spec.id, data.to_vec()))?;
            }
        }
        Ok(())
    }
}
