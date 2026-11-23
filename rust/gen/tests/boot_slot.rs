// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `boot_slot`.

use onerom_config::chip::ChipType;
use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::Board;
use onerom_config::mcu::{Family, Variant};
use onerom_gen::{
    Builder, ChipConfig, ChipSetConfig, ChipSetType, ChipTypeSpec, Config, Error, FileData,
    MIN_BOOT_SLOT_VERSION, recover_config,
};
use onerom_metadata::{
    BOOT_SLOT_NOT_SET, DeviceMemoryView, Generations, METADATA_BASE, OneromMetadataHeader,
};

const V0_8_1: FirmwareVersion = FirmwareVersion::new(0, 8, 1, 0);

const ROM: &str = r#"{ "type": "single", "chips": [
    { "file": "a.bin", "type": "2364", "cs1": "active_low" }] }"#;
const SYSTEM_PLUGIN: &str = r#"{ "type": "single", "chips": [
    { "file": "usb.bin", "type": "system_plugin" }] }"#;
const USER_PLUGIN: &str = r#"{ "type": "single", "chips": [
    { "file": "user.bin", "type": "user_plugin" }] }"#;

/// A config of `sets` with `global` ahead of them.
fn config(global: &str, sets: &[&str]) -> String {
    format!(
        r#"{{ "version": 1, "description": "test", {global} "chip_sets": [{}] }}"#,
        sets.join(", ")
    )
}

