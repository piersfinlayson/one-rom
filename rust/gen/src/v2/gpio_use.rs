// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The GPIOs a ROM slot's serving uses.

use onerom_metadata::{GpioOverride, OneromAlgAddrConfig, OneromAlgConfig, OneromAlgCsConfig};

/// The GPIOs serving a slot with `alg` uses, as a mask with bit N for GPIO N.
///
/// These are the GPIOs `pio_get_gpio_use()` in
/// `firmware/src/piodma/piorom2.c` reports as serving. A GPIO in the address
/// window with its input forced isn't used, as serving reads the forced level.
pub fn used_gpios(alg: &OneromAlgConfig) -> u64 {
    let span = |first: u8, count: u8| {
        (0..count)
            .filter_map(|n| first.checked_add(n))
            .filter(|&gpio| gpio < 64)
            .fold(0u64, |mask, gpio| mask | (1 << gpio))
    };

    let (OneromAlgCsConfig::AlgCs0 {
        gpio_base,
        base_cs_pin,
        num_cs_pins,
        base_data_pin,
        num_data_pins,
        ..
    }
    | OneromAlgCsConfig::AlgCs1 {
        gpio_base,
        base_cs_pin,
        num_cs_pins,
        base_data_pin,
        num_data_pins,
        ..
    }
    | OneromAlgCsConfig::AlgCs2 {
        gpio_base,
        base_cs_pin,
        num_cs_pins,
        base_data_pin,
        num_data_pins,
        ..
    }
    | OneromAlgCsConfig::Unknown {
        gpio_base,
        base_cs_pin,
        num_cs_pins,
        base_data_pin,
        num_data_pins,
        ..
    }) = alg.alg_cs;
    let mut used = span(gpio_base.saturating_add(base_data_pin), num_data_pins)
        | span(gpio_base.saturating_add(base_cs_pin), num_cs_pins);
    if let OneromAlgCsConfig::AlgCs0 { byte_pin, .. } = alg.alg_cs
        && byte_pin != onerom_metadata::GPIO_NONE
    {
        used |= span(gpio_base.saturating_add(byte_pin), 1);
    }

    let (OneromAlgAddrConfig::AlgAddr0 {
        gpio_base,
        base_addr_pin,
        num_addr_pins,
        ..
    }
    | OneromAlgAddrConfig::Unknown {
        gpio_base,
        base_addr_pin,
        num_addr_pins,
        ..
    }) = alg.alg_addr;
    let forced = alg
        .gpio_override_config
        .iter()
        .flat_map(|config| &config.params)
        .filter(|&&entry| {
            let mode = entry >> 6;
            mode == GpioOverride::GpioOverLow as u8 || mode == GpioOverride::GpioOverHigh as u8
        })
        .fold(0u64, |mask, &entry| mask | (1 << (entry & 0x3F)));
    used | (span(gpio_base.saturating_add(base_addr_pin), num_addr_pins) & !forced)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{Chip, ChipSetType, CsConfig, CsLogic, SizeHandling};
    use crate::v2::rom_slot::build_rom_slot;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;
    use onerom_config::chip::ChipType;
    use onerom_config::hw::Board;

    fn chip(chip_type: ChipType) -> Chip {
        let image = vec![0u8; chip_type.size_bytes()];
        Chip::from_raw_rom_image(
            0,
            "test.bin".to_string(),
            None,
            Some(image.as_slice()),
            image.clone(),
            &chip_type.into(),
            CsConfig::new(Some(CsLogic::ActiveLow), None, None, None),
            &SizeHandling::None,
            crate::PAD_BLANK_BYTE,
            None,
            &[],
        )
        .unwrap()
    }

    /// The mask of `gpios`.
    fn mask(gpios: impl IntoIterator<Item = u8>) -> u64 {
        gpios.into_iter().fold(0, |mask, gpio| mask | (1 << gpio))
    }

    /// The slot's alg config, and the GPIOs its layouts identify as used.
    fn slot(board: Board, set_type: ChipSetType, chips: &[Chip]) -> (OneromAlgConfig, u64) {
        let (slot, addr, cs_data, _) =
            build_rom_slot(board, set_type, chips, 0, None, false).unwrap();
        let cs_base = cs_data.gpio_base + cs_data.base_cs_pin;
        let layout = mask(
            addr.addr_pin_gpios
                .iter()
                .chain(&addr.excess_addr_pin_gpios)
                .chain(&addr.x1_gpio)
                .chain(&addr.x2_gpio)
                .chain(&cs_data.data_pin_gpios)
                .copied()
                .chain(cs_base..cs_base + cs_data.num_cs_pins),
        );
        (slot.alg.unwrap(), layout)
    }

    /// The alg config alone identifies the GPIOs the builder's layouts do.
    #[test]
    fn the_alg_config_identifies_the_layouts_gpios() {
        let cases: Vec<(Board, ChipSetType, Vec<Chip>)> = vec![
            (
                Board::Fire24A,
                ChipSetType::Single,
                vec![chip(ChipType::Chip2364)],
            ),
            (
                Board::Fire24F,
                ChipSetType::Banked,
                vec![chip(ChipType::Chip2364), chip(ChipType::Chip2364)],
            ),
            (
                Board::Fire28C,
                ChipSetType::Banked,
                vec![chip(ChipType::Chip27128), chip(ChipType::Chip27128)],
            ),
            (
                Board::Fire24F,
                ChipSetType::Multi,
                vec![
                    chip(ChipType::Chip2364),
                    chip(ChipType::Chip2364),
                    chip(ChipType::Chip2364),
                ],
            ),
        ];
        for (board, set_type, chips) in cases {
            let (alg, layout) = slot(board, set_type, &chips);
            assert_eq!(used_gpios(&alg), layout, "{board} {set_type:?}");
        }
    }

    /// fire-24-a's X pins are inside a 2364's address window, with their inputs
    /// forced.
    #[test]
    fn a_forced_gpio_isnt_used() {
        let (alg, _) = slot(
            Board::Fire24A,
            ChipSetType::Single,
            &[chip(ChipType::Chip2364)],
        );
        let x = mask([Board::Fire24A.pin_x1(), Board::Fire24A.pin_x2()]);
        assert_eq!(used_gpios(&alg) & x, 0);
    }

    /// fire-28-c's X1 is wired to GPIOs 9 and 28, and a banked set reads it on
    /// 28.
    #[test]
    fn a_banked_set_uses_one_gpio_of_a_dual_wired_x_pin() {
        let (alg, _) = slot(
            Board::Fire28C,
            ChipSetType::Banked,
            &[chip(ChipType::Chip27128), chip(ChipType::Chip27128)],
        );
        assert_eq!(used_gpios(&alg) & mask([9, 28]), mask([28]));
    }
}
