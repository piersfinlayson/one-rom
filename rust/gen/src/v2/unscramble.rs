// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Unscrambling a slot's stored data into the ROMs it serves, the reverse of
//! [`build_rom_image`](super::rom_image::build_rom_image).
//!
//! The layout comes from the image's own metadata so the result is what the
//! firmware serves whichever version of onerom-gen built the image.
//!
//! A ROM comes back at the size it is served:
//!
//! - its chip size, after the size handling and transforms it was built with
//! - the size its address lines reach where that's smaller, for example half a
//!   27C080
//!
//! A table entry that doesn't select a word of a ROM holds padding and is
//! skipped. So is an entry a host can't select because a GPIO in the address
//! window has its input forced. A plugin writing the served image doesn't
//! update those entries.

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use onerom_metadata::{
    BitModes, GPIO_NONE, GpioOverride, MaybeKnown, OneromAlgAddrConfig, OneromAlgConfig,
    OneromAlgCsConfig, OneromAlgDataConfig, OneromAlgDmaConfig, OneromHardwareInfo, OneromRomInfo,
    OneromRomSlot, RomSlotType,
};

/// Why a slot's stored data can't be unscrambled into its ROMs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnscrambleError {
    /// The stored data isn't the slot's size.
    WrongSize {
        /// The slot's size.
        expected: u32,
        /// The stored data's length.
        actual: usize,
    },

    /// The slot type is unrecognised.
    UnknownSlotType(u32),

    /// A ROM slot without the serving algorithms or a pin map.
    MissingLayout,

    /// A serving algorithm this build can't read a table for.
    UnknownAlgorithm,

    /// A recorded GPIO is outside the slot's address or data window.
    BadLayout,

    /// The board's X1 or X2 pin doesn't have exactly one GPIO in the slot's
    /// address window.
    XPin {
        /// 1 for X1, 2 for X2.
        pin: u8,
    },

    /// Two table entries for the same ROM address hold different values.
    Inconsistent {
        /// The ROM's index within the slot.
        rom: usize,
        /// The address, in words.
        addr: usize,
    },

    /// A ROM address without a table entry.
    Unserved {
        /// The ROM's index within the slot.
        rom: usize,
        /// The address, in words.
        addr: usize,
    },
}

impl fmt::Display for UnscrambleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongSize { expected, actual } => write!(
                f,
                "The slot's data is {actual} bytes, but the slot is {expected} bytes"
            ),
            Self::UnknownSlotType(raw) => write!(f, "The slot type {raw} isn't recognised"),
            Self::MissingLayout => f.write_str("The slot's pin layout is missing"),
            Self::UnknownAlgorithm => f.write_str("The slot's serving algorithm isn't recognised"),
            Self::BadLayout => f.write_str("The firmware is corrupt"),
            Self::XPin { pin } => write!(
                f,
                "The board's X{pin} pin isn't in the slot's address lines"
            ),
            Self::Inconsistent { rom, addr } => write!(
                f,
                "Chip {rom} has two different values at address {addr:#x}"
            ),
            Self::Unserved { rom, addr } => {
                write!(f, "Chip {rom} doesn't have a value at address {addr:#x}")
            }
        }
    }
}

/// The ROMs `slot` serves, from `data`, the slot's stored bytes.
///
/// A plugin slot comes back as stored, as one ROM. A slot holding several
/// ROMs returns them in slot order. `hw` provides the board's X pins.
///
/// A slot without an image doesn't have stored bytes to pass.
pub fn unscramble_slot(
    hw: &OneromHardwareInfo,
    slot: &OneromRomSlot,
    data: &[u8],
) -> Result<Vec<Vec<u8>>, UnscrambleError> {
    if data.len() != slot.size as usize {
        return Err(UnscrambleError::WrongSize {
            expected: slot.size,
            actual: data.len(),
        });
    }

    let selection = match slot.slot_type {
        MaybeKnown::Known(
            RomSlotType::RomSlotTypePluginSystem
            | RomSlotType::RomSlotTypePluginUser
            | RomSlotType::RomSlotTypePluginPio,
        ) => return Ok(vec![data.to_vec()]),
        MaybeKnown::Known(
            RomSlotType::RomSlotTypeSingleRom | RomSlotType::RomSlotTypeSingleRam,
        ) => Selection::Single,
        MaybeKnown::Known(RomSlotType::RomSlotTypeBankedRom) => Selection::Banked,
        MaybeKnown::Known(RomSlotType::RomSlotTypeMultiRom) => Selection::Multi,
        MaybeKnown::Unknown(raw) => return Err(UnscrambleError::UnknownSlotType(raw)),
    };

    let alg = slot.alg.as_ref().ok_or(UnscrambleError::MissingLayout)?;
    let table = Table::new(alg)?;
    if data.len() != table.entries * table.word_bytes {
        return Err(UnscrambleError::BadLayout);
    }

    let roms: Vec<RomLayout> = slot
        .roms
        .iter()
        .map(|rom| RomLayout::new(rom, &table))
        .collect::<Result<_, _>>()?;
    let select = Select::new(selection, roms.len(), hw, alg, &table)?;

    let mut out: Vec<Vec<u8>> = roms
        .iter()
        .map(|rom| vec![0; rom.words * table.word_bytes])
        .collect();
    let mut written: Vec<Vec<bool>> = roms.iter().map(|rom| vec![false; rom.words]).collect();

    for (index, entry) in data.chunks_exact(table.word_bytes).enumerate() {
        if !table.selectable(index) {
            continue;
        }
        let Some(chip) = select.chip(index) else {
            continue;
        };
        let rom = &roms[chip];
        let addr = rom.addr(index);
        if addr >= rom.words {
            continue;
        }

        let mangled = entry
            .iter()
            .rev()
            .fold(0u32, |acc, &byte| acc << 8 | u32::from(byte));
        let raw = rom.demangle(mangled);
        let word = &mut out[chip][addr * table.word_bytes..(addr + 1) * table.word_bytes];
        let bytes = &raw.to_le_bytes()[..table.word_bytes];
        if written[chip][addr] {
            if word != bytes {
                return Err(UnscrambleError::Inconsistent { rom: chip, addr });
            }
        } else {
            word.copy_from_slice(bytes);
            written[chip][addr] = true;
        }
    }

    for (rom, written) in written.iter().enumerate() {
        if let Some(addr) = written.iter().position(|&done| !done) {
            return Err(UnscrambleError::Unserved { rom, addr });
        }
    }

    Ok(out)
}

