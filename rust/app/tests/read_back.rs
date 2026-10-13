// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Every test config, read back from its image on each board it builds for,
//! rebuilds identically.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use onerom_app::{
    ReadBack, ReadBackError, assemble_image, read_back, read_back_file, read_slot_roms_file,
};
use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::{Board, BoardSize, Model};
use onerom_config::mcu::{Family, RP235X_BASE_FLASH, RP235X_BASE_SRAM, Variant};
use onerom_fw_parser::readers::MemoryReader;
use onerom_fw_parser::{ParsedDevice, Parser, SDRR_INFO_FW_OFFSET};
use onerom_gen::{Builder, ConfigOverrides, FIRMWARE_SIZE, FileData};
use onerom_metadata::{
    FIRMWARE_RELEASE, FLASH_CS0_BASE_ADDR, FirmwareType, METADATA_BASE, METADATA_OFFSET,
    METADATA_SIZE, MaybeKnown, ONEROM_FAMILY_MAGIC, ONEROM_INFO_BUILD_DATE_OFFSET,
    ONEROM_INFO_FIRMWARE_TYPE_OFFSET, ONEROM_INFO_METADATA_OFFSET,
    ONEROM_INFO_MINOR_VERSION_OFFSET, ONEROM_INFO_PATCH_VERSION_OFFSET, ONEROM_INFO_VERSION,
    ONEROM_INFO_VERSION_OFFSET, serialize,
};

const VERSION: FirmwareVersion = FirmwareVersion::new(0, 8, 0, 0);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Base firmware that is a bare `onerom_info_t` pointing at the metadata.
fn base_firmware() -> Vec<u8> {
    let mut image = vec![0u8; 0x1000];
    let base = SDRR_INFO_FW_OFFSET as usize;
    let mut put = |offset: usize, bytes: &[u8]| {
        image[base + offset..base + offset + bytes.len()].copy_from_slice(bytes);
    };
    put(0, ONEROM_FAMILY_MAGIC.as_bytes());
    put(
        ONEROM_INFO_MINOR_VERSION_OFFSET,
        &VERSION.minor().to_le_bytes(),
    );
    // The build date, in the zeroed page after the header.
    put(
        ONEROM_INFO_BUILD_DATE_OFFSET,
        &(FLASH_CS0_BASE_ADDR + 0x800).to_le_bytes(),
    );
    put(ONEROM_INFO_METADATA_OFFSET, &METADATA_BASE.to_le_bytes());
    put(
        ONEROM_INFO_VERSION_OFFSET,
        &ONEROM_INFO_VERSION.to_le_bytes(),
    );
    put(
        ONEROM_INFO_FIRMWARE_TYPE_OFFSET,
        &(FirmwareType::FirmwareTypeOneRom as u16).to_le_bytes(),
    );
    image
}

fn overrides() -> ConfigOverrides {
    ConfigOverrides::default().allow_turbo_boot_multi_slot(true)
}

fn props(board: Board, size: BoardSize) -> FirmwareProperties {
    FirmwareProperties::new(VERSION, board, Variant::RP2350, ServeAlg::Default, false)
        .unwrap()
        .with_board_size(size)
}

/// The metadata and ROM data `json` builds on `board`, its files read from the
/// repository. `None` where it doesn't build there.
fn build(json: &str, board: Board, size: BoardSize) -> Option<(Vec<u8>, Vec<u8>)> {
    let (mut builder, _) =
        Builder::from_json_with_overrides(VERSION, Family::Rp2350, json, &overrides()).unwrap();
    for spec in builder.file_specs() {
        assert!(spec.extract.is_none(), "{}", spec.source);
        let data = std::fs::read(repo_root().join(&spec.source)).unwrap();
        builder.add_file(FileData::new(spec.id, data)).unwrap();
    }
    builder.build(props(board, size)).ok()
}

/// The metadata and ROM data `read_back` builds on `board`.
fn rebuild(read_back: &ReadBack, board: Board, size: BoardSize) -> (Vec<u8>, Vec<u8>) {
    let json = serde_json::to_string(&read_back.config).unwrap();
    let (mut builder, _) =
        Builder::from_json_with_overrides(VERSION, Family::Rp2350, &json, &overrides())
            .unwrap_or_else(|e| panic!("{e}\n{json}"));
    read_back.add_files(&mut builder).unwrap();
    builder.build(props(board, size)).unwrap()
}

/// Runs a future to completion. Everything here is in memory and never
/// pends, so one poll completes it.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("an in-memory future pended"),
    }
}

