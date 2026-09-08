// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Turning the slot list into metadata and ROM images.
//!
//! The path is `onerom-gen`'s own: a config, `Builder::from_json`, one file
//! loaded per [`onerom_gen::FileSpec`], then `build`. The prototype stops at the
//! two buffers — composing them onto a base firmware needs the release
//! download, which is out of scope here.

use std::path::PathBuf;

use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::Board;
use onerom_gen::{
    Builder, ChipConfig, ChipSetConfig, ChipSetType, ChipTypeSpec, Config, CsLogic, FileData,
    LoadAddress,
};

use crate::catalog::{self, MCU, PluginChoice};
use crate::error::{Error, Result};
use crate::slot::Slot;

/// What a completed build produced.
#[derive(Debug, Clone)]
pub struct Built {
    /// The metadata region, to be flashed after the base firmware.
    pub metadata: Vec<u8>,
    /// The ROM images, to be flashed after the metadata.
    pub images: Vec<u8>,
    /// `onerom-gen`'s own summary of what it built.
    pub description: String,
}

impl Built {
    /// Metadata and images end to end, which is what the prototype saves.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = self.metadata.clone();
        out.extend_from_slice(&self.images);
        out
    }
}

/// Everything a build reads, gathered so the build itself borrows nothing from
/// the UI state.
#[derive(Debug, Clone)]
pub struct Request {
    /// The board the image is for.
    pub board: Board,
    /// The firmware version the image targets.
    pub version: FirmwareVersion,
    /// The slots, in boot-select order.
    pub slots: Vec<Slot>,
    /// The plugins that go ahead of the slots, system first.
    pub plugins: Vec<PluginChoice>,
}

/// The config JSON this request builds from.
///
/// Every slot gets a unique `file` key even where two slots name files with
/// the same basename, so `onerom-gen` never folds two distinct images into
/// one. The `label` field carries what a device read-back should show.
pub fn config_json(request: &Request) -> Result<String> {
    let mut chip_sets = Vec::new();

    for plugin in &request.plugins {
        chip_sets.push(plugin_chip_set(plugin)?);
    }

    for (index, slot) in request.slots.iter().enumerate() {
        chip_sets.push(slot_chip_set(index, slot)?);
    }

    let count = request.slots.len();
    let plural = if count == 1 { "" } else { "s" };
    let config = Config::new(format!("ROM Slot Builder: {count} slot{plural}"), chip_sets);

    Ok(serde_json::to_string(&config)?)
}

/// One slot as a single-chip set.
fn slot_chip_set(index: usize, slot: &Slot) -> Result<ChipSetConfig> {
    let chip = slot.chip.ok_or(Error::NoChipType(index))?;
    let file = slot.file.as_ref().ok_or(Error::NoFile(index))?;

    let mut config = ChipConfig::new(
        file_key(index, file),
        ChipTypeSpec::new(chip.alias.to_owned(), chip.chip_type),
    );
    config.size_handling = slot.sizing.clone();
    config.format = slot.format;
    config.label = Some(match slot.label.trim() {
        "" => slot.file_name().unwrap_or_default(),
        label => label.to_owned(),
    });

    if slot.format == onerom_gen::FileFormat::IntelHex && !slot.load_address.trim().is_empty() {
        config.load_address =
            LoadAddress::parse_str(slot.load_address.trim()).map_err(|_| Error::LoadAddress {
                slot: index,
                value: slot.load_address.trim().to_owned(),
            })?;
    }

    let polarities = [config_cs(slot, 0), config_cs(slot, 1), config_cs(slot, 2)];
    let [cs1, cs2, cs3] = polarities;
    config.cs1 = cs1;
    config.cs2 = cs2;
    config.cs3 = cs3;

    Ok(ChipSetConfig::new(ChipSetType::Single, vec![config]))
}

/// The polarity for chip select `line`, or `None` where the chip has no such
/// configurable line.
fn config_cs(slot: &Slot, line: usize) -> Option<CsLogic> {
    if line >= slot.chip_selects() {
        return None;
    }
    Some(if slot.polarities[line].0 {
        CsLogic::ActiveHigh
    } else {
        CsLogic::ActiveLow
    })
}

