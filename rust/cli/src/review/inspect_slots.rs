// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `inspect slots`'s failure for firmware this build doesn't recognise.

use super::{
    UNRECOGNISED, damaged_header_flash, failed, flash_without_firmware, newer_firmware_flash,
    unrecognised_reasons,
};
use crate::inspect::unrecognised_firmware_error;

/// Prints what `inspect slots --unrecognised` prints for a stopped board whose
/// flash holds `image` and whose OTP isn't commissioned. Only the failure
/// comes from running the code.
async fn inspect_slots(image: Vec<u8>) {
    let reasons = unrecognised_reasons(image).await;
    println!("$ onerom inspect slots --unrecognised");
    println!("~ {UNRECOGNISED}");
    failed(unrecognised_firmware_error(&reasons));
}

/// Firmware newer than this build reads. The parser provides one reason.
#[tokio::test]
async fn newer_firmware() {
    inspect_slots(newer_firmware_flash()).await;
}

/// A v0.8.0 header with a null build date pointer, whose metadata is zeros.
/// The parser provides two reasons.
#[tokio::test]
async fn a_damaged_header() {
    inspect_slots(damaged_header_flash()).await;
}

/// Erased flash, then flash reading all zeros. The parser doesn't provide a
/// reason for either.
#[tokio::test]
async fn no_one_rom_firmware() {
    for (name, image) in flash_without_firmware() {
        println!("### {name}");
        inspect_slots(image).await;
        println!();
    }
}

#[tokio::test]
async fn reserved_pins() {
    use crate::inspect::reserved_pins_line;
    use crate::test_board::image_2364;
    use onerom_cli::image::parse_firmware;

    let image = parse_firmware(&image_2364(3, &["sel_c", "x1"])).await;
    println!("$ onerom inspect slots");
    println!("~ One ROM Fire 24 F - Firmware: v0.8.0 State: Running Serial: DE3F9C232F655B6B");
    println!("~   Configured with 3 slots - Slot 0 is active");
    if let Some(line) = reserved_pins_line(&image) {
        println!("  {line}");
    }
    println!("~   Slot 0 (active):");
    println!("~     Chip 0: 2364");
}
