// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Assemble `OneromRomSlot` for one chip set: derives the address/CS-data
//! layouts (once, shared between `build_alg_config`, `build_rom_info`, and
//! `build_rom_image`), then builds the algorithm config, per-chip ROM
//! info, slot type, and ROM table size.

use alloc::vec::Vec;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;

use onerom_metadata::{BitModes, OneromAlgDmaConfig, OneromFirmwareOverrides, OneromRomSlot};

use crate::image::{Chip, ChipSetType};

use super::addr_layout::{AddrLayout, LayoutError, derive_addr_layout};
use super::alg_config::{bit_mode_for, build_alg_config, combined_alg_preference};
use super::alg_preference::CombinedAlgPreference;
use super::cs_data_layout::{CsDataLayout, derive_cs_data_layout};
use super::rom_info::{build_rom_info, rom_slot_type};

/// Bytes per ROM table entry/word, from `alg_dma`'s bit mode: `1` for
/// `BitMode8` (`AlgData0`), `2` for `BitMode16` (`AlgData1`).
///
/// Shared with `rom_image::build_rom_image`, which needs the same value to
/// size the table it generates.
pub(crate) fn bytes_per_word(alg_dma: &OneromAlgDmaConfig) -> u32 {
    match alg_dma {
        OneromAlgDmaConfig::AlgDma0 {
            bit_mode: BitModes::BitMode8,
            ..
        } => 1,
        OneromAlgDmaConfig::AlgDma0 {
            bit_mode: BitModes::BitMode16,
            ..
        } => 2,
    }
}

/// Number of entries in the ROM table: `2^num_addr_pins`.
///
/// Shared with `rom_image::build_rom_image`, which iterates exactly these
/// entries.
pub(crate) fn table_entries(addr_layout: &AddrLayout) -> u32 {
    1u32 << addr_layout.num_addr_pins
}

/// The ROM table size in bytes: `2^num_rom_table_bits` table entries, each
/// `1` byte (`AlgData0`, 8-bit) or `2` bytes (`AlgData1`, 16-bit).
///
/// `num_rom_table_bits` already accounts for X1/X2 as table-index bits for
/// Multi/Banked sets, so this formula is uniform across set types (no
/// separate ROM_IMAGE_SIZE vs ROM_SET_IMAGE_SIZE distinction needed).
fn rom_table_size(addr_layout: &AddrLayout, alg_dma: &OneromAlgDmaConfig) -> u32 {
    table_entries(addr_layout) * bytes_per_word(alg_dma)
}

/// Build `OneromRomSlot` for one chip set, along with the address/CS-data
/// layouts derived for it.
///
/// `chips` is `chip_set.chips` (non-empty, validated by `ChipSet::new`) -
/// deliberately decoupled from `ChipSet` itself, since `id`/`serve_alg`
/// aren't needed here.
///
/// `data` is the absolute flash address of this slot's ROM image data -
/// TODO: Phase 2, currently always `0`/a placeholder pending the top-level
/// layout pass that assigns these once all slots are sized (mirroring v1's
/// `rom_data_ptrs`).
///
/// `firmware_overrides` is the already-converted per-slot override config
/// (point 5, `build_firmware_overrides` - not yet implemented; pass `None`
/// for now).
///
/// `force_16_bit` is chip0's `force_16_bit` override (from the chip set's
/// config, `fire.force_16_bit` - only meaningful when `bit_mode_for`
/// returns `BitMode16`; see `build_alg_data`). Passed through to
/// `build_alg_config`.
///
/// The returned `AddrLayout`/`CsDataLayout` are the same ones used to build
/// `slot.alg`/`slot.roms`; callers building `rom_data_buf` (Phase 2's ROM
/// image generation, `build_rom_image`) should reuse these rather than
/// re-deriving (the derivation is fallible and not free).
pub fn build_rom_slot(
    board: Board,
    set_type: ChipSetType,
    chips: &[Chip],
    data: u32,
    firmware_overrides: Option<OneromFirmwareOverrides>,
    force_16_bit: bool,
) -> Result<
    (
        OneromRomSlot,
        AddrLayout,
        CsDataLayout,
        CombinedAlgPreference,
    ),
    LayoutError,
