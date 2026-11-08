// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Random data survives a build then an unscramble for every chip type, board
//! and set type that builds.
//!
//! For a single chip with a forced GPIO in its address window:
//!
//! - a write to the served table entries unscrambles to the written data
//! - a write to the other entries unscrambles to the original data

use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::{Board, BoardSize, Model};
use onerom_config::mcu::{Family, Variant};
use onerom_gen::{
    Builder, ChipSetType, FileData, FlashChips, SUPPORTED_CHIP_TYPES_V2, requires_half_select_cs1,
    unscramble_slot,
};
use onerom_metadata::{
    DeviceMemoryView, Generations, GpioOverride, METADATA_BASE, OneromAlgAddrConfig,
    OneromMetadataHeader, OneromRomSlot,
};

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

/// `data` has an entry for each chip.
fn build(
    board: Board,
    chip_type: ChipType,
    (set_type, set, count): (ChipSetType, &str, usize),
    data: &[Vec<u8>],
) -> Option<(OneromMetadataHeader, Vec<u8>)> {
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
    let mut builder = Builder::from_json(VERSION, Family::Rp2350, &json).ok()?;
    for (n, data) in data.iter().enumerate() {
        builder.add_file(FileData::new(n, data.clone())).unwrap();
    }
    let props =
        FirmwareProperties::new(VERSION, board, Variant::RP2350, ServeAlg::Default, false).unwrap();
    let (metadata, roms) = builder.build(props).ok()?;

    let view = DeviceMemoryView::new(&metadata, METADATA_BASE);
    let header = OneromMetadataHeader::parse(&view, METADATA_BASE, Generations::UNKNOWN).unwrap();
    let slot = &header.rom_slots[0];
    let rom_data_start = FlashChips::new(Variant::RP2350, BoardSize::M).rom_data_start();
    let offset = (slot.data.addr().unwrap() - rom_data_start) as usize;
    let stored = roms[offset..offset + slot.size as usize].to_vec();
    Some((header, stored))
}

