// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Which magic each parsing path accepts.
//!
//! `ORRM` arrived with firmware v0.8.0 and means One ROM family.  `SDRR` is
//! what every release up to v0.7.x wrote, and those devices stay readable for
//! good, so both are accepted wherever the schema-format header is read.
//!
//! The two are not interchangeable to a host deciding the format.  `SDRR`
//! spans both firmware formats, so which one it is comes from the version.
//! `ORRM` is only ever schema format.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use onerom_fw_parser::readers::MemoryReader;
use onerom_fw_parser::{FirmwareFormat, Parser, SDRR_INFO_FW_OFFSET};
use onerom_metadata::{
    FirmwareType, ONEROM_FAMILY_MAGIC, ONEROM_INFO_FIRMWARE_TYPE_OFFSET as TYPE_OFF,
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

/// A flash image whose header begins with the four bytes given, at the
/// firmware version and structure generation stated.
///
/// `build_date` points at a zero byte inside the image and both pointers are
/// null, so the header parses with the metadata and runtime absent.  The
/// firmware type is One ROM's, as One ROM writes it.
fn image(magic: &[u8], minor: u16, generation: u32) -> Vec<u8> {
    let mut image = vec![0u8; 0x400];
    let base = SDRR_INFO_FW_OFFSET as usize;
    image[base..base + 4].copy_from_slice(magic);
    image[base + 4..base + 6].copy_from_slice(&0u16.to_le_bytes());
    image[base + 6..base + 8].copy_from_slice(&minor.to_le_bytes());
    image[base + 8..base + 10].copy_from_slice(&0u16.to_le_bytes());
    // build_date, into the zeroed tail of the image
    image[base + 12..base + 16].copy_from_slice(&(RP235X_FLASH_BASE + 0x300).to_le_bytes());
    image[base + VERSION_OFF..base + VERSION_OFF + 4].copy_from_slice(&generation.to_le_bytes());
    image[base + TYPE_OFF..base + TYPE_OFF + 2]
        .copy_from_slice(&(FirmwareType::FirmwareTypeOneRom as u16).to_le_bytes());
    image
}

/// A v0.8.0 device, which carries ORRM and structure generation 3.
fn family_image() -> Vec<u8> {
    image(ONEROM_FAMILY_MAGIC.as_bytes(), 8, 3)
}

/// A v0.7.0 device, which carries SDRR and structure generation 2.
fn pre_v0_8_image() -> Vec<u8> {
    image(ONEROM_INFO_MAGIC.as_bytes(), 7, 2)
}

fn parser(reader: &mut MemoryReader) -> Parser<'_, MemoryReader> {
    Parser::with_base_flash_address(reader, RP235X_FLASH_BASE, RP235X_RAM_BASE)
}

#[test]
fn the_family_magic_parses() {
    let mut reader = MemoryReader::new(family_image(), RP235X_FLASH_BASE);
    assert!(block_on(parser(&mut reader).parse_format_schema()).is_ok());
}

#[test]
fn the_pre_v0_8_magic_still_parses() {
    let mut reader = MemoryReader::new(pre_v0_8_image(), RP235X_FLASH_BASE);
    assert!(
        block_on(parser(&mut reader).parse_format_schema()).is_ok(),
        "a device programmed before v0.8.0 stays readable"
    );
}

#[test]
fn the_family_magic_is_schema_format() {
    let mut reader = MemoryReader::new(family_image(), RP235X_FLASH_BASE);
    assert_eq!(
        block_on(parser(&mut reader).detect_format()),
        Some(FirmwareFormat::Schema)
    );
}

#[test]
fn the_family_magic_is_found() {
    let mut reader = MemoryReader::new(family_image(), RP235X_FLASH_BASE);
    assert!(block_on(parser(&mut reader).detect()));
}

#[test]
fn some_other_magic_is_not_a_one_rom() {
    let mut reader = MemoryReader::new(image(b"WXYZ", 8, 3), RP235X_FLASH_BASE);
    assert_eq!(block_on(parser(&mut reader).detect_format()), None);

    let mut reader = MemoryReader::new(image(b"WXYZ", 8, 3), RP235X_FLASH_BASE);
    let err = block_on(parser(&mut reader).parse_format_schema()).unwrap_err();
    assert!(err.contains("Invalid magic"), "unexpected error: {err}");
}

/// The original format is pre-v0.7.0 and its magic never changed, so ORRM is
/// not one of its images.  This is the one new input an existing entry point
/// meets, and `onerom-fw` reaches it when it reads a firmware file.
#[test]
fn the_original_path_refuses_the_family_magic() {
    let mut reader = MemoryReader::new(family_image(), RP235X_FLASH_BASE);
    assert!(block_on(parser(&mut reader).parse_flash()).is_err());
}