/// A file and a One ROM's flash without One ROM firmware fail with different
/// errors.
#[test]
fn data_without_one_rom_firmware_fails() {
    let erased = vec![0xff; 0x10000];
    let e = block_on(read_back_file(&erased)).unwrap_err();
    assert_eq!(e, ReadBackError::FileNotOneRom);

    let mut reader = MemoryReader::new(erased, RP235X_BASE_FLASH);
    let parsed = block_on(
        Parser::with_base_flash_address(&mut reader, RP235X_BASE_FLASH, RP235X_BASE_SRAM)
            .parse_device(),
    );
    let e = block_on(read_back(&parsed, &mut reader, None)).unwrap_err();
    assert_eq!(e, ReadBackError::FlashNotOneRom);
}

/// A fire-24-f image of one 2364.
fn image_2364() -> Vec<u8> {
    let json = r#"{ "version": 1, "description": "Test", "chip_sets": [
        { "type": "single", "chips": [{ "file": "kernal.bin", "type": "2364", "cs1": "active_low" }] }
    ] }"#;
    let mut builder = Builder::from_json(VERSION, Family::Rp2350, json).unwrap();
    builder
        .add_file(FileData::new(0, vec![0x5a; 8192]))
        .unwrap();
    let props = props(Board::Fire24F, BoardSize::M);
    let (metadata, roms) = builder.build(props).unwrap();
    block_on(assemble_image(&props, base_firmware(), metadata, roms))
        .unwrap()
        .image
}

/// A published plugin's slot, reused, rebuilds byte for byte with the data
/// the plugin stored in it, and reads back as the same release.
#[test]
fn a_reused_plugin_slot_rebuilds_identically() {
    let json = r#"{ "version": 1, "description": "Test", "chip_sets": [
        { "type": "single", "chips": [{ "file": "https://images.onerom.org/plugins/system/usb/v0.3.1/plugin.bin", "type": "system_plugin" }] },
        { "type": "single", "chips": [{ "file": "kernal.bin", "type": "2364", "cs1": "active_low" }] }
    ] }"#;
    let mut builder = Builder::from_json(VERSION, Family::Rp2350, json).unwrap();
    let stub = std::fs::read(repo_root().join("images/test/stub-system-plugin.bin")).unwrap();
    builder.add_file(FileData::new(0, stub)).unwrap();
    builder
        .add_file(FileData::new(1, vec![0x5a; 8192]))
        .unwrap();
    let props = props(Board::Fire24F, BoardSize::M);
    let (metadata, roms) = builder.build(props).unwrap();
    let mut image = block_on(assemble_image(&props, base_firmware(), metadata, roms))
        .unwrap()
        .image;

    let mut reader = MemoryReader::new(image.clone(), RP235X_BASE_FLASH);
    let parsed = block_on(
        Parser::with_base_flash_address(&mut reader, RP235X_BASE_FLASH, RP235X_BASE_SRAM)
            .parse_device(),
    );
    let slot = &parsed.as_schema().unwrap().metadata().unwrap().rom_slots[0];
    let end = (slot.data.addr().unwrap() - RP235X_BASE_FLASH + slot.size) as usize;
    image[end - 0x1000..end].fill(0xd0);

    let mut read_back = block_on(read_back_file(&image)).unwrap();
    let plugins = read_back.plugins.clone();
    assert_eq!(plugins.len(), 1);
    read_back.reuse_plugin_slots();
    let (metadata, roms) = rebuild(&read_back, Board::Fire24F, BoardSize::M);
    let rebuilt = block_on(assemble_image(&props, base_firmware(), metadata, roms))
        .unwrap()
        .image;
    assert!(rebuilt == image);
    assert_eq!(block_on(read_back_file(&rebuilt)).unwrap().plugins, plugins);
}

/// `image` with the `u16` or `u32` at `offset` in `onerom_info_t` set to
/// `value`.
fn with_info(mut image: Vec<u8>, offset: usize, value: &[u8]) -> Vec<u8> {
    let at = SDRR_INFO_FW_OFFSET as usize + offset;
    image[at..at + value.len()].copy_from_slice(value);
    image
}

/// A file's and a One ROM's base firmware is the image's first
/// `FIRMWARE_SIZE` bytes.
#[test]
fn the_base_firmware_is_read_back() {
    let mut image = image_2364();
    // Code no release contains.
    image[0x1000..0x1100].fill(0xa5);
    let base_firmware = &image[..FIRMWARE_SIZE];

    let file = block_on(read_back_file(&image)).unwrap();
    assert!(file.base_firmware == base_firmware);

    let mut reader = MemoryReader::new(image.clone(), RP235X_BASE_FLASH);
    let parsed = block_on(
        Parser::with_base_flash_address(&mut reader, RP235X_BASE_FLASH, RP235X_BASE_SRAM)
            .parse_device(),
    );
    let device = block_on(read_back(&parsed, &mut reader, None)).unwrap();
    assert!(device.base_firmware == base_firmware);
}

