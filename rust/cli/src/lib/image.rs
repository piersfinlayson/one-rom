// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Questions about a composed One ROM firmware image.
//!
//! The answers come from a parse of the image itself, so they hold for an image
//! sitting in a file as much as for one read back off a device - in particular
//! for an image that is about to be flashed, which is the only way to know what
//! a device is going to do before it does it.

use onerom_config::chip::ChipType;
use onerom_fw_parser::device::{ParsedDevice, SlotKind};

/// Every chip type the image can serve, plugins excluded.
///
/// An image records a human-readable type per ROM, so this resolves those labels
/// back to [`ChipType`] the way a running device's active type is resolved. A
/// label this build does not know is dropped: it costs one chip type's worth of
/// knowledge about the image, not the answer.
pub fn chip_types(image: &ParsedDevice) -> Vec<ChipType> {
    let mut chips = Vec::new();
    for slot in image.slots().filter(|s| s.kind == SlotKind::Rom) {
        for rom in slot.roms() {
            if let Some(chip) = ChipType::try_from_str(&rom.rom_type)
                && !chips.contains(&chip)
            {
                chips.push(chip);
            }
        }
    }
    chips
}
