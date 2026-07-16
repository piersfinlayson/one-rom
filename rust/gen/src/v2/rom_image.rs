// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! ROM image generation ("Phase 2"): for a chip set, produce the
//! `size`-byte ROM table (the bytes that live at `OneromRomSlot::data` in
//! `rom_data_buf`), from the address/CS-data layouts already derived for
//! that set.
//!
//! Covers Single, Banked (2, 3 or 4 chips), and Multi (2 or 3 chips), both
//! `AlgData0`/`BitMode8` (1 byte/entry) and `AlgData1`-or-`AlgData0`/
//! `BitMode16` (2 bytes/entry - `force_16_bit` doesn't affect the table,
//! only which algorithm serves it, so this function doesn't need it).
//!
//! ## Multi sets
//!
//! For Multi sets, the address PIO's full window (all `num_addr_pins` bits)
//! indexes the ROM table. Within that window, each table entry is identified
//! by three one-hot select bits:
//!
//! - **CS1/CE bit** (`cs1_gpio - addr_layout.gpio_base`): derived from the
//!   primary select line in `cs_data_layout.select_lines` (role `Cs1`, `Ce`,
//!   or `Oe`). Chip0's data lives at addresses where this bit is 1.
//! - **X1 bit** (`addr_layout.x1_gpio - addr_layout.gpio_base`): chip1's
//!   data lives at addresses where this bit is 1.
//! - **X2 bit** (`addr_layout.x2_gpio - addr_layout.gpio_base`): chip2's
//!   data (3-chip set only) at addresses where this bit is 1.
//!
//! CS lines are always logically active-high in the ROM table (the CS-detect
//! PIO uses `serve_cs_low_0 = 1` for Multi, and `GpioOverInvert` is applied
//! to hardware active-low lines). Entries where 0 or >1 select bits are 1
//! receive the mangled form of `PAD_NO_CHIP_BYTE` (hardware can never produce
//! these combinations in normal operation).
//!
//! The CS1/CE GPIO is guaranteed to fall within `addr_layout`'s span
//! `[gpio_base, gpio_base + num_addr_pins)` for supported boards: the CS/data
//! layout derivation requires CS1+X1[+X2] to be contiguous and within the
//! same PIO window, and the address layout derivation includes X1/X2 in its
//! span — since CS1 is adjacent to X1/X2 it falls within the span
//! automatically.
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
use crate::{Error, MAX_IMAGE_SIZE, PAD_NO_CHIP_BYTE, Result};

use super::addr_layout::AddrLayout;
use super::cs_data_layout::CsDataLayout;
use super::rom_slot::{bytes_per_word, table_entries};