/// Firmware newer than the parser reads, a release newer than this build's,
/// and a newer structure generation each fail with the firmware's version.
#[test]
fn newer_firmware_fails_with_its_version() {
    let image = image_2364();
    let next = FirmwareVersion::new(
        FIRMWARE_RELEASE.major(),
        FIRMWARE_RELEASE.minor(),
        FIRMWARE_RELEASE.patch() + 1,
        0,
    );
    let next_release = with_info(
        with_info(
            image.clone(),
            ONEROM_INFO_MINOR_VERSION_OFFSET,
            &next.minor().to_le_bytes(),
        ),
        ONEROM_INFO_PATCH_VERSION_OFFSET,
        &next.patch().to_le_bytes(),
    );
    let next_generation = with_info(
        image,
        ONEROM_INFO_VERSION_OFFSET,
        &(ONEROM_INFO_VERSION + 1).to_le_bytes(),
    );
    let past_parser = with_info(
        base_firmware(),
        ONEROM_INFO_MINOR_VERSION_OFFSET,
        &9u16.to_le_bytes(),
    );
    for (image, version) in [
        (next_release, next),
        (next_generation, VERSION),
        (past_parser, FirmwareVersion::new(0, 9, 0, 0)),
    ] {
        let e = block_on(read_back_file(&image)).unwrap_err();
        assert_eq!(e, ReadBackError::FirmwareTooNew { version });
        let e = block_on(read_slot_roms_file(&image, 0)).unwrap_err();
        assert_eq!(e, ReadBackError::FirmwareTooNew { version });
    }
}

/// An unrecognised value that config recovery reads fails with the firmware's
/// version.
#[test]
fn an_unknown_value_fails_with_the_firmware_version() {
    let mut image = image_2364();
    let mut reader = MemoryReader::new(image.clone(), RP235X_BASE_FLASH);
    let mut parsed = block_on(
        Parser::with_base_flash_address(&mut reader, RP235X_BASE_FLASH, RP235X_BASE_SRAM)
            .parse_device(),
    );
    let ParsedDevice::Schema(onerom) = &mut parsed else {
        panic!("not schema firmware");
    };
    let metadata = onerom.metadata_mut().unwrap();
    metadata.rom_slots[0].slot_type = MaybeKnown::Unknown(99);
    let mut buf = vec![0; METADATA_SIZE];
    serialize(metadata, METADATA_BASE, &mut buf).unwrap();
    let at = METADATA_OFFSET as usize;
    image[at..at + METADATA_SIZE].copy_from_slice(&buf);

    let e = block_on(read_back_file(&image)).unwrap_err();
    assert_eq!(e, ReadBackError::FirmwareTooNew { version: VERSION });
    let e = block_on(read_slot_roms_file(&image, 0)).unwrap_err();
    assert_eq!(e, ReadBackError::FirmwareTooNew { version: VERSION });
}

/// Builds `path` on every board, reads each image back, rebuilds and compares.
/// Returns how many boards it built on.
fn round_trip(path: &Path) -> usize {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let json = std::fs::read_to_string(path).unwrap();
    let mut built = 0;
    for &board in Model::Fire.boards() {
        let first = BoardSize::supported_values()
            .iter()
            .find_map(|&size| build(&json, board, size).map(|parts| (size, parts)));
        let Some((size, (metadata, roms))) = first else {
            continue;
        };
        built += 1;

        let image = block_on(assemble_image(
            &props(board, size),
            base_firmware(),
            metadata.clone(),
            roms.clone(),
        ))
        .unwrap();
        assert!(
            image.problems.is_empty(),
            "{name} on {}: {:?}",
            board.name(),
            image.problems
        );
        let read_back = block_on(read_back_file(&image.image))
            .unwrap_or_else(|e| panic!("{name} on {}: {e}", board.name()));
        assert!(
            read_back.problems.is_empty(),
            "{name} on {}: {:?}",
            board.name(),
            read_back.problems
        );
        assert_eq!(read_back.board, board, "{name}");
        assert_eq!(read_back.version, VERSION, "{name}");

        let (rebuilt_metadata, rebuilt_roms) = rebuild(&read_back, board, size);
        assert!(
            rebuilt_metadata == metadata,
            "{name} on {}: the metadata differs",
            board.name()
        );
        assert!(
            rebuilt_roms == roms,
            "{name} on {}: the ROM data differs",
            board.name()
        );
    }
    built
}

#[test]
fn every_test_config_rebuilds_identically() {
    let dir = repo_root().join("onerom-config/test");
    let configs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();

    // Each config in its own thread, as a debug build takes a while over them
    // all.
    let built: usize = std::thread::scope(|scope| {
        let threads: Vec<_> = configs
            .iter()
            .map(|path| scope.spawn(|| round_trip(path)))
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .sum()
    });
    // Most configs build on several boards. Some are marked unsupported and
    // build on none.
    assert!(built > 100, "only {built} configs built");
}
