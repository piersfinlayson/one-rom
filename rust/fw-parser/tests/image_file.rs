// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for checking an image file's slots against the file's length.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::{Board, BoardSize};
use onerom_config::mcu::{Family, Variant};
use onerom_fw_parser::readers::MemoryReader;
use onerom_fw_parser::{
    ImageFileError, McuLine, McuStorage, ParsedDevice, Parser, SDRR_INFO_FW_OFFSET, Sdrr,
    SdrrCsState, SdrrInfo, SdrrRomSet, SdrrServe, parse_image_file,
};
use onerom_gen::{Builder, FileData, FlashChips};
use onerom_metadata::{
    FirmwareType, METADATA_BASE, ONEROM_FAMILY_MAGIC, ONEROM_INFO_FIRMWARE_TYPE_OFFSET as TYPE_OFF,
    ONEROM_INFO_METADATA_OFFSET as METADATA_OFF, ONEROM_INFO_VERSION,
    ONEROM_INFO_VERSION_OFFSET as VERSION_OFF,
};

const RP235X_FLASH_BASE: u32 = 0x1000_0000;
const RP235X_RAM_BASE: u32 = 0x2008_0000;

/// The length of the firmware and metadata regions, which the ROM data
/// follows.
const ROM_DATA_OFFSET: usize = 0x1_0000;

const KB: usize = 1024;
const MB: usize = 1024 * KB;

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

/// The first chip's addresses on a Fire board.
fn first() -> core::ops::Range<u32> {
    FlashChips::first_for(Variant::RP2350)
}

// ---------------------------------------------------------------------------
// Schema images
// ---------------------------------------------------------------------------

/// A v0.8.0 image file for a `size` Fire32B with a single-chip set of each
/// `chip_types` entry, parsed. Returns the device and the file's length.
fn schema_file(size: BoardSize, chip_types: &[&str]) -> (ParsedDevice, usize) {
    let (image, rom_data) = schema_regions(size, chip_types);
    let len = image.len() + rom_data.len();
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
    (device, len)
}

/// The image file [`schema_file`] parses, as its bytes.
fn schema_file_bytes(size: BoardSize, chip_types: &[&str]) -> Vec<u8> {
    let (mut image, rom_data) = schema_regions(size, chip_types);
    image.extend(rom_data);
    image
}

/// The firmware and metadata regions of [`schema_file`]'s image, then its ROM
/// data.
fn schema_regions(size: BoardSize, chip_types: &[&str]) -> (Vec<u8>, Vec<u8>) {
    let version = FirmwareVersion::new(0, 8, 0, 0);
    let sets: Vec<String> = chip_types
        .iter()
        .enumerate()
        .map(|(n, chip_type)| {
            format!(
                r#"{{ "type": "single", "chips": [{{ "file": "f{n}.bin", "type": "{chip_type}" }}] }}"#
            )
        })
        .collect();
    let json = format!(
        r#"{{ "version": 1, "description": "image file", "chip_sets": [{}] }}"#,
        sets.join(",")
    );
    let mut builder = Builder::from_json(version, Family::Rp2350, &json).unwrap();
    for (n, chip_type) in chip_types.iter().enumerate() {
        let data = match *chip_type {
            "27C040" => vec![0xAA; 512 * KB],
            "system_plugin" | "user_plugin" => plugin_image(),
            other => panic!("no image for {other}"),
        };
        builder.add_file(FileData::new(n, data)).unwrap();
    }
    let props = FirmwareProperties::new(
        version,
        Board::Fire32B,
        Variant::RP2350,
        ServeAlg::Default,
        false,
    )
    .unwrap()
    .with_board_size(size);
    let (metadata, rom_data) = builder.build(props).unwrap();

    // Firmware stands in as a bare onerom_info_t pointing at the metadata.
    let mut image = vec![0u8; ROM_DATA_OFFSET];
    let base = SDRR_INFO_FW_OFFSET as usize;
    let mut put = |offset: usize, bytes: &[u8]| {
        image[base + offset..base + offset + bytes.len()].copy_from_slice(bytes);
    };
    put(0, ONEROM_FAMILY_MAGIC.as_bytes());
    put(6, &8u16.to_le_bytes()); // minor version
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
    (image, rom_data)
}

