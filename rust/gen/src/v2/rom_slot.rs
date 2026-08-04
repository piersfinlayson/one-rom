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

use onerom_metadata::{
    BitModes, OneromAlgDmaConfig, OneromFirmwareOverrides, OneromRomSlot, Pointer,
};

use crate::MAX_IMAGE_SIZE;
use crate::image::{Chip, ChipSetType, CsConfig};

use super::addr_layout::{AddrLayout, LayoutError, derive_addr_layout};
use super::alg_config::{bit_mode_for, build_alg_config, combined_alg_preference};
use super::alg_preference::CombinedAlgPreference;
use super::cs_data_layout::{CsDataLayout, derive_cs_data_layout};
use super::multi_cs_config::{MultiChipCsConfig, derive_multi_cs_config};
use super::rom_info::{build_rom_info, rom_slot_type};
use super::slot_context::{SlotContext, socket_pin_offset};

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
/// `data` is the absolute flash address of this slot's ROM image data.
/// Callers (e.g. `build_v2`) typically pass `0` as a placeholder and patch
/// `slot.data` with the real address once all slot sizes are known.
///
/// `firmware_overrides` is the already-converted per-slot override config,
/// built by `build_firmware_overrides` from the chip set's `FirmwareConfig`.
///
/// `force_16_bit` is chip0's `force_16_bit` override (from the chip set's
/// config, `fire.force_16_bit` - only meaningful when `bit_mode_for`
/// returns `BitMode16`; see `build_alg_data`). Passed through to
/// `build_alg_config`.
///
/// The returned `AddrLayout`/`CsDataLayout` are the same ones used to build
/// `slot.alg`/`slot.roms`; callers building `rom_data_buf` via
/// `build_rom_image` should reuse these rather than re-deriving (the
/// derivation is fallible and not free).
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
    let cs_config = *chips[0].cs_config();

    let pin_offset = socket_pin_offset(chip_types[0].chip_pins(), board.chip_pins()).ok_or(
        LayoutError::IncompatiblePinCount {
            board,
            chip_type: chip_types[0],
        },
    )?;

    let bit_mode = bit_mode_for(chip_types[0], board);

    let multi_cs_config: Option<MultiChipCsConfig> = if set_type == ChipSetType::Multi {
        Some(derive_multi_cs_config(
            chip_types[0],
            chips[0].cs_config(),
            chips[1].cs_config(),
        ))
    } else {
        None
    };

    let ctx = SlotContext {
        board,
        set_type,
        chip_types,
        cs_config,
        bit_mode,
        pin_offset,
        force_16_bit,
        multi_cs_config,
    };

    let addr_layout = derive_addr_layout(&ctx)?;
    let cs_data_layout = derive_cs_data_layout(&ctx, Some(&addr_layout))?;

    let secondary_cs_configs: Vec<CsConfig> = if set_type == ChipSetType::Multi {
        chips[1..].iter().map(|c| *c.cs_config()).collect()
    } else {
        Vec::new()
    };

    let alg = build_alg_config(&ctx, &addr_layout, &cs_data_layout, &secondary_cs_configs);

    let size = rom_table_size(&addr_layout, &alg.alg_dma);

    // Check here rather than in build_rom_image: we have board, chip type,
    // set type, and num_chips in scope, so we can produce a message the
    // user can actually act on.
    if size as usize > MAX_IMAGE_SIZE {
        return Err(LayoutError::RomTableTooLarge {
            board: ctx.board,
            chip_type: ctx.chip_types[0],
            set_type: ctx.set_type,
            num_chips: ctx.chip_types.len(),
            num_addr_pins: addr_layout.num_addr_pins,
            table_size: size as usize,
        });
    }

    let roms = chips
        .iter()
        .map(|chip| build_rom_info(chip, &addr_layout, &cs_data_layout))
        .collect();

    let slot_type = rom_slot_type(ctx.set_type, ctx.chip_types[0]);

    let pref = combined_alg_preference(&alg);

    let slot = OneromRomSlot {
        data: Pointer::new(data),
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
        GPIO_NONE, GpioOverride, MAX_ADDR_PINS, MAX_DATA_PINS, OneromAlgAddrConfig,
        OneromAlgConfig, OneromAlgCsConfig, OneromAlgDataConfig, OneromAlgOverrideConfig,
        OneromRomInfo, OneromRomPinMap, Pointer, RomSlotType,
    };

    use crate::v2::cs_overrides::encode_override;

    use crate::image::{CsConfig, CsLogic, SizeHandling};

    /// End-to-end sentinel: Fire24A, single 2364, CS1 ActiveLow, image data
    /// exactly the right size (8192 bytes) so `SizeHandling::None` applies
    /// without any resizing.
    #[test]
    fn fire24a_2364_single() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let image = vec![0u8; ChipType::Chip2364.size_bytes()];

        let chip = Chip::from_raw_rom_image(
            0,
            "test.bin".to_string(),
            None,
            Some(image.as_slice()),
            vec![0u8; ChipType::Chip2364.size_bytes()],
            &ChipType::Chip2364.into(),
            cs_config,
            &SizeHandling::None,
            crate::PAD_BLANK_BYTE,
            None,
            &[],
        )
        .expect("chip construction should succeed");

        let chips = [chip];

        let (slot, addr_layout, cs_data_layout, _pref) =
            build_rom_slot(Board::Fire24A, ChipSetType::Single, &chips, 0, None, false)
                .expect("build_rom_slot should succeed");

        assert_eq!(slot.data, Pointer::Null);
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
                // GPIO8 (X2) and GPIO9 (X1) are unused on a Single set and
                // sit inside the [0,16) address window → both forced low.
                gpio_override_config: Some(OneromAlgOverrideConfig {
                    params: alloc::vec![
                        encode_override(8, GpioOverride::GpioOverLow),
                        encode_override(9, GpioOverride::GpioOverLow),
                    ],
                }),
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
                chip_size: ChipType::Chip2364.size_bytes() as u32,
                rbcp_rom_type: ChipType::Chip2364.rbcp_chip_type(),
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
    #[test]
    #[allow(clippy::wildcard_enum_match_arm)]
    fn fire28a_23ql384_single() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        let image = vec![0u8; ChipType::Chip23QL384.size_bytes()]; // 49152 bytes

        let chip = Chip::from_raw_rom_image(
            0,
            "test.bin".to_string(),
            None,
            Some(image.as_slice()),
            vec![0u8; ChipType::Chip23QL384.size_bytes()],
            &ChipType::Chip23QL384.into(),
            cs_config,
            &SizeHandling::None,
            crate::PAD_BLANK_BYTE,
            None,
            &[],
        )
        .expect("chip construction should succeed");

        let chips = [chip];

        let (slot, addr_layout, cs_data_layout, _pref) =
            build_rom_slot(Board::Fire28A, ChipSetType::Single, &chips, 0, None, false)
                .expect("build_rom_slot should succeed");

        assert_eq!(slot.data, Pointer::Null);
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

        assert_eq!(addr_layout.num_addr_pins, 17);

        let cs2 = cs_data_layout
            .alg_cs2
            .as_ref()
            .expect("23QL384 must have alg_cs2");
        assert_eq!(cs2.num_qualifier_pins, 2);
        assert_eq!(cs2.qualifier_inactive_pattern, 0b11);
    }

    /// End-to-end sentinel: Fire28C, 2-chip Banked 27128, CS1 ActiveLow.
    #[test]
    #[allow(clippy::wildcard_enum_match_arm)]
    fn fire28c_27128_banked_2chip() {
        let image = vec![0u8; ChipType::Chip27128.size_bytes()];

        let chip0 = Chip::from_raw_rom_image(
            0,
            "bank0.bin".to_string(),
            None,
            Some(image.as_slice()),
            vec![0u8; ChipType::Chip27128.size_bytes()],
            &ChipType::Chip27128.into(),
            CsConfig::new(Some(CsLogic::ActiveLow), None, None, None),
            &SizeHandling::None,
            crate::PAD_BLANK_BYTE,
            None,
            &[],
        )
        .expect("chip 0 construction should succeed");

        let chip1 = Chip::from_raw_rom_image(
            1,
            "bank1.bin".to_string(),
            None,
            Some(image.as_slice()),
            vec![0u8; ChipType::Chip27128.size_bytes()],
            &ChipType::Chip27128.into(),
            CsConfig::new(Some(CsLogic::ActiveLow), None, None, None),
            &SizeHandling::None,
            crate::PAD_BLANK_BYTE,
            None,
            &[],
        )
        .expect("chip 1 construction should succeed");

        let chips = [chip0, chip1];

        let (slot, addr_layout, _cs_data_layout, _pref) =
            build_rom_slot(Board::Fire28C, ChipSetType::Banked, &chips, 0, None, false)
                .expect("build_rom_slot should succeed");

        assert_eq!(slot.data, Pointer::Null);
        assert_eq!(slot.size, 1 << 16);
        assert_eq!(slot.rom_count, 2);
        assert_eq!(slot.slot_type, RomSlotType::RomSlotTypeBankedRom);
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
            OneromAlgCsConfig::AlgCs0 { serve_cs_low_0, .. } => {
                assert_eq!(*serve_cs_low_0, 0);
            }
            other => panic!("expected AlgCs0 for Banked set, got {other:?}"),
        }

        assert_eq!(addr_layout.x1_gpio, Some(28));
        assert_eq!(addr_layout.x2_gpio, None);

        let pull = alg
            .gpio_pull_config
            .as_ref()
            .expect("Banked set must have gpio_pull_config");
        // Fire28C X1 is dual-bonded [GPIO9, GPIO28]: pull-up on both bonds.
        assert_eq!(pull.params, alloc::vec![0x80 | 9u8, 0x80 | 28u8]);

        assert_eq!(slot.roms.len(), 2);
        assert_eq!(slot.roms[0].rom_type, "27128");
        assert_eq!(slot.roms[1].rom_type, "27128");
        assert_eq!(slot.roms[0].filename, Some("bank0.bin".to_string()));
        assert_eq!(slot.roms[1].filename, Some("bank1.bin".to_string()));
    }

    /// Fire28C, 3-chip Banked 23QL512: address pins span GPIOs 10-27 (18
    /// wide), X1/X2 outside that range → span 20 → 1MB table > MAX_IMAGE_SIZE.
    /// Verifies that `build_rom_slot` returns a `LayoutError::RomTableTooLarge`
    /// and that it converts to an `Error::UnsupportedBoardConfig` whose
    /// message names the board, chip count, and address-bit count.
    #[test]
    fn fire28c_23ql512_banked_3chip_too_large() {
        let image = vec![0u8; ChipType::Chip23QL512.size_bytes()];
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);

        let make_chip = |i: usize| {
            Chip::from_raw_rom_image(
                i,
                alloc::format!("bank{i}.bin"),
                None,
                Some(image.as_slice()),
                vec![0u8; ChipType::Chip23QL512.size_bytes()],
                &ChipType::Chip23QL512.into(),
                cs_config,
                &SizeHandling::None,
                crate::PAD_BLANK_BYTE,
                None,
                &[],
            )
            .expect("chip construction should succeed")
        };

        let chips = [make_chip(0), make_chip(1), make_chip(2)];

        let err = build_rom_slot(Board::Fire28C, ChipSetType::Banked, &chips, 0, None, false)
            .expect_err("3-chip Banked 23QL512 on Fire28C must fail");

        // Verify the variant carries the right context before conversion.
        assert!(
            matches!(
                err,
                LayoutError::RomTableTooLarge {
                    num_chips: 3,
                    num_addr_pins: 20,
                    ..
                }
            ),
            "expected RomTableTooLarge with num_chips=3, num_addr_pins=20, got {err:?}"
        );

        // Converted Error must be UnsupportedBoardConfig with a human-readable message.
        let converted: crate::Error = err.into();
        let msg = converted.to_string();
        assert!(
            msg.contains("3-chip"),
            "message should mention chip count: {msg}"
        );
        assert!(
            msg.contains("20 address bits"),
            "message should mention address bit count: {msg}"
        );
        assert!(
            msg.contains("2 chips"),
            "message should suggest reducing to 2 chips: {msg}"
        );
    }
}
