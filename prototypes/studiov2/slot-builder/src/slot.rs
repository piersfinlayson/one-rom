// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The slot list: one boot-selectable ROM image per entry.
//!
//! A slot is user intent. Changing the board revalidates what is here and
//! never discards it, so switching board and back leaves the list exactly as
//! it was.

use std::path::PathBuf;

use onerom_gen::{FileFormat, SizeHandling};

use crate::catalog::{ChipChoice, PolarityChoice};

/// One boot-selectable image.
#[derive(Debug, Clone)]
pub struct Slot {
    /// The image file on disk, once the user has picked one.
    pub file: Option<PathBuf>,
    /// How the file is decoded.
    pub format: FileFormat,
    /// For Intel HEX, the address that maps to ROM byte 0, as typed.
    pub load_address: String,
    /// The chip this slot emulates.
    pub chip: Option<ChipChoice>,
    /// How the image is fitted to the chip.
    pub sizing: SizeHandling,
    /// Chip-select polarities, in CS1..CS3 order. Held at full width so a
    /// change of chip type keeps what the user chose for the lines that stay.
    pub polarities: [PolarityChoice; 3],
    /// The name recorded in metadata, shown on device read-back.
    pub label: String,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            file: None,
            format: FileFormat::Binary,
            load_address: String::new(),
            chip: None,
            sizing: SizeHandling::None,
            polarities: [PolarityChoice(false); 3],
            label: String::new(),
        }
    }
}

impl Slot {
    /// A new slot inheriting the chip settings of the one before it, and none
    /// of its content — adding a second bank of the same ROM type is the
    /// common case, adding the same file twice is not.
    pub fn after(previous: Option<&Slot>) -> Self {
        match previous {
            Some(previous) => Self {
                format: previous.format,
                chip: previous.chip,
                sizing: previous.sizing.clone(),
                polarities: previous.polarities,
                ..Self::default()
            },
            None => Self::default(),
        }
    }

    /// The file's own name, for the picker's "no file selected" line.
    pub fn file_name(&self) -> Option<String> {
        self.file
            .as_ref()?
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    }

    /// Flash this slot's image occupies, or zero until a chip type is chosen.
    pub fn image_bytes(&self) -> u64 {
        self.chip.map_or(0, |chip| u64::from(chip.image_bytes))
    }

    /// How many chip-select polarity pickers this slot shows.
    pub fn chip_selects(&self) -> usize {
        self.chip.map_or(0, |chip| chip.chip_selects)
    }

    /// Whether this slot could be built: it needs a file and a chip type.
    pub fn is_complete(&self) -> bool {
        self.file.is_some() && self.chip.is_some()
    }

    /// The format a file's extension implies, which the user may override.
    pub fn format_for(path: &std::path::Path) -> FileFormat {
        let extension = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();

        match extension.as_str() {
            "hex" | "ihex" | "ihx" | "mcs" => FileFormat::IntelHex,
            "s19" | "s28" | "s37" | "srec" | "mot" => FileFormat::Srec,
            _ => FileFormat::Binary,
        }
    }
}

/// Move the slot at `from` to `to`, leaving the list alone if `to` is off
/// either end.
pub fn move_slot(slots: &mut Vec<Slot>, from: usize, to: isize) {
    if to < 0 || to as usize >= slots.len() || from >= slots.len() {
        return;
    }
    let slot = slots.remove(from);
    slots.insert(to as usize, slot);
}