/// Mangle a raw data word for writing to the ROM table.
///
/// Bit `d` of `raw` (chip data line d) moves to bit `data_bit_positions[d]`
/// of the returned value.  This matches the physical GPIO layout so the DMA
/// output drives the correct data pins directly without any runtime
/// transformation.
///
/// Used for both real chip data and pad bytes — pad bytes must be mangled for
/// the same reason as data bytes, so the host always sees correctly positioned
/// bits on the data lines regardless of the source of the table entry.
fn mangle_word(raw: u32, data_bit_positions: &[u8]) -> u32 {
    let mut mangled = 0u32;
    for (d, &bit_pos) in data_bit_positions.iter().enumerate() {
        mangled |= ((raw >> d) & 1) << bit_pos;
    }
    mangled
}

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
/// - For Single sets: always chip 0.
/// - For Banked sets:
///   - 2 chips: bit `(x1_gpio - gpio_base)` of `i` selects `chips[0]` or
///     `chips[1]`.
///   - 3 or 4 chips: bits `(x1_gpio - gpio_base)` (LSB) and `(x2_gpio -
///     gpio_base)` together form a 2-bit bank index 0..3, selecting
///     `chips[bank_index]`. For a 3-chip set, bank index `3` (X1 and X2
///     both set - "both jumpered") means no chip occupies this portion of
///     the address space; the table entry is the mangled form of
///     `PAD_NO_CHIP_BYTE` for each `bytes_per_word` bytes.
/// - For Multi sets: one-hot CS/X detection — see module docs.
/// - Any other bit of `i` (used by neither of the above) is a "padding
///   pool" bit: it doesn't affect the table entry, so the same value is
///   naturally produced for both its settings.
/// - For a chip whose address space is smaller than the table index range
///   (e.g. a 2332 secondary in a 2364-primary multi-set, or a 23QL384 at
///   48KB), `chip_addr >= chip_word_counts[chip_index]` handling depends on
///   whether the chip's size is a power of two:
///   - Power-of-2 chips: address lines above the chip's own range are not
///     connected to the physical device and are don't-cares; `chip_addr` is
///     masked to the chip's actual address width, mirroring the data.
///   - Non-power-of-2 chips: addresses above the chip's capacity are
///     genuinely absent; the table entry is the mangled form of
///     `PAD_NO_CHIP_BYTE`.
/// - Otherwise, the selected chip's image byte(s) at the chip address are
///   mangled via [`mangle_word`]: bit `d` of the raw value (chip data line
///   `d`) moves to bit `(data_pin_gpios[d] - cs_data_layout.gpio_base -
///   cs_data_layout.base_data_pin)` of the mangled value, which is then
///   written out as `bytes_per_word` little-endian bytes.
///
/// # Errors
///
/// - [`Error::RomTableTooLarge`] if `2^addr_layout.num_addr_pins *
///   bytes_per_word(alg_dma)` exceeds [`crate::MAX_IMAGE_SIZE`] - the
///   per-slot RAM budget (only one slot is served at a time, so this is
///   the limit on a single table, not the sum across slots).
/// - [`Error::InvalidConfig`] if `chips.len()` isn't `1` for `Single`,
///   `2`/`3`/`4` for `Banked`, or `2`/`3` for `Multi`.
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
        return Err(Error::RomTableTooLarge {
            size: image_size,
            max: MAX_IMAGE_SIZE,
        });
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
                    });
                }
            }
        }
        ChipSetType::Multi => {
            if chips.len() < 2 || chips.len() > 3 {
                return Err(Error::InvalidConfig {
                    error: format!("Multi set must have 2 or 3 chips, got {}", chips.len()),
                });
            }
            Vec::new()
        }
    };

    // Multi-set one-hot select bit positions (cs1_bit, x1_bit, x2_bit).
    //
    // CS1/CE's GPIO comes from cs_data_layout.select_lines (the primary
    // select line, role Cs1, Ce, or Oe). Its bit position within the table
    // index `i` is (gpio - addr_layout.gpio_base). CS1/CE falls within the
    // addr PIO window as a "gap" bit even though it isn't in addr_pin_gpios —
    // it's adjacent to X1/X2 (all three are contiguous per the CS layout
    // derivation), so it lands within the addr span automatically.
    //
    // X1/X2 positions come from addr_layout.x1_gpio/x2_gpio (the addr PIO's
    // resolved GPIOs for these pins, which may differ from cs_data_layout's
    // for dual-bonded pins — the addr PIO's values are what index the table).
    let multi_select: Option<(u8, u8, Option<u8>)> = if set_type == ChipSetType::Multi {
        let cs1_gpio = cs_data_layout
            .select_lines
            .first()
            .expect("Multi cs_data_layout must have at least one select line")
            .gpio;
        let cs1_bit = cs1_gpio - addr_layout.gpio_base;

        let x1_bit = addr_layout
            .x1_gpio
            .expect("Multi AddrLayout must have x1_gpio")
            - addr_layout.gpio_base;

        let x2_bit = if chips.len() >= 3 {
            Some(
                addr_layout
                    .x2_gpio
                    .expect("3-chip Multi AddrLayout must have x2_gpio")
                    - addr_layout.gpio_base,
            )
        } else {
            None
        };

        Some((cs1_bit, x1_bit, x2_bit))
    } else {
        None
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

    // Pre-compute the mangled pad value.  All pad entries — whether from a
    // missing bank, no chip selected, or an address beyond a non-power-of-2
    // chip's capacity — must go through the same bit permutation as real data
    // so the host always sees correctly positioned bits on the data lines.
    let pad_raw: u32 =
        (0..word_bytes).fold(0u32, |acc, b| acc | (PAD_NO_CHIP_BYTE as u32) << (8 * b));
    let mangled_pad = mangle_word(pad_raw, &data_bit_positions);

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

    // Valid address count per chip, in words (not bytes): for BitMode8
    // this equals size_bytes(); for BitMode16 it's size_bytes()/2 since
    // chip_addr is a word address. Non-power-of-2 chips (e.g. 23QL384 at
    // 48KB) will have chip_addr values that exceed this for the upper
    // portion of the address space - those entries get the mangled pad,
    // consistent with the missing-bank treatment above.
    let chip_word_counts: Vec<usize> = chips
        .iter()
        .map(|chip| chip.chip_type().size_bytes() / word_bytes)
        .collect();

    let mut image = Vec::with_capacity(entries * word_bytes);

    for i in 0..entries as u32 {
        let mut chip_addr: usize = 0;
        for (n, &bit_pos) in addr_bit_positions.iter().enumerate() {
            let bit = ((i >> bit_pos) & 1) as usize;
            chip_addr |= bit << n;
        }

        // Determine which chip to serve for this table index.
        //
        // Multi: one-hot — exactly one of cs1/x1/x2 must be 1. 0 or >1
        // active → no chip selected → mangled pad.
        //
        // Banked: binary bank index from X1/X2. Out-of-range (e.g. both
        // jumpers fitted on a 3-chip set, bank index 3) → mangled pad.
        //
        // Single: always chip 0.
        let chip_index: Option<usize> = if let Some((cs1_bit, x1_bit, x2_bit)) = multi_select {
            let cs1_active = ((i >> cs1_bit) & 1) == 1;
            let x1_active = ((i >> x1_bit) & 1) == 1;
            let x2_active = x2_bit.is_some_and(|b| ((i >> b) & 1) == 1);

            match (cs1_active, x1_active, x2_active) {
                (true, false, false) => Some(0),
                (false, true, false) => Some(1),
                (false, false, true) => {
                    // chip2 — only valid for 3-chip sets; already validated above
                    if chips.len() >= 3 { Some(2) } else { None }
                }
                _ => None, // no chip active, or multiple active
            }
        } else {
            let mut bank_index: usize = 0;
            for (n, &bit_pos) in bank_bit_positions.iter().enumerate() {
                let bit = ((i >> bit_pos) & 1) as usize;
                bank_index |= bit << n;
            }
            if bank_index < chips.len() {
                Some(bank_index)
            } else {
                None
            }
        };

        let chip_index = match chip_index {
            None => {
                // No chip selected (or invalid bank/CS combination).
                for b in 0..word_bytes {
                    image.push(((mangled_pad >> (8 * b)) & 0xFF) as u8);
                }
                continue;
            }
            Some(idx) => idx,
        };

        let chip_size = chip_word_counts[chip_index];
        if chip_addr >= chip_size {
            if chip_size.is_power_of_two() {
                // Undersized power-of-2 chip (e.g. 2332 secondary in a
                // 2364-primary multi-set): address lines above this chip's own
                // range are not connected to the physical device and are
                // don't-cares.  Mirror by masking chip_addr to the chip's
                // actual address width.
                chip_addr &= chip_size - 1;
            } else {
                // Non-power-of-2 chip (e.g. 23QL384 at 48KB): addresses above
                // the chip's capacity are genuinely absent.
                for b in 0..word_bytes {
                    image.push(((mangled_pad >> (8 * b)) & 0xFF) as u8);
                }
                continue;
            }
        }

        let chip_image = chip_data[chip_index];

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

        // Mangle and write out little-endian: byte 0 = bits 0-7, byte 1 = bits 8-15.
        let mangled = mangle_word(raw, &data_bit_positions);
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

    use super::super::cs_data_layout::{SelectLine, SelectRole};

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
        chip_with_typed_image(filename, &ChipType::Chip2364, image)
    }

    fn chip_with_typed_image(filename: &str, chip_type: &ChipType, image: Vec<u8>) -> Chip {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);
        Chip::from_raw_rom_image(
            0,
            filename.to_string(),
            None,
            Some(image.as_slice()),
            vec![0u8; chip_type.size_bytes()],
            chip_type,
            cs_config,
            &SizeHandling::None,
            None,
        )
        .expect("chip construction should succeed")
    }

    fn alg_dma_8bit() -> OneromAlgDmaConfig {
        OneromAlgDmaConfig::AlgDma0 {
            bit_mode: BitModes::BitMode8,
            continuous: 1,
        }
    }

    fn alg_dma_16bit() -> OneromAlgDmaConfig {
        OneromAlgDmaConfig::AlgDma0 {
            bit_mode: BitModes::BitMode16,
            continuous: 1,
        }
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
            commoned_lines: alloc::vec![],
            alg_cs2: None,
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
            commoned_lines: alloc::vec![],
            alg_cs2: None,
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
            excess_addr_pin_gpios: alloc::vec![],
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
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };

        let image: Vec<u8> = (0..ChipType::Chip2364.size_bytes() as u32)
            .map(|k| k as u8)
            .collect();
        let chips = [chip_with_image("test.bin", image.clone())];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Single,
            &chips,
            &alg_dma_8bit(),
        )
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
            excess_addr_pin_gpios: alloc::vec![],
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
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };

        let chips = [chip_with_bytes("test.bin", &[0b0000_0001, 0b1000_0000])];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Single,
            &chips,
            &alg_dma_8bit(),
        )
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
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = identity_cs_data_layout_8bit();

        let chips = [
            chip_with_bytes("bank0.bin", &[0xAA, 0xBB]),
            chip_with_bytes("bank1.bin", &[0xCC, 0xDD]),
        ];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Banked,
            &chips,
            &alg_dma_8bit(),
        )
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
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = identity_cs_data_layout_8bit();

        let chips = [
            chip_with_bytes("bank0.bin", &[0x00]),
            chip_with_bytes("bank1.bin", &[0x11]),
            chip_with_bytes("bank2.bin", &[0x22]),
            chip_with_bytes("bank3.bin", &[0x33]),
        ];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Banked,
            &chips,
            &alg_dma_8bit(),
        )
        .expect("build_rom_image should succeed");

        assert_eq!(table, alloc::vec![0x00, 0x11, 0x22, 0x33]);
    }

    /// 3-bank Banked set: bank index 3 (X1 and X2 both set - "both
    /// jumpered") has no corresponding chip, so reads as the mangled form
    /// of `PAD_NO_CHIP_BYTE`.  With an identity data mapping the mangled
    /// value equals `PAD_NO_CHIP_BYTE`.
    #[test]
    fn banked_3bank_pad_value() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 2,
            x1_gpio: Some(0),
            x2_gpio: Some(1),
            addr_pin_gpios: Vec::new(),
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = identity_cs_data_layout_8bit();

        let chips = [
            chip_with_bytes("bank0.bin", &[0x00]),
            chip_with_bytes("bank1.bin", &[0x11]),
            chip_with_bytes("bank2.bin", &[0x22]),
        ];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Banked,
            &chips,
            &alg_dma_8bit(),
        )
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
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = identity_cs_data_layout_8bit();
        let chips = [chip_with_bytes("test.bin", &[0x00])];

        let result = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Single,
            &chips,
            &alg_dma_8bit(),
        );

        assert!(matches!(
            result,
            Err(Error::RomTableTooLarge { size, max }) if size == 1 << 20 && max == MAX_IMAGE_SIZE
        ));
    }

    // =========================================================================
    // Multi-set tests
    // =========================================================================

    /// Synthetic Multi select layout helper.
    ///
    /// 4-bit table (num_addr_pins=4, 16 entries). Layout:
    ///   - GPIO 0,1: addr bits A0, A1 (chip_addr = bit0 | (bit1<<1))
    ///   - GPIO 2: CS1 (chip0's select; bit position 2 in `i`)
    ///   - GPIO 3: X1  (chip1's select; bit position 3 in `i`)
    fn multi_2chip_addr_layout() -> AddrLayout {
        AddrLayout {
            gpio_base: 0,
            num_addr_pins: 4,
            x1_gpio: Some(3),
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![0, 1],
            excess_addr_pin_gpios: alloc::vec![],
        }
    }

    fn multi_cs_data_layout_with_cs1_at(cs1_gpio: u8) -> CsDataLayout {
        CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7],
            base_cs_pin: cs1_gpio,
            num_cs_pins: 2,
            cs_ignore_index: None,
            select_lines: alloc::vec![
                SelectLine {
                    role: SelectRole::Cs1,
                    gpio: cs1_gpio
                },
                SelectLine {
                    role: SelectRole::X1,
                    gpio: 3
                },
            ],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        }
    }

    /// 2-chip Multi, 8-bit, identity data mapping.
    ///
    /// Table layout (16 entries, CS1=bit2, X1=bit3, addr=bits0-1):
    ///   i=0..3   (CS1=0, X1=0): no chip → PAD
    ///   i=4..7   (CS1=1, X1=0): chip0, addr 0..3
    ///   i=8..11  (CS1=0, X1=1): chip1, addr 0..3
    ///   i=12..15 (CS1=1, X1=1): both active → PAD
    #[test]
    fn multi_2chip_8bit_one_hot_selection() {
        let addr_layout = multi_2chip_addr_layout();
        let cs_data_layout = multi_cs_data_layout_with_cs1_at(2);

        let chips = [
            chip_with_bytes("chip0.bin", &[0xA0, 0xA1, 0xA2, 0xA3]),
            chip_with_bytes("chip1.bin", &[0xB0, 0xB1, 0xB2, 0xB3]),
        ];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Multi,
            &chips,
            &alg_dma_8bit(),
        )
        .expect("build_rom_image should succeed");

        assert_eq!(table.len(), 16);
        assert_eq!(&table[0..4], &[PAD_NO_CHIP_BYTE; 4]); // no chip active
        assert_eq!(&table[4..8], &[0xA0, 0xA1, 0xA2, 0xA3]); // chip0
        assert_eq!(&table[8..12], &[0xB0, 0xB1, 0xB2, 0xB3]); // chip1
        assert_eq!(&table[12..16], &[PAD_NO_CHIP_BYTE; 4]); // both active
    }

    /// 3-chip Multi, 8-bit. CS1=bit1, X1=bit2, X2=bit3, addr=bit0.
    ///
    /// Layout:
    ///   - GPIO 0: A0 (addr_pin_gpios=[0])
    ///   - GPIO 1: CS1 (chip0)
    ///   - GPIO 2: X1  (chip1)
    ///   - GPIO 3: X2  (chip2)
    ///
    /// num_addr_pins=4 → 16 entries.
    #[test]
    fn multi_3chip_8bit_one_hot_selection() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 4,
            x1_gpio: Some(2),
            x2_gpio: Some(3),
            addr_pin_gpios: alloc::vec![0],
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7],
            base_cs_pin: 1,
            num_cs_pins: 3,
            cs_ignore_index: None,
            select_lines: alloc::vec![
                SelectLine {
                    role: SelectRole::Cs1,
                    gpio: 1
                },
                SelectLine {
                    role: SelectRole::X1,
                    gpio: 2
                },
                SelectLine {
                    role: SelectRole::X2,
                    gpio: 3
                },
            ],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };

        let chips = [
            chip_with_bytes("chip0.bin", &[0xA0, 0xA1]),
            chip_with_bytes("chip1.bin", &[0xB0, 0xB1]),
            chip_with_bytes("chip2.bin", &[0xC0, 0xC1]),
        ];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Multi,
            &chips,
            &alg_dma_8bit(),
        )
        .expect("build_rom_image should succeed");

        assert_eq!(table.len(), 16);
        // CS1_bit=1, X1_bit=2, X2_bit=3, addr_bit=0
        // i=0  (0000): no select → PAD
        // i=1  (0001): only addr bit → PAD
        // i=2  (0010): CS1=1 → chip0, addr=0 → 0xA0
        // i=3  (0011): CS1=1 → chip0, addr=1 → 0xA1
        // i=4  (0100): X1=1  → chip1, addr=0 → 0xB0
        // i=5  (0101): X1=1  → chip1, addr=1 → 0xB1
        // i=6  (0110): CS1+X1 → PAD
        // i=8  (1000): X2=1  → chip2, addr=0 → 0xC0
        // i=9  (1001): X2=1  → chip2, addr=1 → 0xC1
        assert_eq!(table[0], PAD_NO_CHIP_BYTE);
        assert_eq!(table[1], PAD_NO_CHIP_BYTE);
        assert_eq!(table[2], 0xA0);
        assert_eq!(table[3], 0xA1);
        assert_eq!(table[4], 0xB0);
        assert_eq!(table[5], 0xB1);
        assert_eq!(table[6], PAD_NO_CHIP_BYTE);
        assert_eq!(table[8], 0xC0);
        assert_eq!(table[9], 0xC1);
        #[allow(clippy::needless_range_loop)]
        for idx in 10..16usize {
            assert_eq!(table[idx], PAD_NO_CHIP_BYTE, "entry {idx} should be PAD");
        }
    }

    /// Multi set with Ce-role primary select (27-series in multi-ROM).
    /// Same layout as multi_2chip_8bit_one_hot_selection but the primary
    /// select line has role Ce instead of Cs1.
    #[test]
    fn multi_2chip_ce_role_primary_select() {
        let addr_layout = multi_2chip_addr_layout();
        let cs_data_layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7],
            base_cs_pin: 2,
            num_cs_pins: 2,
            cs_ignore_index: None,
            select_lines: alloc::vec![
                SelectLine {
                    role: SelectRole::Ce,
                    gpio: 2
                },
                SelectLine {
                    role: SelectRole::X1,
                    gpio: 3
                },
            ],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };

        let chips = [
            chip_with_bytes("chip0.bin", &[0xC0, 0xC1, 0xC2, 0xC3]),
            chip_with_bytes("chip1.bin", &[0xD0, 0xD1, 0xD2, 0xD3]),
        ];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Multi,
            &chips,
            &alg_dma_8bit(),
        )
        .expect("build_rom_image should succeed for Ce-role primary select");

        assert_eq!(&table[4..8], &[0xC0, 0xC1, 0xC2, 0xC3]); // CE=1 → chip0
        assert_eq!(&table[8..12], &[0xD0, 0xD1, 0xD2, 0xD3]); // X1=1 → chip1
    }

    /// Multi with wrong chip count (1 chip) errors.
    #[test]
    fn multi_with_wrong_chip_count_errors() {
        let addr_layout = multi_2chip_addr_layout();
        let cs_data_layout = multi_cs_data_layout_with_cs1_at(2);
        let chips = [chip_with_bytes("only.bin", &[0x00])];

        let result = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Multi,
            &chips,
            &alg_dma_8bit(),
        );

        assert!(matches!(result, Err(Error::InvalidConfig { .. })));
    }

    #[test]
    fn single_with_wrong_chip_count_errors() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 1,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![0],
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = identity_cs_data_layout_8bit();
        let chips = [
            chip_with_bytes("a.bin", &[0x00, 0x00]),
            chip_with_bytes("b.bin", &[0x00, 0x00]),
        ];

        let result = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Single,
            &chips,
            &alg_dma_8bit(),
        );

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
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = identity_cs_data_layout_8bit();
        let chips = [
            chip_with_bytes("a.bin", &[0x00]),
            chip_with_bytes("b.bin", &[0x00]),
            chip_with_bytes("c.bin", &[0x00]),
            chip_with_bytes("d.bin", &[0x00]),
            chip_with_bytes("e.bin", &[0x00]),
        ];

        let result = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Banked,
            &chips,
            &alg_dma_8bit(),
        );

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
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = identity_cs_data_layout_16bit();

        let chips = [chip_with_bytes("test.bin", &[0x01, 0x02, 0x03, 0x04])];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Single,
            &chips,
            &alg_dma_16bit(),
        )
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
            excess_addr_pin_gpios: alloc::vec![],
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
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };

        // word0 = 0x0001 (bytes 0x01, 0x00), word1 = 0x0080 (bytes 0x80, 0x00).
        let chips = [chip_with_bytes("test.bin", &[0x01, 0x00, 0x80, 0x00])];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Single,
            &chips,
            &alg_dma_16bit(),
        )
        .expect("build_rom_image should succeed");

        assert_eq!(table, alloc::vec![0x00, 0x80, 0x00, 0x01]);
    }

    /// 3-bank Banked set with BitMode16: bank index 3's "no chip" entry is
    /// 2 bytes of the mangled form of `PAD_NO_CHIP_BYTE`.  With an identity
    /// data mapping the mangled value equals `PAD_NO_CHIP_BYTE`.
    #[test]
    fn banked_3bank_pad_value_16bit() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 2,
            x1_gpio: Some(0),
            x2_gpio: Some(1),
            addr_pin_gpios: Vec::new(),
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = identity_cs_data_layout_16bit();

        let chips = [
            chip_with_bytes("bank0.bin", &[0x00, 0x10]),
            chip_with_bytes("bank1.bin", &[0x01, 0x11]),
            chip_with_bytes("bank2.bin", &[0x02, 0x12]),
        ];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Banked,
            &chips,
            &alg_dma_16bit(),
        )
        .expect("build_rom_image should succeed");

        assert_eq!(
            table,
            alloc::vec![
                0x00,
                0x10,
                0x01,
                0x11,
                0x02,
                0x12,
                PAD_NO_CHIP_BYTE,
                PAD_NO_CHIP_BYTE
            ]
        );
    }

    /// Non-power-of-2 chip (23QL384, 48KB = 49152 = 0xC000 bytes): with 16
    /// address lines (A0-A15), chip_addr ranges 0..65535 but only
    /// 0..=49151 (0x0000..=0xBFFF) are valid. chip_addr >= 49152 must
    /// produce the mangled form of `PAD_NO_CHIP_BYTE` with no data-pin
    /// mangling of its own.  With an identity data mapping the mangled
    /// value equals `PAD_NO_CHIP_BYTE`.
    ///
    /// Identity data mapping, so no mangling interference. We verify:
    ///   - the byte at the last valid address (49151 / 0xBFFF) comes through;
    ///   - the first invalid address (49152 / 0xC000) and the last table
    ///     entry (65535 / 0xFFFF) both produce PAD_NO_CHIP_BYTE.
    #[test]
    fn non_power_of_2_chip_pads_upper_addresses() {
        // 16-bit identity address mapping: GPIO0=A0 .. GPIO15=A15.
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 16,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = identity_cs_data_layout_8bit();

        let mut image = vec![0u8; ChipType::Chip23QL384.size_bytes()]; // 49152 bytes
        image[49150] = 0xAB; // penultimate valid address
        image[49151] = 0xCD; // last valid address (0xBFFF)
        let chips = [chip_with_typed_image(
            "test.bin",
            &ChipType::Chip23QL384,
            image,
        )];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Single,
            &chips,
            &alg_dma_8bit(),
        )
        .expect("build_rom_image should succeed");

        assert_eq!(table.len(), 1 << 16); // 65536 entries
        assert_eq!(table[49150], 0xAB); // penultimate valid
        assert_eq!(table[49151], 0xCD); // last valid (0xBFFF)
        assert_eq!(table[49152], PAD_NO_CHIP_BYTE); // first invalid (0xC000)
        assert_eq!(table[65535], PAD_NO_CHIP_BYTE); // last entry (0xFFFF)
    }

    /// Pad bytes under a non-identity data-pin mapping must be mangled before
    /// being written to the table.
    ///
    /// Uses a 3-bank Banked set with a bit-reversal data mapping (same as
    /// `data_pin_mangling_bit_reversal`).  Bank index 3 (X1 and X2 both set)
    /// produces `PAD_NO_CHIP_BYTE`.  Under bit-reversal,
    /// `mangle(0xAA)` = `mangle(0b10101010)` = `0b01010101` = `0x55`.
    ///
    /// Before the fix, pad entries were written as raw `0xAA` regardless of
    /// the data-pin mapping, causing the host to see a permuted bit pattern
    /// rather than `0xAA` on the data lines.
    #[test]
    fn banked_3bank_pad_value_non_identity_mapping_is_mangled() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 2,
            x1_gpio: Some(0),
            x2_gpio: Some(1),
            addr_pin_gpios: Vec::new(),
            excess_addr_pin_gpios: alloc::vec![],
        };
        // Bit-reversal: data bit d → output bit (7-d).
        // data_bit_positions = [7,6,5,4,3,2,1,0] - data_base(0) = [7,6,5,4,3,2,1,0].
        let cs_data_layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0],
            base_cs_pin: 0,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: Vec::new(),
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };

        let chips = [
            chip_with_bytes("bank0.bin", &[0x00]),
            chip_with_bytes("bank1.bin", &[0x11]),
            chip_with_bytes("bank2.bin", &[0x22]),
        ];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Banked,
            &chips,
            &alg_dma_8bit(),
        )
        .expect("build_rom_image should succeed");

        // Sanity: real data bytes are mangled correctly.
        // mangle(0x00 = 0b0000_0000) = 0x00
        // mangle(0x11 = 0b0001_0001): bit0→bit7, bit4→bit3 → 0b1000_1000 = 0x88
        // mangle(0x22 = 0b0010_0010): bit1→bit6, bit5→bit2 → 0b0100_0100 = 0x44
        assert_eq!(table[0], 0x00);
        assert_eq!(table[1], 0x88);
        assert_eq!(table[2], 0x44);

        // Key assertion: bank index 3 (no chip) must write mangle(PAD_NO_CHIP_BYTE)
        // not the raw value.  mangle(0xAA = 0b1010_1010) = 0b0101_0101 = 0x55.
        assert_eq!(
            table[3], 0x55,
            "PAD must be mangled: mangle(0xAA) = 0x55 under bit-reversal; \
             got raw 0xAA means the pad path skipped mangle_word"
        );
    }

    /// Power-of-2 undersized secondary chip in a multi-set (e.g. 2332 behind
    /// a 2364 primary): the extra address line (A12) is a don't-care and the
    /// 2332's 4KB image must mirror across both halves of the 8KB address
    /// window.
    ///
    /// Layout: 15-bit table (num_addr_pins=15, 32768 entries).
    ///   - GPIO  0-11: addr bits A0-A11 (shared by both chips)
    ///   - GPIO 12:    addr bit A12 (primary 2364 only; don't-care for 2332)
    ///   - GPIO 13:    CS1 — primary chip0 select (gap bit within addr window;
    ///     not in addr_pin_gpios)
    ///   - GPIO 14:    X1  — secondary chip1 select
    ///
    /// The previous (broken) version put CS1 at GPIO12 — the same bit
    /// position as A12 — so i_hi = (1<<13)|(1<<12) set both X1 and CS1
    /// simultaneously, hitting the both-active → PAD path instead of the
    /// mirror path.  CS1 must be a distinct GPIO from every address pin.
    ///
    /// For every address i where X1=1, CS1=0 (chip1 selected):
    ///   bits 0-11 of i give the 2332 chip_addr (A0-A11).
    ///   bit 12 of i (A12) is a don't-care: both i and i|(1<<12) must
    ///   return the same oracle byte.
    #[test]
    fn multi_power_of_2_secondary_mirrors_across_extra_address_bit() {
        // 15-bit table: GPIO0-11=A0-A11, GPIO12=A12 (primary only),
        // GPIO13=CS1 (gap bit), GPIO14=X1.
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 15,
            x1_gpio: Some(14),
            x2_gpio: None,
            // Primary (2364) has 13 address pins A0-A12 at GPIOs 0-12.
            addr_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            excess_addr_pin_gpios: alloc::vec![],
        };
        // CS1 at GPIO13, X1 at GPIO14.  GPIO12 (A12) is an addr pin, not a
        // select bit, so it cannot conflict with CS1.
        let cs_data_layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7],
            base_cs_pin: 13,
            num_cs_pins: 2,
            cs_ignore_index: None,
            select_lines: alloc::vec![
                SelectLine {
                    role: SelectRole::Cs1,
                    gpio: 13
                },
                SelectLine {
                    role: SelectRole::X1,
                    gpio: 14
                },
            ],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };

        // 2332: 4096 bytes (2^12).  Fill with address-as-value so each byte
        // is uniquely identifiable.
        let mut secondary_image = vec![0u8; 4096];
        for (i, b) in secondary_image.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
        }
        // Primary (2364, 8192 bytes) — content irrelevant for this test.
        let chips = [
            chip_with_bytes("primary.bin", &[]),
            chip_with_typed_image(
                "secondary.bin",
                &ChipType::Chip2332,
                secondary_image.clone(),
            ),
        ];

        let table = build_rom_image(
            &addr_layout,
            &cs_data_layout,
            ChipSetType::Multi,
            &chips,
            &alg_dma_8bit(),
        )
        .expect("build_rom_image should succeed");

        assert_eq!(table.len(), 1 << 15);

        // X1=bit14, CS1=bit13, A12=bit12.
        // For each lower-12-bit address, verify A12=0 and A12=1 both produce
        // the same correct oracle byte when chip1 (X1=1, CS1=0) is selected.
        for addr12 in [0x000usize, 0x001, 0x7FF, 0xFFF] {
            let i_lo = (1 << 14) | addr12; // X1=1, CS1=0, A12=0
            let i_hi = (1 << 14) | (1 << 12) | addr12; // X1=1, CS1=0, A12=1
            assert_eq!(
                table[i_lo], table[i_hi],
                "addr12={addr12:#05x}: A12=0 entry {i_lo:#06x} \
                 ({:#04x}) != A12=1 entry {i_hi:#06x} ({:#04x})",
                table[i_lo], table[i_hi],
            );
            let expected = secondary_image[addr12 & 0xFFF];
            assert_eq!(
                table[i_lo], expected,
                "addr12={addr12:#05x}: got {:#04x}, expected {expected:#04x}",
                table[i_lo],
            );
        }
    }
}
