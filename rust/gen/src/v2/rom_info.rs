// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Per-ROM metadata (`OneromRomInfo`, `OneromRomPinMap`) and ROM-slot
//! classification (`RomSlotType`), built from a `Chip` plus the
//! address/CS-data layouts already derived for its chip set.

use alloc::string::{String, ToString};

use onerom_config::chip::ChipType;

use onerom_metadata::{
    GPIO_NONE, MAX_ADDR_PINS, MAX_DATA_PINS, OneromRomInfo, OneromRomPinMap, RomSlotType,
};

use crate::image::{Chip, ChipSetType};

use super::addr_layout::AddrLayout;
use super::cs_data_layout::CsDataLayout;

/// Maximum length (in bytes) of `OneromRomInfo::filename`, to bound
/// metadata size. Longer names are truncated at a UTF-8 character
/// boundary.
pub const MAX_ROM_FILENAME_LEN: usize = 128;

/// Build `OneromRomPinMap` from the resolved per-line address/data GPIOs.
///
/// Entries beyond `addr_pin_gpios.len()`/`data_pin_gpios.len()` are left
/// as `GPIO_NONE`.
pub fn build_rom_pin_map(
    addr_layout: &AddrLayout,
    cs_data_layout: &CsDataLayout,
) -> OneromRomPinMap {
    let mut addr = [GPIO_NONE; MAX_ADDR_PINS];
    for (i, &gpio) in addr_layout.addr_pin_gpios.iter().enumerate() {
        addr[i] = gpio;
    }

    let mut data = [GPIO_NONE; MAX_DATA_PINS];
    for (i, &gpio) in cs_data_layout.data_pin_gpios.iter().enumerate() {
        data[i] = gpio;
    }

    OneromRomPinMap { addr, data }
}

/// Truncate `name` to at most `MAX_ROM_FILENAME_LEN` bytes, at a UTF-8
/// character boundary. Returns `None` if `name` is empty (per `Chip`,
/// always a `String`, possibly `""`).
pub(crate) fn truncate_filename(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }

    if name.len() <= MAX_ROM_FILENAME_LEN {
        return Some(name.to_string());
    }

    let mut end = MAX_ROM_FILENAME_LEN;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    Some(name[..end].to_string())
}

/// Build `OneromRomInfo` for one chip in a chip set.
pub fn build_rom_info(
    chip: &Chip,
    addr_layout: &AddrLayout,
    cs_data_layout: &CsDataLayout,
) -> OneromRomInfo {
    OneromRomInfo {
        rom_type: chip.chip_type().name().to_string(),
        filename: truncate_filename(chip.filename()),
        pin_map: Some(build_rom_pin_map(addr_layout, cs_data_layout)),
        chip_size: chip.chip_type().size_bytes() as u32,
        rbcp_rom_type: chip.chip_type().rbcp_chip_type(),
    }
}

/// Determine the `RomSlotType` for a chip set.
///
/// QUESTION/TODO: `RomSlotTypeSingleRam` is assumed to apply when a
/// Single set's chip is `ChipType::Chip6116` (the only SRAM type) -
/// confirm this is the right (and only) trigger before relying on it.
/// `RomSlotTypePlugin*` aren't handled here - plugin slots aren't
/// `ChipSet`s (presumably a separate path, shared with v1 - point 4).
pub fn rom_slot_type(set_type: ChipSetType, chip0_type: ChipType) -> RomSlotType {
    match set_type {
        ChipSetType::Multi => RomSlotType::RomSlotTypeMultiRom,
        ChipSetType::Banked => RomSlotType::RomSlotTypeBankedRom,
        ChipSetType::Single => {
            if chip0_type == ChipType::Chip6116 {
                RomSlotType::RomSlotTypeSingleRam
            } else {
                RomSlotType::RomSlotTypeSingleRom
            }
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::super::cs_data_layout::{SelectLine, SelectRole};
    use super::*;

    fn fire24a_2364_addr_layout() -> AddrLayout {
        AddrLayout {
            gpio_base: 0,
            num_addr_pins: 16,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12],
            excess_addr_pin_gpios: alloc::vec![],
        }
    }

    fn fire24a_2364_cs_data_layout() -> CsDataLayout {
        CsDataLayout {
            gpio_base: 0,
            base_data_pin: 16,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![16, 17, 18, 19, 20, 21, 22, 23],
            base_cs_pin: 13,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: alloc::vec![SelectLine {
                role: SelectRole::Cs1,
                gpio: 13
            }],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        }
    }

    #[test]
    fn fire24a_2364_pin_map() {
        let pin_map =
            build_rom_pin_map(&fire24a_2364_addr_layout(), &fire24a_2364_cs_data_layout());

        let mut expected_addr = [GPIO_NONE; MAX_ADDR_PINS];
        expected_addr[..13].copy_from_slice(&[7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12]);

        let mut expected_data = [GPIO_NONE; MAX_DATA_PINS];
        expected_data[..8].copy_from_slice(&[16, 17, 18, 19, 20, 21, 22, 23]);

        assert_eq!(pin_map.addr, expected_addr);
        assert_eq!(pin_map.data, expected_data);
    }

    #[test]
    fn filename_empty_is_none() {
        assert_eq!(truncate_filename(""), None);
    }

    #[test]
    fn filename_short_unchanged() {
        assert_eq!(truncate_filename("rom.bin"), Some("rom.bin".to_string()));
    }

    #[test]
    fn filename_truncated_at_max_len() {
        let long_name = "a".repeat(MAX_ROM_FILENAME_LEN + 10);
        let truncated = truncate_filename(&long_name).expect("non-empty");
        assert_eq!(truncated.len(), MAX_ROM_FILENAME_LEN);
        assert_eq!(truncated, "a".repeat(MAX_ROM_FILENAME_LEN));
    }

    #[test]
    fn rom_slot_type_mapping() {
        assert_eq!(
            rom_slot_type(ChipSetType::Single, ChipType::Chip2364),
            RomSlotType::RomSlotTypeSingleRom
        );
        assert_eq!(
            rom_slot_type(ChipSetType::Single, ChipType::Chip6116),
            RomSlotType::RomSlotTypeSingleRam
        );
        assert_eq!(
            rom_slot_type(ChipSetType::Multi, ChipType::Chip2364),
            RomSlotType::RomSlotTypeMultiRom
        );
        assert_eq!(
            rom_slot_type(ChipSetType::Banked, ChipType::Chip2364),
            RomSlotType::RomSlotTypeBankedRom
        );
    }
}
