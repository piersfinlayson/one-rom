// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! serde round-trip tests for the generated metadata types.
//!
//! These guard the `serde::Serialize`/`Deserialize` derives emitted by
//! `build/rust_gen.rs`. The compiler proves the derives *exist*; these prove
//! they are mutually inverse in practice, across the three distinct shapes the
//! generator emits:
//!
//! - a plain struct carrying the one oversized fixed array
//!   (`[[u8; 2]; 40]`), which routes through `serde_big_array::BigArray`;
//! - an `Option`-wrapped nested struct with ordinary (<= 32) fixed arrays;
//! - a tagged-FAM enum (struct-variant) — the algorithm-config family.
//!
//! One assertion additionally pins the wire format: `#[repr(u8)]` enums
//! serialise by *variant name*, not numeric discriminant. If that is ever
//! changed (e.g. by introducing `serde_repr`), this test fails on purpose —
//! the web details pane and any stored dumps depend on the current shape.

use onerom_metadata::{
    OneromAlgCsConfig, OneromHardwareInfo, OneromRomInfo, OneromRomPinMap, RomSlotType,
    Rp235xVariant,
};

/// Serialise, deserialise, and assert the value is unchanged.
///
/// Restricted to `Both`-generated types, which derive `PartialEq`; the
/// parse-only roots (`OneromInfo`, `OneromRuntimeInfo`) do not, and are
/// exercised transitively through their `Both` children.
fn round_trip<T>(value: &T) -> String
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + core::fmt::Debug,
{
    let json = serde_json::to_string(value).expect("serialize");
    let decoded: T = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(*value, decoded, "round-trip changed the value");
    json
}

/// Exercises the `serde_big_array::BigArray` path via `gpio_from_phys_pin`
/// (`[[u8; 2]; 40]`), alongside an enum field and the <= 32 arrays. Distinct
/// per-element values are used so a mis-ordered or truncated array is caught,
/// not just a length mismatch.
#[test]
fn hardware_info_round_trips_including_big_array() {
    let hw = OneromHardwareInfo {
        hw_rev: "fire-28-c".into(),
        rp235x: Rp235xVariant::Rp235xa,
        num_phys_pins: 28,
        usb_capable: 1,
        gpio_vbus: 24,
        gpio_ext_flash_cs: 255,
        gpio_status: 25,
        gpio_neopixel: 255,
        gpio_swdio: 26,
        gpio_swclk: 27,
        gpio_sel: [0, 1, 2, 3, 4, 5, 6],
        sel_jumper_pull: 0b0000_0011,
        gpio_from_phys_pin: core::array::from_fn(|r| [r as u8, (r as u8).wrapping_add(0x40)]),
        gpio_x1: [10, 11],
        gpio_x2: [12, 13],
    };

    let json = round_trip(&hw);

    // Pin the wire format: enum by variant name, not numeric discriminant.
    assert!(
        json.contains("\"Rp235xa\""),
        "expected variant-name encoding for rp235x, got: {json}"
    );
}

/// Exercises `Option<nested struct>` plus the ordinary fixed arrays
/// (`[u8; 24]` / `[u8; 16]`) inside the nested pin map.
#[test]
fn rom_info_with_pin_map_round_trips() {
    let rom = OneromRomInfo {
        rom_type: "27C512".into(),
        filename: Some("kernal.rom".into()),
        pin_map: Some(OneromRomPinMap {
            addr: core::array::from_fn(|i| i as u8),
            data: core::array::from_fn(|i| (i as u8).wrapping_add(0x80)),
        }),
        chip_size: 64 * 1024,
        rbcp_rom_type: 3,
    };

    round_trip(&rom);

    // The None arm must survive too.
    let plugin = OneromRomInfo {
        rom_type: "System Plugin".into(),
        filename: None,
        pin_map: None,
        chip_size: 0,
        rbcp_rom_type: 0,
    };
    round_trip(&plugin);
}

/// Exercises a tagged-FAM enum (struct variant) — the discriminated
/// algorithm-config family.
#[test]
fn tagged_fam_alg_cs_round_trips() {
    let cs = OneromAlgCsConfig::AlgCs0 {
        clkdiv_int: 1,
        clkdiv_frac: 0,
        gpio_base: 0,
        base_cs_pin: 20,
        num_cs_pins: 1,
        base_data_pin: 0,
        num_data_pins: 8,
        cs_active_delay: 2,
        cs_inactive_delay: 3,
        serve_cs_low_0: 1,
        byte_pin: 255,
        first_rom_cs_base: 20,
        first_rom_num_cs_pins: 1,
    };

    let json = round_trip(&cs);

    // Externally tagged by default: the variant name appears as a key.
    assert!(
        json.contains("AlgCs0"),
        "expected externally-tagged variant key, got: {json}"
    );
}

/// A struct field of enum type also round-trips, and slot-type variant names
/// are stable.
#[test]
fn rom_slot_type_variant_names_are_stable() {
    let json = serde_json::to_string(&RomSlotType::RomSlotTypeSingleRom).expect("serialize");
    assert_eq!(json, "\"RomSlotTypeSingleRom\"");
    let decoded: RomSlotType = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, RomSlotType::RomSlotTypeSingleRom);
}
