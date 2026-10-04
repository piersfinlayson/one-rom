// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `inspect gpio`'s table.

use onerom_cli::gpio;
use onerom_cli::image::parse_firmware;
use onerom_cli::picobootx::{GpioEntry, GpioUse};
use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_fw_parser::ParsedDevice;
use onerom_metadata::{GPIO_NONE, GpioOverride, OneromAlgAddrConfig, OneromAlgCsConfig};

use crate::inspect::render_gpio_table;
use crate::test_board::{image_2364, image_2364_for};

fn entries(board: &Board) -> Vec<GpioEntry> {
    (0..30u8)
        .map(|gpio| {
            let gpio_use = if board.data_pins().contains(&gpio) {
                GpioUse::ServingDriven
            } else if gpio::rom_function(board, ChipType::Chip2364, gpio).is_some() {
                GpioUse::ServingRead
            } else if !gpio::system_functions(board, gpio).is_empty() {
                GpioUse::SystemPin
            } else {
                GpioUse::Free
            };
            GpioEntry {
                gpio_use_raw: gpio_use as u8,
                level: 0,
                is_output: u8::from(gpio_use == GpioUse::ServingDriven),
            }
        })
        .collect()
}

/// Mirrors `pio_get_gpio_use` in `firmware/src/piodma/piorom2.c` for the first
/// slot of `image`.
pub(super) fn served_entries(image: &ParsedDevice, board: &Board) -> Vec<GpioEntry> {
    let alg = image
        .as_schema()
        .and_then(|onerom| onerom.metadata())
        .and_then(|metadata| metadata.rom_slots.first())
        .and_then(|slot| slot.alg.as_ref())
        .expect("the first slot's serving configuration");
    let within = |gpio: u8, base: u8, count: u8| gpio >= base && gpio - base < count;

    let (OneromAlgCsConfig::AlgCs0 {
        gpio_base,
        base_cs_pin,
        num_cs_pins,
        base_data_pin,
        num_data_pins,
        ..
    }
    | OneromAlgCsConfig::AlgCs1 {
        gpio_base,
        base_cs_pin,
        num_cs_pins,
        base_data_pin,
        num_data_pins,
        ..
    }
    | OneromAlgCsConfig::AlgCs2 {
        gpio_base,
        base_cs_pin,
        num_cs_pins,
        base_data_pin,
        num_data_pins,
        ..
    }) = alg.alg_cs
    else {
        panic!("a chip select algorithm this harness doesn't know");
    };
    let byte_pin = match alg.alg_cs {
        OneromAlgCsConfig::AlgCs0 { byte_pin, .. } => {
            (byte_pin != GPIO_NONE).then(|| gpio_base + byte_pin)
        }
        OneromAlgCsConfig::AlgCs1 { .. }
        | OneromAlgCsConfig::AlgCs2 { .. }
        | OneromAlgCsConfig::Unknown { .. } => None,
    };
    let (OneromAlgAddrConfig::AlgAddr0 {
        gpio_base: addr_base,
        base_addr_pin,
        num_addr_pins,
        ..
    }
    | OneromAlgAddrConfig::Unknown {
        gpio_base: addr_base,
        base_addr_pin,
        num_addr_pins,
        ..
    }) = alg.alg_addr;
    let forced: Vec<u8> = alg
        .gpio_override_config
        .iter()
        .flat_map(|config| config.params.iter())
        .filter(|&&entry| {
            entry >> 6 == GpioOverride::GpioOverLow as u8
                || entry >> 6 == GpioOverride::GpioOverHigh as u8
        })
        .map(|&entry| entry & 0x3F)
        .collect();

    (0..30u8)
        .map(|gpio| {
            let system = !gpio::system_functions(board, gpio).is_empty();
            let gpio_use = if within(gpio, gpio_base + base_data_pin, num_data_pins) {
                GpioUse::ServingDriven
            } else if within(gpio, gpio_base + base_cs_pin, num_cs_pins) || byte_pin == Some(gpio) {
                GpioUse::ServingRead
            } else if forced.contains(&gpio) {
                if system {
                    GpioUse::SystemPin
                } else {
                    GpioUse::InputForced
                }
            } else if within(gpio, addr_base + base_addr_pin, num_addr_pins) {
                GpioUse::ServingRead
            } else if system {
                GpioUse::SystemPin
            } else {
                GpioUse::Free
            };
            GpioEntry {
                gpio_use_raw: gpio_use as u8,
                level: 0,
                is_output: u8::from(gpio_use == GpioUse::ServingDriven),
            }
        })
        .collect()
}

/// A fire-24-a's X1 and X2 are among its address pins.
async fn fire_24_a(reserved: &[&str], verbose: bool) {
    let board = Board::Fire24A;
    let image = parse_firmware(&image_2364_for(board, 1, reserved)).await;
    println!(
        "$ onerom inspect gpio{}",
        if verbose { " --verbose" } else { "" }
    );
    println!("~ One ROM Fire 24 A - Firmware: v0.8.0 State: Running Serial: DE3F9C232F655B6B");
    println!("~");
    println!("~ GPIO state  ·  One ROM Fire 24 (rev A)  ·  RP235xA  ·  serving 2364");
    println!("~");
    print!(
        "{}",
        render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            0,
            &served_entries(&image, &board),
            false,
            verbose,
            onerom_cli::pin::reserved_gpios(&image),
        )
    );
}

#[tokio::test]
async fn input_forced_pins() {
    fire_24_a(&[], true).await;
}

#[tokio::test]
async fn an_input_forced_pin_reserved() {
    fire_24_a(&["x1"], false).await;
}

#[tokio::test]
async fn reserved_pins() {
    let board = Board::Fire24F;
    let image = parse_firmware(&image_2364(1, &["sel_c", "x1"])).await;
    let reserved = onerom_cli::pin::reserved_gpios(&image);
    println!("$ onerom inspect gpio --verbose");
    println!("~ One ROM Fire 24 F - Firmware: v0.8.0 State: Running Serial: DE3F9C232F655B6B");
    println!("~");
    println!("~ GPIO state  ·  One ROM Fire 24 (rev F)  ·  RP235xA  ·  serving 2364");
    println!("~");
    print!(
        "{}",
        render_gpio_table(
            Some(&board),
            Some(ChipType::Chip2364),
            0,
            &entries(&board),
            false,
            true,
            reserved,
        )
    );
}
