// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! ROM image generation ("Phase 2"): for a chip set, produce the
//! `size`-byte ROM table (the bytes that live at `OneromRomSlot::data` in
//! `rom_data_buf`), from the address/CS-data layouts already derived for
//! that set.
//!
//! Covers Single and Banked (2, 3 or 4 chips), both `AlgData0`/`BitMode8`
//! (1 byte/entry) and `AlgData1`-or-`AlgData0`/`BitMode16` (2 bytes/entry -
//! `force_16_bit` doesn't affect the table, only which algorithm serves
//! it, so this function doesn't need it). Multi is out of scope for now -
//! see the One ROM v2 handoff notes.
//!
//! For `BitMode16`, `addr_pin_gpios` has 18 entries (A0-A17 - "A-1",
//! `address_pins()[0]`, is excluded per `addr_layout`'s carve-out) and
//! `data_pin_gpios` has 16 entries (D0-D15). Each table entry is a 16-bit
//! word, decomposed/stored as **2 little-endian bytes**
//! (`table[2i]`/`table[2i+1]` = low/high byte of the mangled word) -
//! matching how the RP2350 DMA reads a `u16` from memory.
//!
//! The image file for a `BitMode16`-capable chip (e.g. 27C400/27C200) is
//! assumed to be laid out in **byte-mode addressing**: byte address =
//! (A-1, A0, A1, ..., A17) with A-1 as the LSB. So for word address
//! `chip_addr` (from `addr_pin_gpios`, A0..A17), the corresponding 16-bit
//! word's low byte (A-1=0) is `image[2*chip_addr]` and its high byte
//! (A-1=1) is `image[2*chip_addr+1]`.

use alloc::format;
use alloc::vec::Vec;

use onerom_metadata::OneromAlgDmaConfig;

use crate::image::{Chip, ChipSetType};
use crate::{Error, Result, MAX_IMAGE_SIZE, PAD_NO_CHIP_BYTE};

use super::addr_layout::AddrLayout;
use super::cs_data_layout::CsDataLayout;
use super::rom_slot::{bytes_per_word, table_entries};