/// How a slot's table entries select a ROM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
    Single,
    Banked,
    Multi,
}

/// The table's address window and word size.
struct Table {
    /// The first GPIO in the address window.
    addr_base: u8,
    /// The GPIOs in the address window, each a bit of the table index.
    addr_bits: u8,
    /// The GPIO data bit 0 of a table entry drives.
    data_base: u8,
    entries: usize,
    word_bytes: usize,
    /// The table index bits whose GPIO's input is forced.
    forced: usize,
    forced_high: usize,
}

impl Table {
    fn new(alg: &OneromAlgConfig) -> Result<Self, UnscrambleError> {
        let (addr_base, addr_bits) = match alg.alg_addr {
            OneromAlgAddrConfig::AlgAddr0 {
                gpio_base,
                base_addr_pin,
                num_addr_pins,
                ..
            } => (
                gpio_base
                    .checked_add(base_addr_pin)
                    .ok_or(UnscrambleError::BadLayout)?,
                num_addr_pins,
            ),
            OneromAlgAddrConfig::Unknown { .. } => return Err(UnscrambleError::UnknownAlgorithm),
        };
        let data_base = match alg.alg_data {
            OneromAlgDataConfig::AlgData0 {
                gpio_base,
                base_data_pin,
                ..
            }
            | OneromAlgDataConfig::AlgData1 {
                gpio_base,
                base_data_pin,
                ..
            } => gpio_base
                .checked_add(base_data_pin)
                .ok_or(UnscrambleError::BadLayout)?,
            OneromAlgDataConfig::Unknown { .. } => return Err(UnscrambleError::UnknownAlgorithm),
        };
        let word_bytes = match alg.alg_dma {
            OneromAlgDmaConfig::AlgDma0 {
                bit_mode: MaybeKnown::Known(BitModes::BitMode8),
                ..
            } => 1,
            OneromAlgDmaConfig::AlgDma0 {
                bit_mode: MaybeKnown::Known(BitModes::BitMode16),
                ..
            } => 2,
            OneromAlgDmaConfig::AlgDma0 {
                bit_mode: MaybeKnown::Unknown(_),
                ..
            }
            | OneromAlgDmaConfig::Unknown { .. } => return Err(UnscrambleError::UnknownAlgorithm),
        };
        if addr_bits >= 32 {
            return Err(UnscrambleError::BadLayout);
        }
        let mut table = Self {
            addr_base,
            addr_bits,
            data_base,
            entries: 1 << addr_bits,
            word_bytes,
            forced: 0,
            forced_high: 0,
        };
        for &entry in alg
            .gpio_override_config
            .iter()
            .flat_map(|config| &config.params)
        {
            let Some(bit) = table.bit(entry & 0x3F) else {
                continue;
            };
            match GpioOverride::try_from(entry >> 6) {
                Ok(GpioOverride::GpioOverLow) => table.forced |= 1 << bit,
                Ok(GpioOverride::GpioOverHigh) => {
                    table.forced |= 1 << bit;
                    table.forced_high |= 1 << bit;
                }
                Ok(GpioOverride::GpioOverNormal | GpioOverride::GpioOverInvert) | Err(()) => {}
            }
        }
        Ok(table)
    }

    fn selectable(&self, index: usize) -> bool {
        index & self.forced == self.forced_high
    }

