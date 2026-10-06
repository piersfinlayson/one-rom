// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for a chip set's `fire.standby` override.

use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::Board;
use onerom_config::mcu::{Family, Variant};
use onerom_gen::{Builder, Error, FileData};
use onerom_metadata::{
    DeviceMemoryView, Generations, METADATA_BASE, METADATA_SIZE, MaybeKnown, OneromMetadataHeader,
    OverrideState, Pointer, ROM_SLOT_NO_IMAGE,
};

const V0_8_0: FirmwareVersion = FirmwareVersion::new(0, 8, 0, 0);

/// Where ROM data starts, after the metadata region.
const ROM_DATA_BASE: u32 = METADATA_BASE + METADATA_SIZE as u32;

/// A 2364's ROM table on a fire-24-a.
const TABLE_2364: u32 = 64 * 1024;

fn config(sets: &[String]) -> String {
    format!(
        r#"{{ "version": 1, "description": "test", "chip_sets": [{}] }}"#,
        sets.join(", ")
    )
}

/// A single 2364 set, with `file` where it isn't empty, and `fire` as the
/// set's Fire overrides where it isn't empty.
fn single(file: &str, fire: &str) -> String {
    let file = match file {
        "" => String::new(),
        file => format!(r#""file": "{file}", "#),
    };
    let overrides = match fire {
        "" => String::new(),
        fire => format!(r#", "firmware_overrides": {{ "fire": {{ {fire} }} }}"#),
    };
    format!(
        r#"{{ "type": "single", "chips": [{{ {file}"type": "2364", "cs1": "active_low" }}]{overrides} }}"#
    )
}

/// A banked set of 2364s, one per entry of `files`, with standby on.
fn banked_standby(files: &[&str]) -> String {
    let chips: Vec<String> = files
        .iter()
        .map(|file| match *file {
            "" => r#"{ "type": "2364", "cs1": "active_low" }"#.to_string(),
            file => format!(r#"{{ "file": "{file}", "type": "2364", "cs1": "active_low" }}"#),
        })
        .collect();
    format!(
        r#"{{ "type": "banked", "chips": [{}], "firmware_overrides": {{ "fire": {{ "standby": true }} }} }}"#,
        chips.join(", ")
    )
}

/// The metadata and ROM data for `json`, with every file filled with 0xEA.
fn build(version: FirmwareVersion, json: &str) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let mut builder = Builder::from_json(version, Family::Rp2350, json)?;
    for id in 0..builder.total_file_count() {
        builder.add_file(FileData::new(id, vec![0xEA; 8192]))?;
    }
    let props = FirmwareProperties::new(
        version,
        Board::Fire24A,
        Variant::RP2350,
        ServeAlg::Default,
        false,
    )
    .unwrap();
    builder.build(props)
}

fn parse(metadata: &[u8]) -> OneromMetadataHeader {
    let view = DeviceMemoryView::new(metadata, METADATA_BASE);
    OneromMetadataHeader::parse(
        &view,
        METADATA_BASE,
        Generations::UNKNOWN.with_firmware_release(V0_8_0),
    )
    .expect("the metadata parses")
}

/// The standby state the first slot's overrides hold, as a host reads it.
fn standby(metadata: &[u8]) -> Option<MaybeKnown<OverrideState>> {
    parse(metadata).rom_slots[0]
        .firmware_overrides
        .as_ref()
        .expect("the slot has overrides")
        .override_states
        .standby
}

#[test]
fn standby_reads_back_as_it_was_set() {
    for (fire, state) in [
        (r#""standby": true"#, OverrideState::OverrideStateOn),
        (r#""standby": false"#, OverrideState::OverrideStateOff),
        (
            r#""cpu_freq": "150MHz""#,
            OverrideState::OverrideStateNotSet,
        ),
    ] {
        let (metadata, _) = build(V0_8_0, &config(&[single("a.bin", fire)])).unwrap();
        assert_eq!(standby(&metadata), Some(MaybeKnown::Known(state)), "{fire}");
    }
}

/// One version per builder, v1 and v2.  Older firmware can't hold standby on or
/// off.
#[test]
fn older_firmware_fails_with_standby() {
    for version in [
        FirmwareVersion::new(0, 7, 3, 0),
        FirmwareVersion::new(0, 6, 0, 0),
    ] {
        for fire in [r#""standby": true"#, r#""standby": false"#] {
            match Builder::from_json(version, Family::Rp2350, &config(&[single("a.bin", fire)])) {
                Err(Error::FirmwareTooOld {
                    feat,
                    version: v,
                    minimum,
                }) => {
                    assert_eq!(feat, "standby");
                    assert_eq!(v, version);
                    assert_eq!(minimum, V0_8_0);
                }
                other => panic!("{version} {fire}: {other:?}"),
            }
        }
        let json = config(&[single("a.bin", r#""cpu_freq": "150MHz""#)]);
        assert!(Builder::from_json(version, Family::Rp2350, &json).is_ok());
    }
}

/// The slot's size is its ROM table's but it doesn't occupy flash, so the next
/// slot starts where ROM data starts.
#[test]
fn a_standby_slot_without_a_file_has_no_image() {
    let json = config(&[single("", r#""standby": true"#), single("b.bin", "")]);
    let (metadata, rom) = build(V0_8_0, &json).unwrap();

    let slots = parse(&metadata).rom_slots;
    assert_eq!(slots[0].data, Pointer::Null);
    assert_eq!(slots[0].size, TABLE_2364);
    assert_eq!(slots[1].data, Pointer::Addr32(ROM_DATA_BASE));
    assert_eq!(rom.len() as u32, slots[1].size);

    // The firmware skips the boot copy for a data pointer of ROM_SLOT_NO_IMAGE.
    let view = DeviceMemoryView::new(&metadata, METADATA_BASE);
    let slots_at = view.read_u32_le(METADATA_BASE + 32).unwrap();
    assert_eq!(view.read_u32_le(slots_at).unwrap(), ROM_SLOT_NO_IMAGE);
}

#[test]
fn a_standby_slot_with_a_file_has_an_image() {
    let (metadata, rom) = build(V0_8_0, &config(&[single("a.bin", r#""standby": true"#)])).unwrap();
    let slots = parse(&metadata).rom_slots;
    assert_eq!(slots[0].data, Pointer::Addr32(ROM_DATA_BASE));
    assert_eq!(rom.len() as u32, slots[0].size);
}

#[test]
fn a_rom_chip_without_a_file_fails_unless_its_set_is_in_standby() {
    for fire in ["", r#""standby": false"#] {
        match Builder::from_json(V0_8_0, Family::Rp2350, &config(&[single("", fire)])) {
            Err(Error::InvalidConfig { .. }) => {}
            other => panic!("{fire}: {other:?}"),
        }
    }
}

#[test]
fn a_standby_set_has_a_file_for_every_chip_or_none() {
    assert!(build(V0_8_0, &config(&[banked_standby(&["", ""])])).is_ok());
    assert!(build(V0_8_0, &config(&[banked_standby(&["a.bin", "b.bin"])])).is_ok());
    match Builder::from_json(
        V0_8_0,
        Family::Rp2350,
        &config(&[banked_standby(&["a.bin", ""])]),
    ) {
        Err(Error::InvalidConfig { .. }) => {}
        other => panic!("{other:?}"),
    }
}