/// Build the ROM image table for one chip set/slot.
///
/// The returned `Vec<u8>` has `2^addr_layout.num_addr_pins *
/// bytes_per_word(alg_dma)` entries (`bytes_per_word` is `1` for
/// `BitMode8`, `2` for `BitMode16`).
///
/// For table index `i` in `0..2^num_addr_pins`:
///
/// - The chip address is decomposed from `i` via
///   `addr_layout.addr_pin_gpios`: bit `n` of the chip address is bit
///   `(addr_pin_gpios[n] - gpio_base)` of `i`. For `BitMode8` this is the
///   byte address directly; for `BitMode16` it's the 16-bit word address
///   (A0..A17 - see module docs for the byte/word addressing convention).
/// - For Banked sets:
///   - 2 chips: bit `(x1_gpio - gpio_base)` of `i` selects `chips[0]` or
///     `chips[1]`.
///   - 3 or 4 chips: bits `(x1_gpio - gpio_base)` (LSB) and `(x2_gpio -
///     gpio_base)` together form a 2-bit bank index 0..3, selecting
///     `chips[bank_index]`. For a 3-chip set, bank index `3` (X1 and X2
///     both set - "both jumpered") means no chip occupies this portion of
///     the address space, and the table entry is `bytes_per_word` copies
///     of `PAD_NO_CHIP_BYTE`, used as-is with no data-pin mangling.
/// - Any other bit of `i` (used by neither of the above) is a "padding
///   pool" bit: it doesn't affect the table entry, so the same value is
///   naturally produced for both its settings.
/// - Otherwise, the selected chip's image byte(s) at the chip address are
///   "mangled" per `cs_data_layout.data_pin_gpios`: bit `d` of the raw
///   value (chip data line `d`) moves to bit `(data_pin_gpios[d] -
///   cs_data_layout.gpio_base - cs_data_layout.base_data_pin)` of the
///   mangled value, which is then written out as `bytes_per_word`
///   little-endian bytes.
///
/// # Errors
///
/// - [`Error::RomTableTooLarge`] if `2^addr_layout.num_addr_pins *
///   bytes_per_word(alg_dma)` exceeds [`crate::MAX_IMAGE_SIZE`] - the
///   per-slot RAM budget (only one slot is served at a time, so this is
///   the limit on a single table, not the sum across slots).
/// - [`Error::UnsupportedFeature`] if `set_type` is `ChipSetType::Multi`
///   (out of scope - see handoff notes).
/// - [`Error::InvalidConfig`] if `chips.len()` isn't `1` for `Single`, or
///   `2`/`3`/`4` for `Banked`.
/// - [`Error::MissingImageData`] if any `chips[n].data()` is `None` - by
///   the time a `Chip` reaches here its image data must already be
///   validated/sized (`from_raw_rom_image` + `SizeHandling`).
///
/// # Panics
///
/// `addr_layout.x1_gpio`/`x2_gpio` are `.expect()`-ed to be `Some` for
/// Banked sets, and all GPIO-offset subtractions assume `gpio_base` is the
/// minimum of the relevant GPIOs - both guaranteed by
/// `derive_addr_layout`/`derive_cs_data_layout`'s construction, not by
/// anything checked in this function.
pub fn build_rom_image(
    addr_layout: &AddrLayout,
    cs_data_layout: &CsDataLayout,
    set_type: ChipSetType,
    chips: &[Chip],
    alg_dma: &OneromAlgDmaConfig,
) -> Result<Vec<u8>> {
    let entries = table_entries(addr_layout) as usize;
    let word_bytes = bytes_per_word(alg_dma) as usize;

    let image_size = entries * word_bytes;
    if image_size > MAX_IMAGE_SIZE {
        return Err(Error::RomTableTooLarge { size: image_size, max: MAX_IMAGE_SIZE });
    }

    // Bit positions (within `i`, relative to addr_layout.gpio_base) for
    // each chip address line, in chip0.address_pins() order (for
    // BitMode16, this is A0..A17 - "A-1" is excluded per addr_layout's
    // carve-out). Guaranteed >= 0 since addr_layout.gpio_base is defined
    // as the minimum of all resolved address-range GPIOs, which includes
    // every addr_pin_gpios entry.
    let addr_bit_positions: Vec<u8> = addr_layout
        .addr_pin_gpios
        .iter()
        .map(|&gpio| gpio - addr_layout.gpio_base)
        .collect();

    // Bit positions (within `i`) for the bank-select GPIOs, X1 first (LSB
    // of the bank index), then X2 if the set has 3 or 4 chips.
    let bank_bit_positions: Vec<u8> = match set_type {
        ChipSetType::Single => {
            if chips.len() != 1 {
                return Err(Error::InvalidConfig {
                    error: format!("Single set must have exactly 1 chip, got {}", chips.len()),
                });
            }
            Vec::new()
        }
        ChipSetType::Banked => {
            // x1_gpio/x2_gpio are guaranteed Some for Banked sets by
            // derive_addr_layout, and (like addr_pin_gpios above) within
            // [gpio_base, gpio_base + num_addr_pins) by the same span
            // computation.
            let x1_bit = addr_layout
                .x1_gpio
                .expect("Banked AddrLayout must have x1_gpio")
                - addr_layout.gpio_base;
            match chips.len() {
                2 => alloc::vec![x1_bit],
                3 | 4 => {
                    let x2_bit = addr_layout
                        .x2_gpio
                        .expect("Banked AddrLayout must have x2_gpio")
                        - addr_layout.gpio_base;
                    alloc::vec![x1_bit, x2_bit]
                }
                n => {
                    return Err(Error::InvalidConfig {
                        error: format!("Banked set must have 2, 3 or 4 chips, got {n}"),
                    })
                }
            }
        }
        ChipSetType::Multi => {
            return Err(Error::UnsupportedFeature {
                feat: "Multi-set ROM image generation",
            })
        }
    };

    // Bit positions (within the mangled value) for each chip data line
    // `d`, in chip0.data_pins() order. Guaranteed >= 0 since the data
    // lines occupy a contiguous range starting at gpio_base +
    // base_data_pin (derive_cs_data_layout rejects non-contiguous data
    // lines).
    let data_base = cs_data_layout.gpio_base + cs_data_layout.base_data_pin;
    let data_bit_positions: Vec<u8> = cs_data_layout
        .data_pin_gpios
        .iter()
        .map(|&gpio| gpio - data_base)
        .collect();

    let chip_data: Vec<&[u8]> = chips
        .iter()
        .enumerate()
        .map(|(index, chip)| {
            chip.data().ok_or(Error::MissingImageData {
                chip_type: *chip.chip_type(),
                index,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut image = Vec::with_capacity(entries * word_bytes);

    for i in 0..entries as u32 {
        let mut chip_addr: usize = 0;
        for (n, &bit_pos) in addr_bit_positions.iter().enumerate() {
            let bit = ((i >> bit_pos) & 1) as usize;
            chip_addr |= bit << n;
        }

        let mut bank_index: usize = 0;
        for (n, &bit_pos) in bank_bit_positions.iter().enumerate() {
            let bit = ((i >> bit_pos) & 1) as usize;
            bank_index |= bit << n;
        }

        if bank_index >= chips.len() {
            // Only reachable for a 3-chip Banked set with X1 and X2 both
            // set ("both jumpered"): no chip occupies this portion of the
            // address space. PAD_NO_CHIP_BYTE is used directly (for each
            // byte of the entry), with no data-pin mangling.
            image.extend(core::iter::repeat_n(PAD_NO_CHIP_BYTE, word_bytes));
            continue;
        }

        let chip_image = chip_data[bank_index];

        // Raw value at chip_addr, as a u32 (8 or 16 significant bits
        // depending on word_bytes): for BitMode8, the byte at chip_addr;
        // for BitMode16, the 16-bit word at word address chip_addr,
        // assembled little-endian from the byte-addressed image (see
        // module docs).
        let raw: u32 = match word_bytes {
            1 => chip_image[chip_addr] as u32,
            2 => {
                let byte_addr = chip_addr * 2;
                chip_image[byte_addr] as u32 | (chip_image[byte_addr + 1] as u32) << 8
            }
            n => unreachable!("bytes_per_word only returns 1 or 2, got {n}"),
        };

        // Mangle: bit `d` of `raw` (chip data line d) moves to bit
        // `data_bit_positions[d]` of `mangled`.
        let mut mangled: u32 = 0;
        for (d, &bit_pos) in data_bit_positions.iter().enumerate() {
            let bit = (raw >> d) & 1;
            mangled |= bit << bit_pos;
        }

        // Write out little-endian: byte 0 = bits 0-7, byte 1 = bits 8-15.
        for b in 0..word_bytes {
            image.push(((mangled >> (8 * b)) & 0xFF) as u8);
        }
    }

    Ok(image)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    use onerom_config::chip::ChipType;
    use onerom_metadata::BitModes;

    use crate::image::{CsConfig, CsLogic, SizeHandling};

    /// Build a `Chip2364` (8192-byte image) with the given leading bytes;
    /// the rest of the image is zero-filled. Test-only convenience -
    /// `Chip2364` is just a convenient "any chip with a large-enough
    /// image" stand-in (used for both BitMode8 and BitMode16 tests, since
    /// `build_rom_image` only ever reads `chip.data()` as raw bytes).
    fn chip_with_bytes(filename: &str, bytes: &[u8]) -> Chip {
        let mut image = vec![0u8; ChipType::Chip2364.size_bytes()];
        image[..bytes.len()].copy_from_slice(bytes);
        chip_with_image(filename, image)
    }

    fn chip_with_image(filename: &str, image: Vec<u8>) -> Chip {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None);
        Chip::from_raw_rom_image(
            0,
            filename.to_string(),
            None,
            Some(image.as_slice()),
            vec![0u8; ChipType::Chip2364.size_bytes()],
            &ChipType::Chip2364,
            cs_config,
            &SizeHandling::None,
            None,
        )
        .expect("chip construction should succeed")
    }

    fn alg_dma_8bit() -> OneromAlgDmaConfig {
        OneromAlgDmaConfig::AlgDma0 { bit_mode: BitModes::BitMode8, continuous: 1 }
    }

    fn alg_dma_16bit() -> OneromAlgDmaConfig {
        OneromAlgDmaConfig::AlgDma0 { bit_mode: BitModes::BitMode16, continuous: 1 }
    }

    /// Identity data-pin mapping: GPIO 0..=7 -> output bits 0..=7.
    fn identity_cs_data_layout_8bit() -> CsDataLayout {
        CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7],
            base_cs_pin: 0,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: Vec::new(),
        }
    }

    /// Identity data-pin mapping: GPIO 0..=15 -> output bits 0..=15
    /// (matches Fire40A/27C400's D0-D15 -> GPIO0-15).
    fn identity_cs_data_layout_16bit() -> CsDataLayout {
        CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 16,
            data_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            base_cs_pin: 16,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: Vec::new(),
        }
    }

    /// Single, 8-bit, Fire24A/2364 layout - exercises the address-line
    /// bit decomposition and a padding-pool bit (GPIO 8, not in
    /// `addr_pin_gpios`). Data-pin mapping here is identity (GPIO
    /// 16..=23 -> bits 0..=7), so the data-mangling step is a no-op.
    #[test]
    fn single_8bit_fire24a_2364_address_mangling() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 16,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12],
        };
        let cs_data_layout = CsDataLayout {
            gpio_base: 13,
            base_data_pin: 3,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![16, 17, 18, 19, 20, 21, 22, 23],
            base_cs_pin: 0,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: Vec::new(),
        };

        let image: Vec<u8> = (0..ChipType::Chip2364.size_bytes() as u32)
            .map(|k| k as u8)
            .collect();
        let chips = [chip_with_image("test.bin", image.clone())];

        let table = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Single, &chips, &alg_dma_8bit())
            .expect("build_rom_image should succeed");

        assert_eq!(table.len(), 1 << 16);

        // i=0 -> chip_addr=0 (all addr_pin_gpios bits of i are 0).
        assert_eq!(table[0], image[0]);
        // i=1 (GPIO0 set) -> addr_pin_gpios[7]=0, so chip address bit 7
        // is set -> chip_addr = 1<<7 = 128.
        assert_eq!(table[1], image[128]);
        // i=2 (GPIO1 set) -> addr_pin_gpios[6]=1, so chip address bit 6
        // is set -> chip_addr = 1<<6 = 64.
        assert_eq!(table[2], image[64]);
        // i=256 (GPIO8 set): GPIO8 isn't in addr_pin_gpios -> padding-pool
        // bit -> same chip_addr (0) as i=0.
        assert_eq!(table[256], table[0]);
        // All 16 GPIOs set -> every addr_pin_gpios bit of i is 1 ->
        // chip_addr = 2^13 - 1 = 8191 (full 13-bit address).
        assert_eq!(table[(1usize << 16) - 1], image[8191]);
    }

    /// Synthetic layout exercising data-pin mangling in isolation: a
    /// single address bit (GPIO0), and a reversed data-pin mapping
    /// (GPIO23..=16 -> output bits 0..=7, i.e. bit-reversal).
    #[test]
    fn data_pin_mangling_bit_reversal() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 1,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![0],
        };
        let cs_data_layout = CsDataLayout {
            gpio_base: 13,
            base_data_pin: 3,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![23, 22, 21, 20, 19, 18, 17, 16],
            base_cs_pin: 0,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: Vec::new(),
        };

        let chips = [chip_with_bytes("test.bin", &[0b0000_0001, 0b1000_0000])];

        let table = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Single, &chips, &alg_dma_8bit())
            .expect("build_rom_image should succeed");

        assert_eq!(table.len(), 2);
        // image[0] bit0 set -> output bit7 set.
        assert_eq!(table[0], 0b1000_0000);
        // image[1] bit7 set -> output bit0 set.
        assert_eq!(table[1], 0b0000_0001);
    }

    /// 2-bank Banked set: X1 selects between chips[0]/chips[1].
    #[test]
    fn banked_2bank_selection() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 2,
            x1_gpio: Some(1),
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![0],
        };
        let cs_data_layout = identity_cs_data_layout_8bit();

        let chips = [
            chip_with_bytes("bank0.bin", &[0xAA, 0xBB]),
            chip_with_bytes("bank1.bin", &[0xCC, 0xDD]),
        ];

        let table = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Banked, &chips, &alg_dma_8bit())
            .expect("build_rom_image should succeed");

        assert_eq!(table, alloc::vec![0xAA, 0xBB, 0xCC, 0xDD]);
    }

    /// 4-bank Banked set: X1 is the LSB, X2 the next bit, of the bank
    /// index.
    #[test]
    fn banked_4bank_selection_x1_is_lsb() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 2,
            x1_gpio: Some(0),
            x2_gpio: Some(1),
            addr_pin_gpios: Vec::new(),
        };
        let cs_data_layout = identity_cs_data_layout_8bit();

        let chips = [
            chip_with_bytes("bank0.bin", &[0x00]),
            chip_with_bytes("bank1.bin", &[0x11]),
            chip_with_bytes("bank2.bin", &[0x22]),
            chip_with_bytes("bank3.bin", &[0x33]),
        ];

        let table = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Banked, &chips, &alg_dma_8bit())
            .expect("build_rom_image should succeed");

        assert_eq!(table, alloc::vec![0x00, 0x11, 0x22, 0x33]);
    }

    /// 3-bank Banked set: bank index 3 (X1 and X2 both set - "both
    /// jumpered") has no corresponding chip, so reads as
    /// `PAD_NO_CHIP_BYTE`.
    #[test]
    fn banked_3bank_pad_value() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 2,
            x1_gpio: Some(0),
            x2_gpio: Some(1),
            addr_pin_gpios: Vec::new(),
        };
        let cs_data_layout = identity_cs_data_layout_8bit();

        let chips = [
            chip_with_bytes("bank0.bin", &[0x00]),
            chip_with_bytes("bank1.bin", &[0x11]),
            chip_with_bytes("bank2.bin", &[0x22]),
        ];

        let table = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Banked, &chips, &alg_dma_8bit())
            .expect("build_rom_image should succeed");

        assert_eq!(table, alloc::vec![0x00, 0x11, 0x22, PAD_NO_CHIP_BYTE]);
    }

    /// `2^num_addr_pins * bytes_per_word` exceeding `MAX_IMAGE_SIZE`
    /// (512KB) is rejected up front, before any layout work.
    #[test]
    fn image_too_large_is_rejected() {
        // 2^20 * 1 byte = 1MB > 512KB.
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 20,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: Vec::new(),
        };
        let cs_data_layout = identity_cs_data_layout_8bit();
        let chips = [chip_with_bytes("test.bin", &[0x00])];

        let result = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Single, &chips, &alg_dma_8bit());

        assert!(matches!(
            result,
            Err(Error::RomTableTooLarge { size, max }) if size == 1 << 20 && max == MAX_IMAGE_SIZE
        ));
    }

    #[test]
    fn multi_set_is_unsupported() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 1,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![0],
        };
        let cs_data_layout = identity_cs_data_layout_8bit();
        let chips = [chip_with_bytes("test.bin", &[0x00, 0x00])];

        let result = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Multi, &chips, &alg_dma_8bit());

        assert!(matches!(result, Err(Error::UnsupportedFeature { .. })));
    }

    #[test]
    fn single_with_wrong_chip_count_errors() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 1,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![0],
        };
        let cs_data_layout = identity_cs_data_layout_8bit();
        let chips = [
            chip_with_bytes("a.bin", &[0x00, 0x00]),
            chip_with_bytes("b.bin", &[0x00, 0x00]),
        ];

        let result = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Single, &chips, &alg_dma_8bit());

        assert!(matches!(result, Err(Error::InvalidConfig { .. })));
    }

    #[test]
    fn banked_with_wrong_chip_count_errors() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 2,
            x1_gpio: Some(0),
            x2_gpio: Some(1),
            addr_pin_gpios: Vec::new(),
        };
        let cs_data_layout = identity_cs_data_layout_8bit();
        let chips = [
            chip_with_bytes("a.bin", &[0x00]),
            chip_with_bytes("b.bin", &[0x00]),
            chip_with_bytes("c.bin", &[0x00]),
            chip_with_bytes("d.bin", &[0x00]),
            chip_with_bytes("e.bin", &[0x00]),
        ];

        let result = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Banked, &chips, &alg_dma_8bit());

        assert!(matches!(result, Err(Error::InvalidConfig { .. })));
    }

    /// BitMode16, identity data mapping (Fire40A/27C400-shaped: D0-D15 ->
    /// GPIO0-15). 2 word addresses (num_addr_pins=1), image bytes laid out
    /// byte-mode-addressed: word0 = bytes[0..2], word1 = bytes[2..4],
    /// little-endian (byte 0 = low byte, A-1=0).
    ///
    /// With identity mangling, the table is just the image bytes verbatim
    /// - this mainly checks the chip_addr*2 byte-address arithmetic and
    ///   little-endian (re)assembly round-trip correctly.
    #[test]
    fn single_16bit_identity_mapping() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 1,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![0],
        };
        let cs_data_layout = identity_cs_data_layout_16bit();

        let chips = [chip_with_bytes("test.bin", &[0x01, 0x02, 0x03, 0x04])];

        let table = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Single, &chips, &alg_dma_16bit())
            .expect("build_rom_image should succeed");

        assert_eq!(table.len(), 4); // 2 entries * 2 bytes/word
        assert_eq!(table, alloc::vec![0x01, 0x02, 0x03, 0x04]);
    }

    /// BitMode16 with a 16-bit bit-reversal data-pin mapping
    /// (data_pin_gpios = [15,14,..,0], i.e. data_bit_positions = [15,14,..,0]).
    ///
    /// word0 (bytes[0..2], little-endian) = 0x0001 -> bit0 set -> moves to
    /// bit15 of the mangled word -> mangled = 0x8000 -> table bytes
    /// [0x00, 0x80] (low, high).
    ///
    /// word1 (bytes[2..4]) = 0x0080 -> bit7 set -> moves to bit8 of the
    /// mangled word -> mangled = 0x0100 -> table bytes [0x00, 0x01].
    #[test]
    fn bitmode16_data_pin_mangling_bit_reversal() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 1,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![0],
        };
        let cs_data_layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 16,
            data_pin_gpios: alloc::vec![15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
            base_cs_pin: 16,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: Vec::new(),
        };

        // word0 = 0x0001 (bytes 0x01, 0x00), word1 = 0x0080 (bytes 0x80, 0x00).
        let chips = [chip_with_bytes("test.bin", &[0x01, 0x00, 0x80, 0x00])];

        let table = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Single, &chips, &alg_dma_16bit())
            .expect("build_rom_image should succeed");

        assert_eq!(table, alloc::vec![0x00, 0x80, 0x00, 0x01]);
    }

    /// 3-bank Banked set with BitMode16: bank index 3's "no chip" entry is
    /// 2 bytes of `PAD_NO_CHIP_BYTE`.
    #[test]
    fn banked_3bank_pad_value_16bit() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 2,
            x1_gpio: Some(0),
            x2_gpio: Some(1),
            addr_pin_gpios: Vec::new(),
        };
        let cs_data_layout = identity_cs_data_layout_16bit();

        let chips = [
            chip_with_bytes("bank0.bin", &[0x00, 0x10]),
            chip_with_bytes("bank1.bin", &[0x01, 0x11]),
            chip_with_bytes("bank2.bin", &[0x02, 0x12]),
        ];

        let table = build_rom_image(&addr_layout, &cs_data_layout, ChipSetType::Banked, &chips, &alg_dma_16bit())
            .expect("build_rom_image should succeed");

        assert_eq!(
            table,
            alloc::vec![0x00, 0x10, 0x01, 0x11, 0x02, 0x12, PAD_NO_CHIP_BYTE, PAD_NO_CHIP_BYTE]
        );
    }
}