fn boot_slot(index: u32, sets: &[&str]) -> String {
    config(&format!(r#""boot_slot": {index},"#), sets)
}

fn parse_config(version: FirmwareVersion, json: &str) -> Result<Builder, Error> {
    Builder::from_json(version, Family::Rp2350, json)
}

/// The metadata header built from `json`.
fn header(version: FirmwareVersion, json: &str) -> OneromMetadataHeader {
    let mut builder = parse_config(version, json).unwrap();
    for spec in builder.file_specs() {
        let data = if spec.chip_type.is_plugin() {
            let mut image = vec![0u8; 256];
            image[0..4].copy_from_slice(b"ORA ");
            image[4..8].copy_from_slice(&1u32.to_le_bytes());
            image
        } else {
            vec![0xEA; spec.rom_size]
        };
        builder.add_file(FileData::new(spec.id, data)).unwrap();
    }
    let props = FirmwareProperties::new(
        version,
        Board::Fire24F,
        Variant::RP2350,
        ServeAlg::Default,
        false,
    )
    .unwrap();
    let (metadata, _) = builder.build(props).unwrap();
    OneromMetadataHeader::parse(
        &DeviceMemoryView::new(&metadata, METADATA_BASE),
        METADATA_BASE,
        Generations::UNKNOWN.with_firmware_release(version),
    )
    .unwrap()
}

#[test]
fn the_metadata_holds_the_index_including_plugins() {
    let sets = [SYSTEM_PLUGIN, USER_PLUGIN, ROM, ROM];
    for index in [2, 3] {
        let header = header(MIN_BOOT_SLOT_VERSION, &boot_slot(index, &sets));
        assert_eq!(u32::from(header.boot_slot), index);
    }
    let header = header(MIN_BOOT_SLOT_VERSION, &boot_slot(0, &[ROM, ROM]));
    assert_eq!(header.boot_slot, 0);
}

#[test]
fn an_absent_boot_slot_reads_not_set() {
    let json = config("", &[ROM]);
    for version in [V0_8_1, MIN_BOOT_SLOT_VERSION] {
        assert_eq!(header(version, &json).boot_slot, BOOT_SLOT_NOT_SET);
    }
    let builder = parse_config(MIN_BOOT_SLOT_VERSION, &json).unwrap();
    let saved = serde_json::to_value(builder.config()).unwrap();
    assert!(saved.get("boot_slot").is_none(), "{saved}");
}

#[test]
fn an_index_that_isnt_a_rom_slot_fails() {
    let sets = [SYSTEM_PLUGIN, USER_PLUGIN, ROM, ROM];
    for index in [0, 1, 4, 255] {
        let error = parse_config(MIN_BOOT_SLOT_VERSION, &boot_slot(index, &sets))
            .err()
            .unwrap();
        assert!(
            matches!(
                error,
                Error::BootSlotNotRomSlot { boot_slot, first: 2, slots: 2 }
                    if u32::from(boot_slot) == index
            ),
            "{index}: {error:?}"
        );
    }
}

#[test]
fn older_firmware_fails() {
    let error = parse_config(V0_8_1, &boot_slot(0, &[ROM])).err().unwrap();
    assert!(
        matches!(error, Error::FirmwareTooOld { minimum, .. } if minimum == MIN_BOOT_SLOT_VERSION),
        "{error:?}"
    );
}

#[test]
fn turbo_boot_accepts_several_slots_with_a_boot_slot() {
    let turbo = r#""turbo_boot": true,"#;
    let sets = [ROM, ROM];
    let error = parse_config(MIN_BOOT_SLOT_VERSION, &config(turbo, &sets))
        .err()
        .unwrap();
    assert!(
        matches!(error, Error::TurboBootMultiSlot { slots: 2 }),
        "{error:?}"
    );
    let both = format!(r#"{turbo} "boot_slot": 1,"#);
    assert!(parse_config(MIN_BOOT_SLOT_VERSION, &config(&both, &sets)).is_ok());
}

#[test]
fn inserting_a_chip_set_keeps_the_boot_slot_on_its_rom() {
    let plugin = || {
        ChipSetConfig::new(
            ChipSetType::Single,
            vec![ChipConfig::new(
                String::new(),
                ChipTypeSpec::from(ChipType::SystemPlugin),
            )],
        )
    };
    for (index, expected) in [(0, 2), (1, 2), (2, 1)] {
        let builder = parse_config(MIN_BOOT_SLOT_VERSION, &boot_slot(1, &[ROM, ROM])).unwrap();
        let mut config = builder.config().clone();
        config.insert_chip_set(index, plugin());
        assert_eq!(config.boot_slot, Some(expected), "{index}");
        assert_eq!(config.chip_sets.len(), 3);
    }
    let mut config = Config::new(String::new(), Vec::new());
    config.insert_chip_set(0, plugin());
    assert_eq!(config.boot_slot, None);
}

#[test]
fn rom_slots_are_numbered_without_plugins() {
    let json = config("", &[SYSTEM_PLUGIN, ROM, ROM]);
    let builder = parse_config(MIN_BOOT_SLOT_VERSION, &json).unwrap();
    let config = builder.config();
    assert_eq!(
        [0, 1, 2].map(|slot| config.chip_set_of_slot(slot)),
        [Some(1), Some(2), None]
    );
}

/// The firmware ignores a boot slot that isn't a ROM slot.
#[test]
fn the_boot_slot_is_recovered() {
    let sets = [SYSTEM_PLUGIN, ROM, ROM];
    let mut header = header(MIN_BOOT_SLOT_VERSION, &boot_slot(2, &sets));
    let stored = |header: &OneromMetadataHeader| {
        header
            .rom_slots
            .iter()
            .map(|slot| Some(vec![0xEA; slot.size as usize]))
            .collect::<Vec<_>>()
    };
    for (value, expected) in [
        (2, Some(2)),
        (1, Some(1)),
        (0, None),
        (3, None),
        (BOOT_SLOT_NOT_SET, None),
    ] {
        header.boot_slot = value;
        let recovered = recover_config(&header, &stored(&header)).unwrap();
        assert_eq!(recovered.config.boot_slot, expected, "{value}");
    }
}

/// The boot slot is marked in the description and the jumpers aren't shown.
#[test]
fn the_description_has_the_boot_slot() {
    let lines = |json: &str| {
        let builder = parse_config(MIN_BOOT_SLOT_VERSION, json).unwrap();
        let desc = builder.description_for_board(Board::Fire24F).unwrap();
        desc.lines().map(str::to_string).collect::<Vec<_>>()
    };
    let sets = [SYSTEM_PLUGIN, ROM, ROM];
    let plain = lines(&config("", &sets));
    assert!(plain.iter().any(|line| line.contains("SEL_A")), "{plain:?}");
    let chosen = lines(&boot_slot(2, &sets));
    assert!(
        !chosen.iter().any(|line| line.contains("SEL_A")),
        "{chosen:?}"
    );
    let marked: Vec<_> = chosen
        .iter()
        .filter(|line| line.contains("boot slot"))
        .collect();
    assert!(
        matches!(marked[..], [line] if line.starts_with("2:")),
        "{chosen:?}"
    );
    let both = lines(&config(r#""turbo_boot": true, "boot_slot": 2,"#, &sets));
    assert!(both.last().unwrap().contains("Image 2 served"), "{both:?}");
}