> {
    let chip_types: Vec<ChipType> = chips.iter().map(|c| *c.chip_type()).collect();
    let cs_config = chips[0].cs_config();

    // Effective bit mode for chip0 - shared between the address-layout
    // derivation (BitMode16 excludes address_pins()[0], "A-1") and
    // build_alg_config's algorithm choice, so they can't disagree.
    // Independent of force_16_bit (see bit_mode_for's docs).
    let bit_mode = bit_mode_for(chip_types[0], board);

    let addr_layout = derive_addr_layout(board, set_type, &chip_types, bit_mode)?;
    let cs_data_layout =
        derive_cs_data_layout(board, set_type, &chip_types, cs_config, Some(&addr_layout))?;

    let alg = build_alg_config(
        board,
        set_type,
        &addr_layout,
        &cs_data_layout,
        bit_mode,
        force_16_bit,
        chip_types.len(),
        cs_config,
    );

    let size = rom_table_size(&addr_layout, &alg.alg_dma);

    let roms = chips
        .iter()
        .map(|chip| build_rom_info(chip, &addr_layout, &cs_data_layout))
        .collect();

    let slot_type = rom_slot_type(set_type, chip_types[0]);

    let pref = combined_alg_preference(&alg);

    let slot = OneromRomSlot {
        data,
        size,
        roms,
        rom_count: chips.len() as u8,
        slot_type,
        alg: Some(alg),
        firmware_overrides,
    };

    Ok((slot, addr_layout, cs_data_layout, pref))
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    use onerom_metadata::{
        GPIO_NONE, MAX_ADDR_PINS, MAX_DATA_PINS, OneromAlgAddrConfig, OneromAlgConfig,
        OneromAlgCsConfig, OneromAlgDataConfig, OneromRomInfo, OneromRomPinMap, RomSlotType,
    };

    use crate::image::{CsConfig, CsLogic, SizeHandling};

    /// End-to-end sentinel: Fire24A, single 2364, CS1 ActiveLow, image data
    /// exactly the right size (8192 bytes) so `SizeHandling::None` applies
    /// without any resizing.
    #[test]
    fn fire24a_2364_single() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None);
        let image = vec![0u8; ChipType::Chip2364.size_bytes()];

        let chip = Chip::from_raw_rom_image(
            0,
            "test.bin".to_string(),
            None,
            Some(image.as_slice()),
            vec![0u8; ChipType::Chip2364.size_bytes()],
            &ChipType::Chip2364,
            cs_config,
            &SizeHandling::None,
            None,
        )
        .expect("chip construction should succeed");

        let chips = [chip];

        let (slot, addr_layout, cs_data_layout, _pref) =
            build_rom_slot(Board::Fire24A, ChipSetType::Single, &chips, 0, None, false)
                .expect("build_rom_slot should succeed");

        assert_eq!(slot.data, 0);
        assert_eq!(slot.size, 1 << 16); // 2^16 * 1 byte/word
        assert_eq!(slot.rom_count, 1);
        assert_eq!(slot.slot_type, RomSlotType::RomSlotTypeSingleRom);
        assert_eq!(slot.firmware_overrides, None);

        assert_eq!(
            slot.alg,
            Some(OneromAlgConfig {
                alg_cs: OneromAlgCsConfig::AlgCs0 {
                    clkdiv_int: 1,
                    clkdiv_frac: 0,
                    gpio_base: 0,
                    base_cs_pin: 13,
                    num_cs_pins: 1,
                    base_data_pin: 16,
                    num_data_pins: 8,
                    cs_active_delay: 0,
                    cs_inactive_delay: 0,
                    serve_cs_low_0: 0,
                    byte_pin: GPIO_NONE,
                    first_rom_cs_base: 13,
                    first_rom_num_cs_pins: 1,
                },
                alg_addr: OneromAlgAddrConfig::AlgAddr0 {
                    clkdiv_int: 1,
                    clkdiv_frac: 0,
                    gpio_base: 0,
                    num_delay_cycles: 2,
                    base_addr_pin: 0,
                    num_addr_pins: 16,
                    num_rom_table_bits: 16,
                },
                alg_data: OneromAlgDataConfig::AlgData0 {
                    clkdiv_int: 1,
                    clkdiv_frac: 0,
                    gpio_base: 0,
                    base_data_pin: 16,
                    word_size: 8,
                },
                alg_dma: OneromAlgDmaConfig::AlgDma0 {
                    bit_mode: BitModes::BitMode8,
                    continuous: 1,
                },
                gpio_pull_config: None,
                gpio_override_config: None,
            })
        );

        let mut expected_addr = [GPIO_NONE; MAX_ADDR_PINS];
        expected_addr[..13].copy_from_slice(&[7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12]);
        let mut expected_data = [GPIO_NONE; MAX_DATA_PINS];
        expected_data[..8].copy_from_slice(&[16, 17, 18, 19, 20, 21, 22, 23]);

        assert_eq!(
            slot.roms,
            vec![OneromRomInfo {
                rom_type: "2364".to_string(),
                filename: Some("test.bin".to_string()),
                pin_map: Some(OneromRomPinMap {
                    addr: expected_addr,
                    data: expected_data,
                }),
            }]
        );

        // Sanity: the returned layouts are the same ones baked into
        // slot.alg/slot.roms above.
        assert_eq!(addr_layout.gpio_base, 0);
        assert_eq!(addr_layout.num_addr_pins, 16);
        assert_eq!(cs_data_layout.gpio_base, 0);
        assert_eq!(cs_data_layout.base_data_pin, 16);
    }

    /// End-to-end: Fire28A, single 23QL384, CS1 ActiveLow.
    ///
    /// Key properties under test:
    /// - `slot.size` = 65536: 2^16 address-table entries * 1 byte/word.
    ///   The 23QL384 has 16 address lines (A0-A15) covering the full
    ///   64KB address space; only 0x0000-0xBFFF (48KB) are valid on the
    ///   chip - the upper 16KB is padded by `build_rom_image`, not here.
    /// - `alg_cs` = `AlgCs2` with `num_qualifier_pins=2`,
    ///   `qualifier_inactive_pattern=0b11` (A14 and A15 both high =
    ///   deselected), `serve_cs_low_0=0` (Single = active-low),
    ///   `byte_pin=GPIO_NONE` (8-bit chip).
    /// - `alg_dma` = `BitMode8`.
    /// - The returned `cs_data_layout.alg_cs2` matches what was used to
    ///   build `slot.alg`.
    ///
    /// Exact GPIO values are board-specific and covered by
    /// `cs_data_layout::tests::fire28a_23ql384_single_alg_cs2_populated`.
    #[test]
    fn fire28a_23ql384_single() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None);
        let image = vec![0u8; ChipType::Chip23QL384.size_bytes()]; // 49152 bytes

        let chip = Chip::from_raw_rom_image(
            0,
            "test.bin".to_string(),
            None,
            Some(image.as_slice()),
            vec![0u8; ChipType::Chip23QL384.size_bytes()],
            &ChipType::Chip23QL384,
            cs_config,
            &SizeHandling::None,
            None,
        )
        .expect("chip construction should succeed");

        let chips = [chip];

        let (slot, addr_layout, cs_data_layout, _pref) =
            build_rom_slot(Board::Fire28A, ChipSetType::Single, &chips, 0, None, false)
                .expect("build_rom_slot should succeed");

        assert_eq!(slot.data, 0);
        // 23QL384 has 16 address lines (A0-A15), but on Fire28A they span
        // 17 GPIO positions (one padding-pool GPIO falls within the range),
        // so num_addr_pins=17 and the table is 2^17 * 1 byte/word = 131072.
        assert_eq!(slot.size, 1 << 17);
        assert_eq!(slot.rom_count, 1);
        assert_eq!(slot.slot_type, RomSlotType::RomSlotTypeSingleRom);
        assert_eq!(slot.firmware_overrides, None);

        let alg = slot.alg.as_ref().expect("alg must be present");

        assert_eq!(
            alg.alg_dma,
            OneromAlgDmaConfig::AlgDma0 {
                bit_mode: BitModes::BitMode8,
                continuous: 1
            }
        );

        match &alg.alg_cs {
            OneromAlgCsConfig::AlgCs2 {
                clkdiv_int: _,
                clkdiv_frac: _,
                gpio_base: _,
                base_cs_pin: _,
                num_cs_pins: _,
                base_data_pin: _,
                num_data_pins: _,
                cs_active_delay: _,
                cs_inactive_delay: _,
                base_qualifier_pin,
                num_qualifier_pins,
                qualifier_inactive_pattern,
            } => {
                assert_eq!(*num_qualifier_pins, 2);
                assert_eq!(*qualifier_inactive_pattern, 0b11);
                // base_qualifier_pin is board-specific; verify it's within
                // the 32-GPIO PIO window and consistent with the returned layout.
                assert!(
                    *base_qualifier_pin < 32,
                    "base_qualifier_pin {base_qualifier_pin} out of PIO window"
                );
                assert_eq!(
                    *base_qualifier_pin,
                    cs_data_layout.alg_cs2.as_ref().unwrap().base_qualifier_pin,
                    "alg_cs base_qualifier_pin must match cs_data_layout"
                );
            }
            other => panic!("expected AlgCs2 for 23QL384, got {other:?}"),
        }

        // 23QL384 address lines span 17 GPIO positions on Fire28A (see slot.size above).
        assert_eq!(addr_layout.num_addr_pins, 17);

        // The returned cs_data_layout must have alg_cs2 populated and
        // consistent with what was baked into slot.alg above.
        let cs2 = cs_data_layout
            .alg_cs2
            .as_ref()
            .expect("23QL384 must have alg_cs2");
        assert_eq!(cs2.num_qualifier_pins, 2);
        assert_eq!(cs2.qualifier_inactive_pattern, 0b11);
    }
}
