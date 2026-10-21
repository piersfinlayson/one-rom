// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `addr_mon_min_cs_act_ns`.

use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::Board;
use onerom_config::mcu::{Family, Variant};
use onerom_gen::{
    Builder, Error, FileData, MAX_ADDR_MON_MIN_CS_ACT_NS, MIN_ADDR_MON_MIN_CS_ACT_VERSION,
};
use onerom_metadata::{
    ADDR_MON_MIN_CS_ACT_NOT_SET, DeviceMemoryView, Generations, METADATA_BASE,
    OneromMetadataHeader, metadata_generation_for,
};

const V0_8_0: FirmwareVersion = FirmwareVersion::new(0, 8, 0, 0);
const V0_8_1: FirmwareVersion = FirmwareVersion::new(0, 8, 1, 0);

const NOT_SET: u8 = ADDR_MON_MIN_CS_ACT_NOT_SET;
const MAX: u16 = MAX_ADDR_MON_MIN_CS_ACT_NS;

/// A config with `global` beside `chip_sets` where it isn't empty.
fn config(global: &str, sets: &[String]) -> String {
    let global = match global {
        "" => String::new(),
        global => format!("{global}, "),
    };
    format!(
        r#"{{ "version": 1, "description": "test", {global}"chip_sets": [{}] }}"#,
        sets.join(", ")
    )
}

