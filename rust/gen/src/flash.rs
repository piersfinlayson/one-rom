// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! A board's flash chips, and where the v2 builder places ROM slots on them.

use alloc::vec::Vec;
use core::ops::Range;

use onerom_config::hw::BoardSize;
use onerom_config::mcu::{Family, Variant};
use onerom_metadata::otp::{FlashLayout, flash_size_bytes};
use onerom_metadata::{FLASH_CS1_BASE_ADDR, METADATA_OFFSET, METADATA_SIZE, OneromFlashSize};

use crate::{Error, Result};

/// The addresses of a board's flash chips.
///
/// Every board has a first chip. An RP2350 board's chips are the layout
/// [`FLASH_LAYOUTS`](onerom_metadata::otp::FLASH_LAYOUTS) lists for its size,
/// so the tools build every L image for a second chip on chip select 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashChips {
    size: BoardSize,
    first_base: u32,
    first_len: u32,
    second_len: Option<u32>,
}

impl FlashChips {
    /// The flash chips of a `size` board with `mcu`.
    pub fn new(mcu: Variant, size: BoardSize) -> Self {
        let (first_len, second_len) = match mcu.family() {
            Family::Rp2350 => {
                let layout = FlashLayout::of(size);
                let second = (layout.cs1 != OneromFlashSize::FlashSizeNone)
                    .then(|| flash_size_bytes(layout.cs1) as u32);
                (flash_size_bytes(layout.cs0) as u32, second)
            }
            Family::Stm32f4 => (mcu.flash_storage_bytes() as u32, None),
        };
        Self {
            size,
            first_base: mcu.family().get_flash_base(),
            first_len,
            second_len,
        }
    }

    /// The first chip's addresses on a board with `mcu`. They're the same
    /// whatever the board's size.
    pub fn first_for(mcu: Variant) -> Range<u32> {
        Self::new(mcu, BoardSize::M).first()
    }

    /// The size of board these are the chips of.
    pub fn size(&self) -> BoardSize {
        self.size
    }

    /// The first chip's addresses.
    pub fn first(&self) -> Range<u32> {
        self.first_base..self.first_base + self.first_len
    }

    /// The second chip's addresses, or `None` where the board doesn't have
    /// one.
    pub fn second(&self) -> Option<Range<u32>> {
        self.second_len
            .map(|len| FLASH_CS1_BASE_ADDR..FLASH_CS1_BASE_ADDR + len)
    }

    /// Where ROM data starts on the first chip, after the firmware and the
    /// metadata region.
    pub fn rom_data_start(&self) -> u32 {
        self.first_base + METADATA_OFFSET + METADATA_SIZE as u32
    }
}

/// The address of each slot, in order, from each slot's size in `sizes`.
/// Slots on the first chip start at [`FlashChips::rom_data_start`].
///
/// A slot goes on the first chip after the slots already there if it fits.
/// Otherwise it goes on the second chip after the slots already there. A slot
/// never spans the two because the firmware copies a slot in one transfer.
///
/// [`Error::SlotDoesNotFit`] identifies the first slot that fits neither chip.
pub fn slot_addresses(chips: &FlashChips, sizes: &[u32]) -> Result<Vec<u32>> {
    let mut first = Space {
        next: chips.rom_data_start(),
        end: chips.first().end,
    };
    let mut second = chips.second().map(|chip| Space {
        next: chip.start,
        end: chip.end,
    });
    sizes
        .iter()
        .enumerate()
        .map(|(slot, &size)| {
            first
                .take(size)
                .or_else(|| second.as_mut()?.take(size))
                .ok_or(Error::SlotDoesNotFit { slot })
        })
        .collect()
}

/// The unused part of a chip.
struct Space {
    next: u32,
    end: u32,
}

