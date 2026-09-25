// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What `onerom_info_t.firmware_type` reads as, across the generation it
//! arrived in.
//!
//! The field came out of the structure's trailing reserved padding at
//! generation 3.  A device written at generation 2 has no bytes for it, and a
//! host reading one must see One ROM rather than whatever those bytes happen
//! to hold - the images here zero them, so a parser reading them raw would
//! answer 0 and fail these tests.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use onerom_fw_parser::readers::MemoryReader;
use onerom_fw_parser::{Parser, SDRR_INFO_FW_OFFSET};
use onerom_metadata::{
    FirmwareType, MaybeKnown, ONEROM_FAMILY_MAGIC, ONEROM_INFO_FIRMWARE_TYPE_OFFSET as TYPE_OFF,
    ONEROM_INFO_MAGIC, ONEROM_INFO_VERSION_OFFSET as VERSION_OFF,
};

const RP235X_FLASH_BASE: u32 = 0x1000_0000;
const RP235X_RAM_BASE: u32 = 0x2008_0000;

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

/// A flash image whose `onerom_info_t` carries the given generation.
///
/// Generation 3 arrived in firmware v0.8.0 and generation 2 in v0.7.0, so each
/// gets the version and the magic a real device of that generation carries.
///
/// `firmware_type` is left zeroed, which is not a value any generation
/// allocates.  A caller wanting one writes it with [`with_firmware_type`].
fn image(info_generation: u32) -> Vec<u8> {
    let mut image = vec![0u8; 0x400];
    let base = SDRR_INFO_FW_OFFSET as usize;
    let (minor, magic) = if info_generation >= 3 {
        (8u16, ONEROM_FAMILY_MAGIC)
    } else {
        (7u16, ONEROM_INFO_MAGIC)
    };
    image[base..base + 4].copy_from_slice(magic.as_bytes());
    image[base + 4..base + 6].copy_from_slice(&0u16.to_le_bytes());
    image[base + 6..base + 8].copy_from_slice(&minor.to_le_bytes());
    image[base + 8..base + 10].copy_from_slice(&0u16.to_le_bytes());
    // build_date, into the zeroed tail of the image
    image[base + 12..base + 16].copy_from_slice(&(RP235X_FLASH_BASE + 0x300).to_le_bytes());
    image[base + VERSION_OFF..base + VERSION_OFF + 4]
        .copy_from_slice(&info_generation.to_le_bytes());
    image
}

fn with_firmware_type(mut image: Vec<u8>, value: u16) -> Vec<u8> {
    let base = SDRR_INFO_FW_OFFSET as usize;
    image[base + TYPE_OFF..base + TYPE_OFF + 2].copy_from_slice(&value.to_le_bytes());
    image
}

fn firmware_type_of(image: Vec<u8>) -> MaybeKnown<FirmwareType> {
    let mut reader = MemoryReader::new(image, RP235X_FLASH_BASE);
    let mut parser =
        Parser::with_base_flash_address(&mut reader, RP235X_FLASH_BASE, RP235X_RAM_BASE);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");
    onerom.info().expect("info should be present").firmware_type
}

#[test]
fn a_generation_2_device_reads_as_one_rom() {
    assert_eq!(
        firmware_type_of(image(2)),
        MaybeKnown::Known(FirmwareType::FirmwareTypeOneRom),
        "a device written before the field existed must read as One ROM"
    );
}

#[test]
fn a_generation_3_device_naming_one_rom_reads_as_one_rom() {
    assert_eq!(
        firmware_type_of(with_firmware_type(image(3), 0xFFFF)),
        MaybeKnown::Known(FirmwareType::FirmwareTypeOneRom)
    );
}

#[test]
fn a_generation_3_device_naming_lab_reads_as_lab() {
    assert_eq!(
        firmware_type_of(with_firmware_type(image(3), 1)),
        MaybeKnown::Known(FirmwareType::FirmwareTypeLab)
    );
}

#[test]
fn an_unallocated_value_stays_visible() {
    assert_eq!(
        firmware_type_of(with_firmware_type(image(3), 2)),
        MaybeKnown::Unknown(2),
        "a value this build has no name for is shown as the device stored it"
    );
}
