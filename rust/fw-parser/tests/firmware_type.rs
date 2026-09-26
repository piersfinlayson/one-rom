// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What `onerom_info_t.firmware_type` reads as, across the generation it
//! arrived in, and what each value makes of the device.
//!
//! The field came out of the structure's trailing reserved padding at
//! generation 3.  A device written at generation 2 has no bytes for it, and a
//! host reading one must see One ROM rather than whatever those bytes happen
//! to hold - the images here zero them, so a parser reading them raw would
//! answer 0 and fail these tests.
//!
//! A Lab is named and not parsed, and One ROM's version limits don't apply
//! to it.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use airfrog_rpc::io::Reader;
use onerom_fw_parser::readers::MemoryReader;
use onerom_fw_parser::{ParsedDevice, Parser, SDRR_INFO_FW_OFFSET};
use onerom_metadata::{
    FirmwareType, MaybeKnown, ONEROM_FAMILY_MAGIC, ONEROM_INFO_FIRMWARE_TYPE_OFFSET as TYPE_OFF,
    ONEROM_INFO_MAGIC, ONEROM_INFO_METADATA_OFFSET as METADATA_OFF,
    ONEROM_INFO_RUNTIME_OFFSET as RUNTIME_OFF, ONEROM_INFO_VERSION_OFFSET as VERSION_OFF,
};

const RP235X_FLASH_BASE: u32 = 0x1000_0000;
const RP235X_RAM_BASE: u32 = 0x2008_0000;

const LAB: u16 = FirmwareType::FirmwareTypeLab as u16;
const ONE_ROM: u16 = FirmwareType::FirmwareTypeOneRom as u16;

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

fn with_version(mut image: Vec<u8>, major: u16, minor: u16, patch: u16) -> Vec<u8> {
    let base = SDRR_INFO_FW_OFFSET as usize;
    image[base + 4..base + 6].copy_from_slice(&major.to_le_bytes());
    image[base + 6..base + 8].copy_from_slice(&minor.to_le_bytes());
    image[base + 8..base + 10].copy_from_slice(&patch.to_le_bytes());
    image
}

/// A generation 3 Lab at the version given.
fn lab(major: u16, minor: u16, patch: u16) -> Vec<u8> {
    with_version(with_firmware_type(image(3), LAB), major, minor, patch)
}

fn parser(reader: &mut MemoryReader) -> Parser<'_, MemoryReader> {
    Parser::with_base_flash_address(reader, RP235X_FLASH_BASE, RP235X_RAM_BASE)
}

fn firmware_type_of(image: Vec<u8>) -> MaybeKnown<FirmwareType> {
    let mut reader = MemoryReader::new(image, RP235X_FLASH_BASE);
    let onerom = block_on(parser(&mut reader).parse_format_schema()).expect("image should parse");
    onerom.info().expect("info should be present").firmware_type
}

fn device_of(image: Vec<u8>) -> ParsedDevice {
    let mut reader = MemoryReader::new(image, RP235X_FLASH_BASE);
    block_on(parser(&mut reader).parse_device())
}

/// A reader that records the address of every read it serves.
struct Recording {
    inner: MemoryReader,
    reads: Vec<u32>,
}

impl Reader for Recording {
    type Error = <MemoryReader as Reader>::Error;

    async fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), Self::Error> {
        self.reads.push(addr);
        self.inner.read(addr, buf).await
    }

    fn update_base_address(&mut self, base_address: u32) {
        self.inner.update_base_address(base_address);
    }
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
        firmware_type_of(with_firmware_type(image(3), ONE_ROM)),
        MaybeKnown::Known(FirmwareType::FirmwareTypeOneRom)
    );
}

#[test]
fn a_generation_3_device_naming_lab_is_a_lab() {
    let device = device_of(with_firmware_type(image(3), LAB));
    assert!(matches!(device, ParsedDevice::Lab));
    assert!(device.is_recognised());
}