/// One plugin as a single-chip set, padded out to its slot.
fn plugin_chip_set(plugin: &PluginChoice) -> Result<ChipSetConfig> {
    let chip_type = plugin.chip_type();
    let length = std::fs::metadata(&plugin.path)
        .map_err(|source| Error::Read {
            path: plugin.path.clone(),
            source,
        })?
        .len();

    let mut config = ChipConfig::new(
        plugin.path.to_string_lossy().into_owned(),
        ChipTypeSpec::new(plugin.config_chip_type().to_owned(), chip_type),
    );
    config.size_handling = if length == chip_type.size_bytes() as u64 {
        onerom_gen::SizeHandling::None
    } else {
        onerom_gen::SizeHandling::Pad
    };

    Ok(ChipSetConfig::new(ChipSetType::Single, vec![config]))
}

/// The config `file` key for a slot: its index keeps two same-named files
/// apart.
fn file_key(index: usize, path: &std::path::Path) -> String {
    format!("{index}:{}", path.display())
}

/// Run the build.
///
/// Blocking: it reads the slot files and does the layout work, so the caller
/// hands it to a background task rather than the UI thread.
pub fn run(request: Request) -> Result<Built> {
    let json = config_json(&request)?;
    let mut builder = Builder::from_json(request.version, request.board.mcu_family(), &json)?;

    for spec in builder.file_specs() {
        let path = source_path(&request, &spec.source);
        let data = std::fs::read(&path).map_err(|source| Error::Read {
            path: path.clone(),
            source,
        })?;
        builder.add_file(FileData::new(spec.id, data))?;
    }

    let properties =
        FirmwareProperties::new(request.version, request.board, MCU, ServeAlg::Default, true)?;
    let description = builder.description();
    let (metadata, images) = builder.build(properties)?;

    Ok(Built {
        metadata,
        images,
        description,
    })
}

/// The file behind a spec's source, undoing the index prefix a slot key
/// carries.
fn source_path(request: &Request, source: &str) -> PathBuf {
    for (index, slot) in request.slots.iter().enumerate() {
        if let Some(file) = &slot.file
            && file_key(index, file) == source
        {
            return file.clone();
        }
    }
    PathBuf::from(source)
}

/// Where a build would be saved, following the CLI's naming.
pub fn suggested_name(board: Board, version: FirmwareVersion) -> String {
    format!(
        "onerom-{}-v{}.{}.{}-slots.bin",
        board.name(),
        version.major(),
        version.minor(),
        version.patch()
    )
}

/// The flash a request occupies, segment by segment.
///
/// Kept beside the build so the bar and the built image cannot describe
/// different things.
pub fn usage(slots: &[Slot], plugins: usize) -> Usage {
    let mut segments = vec![Segment {
        kind: SegmentKind::Firmware,
        bytes: catalog::reserved_bytes(),
        label: format!("Firmware {}", catalog::kb(catalog::reserved_bytes())),
    }];

    for index in 0..plugins {
        segments.push(Segment {
            kind: SegmentKind::Plugin,
            bytes: catalog::PLUGIN_SLOT_BYTES,
            label: format!("Plugin {index}"),
        });
    }

    for (index, slot) in slots.iter().enumerate() {
        let bytes = slot.image_bytes();
        if bytes > 0 {
            segments.push(Segment {
                kind: SegmentKind::Rom,
                bytes,
                label: format!("Slot {index} {}", catalog::kb(bytes)),
            });
        }
    }

    let used = segments.iter().map(|segment| segment.bytes).sum();
    Usage {
        segments,
        used,
        total: catalog::flash_bytes(),
    }
}

/// What one bar segment stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    /// The firmware and metadata reservation.
    Firmware,
    /// One plugin slot.
    Plugin,
    /// One ROM image.
    Rom,
}

/// One proportional piece of the flash bar.
#[derive(Debug, Clone)]
pub struct Segment {
    /// What it stands for.
    pub kind: SegmentKind,
    /// Its size in flash.
    pub bytes: u64,
    /// The text shown against it.
    pub label: String,
}

/// The whole flash tally.
#[derive(Debug, Clone)]
pub struct Usage {
    /// The segments, in flash order.
    pub segments: Vec<Segment>,
    /// What they add up to.
    pub used: u64,
    /// What the MCU has.
    pub total: u64,
}

impl Usage {
    /// Whether the build no longer fits.
    pub fn over(&self) -> bool {
        self.used > self.total
    }

    /// Flash left, or zero once over.
    pub fn free(&self) -> u64 {
        self.total.saturating_sub(self.used)
    }

    /// Flash beyond capacity, or zero while it still fits.
    pub fn excess(&self) -> u64 {
        self.used.saturating_sub(self.total)
    }
}
