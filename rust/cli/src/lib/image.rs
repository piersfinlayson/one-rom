// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Questions about a composed One ROM firmware image.
//!
//! The answers come from a parse of the image itself, so they hold for an image
//! sitting in a file as much as for one read back off a device - in particular
//! for an image that is about to be flashed, which is the only way to know what
//! a device is going to do before it does it.

use onerom_config::mcu::Variant;
use onerom_fw_parser::device::{ParsedDevice, SlotKind};
use onerom_fw_parser::parse_image_file;
use onerom_gen::FlashChips;

/// Parses an image file.
///
/// A file longer than the first flash chip holds the second chip's contents
/// after the first chip's, and they're read at the second chip's address.
pub async fn parse_firmware(data: &[u8]) -> ParsedDevice {
    parse_image_file(data, FlashChips::first_for(Variant::RP2350)).await
}

/// The ROM slots of `image` that use `gpio`, or another GPIO wired to the same
/// X pin, numbered as `inspect slots` numbers them.
///
/// The X pin wiring and each slot's GPIOs are read from the image's metadata,
/// which is what the firmware acts on. Empty for an image without that
/// metadata.
pub fn slots_using(image: &ParsedDevice, gpio: u8) -> Vec<usize> {
    let Some(metadata) = image.as_schema().and_then(|onerom| onerom.metadata()) else {
        return Vec::new();
    };
    let x_pin = [&metadata.hw.gpio_x1, &metadata.hw.gpio_x2]
        .into_iter()
        .find(|x| x.contains(&gpio));
    let wired = x_pin
        .map_or(&[gpio][..], |x| &x[..])
        .iter()
        .filter_map(|&gpio| 1u64.checked_shl(u32::from(gpio)))
        .fold(0, |mask, bit| mask | bit);
    image
        .slots()
        .zip(&metadata.rom_slots)
        .filter(|(view, slot)| {
            view.kind == SlotKind::Rom
                && slot
                    .alg
                    .as_ref()
                    .is_some_and(|alg| onerom_gen::used_gpios(alg) & wired != 0)
        })
        .filter_map(|(view, _)| view.user_index)
        .collect()
}