impl Space {
    /// Takes `size` bytes from the start of the space and returns their
    /// address, or `None` if they don't fit.
    fn take(&mut self, size: u32) -> Option<u32> {
        let end = self.next.checked_add(size).filter(|&end| end <= self.end)?;
        let addr = self.next;
        self.next = end;
        Some(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KB: u32 = 1024;
    const MB: u32 = 1024 * KB;

    #[test]
    fn an_m_rp2350_board_has_one_2mb_chip() {
        for mcu in [Variant::RP2350, Variant::RP2350B] {
            let chips = FlashChips::new(mcu, BoardSize::M);
            assert_eq!(chips.first(), 0x1000_0000..0x1020_0000, "{mcu}");
            assert_eq!(chips.second(), None, "{mcu}");
        }
    }

    #[test]
    fn an_l_rp2350_board_has_a_second_2mb_chip_at_chip_select_1() {
        for mcu in [Variant::RP2350, Variant::RP2350B] {
            let chips = FlashChips::new(mcu, BoardSize::L);
            assert_eq!(chips.first(), 0x1000_0000..0x1020_0000, "{mcu}");
            assert_eq!(chips.second(), Some(0x1100_0000..0x1120_0000), "{mcu}");
        }
    }

    #[test]
    fn an_stm32_board_has_its_own_flash_alone_whatever_the_size() {
        for size in [BoardSize::M, BoardSize::L] {
            let chips = FlashChips::new(Variant::F411RE, size);
            assert_eq!(chips.first(), 0x0800_0000..0x0808_0000, "{size}");
            assert_eq!(chips.second(), None, "{size}");
        }
    }

    #[test]
    fn the_first_chip_is_the_same_whatever_the_size() {
        for mcu in [Variant::RP2350, Variant::RP2350B, Variant::F411RE] {
            for &size in BoardSize::supported_values() {
                let chips = FlashChips::new(mcu, size);
                assert_eq!(chips.first(), FlashChips::first_for(mcu), "{mcu} {size}");
                assert_eq!(chips.size(), size, "{mcu} {size}");
            }
        }
    }

    /// The v2 schema places the metadata region, and ROM data follows it.
    #[test]
    fn rom_data_starts_after_the_metadata_region() {
        for mcu in [Variant::RP2350, Variant::RP2350B] {
            for &size in BoardSize::supported_values() {
                let chips = FlashChips::new(mcu, size);
                assert_eq!(chips.rom_data_start(), 0x1001_0000, "{mcu} {size}");
                assert_eq!(
                    chips.rom_data_start(),
                    onerom_metadata::METADATA_BASE + METADATA_SIZE as u32,
                    "{mcu} {size}"
                );
            }
        }
    }

    #[test]
    fn rom_data_fills_the_rest_of_the_first_chip() {
        for mcu in [Variant::RP2350, Variant::RP2350B, Variant::F411RE] {
            let chips = FlashChips::new(mcu, BoardSize::M);
            assert_eq!(
                (chips.first().end - chips.rom_data_start()) as usize,
                crate::rom_data_space(mcu),
                "{mcu}"
            );
        }
    }

    #[test]
    fn slots_fill_the_first_chip_in_order() {
        let chips = FlashChips::new(Variant::RP2350, BoardSize::L);
        let addrs = slot_addresses(&chips, &[64 * KB, 64 * KB, 16 * KB]).unwrap();
        assert_eq!(addrs, [0x1001_0000, 0x1002_0000, 0x1003_0000]);
    }

    #[test]
    fn slots_on_an_stm32_board_start_after_its_own_metadata_region() {
        let chips = FlashChips::new(Variant::F411RE, BoardSize::M);
        let addrs = slot_addresses(&chips, &[16 * KB, 16 * KB]).unwrap();
        assert_eq!(addrs, [0x0801_0000, 0x0801_4000]);
    }

    #[test]
    fn a_slot_that_doesnt_fit_the_first_chip_goes_on_the_second() {
        let chips = FlashChips::new(Variant::RP2350, BoardSize::L);
        // 1MB, 512KB, 512KB and then 256KB, which fits the 448KB left on the
        // first chip after the first two.
        let addrs = slot_addresses(&chips, &[MB, 512 * KB, 512 * KB, 256 * KB]).unwrap();
        assert_eq!(addrs, [0x1001_0000, 0x1011_0000, 0x1100_0000, 0x1019_0000]);
    }

    #[test]
    fn a_slot_can_end_exactly_at_a_chip_end() {
        let chips = FlashChips::new(Variant::RP2350, BoardSize::L);
        let addrs = slot_addresses(&chips, &[1984 * KB, 2 * MB]).unwrap();
        assert_eq!(addrs, [0x1001_0000, 0x1100_0000]);
    }

    #[test]
    fn a_slot_that_fits_neither_chip_is_refused_by_index() {
        let chips = FlashChips::new(Variant::RP2350, BoardSize::L);
        let err = slot_addresses(&chips, &[MB, 2 * MB, MB]).unwrap_err();
        assert!(matches!(err, Error::SlotDoesNotFit { slot: 2 }), "{err:?}");

        let chips = FlashChips::new(Variant::RP2350, BoardSize::M);
        let err = slot_addresses(&chips, &[MB, MB]).unwrap_err();
        assert!(matches!(err, Error::SlotDoesNotFit { slot: 1 }), "{err:?}");
    }

    #[test]
    fn a_slot_past_the_end_of_the_address_space_is_refused() {
        let chips = FlashChips::new(Variant::RP2350, BoardSize::L);
        let err = slot_addresses(&chips, &[u32::MAX]).unwrap_err();
        assert!(matches!(err, Error::SlotDoesNotFit { slot: 0 }), "{err:?}");
    }
}
