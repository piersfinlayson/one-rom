// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Which firmware versions each parsing path accepts.
//!
//! Two boundaries are asserted here.  The first is between the two firmware
//! formats.  Everything below v0.7.0 is the original format and everything
//! from v0.7.0 up is schema format, whatever its major version.  The second
//! is the ceiling on the schema path, `MAX_VERSION_*`, above which the
//! offsets this build reads are not known to be right.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use onerom_fw_parser::readers::MemoryReader;
use onerom_fw_parser::{FirmwareFormat, Parser, SDRR_INFO_FW_OFFSET};

const STM32F4_FLASH_BASE: u32 = 0x0800_0000;
const STM32F4_RAM_BASE: u32 = 0x2000_0000;
const RP235X_FLASH_BASE: u32 = 0x1000_0000;
const RP235X_RAM_BASE: u32 = 0x2008_0000;

/// Runs a future to completion.
///
/// Every reader used here is backed by memory and never pends, so a single
/// poll always completes.
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

/// A flash image holding a pre-v0.7.0 `sdrr_info_t` at the usual offset.
///
/// Every field beyond the version is zero, which parses.  The two MCU enums
/// both take 0, and the pointers are caught as invalid while the header
/// itself is read.
fn original_image(major: u16, minor: u16, patch: u16) -> Vec<u8> {
    let mut image = vec![0u8; 0x400];
    let base = SDRR_INFO_FW_OFFSET as usize;
    image[base..base + 4].copy_from_slice(b"SDRR");
    image[base + 4..base + 6].copy_from_slice(&major.to_le_bytes());
    image[base + 6..base + 8].copy_from_slice(&minor.to_le_bytes());
    image[base + 8..base + 10].copy_from_slice(&patch.to_le_bytes());
    image
}

/// A flash image holding a v0.7.0+ `onerom_info_t` at the usual offset.
///
/// `build_date` points at a zero byte inside the image, and the metadata and
/// runtime pointers are null, so the whole header parses with those two
/// absent.
fn schema_image(major: u16, minor: u16, patch: u16) -> Vec<u8> {
    let mut image = vec![0u8; 0x400];
    let base = SDRR_INFO_FW_OFFSET as usize;
    image[base..base + 4].copy_from_slice(b"SDRR");
    image[base + 4..base + 6].copy_from_slice(&major.to_le_bytes());
    image[base + 6..base + 8].copy_from_slice(&minor.to_le_bytes());
    image[base + 8..base + 10].copy_from_slice(&patch.to_le_bytes());
    // build_date, into the zeroed tail of the image
    image[base + 12..base + 16].copy_from_slice(&(RP235X_FLASH_BASE + 0x300).to_le_bytes());
    // onerom_info_t structure version
    image[base + 24..base + 28].copy_from_slice(&2u32.to_le_bytes());
    image
}

fn original_parser(reader: &mut MemoryReader) -> Parser<'_, MemoryReader> {
    Parser::with_base_flash_address(reader, STM32F4_FLASH_BASE, STM32F4_RAM_BASE)
}

/// A parser reading RP2350 flash, which is where schema-format firmware sits.
fn schema_parser(reader: &mut MemoryReader) -> Parser<'_, MemoryReader> {
    Parser::with_base_flash_address(reader, RP235X_FLASH_BASE, RP235X_RAM_BASE)
}

// ---------------------------------------------------------------------------
// Which format each version is
// ---------------------------------------------------------------------------

#[test]
fn v0_6_0_is_original_format() {
    let mut reader = MemoryReader::new(original_image(0, 6, 0), STM32F4_FLASH_BASE);
    let mut parser = original_parser(&mut reader);
    assert_eq!(
        block_on(parser.detect_format()),
        Some(FirmwareFormat::Original)
    );
}