fn global(ns: u16) -> String {
    format!(r#""addr_mon_min_cs_act_ns": {ns}"#)
}

/// A chip set of `chip`, with `fire` as its Fire overrides where it isn't
/// empty.
fn set(chip: &str, fire: &str) -> String {
    let overrides = match fire {
        "" => String::new(),
        fire => format!(r#", "firmware_overrides": {{ "fire": {{ {fire} }} }}"#),
    };
    format!(r#"{{ "type": "single", "chips": [{chip}]{overrides} }}"#)
}

/// A 2364, which a fire-24-a serves with CS algorithm 0.
fn cs0(file: &str) -> String {
    format!(r#"{{ "file": "{file}", "type": "2364", "cs1": "active_low" }}"#)
}

const SYSTEM_PLUGIN: &str = r#"{ "file": "usb.bin", "type": "system_plugin" }"#;
const USER_PLUGIN: &str = r#"{ "file": "user.bin", "type": "user_plugin" }"#;

/// A minimal valid plugin image.
fn plugin_image() -> Vec<u8> {
    let mut image = vec![0u8; 256];
    image[0..4].copy_from_slice(b"ORA ");
    image[4..8].copy_from_slice(&1u32.to_le_bytes());
    image
}

/// A builder for `json`, with every ROM file filled to its chip's size.
fn loaded(version: FirmwareVersion, json: &str) -> Result<Builder, Error> {
    let mut builder = Builder::from_json(version, Family::Rp2350, json)?;
    for spec in builder.file_specs() {
        let data = match spec.chip_type.is_plugin() {
            true => plugin_image(),
            false => vec![0xEA; spec.rom_size],
        };
        builder.add_file(FileData::new(spec.id, data))?;
    }
    Ok(builder)
}

fn props(version: FirmwareVersion, board: Board) -> FirmwareProperties {
    FirmwareProperties::new(version, board, Variant::RP2350, ServeAlg::Default, false).unwrap()
}

/// The metadata for `json`.
fn build(version: FirmwareVersion, board: Board, json: &str) -> Result<Vec<u8>, Error> {
    loaded(version, json)?
        .build(props(version, board))
        .map(|(metadata, _)| metadata)
}

fn parse(version: FirmwareVersion, metadata: &[u8]) -> OneromMetadataHeader {
    let view = DeviceMemoryView::new(metadata, METADATA_BASE);
    OneromMetadataHeader::parse(
        &view,
        METADATA_BASE,
        Generations::UNKNOWN.with_firmware_release(version),
    )
    .expect("the metadata parses")
}

/// The header's value and each slot's, as a host reads them.
fn values(version: FirmwareVersion, metadata: &[u8]) -> (u8, Vec<Option<u8>>) {
    let header = parse(version, metadata);
    let slots = header
        .rom_slots
        .iter()
        .map(|slot| {
            slot.firmware_overrides
                .as_ref()
                .map(|overrides| overrides.addr_mon_min_cs_act)
        })
        .collect();
    (header.addr_mon_min_cs_act, slots)
}

#[test]
fn both_values_read_back_in_units() {
    let json = config(
        &global(140),
        &[set(&cs0("a.bin"), r#""addr_mon_min_cs_act_ns": 300"#)],
    );
    let metadata = build(V0_8_1, Board::Fire24A, &json).unwrap();
    assert_eq!(values(V0_8_1, &metadata), (14, vec![Some(30)]));
}

#[test]
fn a_config_saves_both_keys_as_written() {
    let json = config(
        &global(141),
        &[set(&cs0("a.bin"), r#""addr_mon_min_cs_act_ns": 299"#)],
    );
    let builder = Builder::from_json(V0_8_1, Family::Rp2350, &json).unwrap();
    let saved = serde_json::to_value(builder.config()).unwrap();
    assert_eq!(saved["addr_mon_min_cs_act_ns"], 141);
    assert_eq!(
        saved["chip_sets"][0]["firmware_overrides"]["fire"]["addr_mon_min_cs_act_ns"],
        299
    );
}

#[test]
fn an_absent_key_reads_not_set() {
    let json = config("", &[set(&cs0("a.bin"), r#""cpu_freq": "150MHz""#)]);
    let builder = Builder::from_json(V0_8_1, Family::Rp2350, &json).unwrap();
    let saved = serde_json::to_value(builder.config()).unwrap();
    assert!(saved.get("addr_mon_min_cs_act_ns").is_none(), "{saved}");
    assert!(
        saved["chip_sets"][0]["firmware_overrides"]["fire"]
            .get("addr_mon_min_cs_act_ns")
            .is_none(),
        "{saved}"
    );

    let metadata = build(V0_8_1, Board::Fire24A, &json).unwrap();
    assert_eq!(values(V0_8_1, &metadata), (NOT_SET, vec![Some(NOT_SET)]));
}

#[test]
fn generation_3_metadata_reads_not_set() {
    let json = config("", &[set(&cs0("a.bin"), r#""cpu_freq": "150MHz""#)]);
    let metadata = build(V0_8_0, Board::Fire24A, &json).unwrap();
    assert_eq!(parse(V0_8_0, &metadata).version, 3);
    assert_eq!(values(V0_8_0, &metadata), (NOT_SET, vec![Some(NOT_SET)]));
}

#[test]
fn a_value_is_stored_rounded_up() {
    for (ns, stored) in [(1, 1), (9, 1), (10, 1), (11, 2), (2531, 254)] {
        let slot = format!(r#""addr_mon_min_cs_act_ns": {ns}"#);
        let json = config(&global(ns), &[set(&cs0("a.bin"), &slot)]);
        let metadata = build(V0_8_1, Board::Fire24A, &json).unwrap();
        assert_eq!(
            values(V0_8_1, &metadata),
            (stored, vec![Some(stored)]),
            "{ns}"
        );
    }
}

#[test]
fn both_ends_of_the_range_build() {
    for (ns, stored) in [(0, 0), (MAX, 254)] {
        let slot = format!(r#""addr_mon_min_cs_act_ns": {ns}"#);
        let json = config(&global(ns), &[set(&cs0("a.bin"), &slot)]);
        let metadata = build(V0_8_1, Board::Fire24A, &json).unwrap();
        assert_eq!(
            values(V0_8_1, &metadata),
            (stored, vec![Some(stored)]),
            "{ns}"
        );
    }
}

#[test]
fn a_value_above_the_range_fails() {
    let above = MAX + 1;
    let json = config(&global(above), &[set(&cs0("a.bin"), "")]);
    match Builder::from_json(V0_8_1, Family::Rp2350, &json) {
        Err(Error::AddrMonMinCsActOutOfRange { slot: None, ns }) => assert_eq!(ns, above),
        other => panic!("{other:?}"),
    }

    let slot = format!(r#""addr_mon_min_cs_act_ns": {above}"#);
    for sets in [
        vec![set(&cs0("a.bin"), ""), set(&cs0("b.bin"), &slot)],
        vec![
            set(SYSTEM_PLUGIN, ""),
            set(&cs0("a.bin"), ""),
            set(&cs0("b.bin"), &slot),
        ],
    ] {
        match Builder::from_json(V0_8_1, Family::Rp2350, &config("", &sets)) {
            Err(Error::AddrMonMinCsActOutOfRange { slot: Some(1), ns }) => assert_eq!(ns, above),
            other => panic!("{other:?}"),
        }
    }

    // Too large for the field's type, so parsing fails.
    let json = config(
        r#""addr_mon_min_cs_act_ns": 70000"#,
        &[set(&cs0("a.bin"), "")],
    );
    assert!(matches!(
        Builder::from_json(V0_8_1, Family::Rp2350, &json),
        Err(Error::InvalidConfig { .. })
    ));
}

/// A deserialized builder doesn't pass through `from_json`.
#[test]
fn a_deserialized_builder_fails_to_build_with_a_value_above_the_range() {
    let above = MAX + 1;
    let json = config(
        &global(140),
        &[set(&cs0("a.bin"), r#""addr_mon_min_cs_act_ns": 300"#)],
    );
    for (pointer, slot) in [
        ("/config/addr_mon_min_cs_act_ns", None),
        (
            "/config/chip_sets/0/firmware_overrides/fire/addr_mon_min_cs_act_ns",
            Some(0),
        ),
    ] {
        let mut value = serde_json::to_value(loaded(V0_8_1, &json).unwrap()).unwrap();
        *value.pointer_mut(pointer).expect(pointer) = above.into();
        let builder: Builder = serde_json::from_value(value).unwrap();
        match builder.build(props(V0_8_1, Board::Fire24A)) {
            Err(Error::AddrMonMinCsActOutOfRange { slot: s, ns }) => {
                assert_eq!((s, ns), (slot, above), "{pointer}")
            }
            other => panic!("{pointer}: {other:?}"),
        }
    }
}

#[test]
fn a_plugin_chip_set_fails_with_its_own_value() {
    let slot = r#""addr_mon_min_cs_act_ns": 140"#;
    let user = |chip: &str| vec![set(SYSTEM_PLUGIN, ""), set(chip, slot)];
    for (sets, name) in [
        (
            vec![set(SYSTEM_PLUGIN, slot), set(&cs0("a.bin"), "")],
            "usb",
        ),
        (user(USER_PLUGIN), "user"),
        (
            user(r#"{ "plugin": "host-control", "type": "user_plugin" }"#),
            "host-control",
        ),
        (
            user(
                r#"{ "file": "https://images.onerom.org/plugins/user/host-control/v0.1.5/plugin.bin", "type": "user_plugin" }"#,
            ),
            "host-control",
        ),
        (
            user(r#"{ "file": "/plugins/my-plugin.bin", "type": "user_plugin" }"#),
            "my-plugin",
        ),
        // The plugin check comes before the range check.
        (
            vec![
                set(SYSTEM_PLUGIN, r#""addr_mon_min_cs_act_ns": 3000"#),
                set(&cs0("a.bin"), ""),
            ],
            "usb",
        ),
    ] {
        match Builder::from_json(V0_8_1, Family::Rp2350, &config("", &sets)) {
            Err(Error::AddrMonMinCsActOnPlugin { plugin }) => assert_eq!(plugin, name),
            other => panic!("{name}: {other:?}"),
        }
    }
}

/// The global value applies to ROM slots alone.
#[test]
fn a_config_with_a_plugin_can_set_the_global_value() {
    let json = config(
        &global(140),
        &[set(SYSTEM_PLUGIN, ""), set(&cs0("a.bin"), "")],
    );
    let metadata = build(V0_8_1, Board::Fire24A, &json).unwrap();
    assert_eq!(values(V0_8_1, &metadata), (14, vec![None, None]));
}

#[test]
fn both_values_read_back_alongside_a_plugin() {
    let json = config(
        &global(700),
        &[
            set(SYSTEM_PLUGIN, ""),
            set(&cs0("a.bin"), r#""addr_mon_min_cs_act_ns": 600"#),
        ],
    );
    let metadata = build(V0_8_1, Board::Fire24A, &json).unwrap();
    assert_eq!(values(V0_8_1, &metadata), (70, vec![None, Some(60)]));
}

/// The release before and one older version per builder, v1 and v2.
#[test]
fn older_firmware_fails_with_either_key() {
    for version in [
        V0_8_0,
        FirmwareVersion::new(0, 7, 3, 0),
        FirmwareVersion::new(0, 6, 0, 0),
    ] {
        for json in [
            config(&global(140), &[set(&cs0("a.bin"), "")]),
            config(
                "",
                &[set(&cs0("a.bin"), r#""addr_mon_min_cs_act_ns": 140"#)],
            ),
        ] {
            match Builder::from_json(version, Family::Rp2350, &json) {
                Err(Error::FirmwareTooOld {
                    feat,
                    version: v,
                    minimum,
                }) => {
                    assert_eq!(feat, "addr_mon_min_cs_act_ns");
                    assert_eq!(v, version);
                    assert_eq!(minimum, MIN_ADDR_MON_MIN_CS_ACT_VERSION);
                }
                other => panic!("{version}: {other:?}"),
            }
        }
        let json = config("", &[set(&cs0("a.bin"), "")]);
        assert!(Builder::from_json(version, Family::Rp2350, &json).is_ok());
    }
}

#[test]
fn the_minimum_version_is_where_the_metadata_holds_the_values() {
    assert_eq!(metadata_generation_for(V0_8_0), Some(3));
    assert_eq!(
        metadata_generation_for(MIN_ADDR_MON_MIN_CS_ACT_VERSION),
        Some(4)
    );
    let json = config(&global(140), &[set(&cs0("a.bin"), "")]);
    let metadata = build(V0_8_1, Board::Fire24A, &json).unwrap();
    assert_eq!(parse(V0_8_1, &metadata).version, 4);
}
