// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Random data survives a build then an unscramble, for every chip type, board
//! and set type that builds.

use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::{Board, BoardSize, Model};
use onerom_config::mcu::{Family, Variant};
use onerom_gen::{
    Builder, ChipSetType, FileData, FlashChips, SUPPORTED_CHIP_TYPES_V2, requires_half_select_cs1,
    unscramble_slot,
};
use onerom_metadata::{DeviceMemoryView, Generations, METADATA_BASE, OneromMetadataHeader};

const VERSION: FirmwareVersion = FirmwareVersion::new(0, 8, 0, 0);

const SHAPES: &[(ChipSetType, &str, usize)] = &[
    (ChipSetType::Single, "single", 1),
    (ChipSetType::Banked, "banked", 2),
    (ChipSetType::Banked, "banked", 3),
    (ChipSetType::Banked, "banked", 4),
    (ChipSetType::Multi, "multi", 2),
    (ChipSetType::Multi, "multi", 3),
];

/// The number of bytes a chip serves.
fn served_size(chip_type: ChipType) -> usize {
    if requires_half_select_cs1(&chip_type) {
        chip_type.size_bytes() / 2
    } else {
        chip_type.size_bytes()
    }
}

fn random(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005) | 1;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 56) as u8
        })
        .collect()
}

/// A chip's chip-select fields, as JSON. A multi set's secondary selects on
/// one line and ignores the rest.
fn cs_fields(chip_type: ChipType, secondary: bool) -> String {
    let lines = chip_type.control_lines();
    let has = |name: &str| lines.iter().any(|line| line.name == name);
    let select = if has("cs1") || requires_half_select_cs1(&chip_type) {
        "cs1"
    } else {
        "ce"
    };
    let mut fields = Vec::new();
    if requires_half_select_cs1(&chip_type) {
        fields.push(r#""cs1": "active_low""#.to_string());
    }
    for line in lines {
        if !matches!(line.name, "cs1" | "cs2" | "cs3" | "cs4" | "ce" | "oe") {
            continue;
        }
        let value = if secondary && line.name != select {
            Some("ignore")
        } else if line.line_type == ControlLineType::Configurable {
            Some("active_low")
        } else {
            None
        };
        if let Some(value) = value {
            fields.push(format!(r#""{}": "{value}""#, line.name));
        }
    }
    fields.join(", ")
}

/// Builds a set of `count` `chip_type` chips on `board` from random data, then
/// checks each chip unscrambles to its data. `false` where the set doesn't
/// build.
fn round_trip(
    board: Board,
    chip_type: ChipType,
    (set_type, set, count): (ChipSetType, &str, usize),
) -> bool {
    let chips: Vec<String> = (0..count)
        .map(|n| {
            let secondary = set_type == ChipSetType::Multi && n > 0;
            let cs = cs_fields(chip_type, secondary);
            let cs = if cs.is_empty() { cs } else { format!(", {cs}") };
            format!(
                r#"{{ "file": "{n}.bin", "type": "{}"{cs} }}"#,
                chip_type.name()
            )
        })
        .collect();
    let json = format!(
        r#"{{ "version": 1, "description": "unscramble", "chip_sets": [{{ "type": "{set}", "chips": [{}] }}] }}"#,
        chips.join(", ")
    );
    let Ok(mut builder) = Builder::from_json(VERSION, Family::Rp2350, &json) else {
        return false;
    };
    let data: Vec<Vec<u8>> = (0..count)
        .map(|n| random((n as u64) << 32 | count as u64, served_size(chip_type)))
        .collect();
    for (n, data) in data.iter().enumerate() {
        builder.add_file(FileData::new(n, data.clone())).unwrap();
    }
    let props =
        FirmwareProperties::new(VERSION, board, Variant::RP2350, ServeAlg::Default, false).unwrap();
    let Ok((metadata, roms)) = builder.build(props) else {
        return false;
    };

    let view = DeviceMemoryView::new(&metadata, METADATA_BASE);
    let header = OneromMetadataHeader::parse(&view, METADATA_BASE, Generations::UNKNOWN).unwrap();
    let slot = &header.rom_slots[0];
    let rom_data_start = FlashChips::new(Variant::RP2350, BoardSize::M).rom_data_start();
    let offset = (slot.data.addr().unwrap() - rom_data_start) as usize;
    let stored = &roms[offset..offset + slot.size as usize];

    let unscrambled = unscramble_slot(&header.hw, slot, stored).unwrap_or_else(|e| {
        panic!(
            "{} {set} x{count} on {}: {e}",
            chip_type.name(),
            board.name()
        )
    });
    assert_eq!(unscrambled.len(), count);
    for (n, (got, want)) in unscrambled.iter().zip(&data).enumerate() {
        assert!(
            got == want,
            "{} {set} x{count} on {}: chip {n} differs",
            chip_type.name(),
            board.name()
        );
    }
    true
}

#[test]
fn random_data_survives_build_then_unscramble() {
    // Each board in its own thread, as a debug build takes a while over them all.
    let built: usize = std::thread::scope(|scope| {
        let boards: Vec<_> = Model::Fire
            .boards()
            .iter()
            .map(|&board| {
                scope.spawn(move || {
                    let mut built = 0;
                    for &chip_type in SUPPORTED_CHIP_TYPES_V2 {
                        if chip_type.is_plugin() {
                            continue;
                        }
                        for &shape in SHAPES {
                            if round_trip(board, chip_type, shape) {
                                built += 1;
                            }
                        }
                    }
                    built
                })
            })
            .collect();
        boards.into_iter().map(|board| board.join().unwrap()).sum()
    });
    // Every board serves single chips of most types and banked and multi
    // sets of some, so far fewer than this means the configs are wrong.
    assert!(built > 500, "only {built} sets built");
}