#[test]
fn an_unallocated_value_is_not_recognised_and_is_named() {
    let device = device_of(with_firmware_type(image(3), 2));
    assert!(!device.is_recognised());
    assert!(
        device
            .parse_errors()
            .iter()
            .any(|e| e.reason.contains("0x0002")),
        "the error names the value the device stored: {:?}",
        device.parse_errors()
    );
}

#[test]
fn a_lab_below_one_roms_oldest_version_is_a_lab() {
    assert!(matches!(device_of(lab(0, 4, 0)), ParsedDevice::Lab));
}

#[test]
fn a_lab_above_one_roms_newest_version_is_a_lab() {
    assert!(matches!(device_of(lab(0, 9, 0)), ParsedDevice::Lab));
}

#[test]
fn the_one_rom_parse_refuses_a_lab() {
    let mut reader = MemoryReader::new(lab(0, 4, 0), RP235X_FLASH_BASE);
    let err = block_on(parser(&mut reader).parse_format_schema()).unwrap_err();
    assert!(err.contains("Lab"), "unexpected error: {err}");
}

#[test]
fn a_labs_own_structures_are_not_read() {
    let metadata = RP235X_FLASH_BASE + 0x380;
    let runtime = RP235X_RAM_BASE;
    let mut image = lab(0, 4, 0);
    let base = SDRR_INFO_FW_OFFSET as usize;
    image[base + METADATA_OFF..base + METADATA_OFF + 4].copy_from_slice(&metadata.to_le_bytes());
    image[base + RUNTIME_OFF..base + RUNTIME_OFF + 4].copy_from_slice(&runtime.to_le_bytes());

    let mut reader = Recording {
        inner: MemoryReader::new(image, RP235X_FLASH_BASE),
        reads: Vec::new(),
    };
    let device = block_on(
        Parser::with_base_flash_address(&mut reader, RP235X_FLASH_BASE, RP235X_RAM_BASE)
            .parse_device(),
    );

    assert!(matches!(device, ParsedDevice::Lab));
    assert!(
        !reader.reads.contains(&metadata) && !reader.reads.contains(&runtime),
        "read {:#010x?}",
        reader.reads
    );
}

#[test]
fn a_lab_answers_none_of_one_roms_questions() {
    let device = device_of(lab(0, 4, 0));
    assert!(device.parse_errors().is_empty());
    assert!(device.version().is_none());
    assert!(!device.is_running());
    assert!(device.metadata_present().is_none());
    assert!(device.get_board().is_none());
    assert!(!device.is_usb_run_capable());
    assert!(device.mcu_name().is_none());
    assert!(device.active_slot_index().is_none());
    assert!(device.slots().next().is_none());
    assert!(device.as_original().is_none() && device.as_schema().is_none());
}

/// Only One ROM wrote SDRR, and below v0.7.0 the bytes are an `sdrr_info_t`.
/// Lab's value where `onerom_info_t` keeps `firmware_type` doesn't make it a
/// Lab.
#[test]
fn an_sdrr_image_below_v0_7_0_is_never_a_lab() {
    let mut image = with_version(with_firmware_type(image(3), LAB), 0, 6, 0);
    let base = SDRR_INFO_FW_OFFSET as usize;
    image[base..base + 4].copy_from_slice(ONEROM_INFO_MAGIC.as_bytes());

    let mut reader = MemoryReader::new(image.clone(), RP235X_FLASH_BASE);
    let err = block_on(parser(&mut reader).parse_format_schema()).unwrap_err();
    assert!(err.contains("not schema format"), "unexpected error: {err}");

    assert!(matches!(device_of(image), ParsedDevice::Original(_)));
}

#[test]
fn the_one_rom_parse_refuses_an_unknown_firmware_type() {
    let mut reader = MemoryReader::new(with_firmware_type(image(3), 2), RP235X_FLASH_BASE);
    let err = block_on(parser(&mut reader).parse_format_schema()).unwrap_err();
    assert!(err.contains("0x0002"), "unexpected error: {err}");
}
