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
use onerom_config::mcu::Variant;
use onerom_fw_parser::Parser;
use onerom_fw_parser::device::{ParsedDevice, SlotKind};
use onerom_fw_parser::readers::{MemoryReader, RegionKind};
use onerom_gen::FlashChips;
use onerom_metadata::FLASH_CS1_BASE_ADDR;

/// Parses an image file.
///
/// A file longer than the first flash chip holds the second chip's contents
/// after the first chip's, and they're read at the second chip's address.
pub async fn parse_firmware(data: &[u8]) -> ParsedDevice {
    let first_len = FlashChips::first_for(Variant::RP2350).len();
    let (first, second) = data.split_at(data.len().min(first_len));
    // The hardcoded base address looks odd here, as the STM32's base flash
    // address, but when using a memory reader, onerom-fw-parser will just figure
    // it out for itself based on what it finds in the image.
    let mut reader = MemoryReader::new(first.to_vec(), 0x0800_0000);
    if !second.is_empty() {
        reader.add_region(RegionKind::Flash, second.to_vec(), FLASH_CS1_BASE_ADDR);
    }
    let mut parser = Parser::new(&mut reader);
    parser.parse_device().await
}

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