/// A plugin image the builder accepts.
fn plugin_image() -> Vec<u8> {
    let mut image = vec![0u8; 256];
    image[0..4].copy_from_slice(b"ORA ");
    image[4..8].copy_from_slice(&1u32.to_le_bytes());
    image
}

#[test]
fn a_first_chip_image_passes() {
    let (device, len) = schema_file(BoardSize::M, &["27C040"; 3]);
    assert_eq!(device.check_image_file(len, first()), Ok(()));
}

/// Three 512KB sets fill the first chip, so a fourth is on the second.
#[test]
fn an_image_using_the_second_chip_passes() {
    let (device, len) = schema_file(BoardSize::L, &["27C040"; 4]);
    assert_eq!(len, 2 * MB + 512 * KB);
    assert_eq!(device.check_image_file(len, first()), Ok(()));
}

#[test]
fn a_file_cut_short_of_a_slot_on_the_first_chip_is_refused() {
    let (device, len) = schema_file(BoardSize::M, &["27C040"; 3]);
    assert_eq!(len, ROM_DATA_OFFSET + 3 * 512 * KB);
    assert_eq!(
        device.check_image_file(len - 1, first()),
        Err(ImageFileError::TooShort { short_by: 1 })
    );
}

#[test]
fn a_file_cut_short_of_a_slot_on_the_second_chip_is_refused() {
    let (device, len) = schema_file(BoardSize::L, &["27C040"; 4]);
    for short in [len - 1, 2 * MB + 1, 2 * MB] {
        assert_eq!(
            device.check_image_file(short, first()),
            Err(ImageFileError::TooShort {
                short_by: len - short
            }),
            "{short:#x}"
        );
    }
}

/// The file must hold the last slot's data, and plugins count among the
/// slots.
#[test]
fn a_file_must_hold_the_slot_that_ends_last() {
    let (device, len) = schema_file(BoardSize::M, &["system_plugin", "user_plugin", "27C040"]);
    assert_eq!(
        device.check_image_file(len - 1, first()),
        Err(ImageFileError::TooShort { short_by: 1 })
    );
}

#[test]
fn a_file_longer_than_the_first_chip_needs_a_slot_on_the_second() {
    let (device, _) = schema_file(BoardSize::M, &["27C040"; 3]);
    assert_eq!(
        device.check_image_file(2 * MB + 4 * KB, first()),
        Err(ImageFileError::TooLong {
            too_long_by: 4 * KB
        })
    );
    assert_eq!(device.check_image_file(2 * MB, first()), Ok(()));
}

/// Each slot's data address in `device`'s metadata.
fn slot_addresses(device: &ParsedDevice) -> Vec<Option<u32>> {
    let metadata = device.as_schema().and_then(|onerom| onerom.metadata());
    metadata
        .expect("the image's metadata should parse")
        .rom_slots
        .iter()
        .map(|slot| slot.data.addr())
        .collect()
}

#[test]
fn an_image_file_on_the_first_chip_parses() {
    let file = schema_file_bytes(BoardSize::M, &["27C040"; 3]);
    let device = block_on(parse_image_file(&file, first()));
    assert_eq!(
        slot_addresses(&device),
        [0x1001_0000, 0x1009_0000, 0x1011_0000].map(Some)
    );
    assert_eq!(device.check_image_file(file.len(), first()), Ok(()));
}

/// The part of the file past the first chip is the second chip.
#[test]
fn an_image_file_using_the_second_chip_parses() {
    let file = schema_file_bytes(BoardSize::L, &["27C040"; 4]);
    assert_eq!(file.len(), 2 * MB + 512 * KB);
    let device = block_on(parse_image_file(&file, first()));
    assert_eq!(
        slot_addresses(&device),
        [0x1001_0000, 0x1009_0000, 0x1011_0000, 0x1100_0000].map(Some)
    );
    assert_eq!(device.check_image_file(file.len(), first()), Ok(()));
}