/// `false` if the set doesn't build.
fn round_trip(
    board: Board,
    chip_type: ChipType,
    (set_type, set, count): (ChipSetType, &str, usize),
) -> bool {
    let data: Vec<Vec<u8>> = (0..count)
        .map(|n| random((n as u64) << 32 | count as u64, served_size(chip_type)))
        .collect();
    let Some((header, stored)) = build(board, chip_type, (set_type, set, count), &data) else {
        return false;
    };

    let unscrambled =
        unscramble_slot(&header.hw, &header.rom_slots[0], &stored).unwrap_or_else(|e| {
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

/// A single chip built from random data with a forced GPIO in its address
/// window.
struct Forced {
    board: Board,
    chip_type: ChipType,
    data: Vec<u8>,
    header: OneromMetadataHeader,
    stored: Vec<u8>,
    /// The table index bits of the forced GPIOs.
    mask: usize,
    /// The forced level of each bit in `mask`.
    level: usize,
}

fn forced_bits(slot: &OneromRomSlot) -> (usize, usize) {
    let alg = slot.alg.as_ref().unwrap();
    let OneromAlgAddrConfig::AlgAddr0 {
        gpio_base,
        base_addr_pin,
        num_addr_pins,
        ..
    } = alg.alg_addr
    else {
        panic!("unknown address algorithm");
    };
    let window = gpio_base + base_addr_pin..gpio_base + base_addr_pin + num_addr_pins;
    let mut mask = 0;
    let mut level = 0;
    for &entry in alg
        .gpio_override_config
        .iter()
        .flat_map(|config| &config.params)
    {
        let gpio = entry & 0x3F;
        if !window.contains(&gpio) {
            continue;
        }
        let bit = 1 << (gpio - window.start);
        match GpioOverride::try_from(entry >> 6) {
            Ok(GpioOverride::GpioOverLow) => mask |= bit,
            Ok(GpioOverride::GpioOverHigh) => {
                mask |= bit;
                level |= bit;
            }
            _ => {}
        }
    }
    (mask, level)
}

impl Forced {
    /// `None` if the chip doesn't build on `board` or its address window
    /// doesn't contain a forced GPIO.
    fn new(board: Board, chip_type: ChipType) -> Option<Self> {
        let data = random(0x4B39, served_size(chip_type));
        let (header, stored) = build(board, chip_type, SHAPES[0], core::slice::from_ref(&data))?;
        let (mask, level) = forced_bits(&header.rom_slots[0]);
        (mask != 0).then_some(Self {
            board,
            chip_type,
            data,
            header,
            stored,
            mask,
            level,
        })
    }

    /// The stored bytes with ROM byte `addr` inverted in the table entries
    /// `pick` selects, and the ROM data with that byte inverted.
    fn write(&self, addr: usize, pick: impl Fn(usize) -> bool) -> (Vec<u8>, Vec<u8>) {
        let mut changed = self.data.clone();
        changed[addr] ^= 0xFF;
        let (_, rebuilt) = build(
            self.board,
            self.chip_type,
            SHAPES[0],
            core::slice::from_ref(&changed),
        )
        .unwrap();

        let word_bytes = self.stored.len() / table_entries(&self.header.rom_slots[0]);
        let mut stored = self.stored.clone();
        let mut placed = 0;
        for (index, (word, new)) in stored
            .chunks_exact_mut(word_bytes)
            .zip(rebuilt.chunks_exact(word_bytes))
            .enumerate()
        {
            if word != new && pick(index) {
                word.copy_from_slice(new);
                placed += 1;
            }
        }
        assert!(placed > 0, "{}: no entries written", self.name());
        (stored, changed)
    }

    fn served(&self, index: usize) -> bool {
        index & self.mask == self.level
    }

    fn name(&self) -> String {
        format!("{} on {}", self.chip_type.name(), self.board.name())
    }

    fn check(&self, stored: &[u8], want: &[u8]) {
        let roms = unscramble_slot(&self.header.hw, &self.header.rom_slots[0], stored)
            .unwrap_or_else(|e| panic!("{}: {e}", self.name()));
        assert!(roms == [want], "{}: unscrambled data differs", self.name());
    }
}

fn table_entries(slot: &OneromRomSlot) -> usize {
    let OneromAlgAddrConfig::AlgAddr0 { num_addr_pins, .. } = slot.alg.as_ref().unwrap().alg_addr
    else {
        panic!("unknown address algorithm");
    };
    1 << num_addr_pins
}

/// A ROM byte address with a mix of address bits set.
fn mixed_addr(forced: &Forced) -> usize {
    0x5A5A5 % forced.data.len()
}

/// Every one-chip set with a forced GPIO in its address window.
fn every_forced(each: impl Fn(&Forced) + Sync) -> Vec<(Board, ChipType)> {
    std::thread::scope(|scope| {
        let boards: Vec<_> = Model::Fire
            .boards()
            .iter()
            .map(|&board| {
                let each = &each;
                scope.spawn(move || {
                    SUPPORTED_CHIP_TYPES_V2
                        .iter()
                        .filter(|chip_type| !chip_type.is_plugin())
                        .filter_map(|&chip_type| Forced::new(board, chip_type))
                        .map(|forced| {
                            each(&forced);
                            (board, forced.chip_type)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        boards
            .into_iter()
            .flat_map(|board| board.join().unwrap())
            .collect()
    })
}

#[test]
fn served_write_to_27256_on_fire_28_c_unscrambles() {
    let forced = Forced::new(Board::Fire28C, ChipType::Chip27256).unwrap();
    let (stored, want) = forced.write(0, |index| forced.served(index));
    forced.check(&stored, &want);
}

#[test]
fn served_write_unscrambles() {
    let checked = every_forced(|forced| {
        let (stored, want) = forced.write(mixed_addr(forced), |index| forced.served(index));
        forced.check(&stored, &want);
    });
    assert!(checked.contains(&(Board::Fire28C, ChipType::Chip27256)));
}

#[test]
fn never_served_write_unscrambles_to_the_original() {
    let checked = every_forced(|forced| {
        let (stored, _) = forced.write(mixed_addr(forced), |index| !forced.served(index));
        forced.check(&stored, &forced.data);
    });
    assert!(checked.contains(&(Board::Fire28C, ChipType::Chip27256)));
}
