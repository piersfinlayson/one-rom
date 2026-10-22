// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! A recovered config rebuilds identical metadata for every chip-select
//! setting of every chip type on every board, in single and banked sets.

use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::{Board, BoardSize, Model};
use onerom_config::mcu::{Family, Variant};
use onerom_gen::{
    Builder, FileData, FlashChips, SUPPORTED_CHIP_TYPES_V2, recover_config,
    requires_half_select_cs1,
};
use onerom_metadata::{DeviceMemoryView, Generations, METADATA_BASE, OneromMetadataHeader};

const VERSION: FirmwareVersion = FirmwareVersion::new(0, 8, 0, 0);

fn served_size(chip_type: ChipType) -> usize {
    if requires_half_select_cs1(&chip_type) {
        chip_type.size_bytes() / 2
    } else {
        chip_type.size_bytes()
    }
}

/// Every chip-select setting a config can have for `chip_type`, as JSON fields.
fn chip_selects(chip_type: ChipType) -> Vec<String> {
    let mut settings = vec![Vec::new()];
    for name in ["cs1", "cs2", "cs3", "cs4", "ce", "oe"] {
        let line = chip_type
            .control_lines()
            .iter()
            .find(|line| line.name == name);
        let values: &[Option<&str>] = match line {
            Some(line) if line.line_type == ControlLineType::Configurable => {
                &[Some("active_low"), Some("active_high"), Some("ignore")]
            }
            Some(_) => &[None, Some("ignore")],
            None if name == "cs1" && requires_half_select_cs1(&chip_type) => {
                &[Some("active_low"), Some("active_high")]
            }
            None => &[None],
        };
        settings = settings
            .iter()
            .flat_map(|setting: &Vec<String>| {
                values.iter().map(move |value| {
                    let mut setting = setting.clone();
                    if let Some(value) = value {
                        setting.push(format!(r#""{name}": "{value}""#));
                    }
                    setting
                })
            })
            .collect();
    }
    settings
        .into_iter()
        .map(|mut setting| {
            if setting.iter().any(|field| field.contains("ignore")) {
                setting.push(r#""allow_cs_ignore": true"#.to_string());
            }
            setting.join(", ")
        })
        .collect()
}

/// The metadata `json` builds on `board`, each chip's file the bytes `data`
/// returns for it. `None` where it doesn't build.
fn build(json: &str, board: Board, data: impl Fn(&str) -> Vec<u8>) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut builder = Builder::from_json(VERSION, Family::Rp2350, json).ok()?;
    for spec in builder.file_specs() {
        builder
            .add_file(FileData::new(spec.id, data(&spec.source)))
            .unwrap();
    }
    let props =
        FirmwareProperties::new(VERSION, board, Variant::RP2350, ServeAlg::Default, false).unwrap();
    builder.build(props).ok()
}

/// Builds `count` `chip_type` chips in a `set` with chip-select `fields` on
/// `board`, recovers the config and rebuilds it. `false` where the set doesn't
/// build.
fn round_trip(board: Board, chip_type: ChipType, set: &str, count: usize, fields: &str) -> bool {
    let fields = if fields.is_empty() {
        String::new()
    } else {
        format!(", {fields}")
    };
    let chips: Vec<String> = (0..count)
        .map(|n| {
            format!(
                r#"{{ "file": "{n}", "type": "{}"{fields} }}"#,
                chip_type.name()
            )
        })
        .collect();
    let json = format!(
        r#"{{ "version": 1, "description": "recover", "chip_sets": [{{ "type": "{set}", "chips": [{}] }}] }}"#,
        chips.join(", ")
    );
    let size = served_size(chip_type);
    let Some((metadata, roms)) = build(&json, board, |source| {
        vec![source.parse::<u8>().unwrap(); size]
    }) else {
        return false;
    };

    let view = DeviceMemoryView::new(&metadata, METADATA_BASE);
    let header = OneromMetadataHeader::parse(&view, METADATA_BASE, Generations::UNKNOWN).unwrap();
    let rom_data_start = FlashChips::new(Variant::RP2350, BoardSize::M).rom_data_start();
    let slots: Vec<Option<Vec<u8>>> = header
        .rom_slots
        .iter()
        .map(|slot| {
            let offset = (slot.data.addr()? - rom_data_start) as usize;
            Some(roms[offset..offset + slot.size as usize].to_vec())
        })
        .collect();

    let context = format!(
        "{} {set} x{count} {fields} on {}",
        chip_type.name(),
        board.name()
    );
    let mut recovered = recover_config(&header, &slots).unwrap();
    assert!(
        recovered.problems.is_empty(),
        "{context}: {:?}",
        recovered.problems
    );
    let mut files = Vec::new();
    for (chip_set, data) in recovered.config.chip_sets.iter_mut().zip(&recovered.data) {
        for (chip, data) in chip_set.chips.iter_mut().zip(data) {
            chip.file = files.len().to_string();
            files.push(data.clone().unwrap());
        }
    }
    let json = serde_json::to_string(&recovered.config).unwrap();
    let (rebuilt, _) = build(&json, board, |source| {
        files[source.parse::<usize>().unwrap()].clone()
    })
    .unwrap_or_else(|| panic!("{context}: the recovered config doesn't build\n{json}"));
    assert!(
        rebuilt == metadata,
        "{context}: the metadata differs\n{json}"
    );
    true
}

#[test]
fn every_chip_select_setting_is_recovered() {
    let pairs: Vec<(Board, ChipType)> = Model::Fire
        .boards()
        .iter()
        .flat_map(|&board| {
            SUPPORTED_CHIP_TYPES_V2
                .iter()
                .filter(|chip_type| !chip_type.is_plugin())
                .map(move |&chip_type| (board, chip_type))
        })
        .collect();
    // Each board and chip type in its own thread, as a debug build takes a
    // while over them all.
    let built: usize = std::thread::scope(|scope| {
        let threads: Vec<_> = pairs
            .iter()
            .map(|&(board, chip_type)| {
                scope.spawn(move || {
                    let mut built = 0;
                    for fields in chip_selects(chip_type) {
                        for (set, count) in [("single", 1), ("banked", 2)] {
                            if round_trip(board, chip_type, set, count, &fields) {
                                built += 1;
                            }
                        }
                    }
                    built
                })
            })
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .sum()
    });
    assert!(built > 1000, "only {built} sets built");
}