#[test]
fn a_file_that_isnt_an_image_isnt_recognised() {
    let device = block_on(parse_image_file(&[0xFF; 64 * KB], first()));
    assert!(!device.is_recognised());
}

#[test]
fn a_lab_passes() {
    assert_eq!(ParsedDevice::Lab.check_image_file(4 * MB, first()), Ok(()));
}

// ---------------------------------------------------------------------------
// Original-format images
// ---------------------------------------------------------------------------

/// A parsed pre-v0.7.0 device whose ROM sets have these data pointers and
/// sizes.
fn original_device(sets: &[(u32, u32)]) -> ParsedDevice {
    let rom_sets = sets
        .iter()
        .map(|&(data_ptr, size)| SdrrRomSet {
            data_ptr,
            size,
            roms: Vec::new(),
            rom_count: 0,
            serve: SdrrServe::AddrOnCs,
            multi_rom_cs1_state: SdrrCsState::ActiveLow,
            firmware_overrides: None,
        })
        .collect();
    ParsedDevice::Original(Sdrr {
        flash: Some(SdrrInfo {
            major_version: 0,
            minor_version: 6,
            patch_version: 0,
            build_number: 0,
            commit: [0; 8],
            stm_line: McuLine::Rp2350,
            stm_storage: McuStorage::Storage2MB,
            freq: 150,
            overclock: false,
            swd_enabled: true,
            preload_image_to_ram: true,
            bootloader_capable: false,
            status_led_enabled: false,
            boot_logging_enabled: false,
            mco_enabled: false,
            rom_set_count: sets.len() as u8,
            count_rom_access: false,
            boot_config: [0; 4],
            build_date: None,
            hw_rev: None,
            rom_sets,
            pins: None,
            parse_errors: Vec::new(),
            extra_info: None,
            metadata_present: true,
            version: FirmwareVersion::new(0, 6, 0, 0),
            board: Some(Board::Fire24A),
            model: None,
            mcu_variant: Some(Variant::RP2350),
            runtime_info_ptr: 0,
        }),
        ram: None,
    })
}

/// Two 16KB sets from 0x10010000, and a set without data between them.
#[test]
fn an_original_image_is_checked_the_same_way() {
    let device = original_device(&[(0x1001_0000, 0x4000), (0, 0x4000), (0x1001_4000, 0x4000)]);
    let len = ROM_DATA_OFFSET + 0x8000;
    assert_eq!(device.check_image_file(len, first()), Ok(()));
    assert_eq!(
        device.check_image_file(len - 1, first()),
        Err(ImageFileError::TooShort { short_by: 1 })
    );
    assert_eq!(
        device.check_image_file(2 * MB + 1, first()),
        Err(ImageFileError::TooLong { too_long_by: 1 })
    );
}

/// A slot that spans the end of the first chip, or sits between the chips,
/// is in neither, so a file can't hold it.
#[test]
fn a_slot_outside_both_chips_is_refused_whatever_the_length() {
    for set in [(0x101f_c000, 0x8000), (0x1080_0000, 0x4000)] {
        let device = original_device(&[(0x1001_0000, 0x4000), set]);
        for len in [ROM_DATA_OFFSET + 0x4000, 2 * MB, 4 * MB] {
            assert_eq!(
                device.check_image_file(len, first()),
                Err(ImageFileError::BadAddress {
                    slot: 1,
                    addr: set.0
                }),
                "{set:x?} {len:#x}"
            );
        }
    }
}

/// The first slot outside both chips is reported. Its index counts every
/// slot, including one without data.
#[test]
fn a_slot_outside_both_chips_is_reported_by_its_absolute_index() {
    let device = original_device(&[
        (0x1001_0000, 0x4000),
        (0, 0x4000),
        (0x1080_0000, 0x4000),
        (0x1090_0000, 0x4000),
    ]);
    assert_eq!(
        device.check_image_file(ROM_DATA_OFFSET + 0x4000, first()),
        Err(ImageFileError::BadAddress {
            slot: 2,
            addr: 0x1080_0000
        })
    );
}