#[test]
fn v0_7_0_is_schema_format() {
    let mut reader = MemoryReader::new(schema_image(0, 7, 0), RP235X_FLASH_BASE);
    let mut parser = schema_parser(&mut reader);
    assert_eq!(
        block_on(parser.detect_format()),
        Some(FirmwareFormat::Schema)
    );
}

#[test]
fn v1_0_0_is_schema_format() {
    let mut reader = MemoryReader::new(schema_image(1, 0, 0), RP235X_FLASH_BASE);
    let mut parser = schema_parser(&mut reader);
    assert_eq!(
        block_on(parser.detect_format()),
        Some(FirmwareFormat::Schema)
    );
}

// ---------------------------------------------------------------------------
// The original path refuses every schema-format version
// ---------------------------------------------------------------------------

#[test]
fn original_path_takes_v0_6_0() {
    let mut reader = MemoryReader::new(original_image(0, 6, 0), STM32F4_FLASH_BASE);
    let mut parser = original_parser(&mut reader);
    let info = block_on(parser.parse_flash()).expect("v0.6.0 should parse");
    assert_eq!(
        (info.major_version, info.minor_version, info.patch_version),
        (0, 6, 0)
    );
}

#[test]
fn original_path_refuses_v0_7_0() {
    let mut reader = MemoryReader::new(original_image(0, 7, 0), STM32F4_FLASH_BASE);
    let mut parser = original_parser(&mut reader);
    assert!(block_on(parser.parse_flash()).is_err());
}

#[test]
fn original_path_refuses_v1_0_0() {
    let mut reader = MemoryReader::new(original_image(1, 0, 0), STM32F4_FLASH_BASE);
    let mut parser = original_parser(&mut reader);
    assert!(block_on(parser.parse_flash()).is_err());
}

// ---------------------------------------------------------------------------
// The schema path's ceiling
// ---------------------------------------------------------------------------

#[test]
fn schema_path_takes_v0_7_999() {
    let mut reader = MemoryReader::new(schema_image(0, 7, 999), RP235X_FLASH_BASE);
    let mut parser = schema_parser(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("v0.7.999 should parse");
    let info = onerom.info().expect("info should be present");
    assert_eq!((info.minor_version, info.patch_version), (7, 999));
}

#[test]
fn schema_path_refuses_v0_8_0() {
    let mut reader = MemoryReader::new(schema_image(0, 8, 0), RP235X_FLASH_BASE);
    let mut parser = schema_parser(&mut reader);
    let err = block_on(parser.parse_format_schema()).unwrap_err();
    assert!(err.contains("unsupported"), "unexpected error: {err}");
}

#[test]
fn schema_path_refuses_v1_0_0() {
    let mut reader = MemoryReader::new(schema_image(1, 0, 0), RP235X_FLASH_BASE);
    let mut parser = schema_parser(&mut reader);
    let err = block_on(parser.parse_format_schema()).unwrap_err();
    assert!(err.contains("unsupported"), "unexpected error: {err}");
}

#[test]
fn a_v0_8_0_device_is_not_recognised() {
    let mut reader = MemoryReader::new(schema_image(0, 8, 0), RP235X_FLASH_BASE);
    let mut parser = schema_parser(&mut reader);
    let device = block_on(parser.parse_device());
    assert!(!device.is_recognised());
}

// ---------------------------------------------------------------------------
// detect() answers "is this a One ROM", not "can this build read it"
// ---------------------------------------------------------------------------

#[test]
fn detect_finds_a_v0_8_0_device() {
    let mut reader = MemoryReader::new(schema_image(0, 8, 0), RP235X_FLASH_BASE);
    let mut parser = schema_parser(&mut reader);
    assert!(block_on(parser.detect()));
}

#[test]
fn detect_rejects_data_that_is_not_a_one_rom() {
    let mut reader = MemoryReader::new(vec![0u8; 0x400], RP235X_FLASH_BASE);
    let mut parser = schema_parser(&mut reader);
    assert!(!block_on(parser.detect()));
}
