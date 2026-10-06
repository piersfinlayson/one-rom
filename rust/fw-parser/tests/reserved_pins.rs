// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the reserved pins a device's metadata records.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::Board;
use onerom_config::mcu::{Family, Variant};
use onerom_config::pin::{HeaderPin, ReservedPins};
use onerom_fw_parser::readers::MemoryReader;
use onerom_fw_parser::{ParsedDevice, Parser, SDRR_INFO_FW_OFFSET};
use onerom_gen::{Builder, FileData};
use onerom_metadata::{
    FLASH_CS0_BASE_ADDR, FirmwareType, METADATA_BASE, ONEROM_FAMILY_MAGIC,
    ONEROM_INFO_FIRMWARE_TYPE_OFFSET as TYPE_OFF, ONEROM_INFO_METADATA_OFFSET as METADATA_OFF,
    ONEROM_INFO_VERSION, ONEROM_INFO_VERSION_OFFSET as VERSION_OFF,
};

const RP235X_FLASH_BASE: u32 = FLASH_CS0_BASE_ADDR;
const RP235X_RAM_BASE: u32 = 0x2008_0000;

/// The length of the firmware and metadata regions.
const ROM_DATA_OFFSET: usize = 0x1_0000;

/// Runs a future to completion.  The reader is backed by memory and never
/// pends, so one poll always completes.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("in-memory reader pended"),
    }
}

/// A v0.`minor`.0 fire-24-f image with one 2364 and `reserved`, parsed from its
/// firmware and metadata regions.
fn device(minor: u16, reserved: &[&str]) -> ParsedDevice {
    let version = FirmwareVersion::new(0, minor, 0, 0);
    let reserved: Vec<String> = reserved.iter().map(|pin| format!("\"{pin}\"")).collect();
    let json = format!(
        r#"{{ "version": 1, "description": "reserved pins", "reserved_pins": [{}],
              "chip_sets": [{{ "type": "single",
                  "chips": [{{ "file": "f.bin", "type": "2364", "cs1": "active_low" }}] }}] }}"#,
        reserved.join(",")
    );
    let mut builder = Builder::from_json(version, Family::Rp2350, &json).unwrap();
    builder.add_file(FileData::new(0, vec![0; 8192])).unwrap();
    let props = FirmwareProperties::new(
        version,
        Board::Fire24F,
        Variant::RP2350,
        ServeAlg::Default,
        false,
    )
    .unwrap();
    let (metadata, _) = builder.build(props).unwrap();

    // Firmware stands in as a bare onerom_info_t pointing at the metadata.
    let mut image = vec![0u8; ROM_DATA_OFFSET];
    let base = SDRR_INFO_FW_OFFSET as usize;
    let mut put = |offset: usize, bytes: &[u8]| {
        image[base + offset..base + offset + bytes.len()].copy_from_slice(bytes);
    };
    put(0, ONEROM_FAMILY_MAGIC.as_bytes());
    put(6, &minor.to_le_bytes());
    // build_date, into the zeroed tail of the header's page
    put(12, &(RP235X_FLASH_BASE + 0x300).to_le_bytes());
    put(METADATA_OFF, &METADATA_BASE.to_le_bytes());
    put(VERSION_OFF, &ONEROM_INFO_VERSION.to_le_bytes());
    put(
        TYPE_OFF,
        &(FirmwareType::FirmwareTypeOneRom as u16).to_le_bytes(),
    );
    let metadata_offset = (METADATA_BASE - RP235X_FLASH_BASE) as usize;
    image[metadata_offset..metadata_offset + metadata.len()].copy_from_slice(&metadata);

    let mut reader = MemoryReader::new(image, RP235X_FLASH_BASE);
    let device = block_on(
        Parser::with_base_flash_address(&mut reader, RP235X_FLASH_BASE, RP235X_RAM_BASE)
            .parse_device(),
    );
    assert!(
        device
            .as_schema()
            .and_then(|onerom| onerom.metadata())
            .is_some(),
        "the image's metadata should parse"
    );
    device
}

#[test]
fn the_reserved_pins_are_read_from_the_metadata() {
    let mut expected = ReservedPins::new();
    expected.insert(HeaderPin::Select(2));
    expected.insert(HeaderPin::X1);
    assert_eq!(device(8, &["sel_c", "x1"]).reserved_pins(), Some(expected));
}

#[test]
fn metadata_without_reserved_pins_has_an_empty_set() {
    assert_eq!(device(8, &[]).reserved_pins(), Some(ReservedPins::new()));
}

/// v0.7.0 metadata predates reserved pins.
#[test]
fn older_metadata_has_none() {
    assert_eq!(device(7, &[]).reserved_pins(), None);
}

#[test]
fn a_one_rom_lab_has_none() {
    assert_eq!(ParsedDevice::Lab.reserved_pins(), None);
}