    /// The table index bit `gpio` drives, where it's in the address window.
    fn bit(&self, gpio: u8) -> Option<u8> {
        gpio.checked_sub(self.addr_base)
            .filter(|&bit| bit < self.addr_bits)
    }
}

/// Where one ROM's address and data bits are in a table entry.
struct RomLayout {
    /// The table index bit of each address line, A0 first.
    addr_bits: Vec<u8>,
    /// The entry bit of each data line, D0 first.
    data_bits: Vec<u8>,
    /// The words the ROM serves.
    words: usize,
}

impl RomLayout {
    fn new(rom: &OneromRomInfo, table: &Table) -> Result<Self, UnscrambleError> {
        let pin_map = rom.pin_map.as_ref().ok_or(UnscrambleError::MissingLayout)?;
        let addr_bits: Vec<u8> = pin_map
            .addr
            .iter()
            .take_while(|&&gpio| gpio != GPIO_NONE)
            .map(|&gpio| table.bit(gpio).ok_or(UnscrambleError::BadLayout))
            .collect::<Result<_, _>>()?;
        let data_bits: Vec<u8> = pin_map
            .data
            .iter()
            .take_while(|&&gpio| gpio != GPIO_NONE)
            .map(|&gpio| {
                gpio.checked_sub(table.data_base)
                    .filter(|&bit| usize::from(bit) < 8 * table.word_bytes)
                    .ok_or(UnscrambleError::BadLayout)
            })
            .collect::<Result<_, _>>()?;
        if data_bits.len() != 8 * table.word_bytes {
            return Err(UnscrambleError::BadLayout);
        }
        let words = (rom.chip_size as usize / table.word_bytes).min(1 << addr_bits.len());
        Ok(Self {
            addr_bits,
            data_bits,
            words,
        })
    }

    fn addr(&self, index: usize) -> usize {
        self.addr_bits
            .iter()
            .enumerate()
            .fold(0, |addr, (line, &bit)| addr | ((index >> bit) & 1) << line)
    }

    fn demangle(&self, mangled: u32) -> u32 {
        self.data_bits
            .iter()
            .enumerate()
            .fold(0, |raw, (line, &bit)| raw | ((mangled >> bit) & 1) << line)
    }
}

/// Which ROM a table entry serves.
struct Select {
    selection: Selection,
    roms: usize,
    /// The table index bits of X1 and X2 where the slot uses them.
    x1: Option<u8>,
    x2: Option<u8>,
    /// The table index bit of the first ROM's select line, in a multi slot.
    cs1: Option<u8>,
}

impl Select {
    fn new(
        selection: Selection,
        roms: usize,
        hw: &OneromHardwareInfo,
        alg: &OneromAlgConfig,
        table: &Table,
    ) -> Result<Self, UnscrambleError> {
        let counts_ok = match selection {
            Selection::Single => roms == 1,
            Selection::Banked => (2..=4).contains(&roms),
            Selection::Multi => (2..=3).contains(&roms),
        };
        if !counts_ok {
            return Err(UnscrambleError::BadLayout);
        }

        let x_bit = |gpios: &[u8], pin: u8| {
            let mut bits = gpios
                .iter()
                .filter(|&&gpio| gpio != GPIO_NONE)
                .filter_map(|&gpio| table.bit(gpio));
            match (bits.next(), bits.next()) {
                (Some(bit), None) => Ok(bit),
                _ => Err(UnscrambleError::XPin { pin }),
            }
        };
        let (x1, x2) = match selection {
            Selection::Single => (None, None),
            Selection::Banked | Selection::Multi => (
                Some(x_bit(&hw.gpio_x1, 1)?),
                if roms >= 3 {
                    Some(x_bit(&hw.gpio_x2, 2)?)
                } else {
                    None
                },
            ),
        };
        let cs1 = match (selection, &alg.alg_cs) {
            (
                Selection::Multi,
                OneromAlgCsConfig::AlgCs0 {
                    gpio_base,
                    first_rom_cs_base,
                    ..
                },
            ) => Some(
                gpio_base
                    .checked_add(*first_rom_cs_base)
                    .and_then(|gpio| table.bit(gpio))
                    .ok_or(UnscrambleError::BadLayout)?,
            ),
            (Selection::Multi, _) => return Err(UnscrambleError::UnknownAlgorithm),
            (Selection::Single | Selection::Banked, _) => None,
        };

        Ok(Self {
            selection,
            roms,
            x1,
            x2,
            cs1,
        })
    }

    fn chip(&self, index: usize) -> Option<usize> {
        let set = |bit: Option<u8>| bit.is_some_and(|bit| (index >> bit) & 1 == 1);
        let chip = match self.selection {
            Selection::Single => 0,
            Selection::Banked => usize::from(set(self.x1)) | usize::from(set(self.x2)) << 1,
            Selection::Multi => match (set(self.cs1), set(self.x1), set(self.x2)) {
                (true, false, false) => 0,
                (false, true, false) => 1,
                (false, false, true) => 2,
                _ => return None,
            },
        };
        (chip < self.roms).then_some(chip)
    }
}
