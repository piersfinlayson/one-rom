// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Image generator for One ROM
//!
//! Used to create the images to be flashed to One ROM, pointed to by the
//! metadata.
//!
//! Create one or more [`Chip`] instances, and group them into one or more
//! [`ChipSet`] instances.
//!
//! Then use [`ChipSet::get_byte()`] to retrieve bytes from the Chip set, as the
//! MCU would address them, and needs to serve bytes - store these off in order
//! into a final Chip image to be flashed to One ROM, at an offset pointed to by
//! the metadata.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::panic;

use onerom_config::chip::{ChipFunction, ChipType};
use onerom_config::fw::{FirmwareVersion, ServeAlg};
use onerom_config::hw::Board;
use onerom_config::mcu::Family as McuFamily;

use crate::meta::{
    CHIP_SET_FIRMWARE_OVERRIDES_METADATA_LEN, CHIP_SET_METADATA_LEN,
    CHIP_SET_METADATA_LEN_EXTRA_INFO,
};
use crate::{Error, Result, builder::FirmwareConfig};
use crate::{MIN_FIRMWARE_OVERRIDES_VERSION, PAD_METADATA_BYTE};

/// Value to use when told to pad a Chip image
pub const PAD_BLANK_BYTE: u8 = 0xAA;

/// Value to use when no Chip in portion of address space
pub const PAD_NO_CHIP_BYTE: u8 = 0xAA;

/// Value to return when a RAM Chip is read
pub const PAD_RAM_BYTE: u8 = 0x55;

const CHIP_METADATA_LEN_NO_FILENAME: usize = 4;
const CHIP_METADATA_LEN_WITH_FILENAME: usize = 8;

// From 0.6.3 28 pin Fire boards report 18 address pins, up from 16.
const MIN_FW_VER_FIRE_28_18_ADDR_PINS: FirmwareVersion = FirmwareVersion::new(0, 6, 3, 0);

/// How to handle Chip images that are too small for the Chip type
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum SizeHandling {
    /// No special handling.  Errors if the image size does not exactly match
    /// the Chip size.
    #[default]
    None,

    /// Duplicates the image as many times as needed to fill the Chip.  Errors
    /// if the image size is not an exact divisor of the Chip size.
    #[serde(alias = "dup")]
    Duplicate,

    /// Truncates the image to fit the Chip size.  Errors if the image is an
    /// exact match size-wise.
    #[serde(alias = "trunc")]
    Truncate,

    /// Pads the image out with [`PAD_BLANK_BYTE`].
    Pad,
}

impl SizeHandling {
    pub fn supported_values() -> &'static [Self; 4] {
        &[
            SizeHandling::None,
            SizeHandling::Duplicate,
            SizeHandling::Truncate,
            SizeHandling::Pad,
        ]
    }

    pub fn is_none(&self) -> bool {
        matches!(self, SizeHandling::None)
    }
}

impl core::fmt::Display for SizeHandling {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SizeHandling::None => write!(f, "none"),
            SizeHandling::Duplicate => write!(f, "duplicate"),
            SizeHandling::Truncate => write!(f, "truncate"),
            SizeHandling::Pad => write!(f, "pad"),
        }
    }
}

/// Possible Chip Select line logic options
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CsLogic {
    /// Chip Select line is active low
    ActiveLow,

    /// Chip Select line is active high
    ActiveHigh,

    /// Used for 2332/2316 ROMs, when a CS line isn't used because it's always
    /// tied active.
    Ignore,
}

impl core::fmt::Display for CsLogic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CsLogic::ActiveLow => write!(f, "active low"),
            CsLogic::ActiveHigh => write!(f, "active high"),
            CsLogic::Ignore => write!(f, "ignore"),
        }
    }
}

/// Location within a larger Chip image that the specific image to use resides
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Location {
    /// Start of the image within the larger Chip image
    pub start: usize,

    /// Length of the image within the larger Chip image.  Must match the
    /// selected Chip type, or SizeHandling will be applied.
    pub length: usize,
}

impl CsLogic {
    pub fn try_from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "0" => Some(CsLogic::ActiveLow),
            "1" => Some(CsLogic::ActiveHigh),
            "ignore" => Some(CsLogic::Ignore),
            _ => None,
        }
    }

    pub fn c_value(&self) -> &str {
        match self {
            CsLogic::ActiveLow => "CS_ACTIVE_LOW",
            CsLogic::ActiveHigh => "CS_ACTIVE_HIGH",
            CsLogic::Ignore => "CS_NOT_USED",
        }
    }

    pub fn c_enum_val(&self) -> u8 {
        match self {
            CsLogic::ActiveLow => 0,
            CsLogic::ActiveHigh => 1,
            CsLogic::Ignore => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CsConfig {
    /// Configuration of the 3 possible Chip Select lines
    ChipSelect {
        /// Where type is ChipSelect, CS1 is always required
        cs1: CsLogic,

        /// Second chip select line, required for certain Chip Types
        cs2: Option<CsLogic>,

        /// Third chip select line, required for certain Chip Types
        cs3: Option<CsLogic>,
    },
    /// Configuration using CE/OE instead of chip select
    CeOe,
}

impl CsConfig {
    pub fn new(cs1: Option<CsLogic>, cs2: Option<CsLogic>, cs3: Option<CsLogic>) -> Self {
        if cs1.is_none() && cs2.is_none() && cs3.is_none() {
            Self::CeOe
        } else {
            let cs1 = cs1.expect("CS1 must be specified if any CS lines are used");
            Self::ChipSelect { cs1, cs2, cs3 }
        }
    }

    pub fn cs1_logic(&self) -> CsLogic {
        match self {
            CsConfig::ChipSelect { cs1, .. } => *cs1,
            CsConfig::CeOe => CsLogic::ActiveLow,
        }
    }

    pub fn cs2_logic(&self) -> Option<CsLogic> {
        match self {
            CsConfig::ChipSelect { cs2, .. } => *cs2,
            CsConfig::CeOe => Some(CsLogic::ActiveLow),
        }
    }

    pub fn cs3_logic(&self) -> Option<CsLogic> {
        match self {
            CsConfig::ChipSelect { cs3, .. } => *cs3,
            CsConfig::CeOe => None,
        }
    }
}

/// Single Chip image.  May be part of a Chip set
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Chip {
    index: usize,

    filename: String,

    // Optional alternative label for the Chip, replacing filename
    label: Option<String>,

    chip_type: ChipType,

    cs_config: CsConfig,

    data: Option<Vec<u8>>,

    // Optional location within a larger Chip image
    location: Option<Location>,
}

impl Chip {
    fn new(
        index: usize,
        filename: String,
        label: Option<String>,
        chip_type: &ChipType,
        cs_config: CsConfig,
        data: Option<Vec<u8>>,
        location: Option<Location>,
    ) -> Self {
        Self {
            index,
            filename,
            label,
            chip_type: *chip_type,
            cs_config,
            data,
            location,
        }
    }

    /// Returns the index of the Chip in the configuration
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the chip select configuration for the Chip.
    pub fn cs_config(&self) -> &CsConfig {
        &self.cs_config
    }

    /// Returns the Chip filename to use in metadata.  Uses label if specified,
    /// otherwise the actual filename string.
    pub fn filename(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.filename)
    }

    /// Returns the Chip type.
    pub fn chip_type(&self) -> &ChipType {
        &self.chip_type
    }

    pub fn has_data(&self) -> bool {
        self.data.is_some()
    }

    /// Returns a [`Chip`] instance.
    ///
    /// Takes a raw Chip image (binary data, loaded from file) and processes it
    /// according to the specified size handling (none, duplicate, pad) to
    /// ensure it matches the expected size for the given Chip type.
    #[allow(clippy::too_many_arguments)]
    pub fn from_raw_rom_image(
        index: usize,
        filename: String,
        label: Option<String>,
        source: Option<&[u8]>,
        mut dest: Vec<u8>,
        chip_type: &ChipType,
        cs_config: CsConfig,
        size_handling: &SizeHandling,
        location: Option<Location>,
    ) -> Result<Self> {
        if source.is_none() {
            if chip_type.chip_function() == ChipFunction::Ram {
                return Ok(Self::new(
                    index, filename, label, chip_type, cs_config, None, location,
                ));
            } else {
                // This is an internal error
                return Err(Error::MissingFile { id: index });
            }
        }

        let source = source.unwrap();

        // Slice source if location specified
        let source = if let Some(loc) = location {
            // Bounds check
            let end = loc
                .start
                .checked_add(loc.length)
                .ok_or(Error::BadLocation {
                    id: index,
                    reason: format!(
                        "Location overflow: start={:#X} length={:#X}",
                        loc.start, loc.length
                    ),
                })?;

            if end > source.len() {
                return Err(Error::ImageTooSmall {
                    chip_type: *chip_type,
                    index,
                    expected: end,
                    actual: source.len(),
                });
            }

            &source[loc.start..end]
        } else {
            source
        };

        let expected_size = chip_type.size_bytes();
        if dest.len() < expected_size {
            return Err(Error::BufferTooSmall {
                location: "Chip::from_raw_rom_image",
                expected: expected_size,
                actual: dest.len(),
            });
        }
        let expected_size = if *chip_type == ChipType::Chip27C080 {
            // For a 27C080, we only use a half size image, as a single One
            // ROM can only serve half.
            expected_size / 2
        } else {
            expected_size
        };

        // See what handling is required, if any
        match source.len().cmp(&expected_size) {
            Ordering::Equal => {
                // Exact match - error if dup/pad specified unnecessarily
                match size_handling {
                    SizeHandling::None => {
                        // Copy source to dest as-is
                        dest[..expected_size].copy_from_slice(&source[..expected_size]);
                    }
                    _ => {
                        return Err(Error::RightSize {
                            chip_type: *chip_type,
                            size: expected_size,
                            size_handling: size_handling.clone(),
                        });
                    }
                }
            }
            Ordering::Less => {
                // File too small - handle with dup/pad
                match size_handling {
                    SizeHandling::None => {
                        if chip_type.is_plugin() {
                            // Automatically pad a plugin
                            dest[..source.len()].copy_from_slice(source);
                            for byte in &mut dest[source.len()..expected_size] {
                                *byte = PAD_BLANK_BYTE;
                            }
                        } else {
                            return Err(Error::ImageTooSmall {
                                chip_type: *chip_type,
                                index,
                                expected: expected_size,
                                actual: source.len(),
                            });
                        }
                    }
                    SizeHandling::Duplicate => {
                        if !expected_size.is_multiple_of(source.len()) {
                            return Err(Error::DuplicationNotExactDivisor {
                                chip_type: *chip_type,
                                image_size: source.len(),
                                expected_size,
                            });
                        }
                        let multiples = expected_size / source.len();

                        // Copy multiplies of source into dest
                        for i in 0..multiples {
                            let start = i * source.len();
                            let end = start + source.len();
                            dest[start..end].copy_from_slice(source);
                        }
                    }
                    SizeHandling::Pad => {
                        // Copy source to dest and pad the rest with 0xAA
                        dest[..source.len()].copy_from_slice(source);
                        for byte in &mut dest[source.len()..expected_size] {
                            *byte = PAD_BLANK_BYTE;
                        }
                    }
                    SizeHandling::Truncate => {
                        return Err(Error::ImageTooLarge {
                            chip_type: *chip_type,
                            image_size: source.len(),
                            expected_size,
                        });
                    }
                }
            }
            Ordering::Greater => {
                match size_handling {
                    SizeHandling::Truncate => {
                        // Copy only up to expected size
                        dest[..expected_size].copy_from_slice(&source[..expected_size]);
                    }
                    _ => {
                        return Err(Error::ImageTooLarge {
                            chip_type: *chip_type,
                            image_size: source.len(),
                            expected_size,
                        });
                    }
                }
            }
        }

        Ok(Self::new(
            index,
            filename,
            label,
            chip_type,
            cs_config,
            Some(dest),
            location,
        ))
    }

    // Transforms from a physical address (based on the hardware pins) to
    // a logical Chip address, so we store the physical Chip mapping, rather
    // than the logical one.
    pub(crate) fn address_to_logical(
        phys_pin_to_addr_map: &[Option<usize>],
        address: usize,
        _board: &Board,
        num_addr_lines: usize,
    ) -> usize {
        let mut result = 0;

        for (pin, item) in phys_pin_to_addr_map.iter().enumerate() {
            #[allow(clippy::collapsible_if)]
            if let Some(addr_bit) = item {
                // Only use this mapping if it's within the Chip's address lines
                if *addr_bit < num_addr_lines {
                    if (address & (1 << pin)) != 0 {
                        result |= 1 << addr_bit;
                    }
                }
            }
        }

        result
    }

    // Transforms a data byte by rearranging its bit positions to match the hardware's
    // data pin connections.
    //
    // The hardware has a non-standard mapping for data pins, so we need to rearrange
    // the bits to ensure correct data is read/written.
    //
    // Bit mapping:
    // Original:  7 6 5 4 3 2 1 0
    // Mapped to: 3 4 5 6 7 2 1 0
    //
    // For example:
    // - Original bit 7 (MSB) moves to position 3
    // - Original bit 3 moves to position 7 (becomes new MSB)
    // - Bits 2, 1, and 0 remain in the same positions
    //
    // This transformation ensures that when the hardware reads a byte through its
    // data pins, it gets the correct bit values despite the non-standard connections.
    fn byte_mangled(byte: u8, board: &Board) -> u8 {
        // Start with 0 result
        let mut result = 0;

        // Retrieve data pin mapping - not physical pin to bit mapping, as that would be
        // the wrong way round.
        let data_pins = board.data_pins();

        // For each bit in the original byte
        #[allow(clippy::needless_range_loop)]
        for bit_pos in 0..8 {
            // Check if this bit is set in the original byte
            if (byte & (1 << bit_pos)) != 0 {
                // Get the new position for this bit
                let mut new_pos = data_pins[bit_pos];
                if new_pos > 15 {
                    // Fire rev A
                    assert!(new_pos < 24);
                    new_pos -= 16;
                } else {
                    // All other boards
                    assert!(new_pos < 8);
                }
                // Set the bit in the result at its new position
                result |= 1 << new_pos;
            }
        }

        result
    }

    // Gets the actual byte at the actual address into the image.  This is
    // used for plugins, where the address/bytes are not mangled/demangled,
    // but rather stored on flash as-is so they can be executed.
    fn get_byte_raw(&self, address: usize) -> u8 {
        let data = self
            .data
            .as_ref()
            .expect("Shouldn't be called get_byte_raw on empty image");

        data.get(address).copied().unwrap_or_else(|| {
            panic!(
                "Address {} out of bounds for Chip image of size {}",
                address,
                data.len()
            )
        })
    }

    // Get byte at the given address with both address and data
    // transformations applied.
    //
    // This function:
    // 1. Transforms the address to match the hardware's address pin mapping
    // 2. Retrieves the byte at that transformed address
    // 3. Transforms the byte's bit pattern to match the hardware's data pin
    //    mapping
    //
    // This ensures that when the hardware reads from a certain address
    // through its GPIO pins, it gets the correct byte value with bits
    // arranged according to its data pin connections.
    //
    // Chip::get_byte()
    fn get_byte(
        &self,
        phys_pin_to_addr_map: &[Option<usize>],
        address: usize,
        board: &Board,
    ) -> u8 {
        let data = self
            .data
            .as_ref()
            .expect("Shouldn't be called get_byte on empty image");

        // We have been passed a physical address based on the hardware pins,
        // so we need to transform it to a logical address based on the Chip
        // image.
        let num_addr_lines = self.chip_type.num_addr_lines();
        let transformed_address =
            Self::address_to_logical(phys_pin_to_addr_map, address, board, num_addr_lines);

        // Sanity check that we did get a logical address, which must by
        // definition fit within the actual Chip size.
        if transformed_address >= data.len() {
            panic!(
                "Transformed address {} out of bounds for Chip image of size {}",
                transformed_address,
                data.len()
            );
        }

        // Get the byte from the logical Chip address.
        let byte = data.get(transformed_address).copied().unwrap_or_else(|| {
            panic!(
                "Address {} out of bounds for Chip image of size {}",
                transformed_address,
                data.len()
            )
        });

        // Now transform the byte, as the physical data lines are not in the
        // expected order (0-7).
        Self::byte_mangled(byte, board)
    }

    // See `sdrr/include/enums.h`
    fn chip_type_c_enum_val(&self) -> u8 {
        match self.chip_type {
            ChipType::Chip2316 => 0,
            ChipType::Chip2332 => 1,
            ChipType::Chip2364 => 2,
            ChipType::Chip23128 => 3,
            ChipType::Chip23256 => 4,
            ChipType::Chip23512 => 5,
            ChipType::Chip2704 => 6,
            ChipType::Chip2708 => 7,
            ChipType::Chip2716 => 8,
            ChipType::Chip2732 => 9,
            ChipType::Chip2764 => 10,
            ChipType::Chip27128 => 11,
            ChipType::Chip27256 => 12,
            ChipType::Chip27512 => 13,
            ChipType::Chip231024 => 14,
            ChipType::Chip23C1010 | ChipType::Chip27C010 => 15,
            ChipType::Chip27C020 => 16,
            ChipType::Chip27C040 => 17,
            ChipType::Chip27C080 => 18,
            ChipType::Chip27C400 => 19,
            ChipType::Chip6116 => 20,
            ChipType::Chip27C301 => 21,
            ChipType::SystemPlugin => 22,
            ChipType::UserPlugin => 23,
            ChipType::PioPlugin => 24,
            ChipType::ChipSST39SF040 => 25,
            ChipType::Chip28C16 => 26,
            ChipType::Chip28C64 => 27,
            ChipType::Chip28C256 => 28,
            ChipType::Chip28C512 => 29,
            ChipType::Chip23QL512 => 30,
            ChipType::Chip23QL384 => 31,
            ChipType::Chip23C1001 => 32,
            ChipType::Chip27C200 => 33,
        }
    }
}

/// Type of Chip set
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ChipSetType {
    /// Single Chip - the default
    #[default]
    Single,

    /// Set of dynamically banked Chips. Used to switch between active Chip at
    /// runtime using jumpers
    Banked,

    /// Set of multiple Chips selected by CS lines.  This allows a single One
    /// Chip to serve up to 3 Chip sockets simultaneously.
    Multi,
}

/// A set of Chips, where the set type is ChipSetType
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ChipSet {
    /// ID of the Chip set
    pub id: usize,

    /// Type of Chip set
    pub set_type: ChipSetType,

    /// Serving algorithm for the Chip set
    pub serve_alg: ServeAlg,

    /// Chips in the set
    pub chips: Vec<Chip>,

    /// Optional firmware configuration overrides for this Chip set
    pub firmware_overrides: Option<FirmwareConfig>,
}

impl ChipSet {
    /// Creates a new Chip set of the specified ID, type, and containing the
    /// given Chips.
    ///
    /// The ID is an arbitrary index, usually the set ID from the config,
    /// starting at 0.
    pub fn new(
        id: usize,
        set_type: ChipSetType,
        serve_alg: ServeAlg,
        chips: Vec<Chip>,
        firmware_overrides: Option<crate::builder::FirmwareConfig>,
    ) -> Result<Self> {
        // Check some Chips were supplied
        if chips.is_empty() {
            return Err(Error::NoChips { id });
        }

        // Check set type matches number of Chips
        if chips.len() > 1 && set_type == ChipSetType::Single {
            return Err(Error::TooManyChips {
                id,
                expected: 1,
                actual: chips.len(),
            });
        }

        if chips.len() == 1 && set_type != ChipSetType::Single {
            return Err(Error::TooFewChips {
                id,
                expected: 2,
                actual: chips.len(),
            });
        }

        // Correct the serving algorithm if necessary - we accept any value
        // if a multi-rom set, and correct it.  But we don't accept an invalid
        // value for the other set types.
        let serve_alg = match set_type {
            ChipSetType::Single | ChipSetType::Banked => {
                if !matches!(
                    serve_alg,
                    ServeAlg::Default | ServeAlg::AddrOnCs | ServeAlg::TwoCsOneAddr
                ) {
                    return Err(Error::InvalidServeAlg { serve_alg });
                } else {
                    serve_alg
                }
            }
            ChipSetType::Multi => ServeAlg::AddrOnAnyCs,
        };

        // Validate firmware overrides if present
        #[allow(clippy::collapsible_if)]
        if let Some(ref overrides) = firmware_overrides {
            if overrides.ice.is_none()
                && overrides.fire.is_none()
                && overrides.led.is_none()
                && overrides.swd.is_none()
                && overrides.serve_alg_params.is_none()
            {
                return Err(Error::InvalidConfig {
                    error: "firmware_overrides specified but all fields are None".to_string(),
                });
            }

            // Validate serve_alg_params if present within firmware_overrides
            if let Some(ref params) = overrides.serve_alg_params {
                if params.params.is_empty() {
                    return Err(Error::InvalidConfig {
                        error: "serve_alg_params specified but params vec is empty".to_string(),
                    });
                }
            }
        }

        Ok(Self {
            id,
            set_type,
            serve_alg,
            chips,
            firmware_overrides,
        })
    }

    pub fn has_data(&self) -> bool {
        self.chips[0].has_data()
    }

    pub fn multi_cs_logic(&self) -> Result<CsLogic> {
        let first_cs1 = self.chips[0].cs_config.cs1_logic();
        if self.chips.len() == 1 {
            // Unused
            Ok(CsLogic::Ignore)
        } else {
            // For multi and banked chip sets we need to check all CS1 logic is
            // the same
            for chip in &self.chips {
                if chip.cs_config.cs1_logic() != first_cs1 {
                    return Err(Error::InconsistentCsLogic {
                        first: first_cs1,
                        other: chip.cs_config.cs1_logic(),
                    });
                }
            }

            // For multi-Chip sets we also need to check CS2 and CS3 are ignored
            // for all Chips
            #[allow(clippy::collapsible_if)]
            if self.set_type == ChipSetType::Multi {
                for chip in &self.chips {
                    if let Some(cs2) = chip.cs_config.cs2_logic() {
                        if cs2 != CsLogic::Ignore {
                            return Err(Error::InconsistentCsLogic {
                                first: CsLogic::Ignore,
                                other: cs2,
                            });
                        }
                    }
                    if let Some(cs3) = chip.cs_config.cs3_logic() {
                        if cs3 != CsLogic::Ignore {
                            return Err(Error::InconsistentCsLogic {
                                first: CsLogic::Ignore,
                                other: cs3,
                            });
                        }
                    }
                }
            }

            Ok(self.chips[0].cs_config.cs1_logic())
        }
    }

    /// Returns the ChipFunction for this set
    pub fn chip_function(&self) -> ChipFunction {
        self.chips[0].chip_type.chip_function()
    }

    /// Returns the size of the data required for this Chip set, in bytes.
    pub fn image_size(&self, board: &Board, fw_version: &FirmwareVersion) -> usize {
        let family = board.mcu_family();
        let num_addr_pins = board.addr_pins().len();
        let board_pins = board.chip_pins();
        let num_chips = self.chips().len();
        let set_type = &self.set_type;
        assert!(num_chips >= 1);

        if (*set_type == ChipSetType::Multi) || (*set_type == ChipSetType::Banked) {
            // Multi-ROM/banked sets: combined 64KB image.  Only supported for
            // 24-pin boards.
            assert!(board_pins == 24);
            return 2_usize.pow(16); // 64KB
        }

        // Single chip image.
        assert!(self.chips.len() == 1);
        let chip = &self.chips[0];
        if chip.chip_type().is_plugin() {
            // For plugins, the image size is always 64KB.
            return 65536;
        }
        match (board_pins, family) {
            (24, McuFamily::Stm32f4) => {
                assert!(num_addr_pins == 13);
                2_usize.pow(14) // 16KB
            }
            (28, McuFamily::Stm32f4) => {
                // Only ever developed a 14 address pin version, ice-28-a
                assert!(num_addr_pins == 14);
                2_usize.pow(16) // 64KB
            }
            (24, McuFamily::Rp2350) => {
                assert!(num_addr_pins == 13);
                2_usize.pow(16) // 64KB
            }
            (28, McuFamily::Rp2350) => {
                if *fw_version < MIN_FW_VER_FIRE_28_18_ADDR_PINS {
                    2_usize.pow(16) // 64KB
                } else {
                    // This firmware supports 18 address pins, but only the
                    // first 16 are used for chip types other than 231024.
                    // The 2364 is served as a 231024 by the firmware, so uses
                    // the same image size.
                    assert!(num_addr_pins == 18);
                    match chip.chip_type() {
                        ChipType::Chip2364 | ChipType::Chip231024 => {
                            2_usize.pow(18)
                        } // 256KB
                        ChipType::Chip23QL384 | ChipType::Chip23QL512 => {
                            // /CE is used as A15 for these chips.  On the fire-28-c,
                            // /CE is before /OE, hence requires 18 bits
                            if matches!(board, Board::Fire28A | Board::Fire28B) {
                                2_usize.pow(17)
                            } else {
                                2_usize.pow(18)
                            }
                        }
                        _ => 2_usize.pow(16), // 64KB
                    }
                }
            }
            (32, McuFamily::Rp2350) => {
                assert!(num_addr_pins == 19);
                2_usize.pow(19) // 512KB
            }
            (40, McuFamily::Rp2350) => {
                assert!(num_addr_pins == 19);
                2_usize.pow(19) // 512KB
            }
            (_, _) => {
                panic!(
                    "Unsupported board configuration: {} pins, family {:?}",
                    board_pins, family
                );
            }
        }
    }

    fn truncate_phys_pin_to_addr_map(
        phys_pin_to_addr_map: &mut [Option<usize>],
        num_addr_lines: usize,
    ) {
        // Clear any address lines beyond the number of address lines the Chip supports
        for item in phys_pin_to_addr_map.iter_mut() {
            #[allow(clippy::collapsible_if)]
            if let Some(addr_bit) = item {
                if *addr_bit >= num_addr_lines {
                    *item = None;
                }
            }
        }
    }

    /// Gets a byte from the chip set at the given address (as far as the MCU is
    /// concerned) and returns the byte, ready for the MCU to serve.
    ///
    /// ChipSet::get_byte
    pub fn get_byte(
        &self,
        address: usize,
        board: &Board,
        fw_version: &FirmwareVersion,
        invert_cs1_x: bool,
    ) -> u8 {
        // Deal with RAM chips
        if (!self.has_data()) && (self.chip_function() == ChipFunction::Ram) {
            return Chip::byte_mangled(PAD_RAM_BYTE, board);
        }
        if self.chip_function().is_plugin() {
            return self.chips[0].get_byte_raw(address);
        }

        // Early returns above

        // Hard-coded assumption that X1/X2 (STM32F4) are pins 14/15 for
        // single chip sets and banked chip sets.  However, for RP2350 they may
        // be other pins.
        assert!(address < self.image_size(board, fw_version));
        if (self.chips.len() == 1) || (self.set_type == ChipSetType::Banked) {
            let (chip_index, masked_address) = if self.set_type != ChipSetType::Banked {
                // Common case - single ROM chip image
                (0, address)
            } else {
                // Banked mode: use X1/X2 to select Chip.  Only supported on 24 pin boards
                assert!(address < 65536, "Address out of bounds for banked Chip set");
                let x1_pin = board.bit_x1();
                let x2_pin = board.bit_x2();
                let bank = if board.x_jumper_pull() == 1 {
                    ((address >> x1_pin) & 1) | (((address >> x2_pin) & 1) << 1)
                } else {
                    // Invert the logic if the jumpers pull to GND
                    (!(address >> x1_pin) & 1) | ((!((address >> x2_pin) & 1)) << 1)
                };
                let mask = !(1 << x1_pin) & !(1 << x2_pin);
                let masked_address = address & mask;
                let chip_index = bank % self.chips.len(); // Wrap around
                (chip_index, masked_address)

                // Note that this code fills sections of the overall 64KB image with the bank Chip
                // images even if the CS value is set to inactive
            };

            let num_addr_lines = self.chips[chip_index].chip_type.num_addr_lines();
            let mut phys_pin_to_addr_map = handle_snowflake_chip_types(
                board,
                board.phys_pin_to_addr_map(),
                &self.chips[chip_index].chip_type,
            );
            Self::truncate_phys_pin_to_addr_map(&mut phys_pin_to_addr_map, num_addr_lines);

            let transformed = Chip::address_to_logical(
                &phys_pin_to_addr_map,
                masked_address,
                board,
                num_addr_lines,
            );
            if transformed >= self.chips[chip_index].chip_type.size_bytes() {
                // Only valid for non-power-of-2 chip types (e.g. 23QL384 at 48KB), where
                // the logical address space implied by num_addr_lines exceeds the actual
                // chip data size.  For power-of-2 types this indicates an internal error.
                assert!(
                    !self.chips[chip_index]
                        .chip_type
                        .size_bytes()
                        .is_power_of_two(),
                    "Transformed address {} out of bounds for power-of-2 chip type - internal error",
                    transformed
                );
                return Chip::byte_mangled(PAD_NO_CHIP_BYTE, board);
            }

            return self.chips[chip_index].get_byte(&phys_pin_to_addr_map, masked_address, board);
        }
        // Early return above

        // Multiple Chips: check CS line states to select responding Chip.  This
        // code can handle any X1/X2 positions - but the above can't.  Again,
        // multi-chip sets are 24 pin only concepts
        assert!(address < 65536, "Address out of bounds for multi-Chip set");

        for (index, chip_in_set) in self.chips.iter().enumerate() {
            // Get the physical addr and data pin mappings.  We have to
            // retrieve this for each Chip in the set, as each Chip may be
            // a different type (size).
            let num_addr_lines = chip_in_set.chip_type.num_addr_lines();
            let mut phys_pin_to_addr_map = handle_snowflake_chip_types(
                board,
                board.phys_pin_to_addr_map(),
                &chip_in_set.chip_type,
            );
            Self::truncate_phys_pin_to_addr_map(&mut phys_pin_to_addr_map, num_addr_lines);

            // All of CS1/X1/X2 have to have the same active low/high status
            // so we retrieve that from CS1 (as X1/X2 aren't specifically
            // configured in the chip sets).
            let pins_active_high = chip_in_set.cs_config.cs1_logic() == CsLogic::ActiveHigh;

            // Get the CS pin that controls this chip's selection
            let cs_pin = board.cs_bit_for_chip_in_set(chip_in_set.chip_type, index);
            assert!(cs_pin <= 15, "Internal error: CS pin is > 15");

            fn is_pin_active(
                active_high: bool,
                invert_cs1_x: bool,
                address: usize,
                pin: u8,
            ) -> bool {
                if !invert_cs1_x {
                    if active_high {
                        (address & (1 << pin)) != 0
                    } else {
                        (address & (1 << pin)) == 0
                    }
                } else {
                    // Invert the logic for this read
                    if active_high {
                        (address & (1 << pin)) == 0
                    } else {
                        (address & (1 << pin)) != 0
                    }
                }
            }

            let cs_active = is_pin_active(pins_active_high, invert_cs1_x, address, cs_pin);

            if cs_active {
                // Verify exactly one CS pin is active
                let cs1_pin = board.bit_cs1(chip_in_set.chip_type);
                let x1_pin = board.bit_x1();
                let x2_pin = board.bit_x2();

                let cs1_is_active = is_pin_active(pins_active_high, invert_cs1_x, address, cs1_pin);
                let x1_is_active = is_pin_active(pins_active_high, invert_cs1_x, address, x1_pin);
                let x2_is_active = is_pin_active(pins_active_high, invert_cs1_x, address, x2_pin);

                let active_count = [cs1_is_active, x1_is_active, x2_is_active]
                    .iter()
                    .filter(|&&x| x)
                    .count();

                if active_count == 1 && self.check_chip_cs_requirements(chip_in_set, address, board)
                {
                    return chip_in_set.get_byte(&phys_pin_to_addr_map, address, board);
                }
            }
        }

        // No Chip is selected, so this part of the address space is set to blank value
        Chip::byte_mangled(PAD_NO_CHIP_BYTE, board)
    }

    fn check_chip_cs_requirements(
        &self,
        chip_in_set: &Chip,
        address: usize,
        board: &Board,
    ) -> bool {
        let cs_config = &chip_in_set.cs_config;
        let chip_type = chip_in_set.chip_type;

        // Check CS2 if specified
        if let Some(cs2_logic) = cs_config.cs2_logic() {
            match cs2_logic {
                CsLogic::Ignore => {
                    // CS2 state doesn't matter
                }
                CsLogic::ActiveLow => {
                    let cs2_pin = board.bit_cs2(chip_type);
                    let cs2_active = (address & (1 << cs2_pin)) == 0;
                    if !cs2_active {
                        return false;
                    }
                }
                CsLogic::ActiveHigh => {
                    let cs2_pin = board.bit_cs2(chip_type);
                    let cs2_active = (address & (1 << cs2_pin)) != 0;
                    if cs2_active {
                        return false;
                    }
                }
            }
        }

        // Check CS3 if specified
        if let Some(cs3_logic) = cs_config.cs3_logic() {
            match cs3_logic {
                CsLogic::Ignore => {
                    // CS3 state doesn't matter
                }
                CsLogic::ActiveLow => {
                    let cs3_pin = board.bit_cs3(chip_type);
                    let cs3_active = (address & (1 << cs3_pin)) == 0;
                    if !cs3_active {
                        return false;
                    }
                }
                CsLogic::ActiveHigh => {
                    let cs3_pin = board.bit_cs3(chip_type);
                    let cs3_active = (address & (1 << cs3_pin)) != 0;
                    if cs3_active {
                        return false;
                    }
                }
            }
        }

        true
    }

    #[allow(dead_code)]
    fn mask_cs_selection_bits(&self, address: usize, chip_type: ChipType, board: &Board) -> usize {
        let mut masked_address = address;

        // Remove the CS selection bits - only mask bits that exist on this hardware
        masked_address &= !(1 << board.bit_cs1(chip_type));

        // Only mask X1/X2 on hardware that has them
        if board.supports_multi_chip_sets() {
            let x1 = board.bit_x1();
            let x2 = board.bit_x2();
            assert!(x1 < 15 && x2 < 15, "X1/X2 pins must be less than 15");
            masked_address &= !(1 << x1);
            masked_address &= !(1 << x2);
        }

        // Remove CS2/CS3 bits based on chip type
        match chip_type {
            ChipType::Chip2332 => {
                masked_address &= !(1 << board.bit_cs2(chip_type));
            }
            ChipType::Chip2316 => {
                masked_address &= !(1 << board.bit_cs2(chip_type));
                masked_address &= !(1 << board.bit_cs3(chip_type));
            }
            ChipType::Chip2364 => {
                // 2364 only uses CS1, no additional bits to remove
            }
            ChipType::Chip23128 => {
                // No additional bits to remove
            }
            _ => {
                panic!(
                    "Internal error: unsupported chip type {} in mask_cs_selection_bits",
                    chip_type.name()
                );
            }
        }

        // Ensure address fits within Chip size
        masked_address & ((1 << 13) - 1) // Mask to 13 bits max (8KB)
    }

    /// Returns a slice of the chips in this set.
    pub fn chips(&self) -> &[Chip] {
        &self.chips
    }

    /// Returns the length of metadata required for all of the chips.  This
    /// includes all chip structs, plus the array of pointers to them.
    pub fn chips_metadata_len(&self, include_filenames: bool) -> usize {
        let num_chips = self.chips.len();

        // Size of all chip metadata structs
        let chip_metadata_len = if include_filenames {
            CHIP_METADATA_LEN_WITH_FILENAME
        } else {
            CHIP_METADATA_LEN_NO_FILENAME
        } * num_chips;

        #[allow(clippy::let_and_return)]
        chip_metadata_len
    }

    /// Writes chip metadata structs for all chips in this set and store off
    /// offsets to them.
    ///
    /// Returns the number of bytes written and also pointers to each, so
    /// that the array of chip pointers can be written.
    pub fn write_chip_metadata(
        &self,
        buf: &mut [u8],
        chip_filename_ptrs: &[u32],
        chip_metadata_ptrs: &mut [u32],
        include_filenames: bool,
    ) -> Result<usize> {
        let num_chips = self.chips.len();

        // Check enough buffer space
        let expected_len = self.chips_metadata_len(include_filenames);
        if buf.len() < expected_len {
            return Err(Error::BufferTooSmall {
                location: "write_chip_metadata1",
                expected: expected_len,
                actual: buf.len(),
            });
        }

        // Check enough space for pointers
        if chip_metadata_ptrs.len() < num_chips {
            return Err(Error::BufferTooSmall {
                location: "write_chip_metadata2",
                expected: num_chips,
                actual: chip_metadata_ptrs.len(),
            });
        }

        let mut offset = 0;

        // Write chip metadata.
        for (ii, chip) in self.chips.iter().enumerate() {
            // Set up the pointer to be returned first
            chip_metadata_ptrs[ii] = offset as u32;

            // Write the chip_type
            buf[offset] = chip.chip_type_c_enum_val();
            offset += 1;

            // Write the CS states
            let is_plugin = chip.chip_type.chip_function().is_plugin();
            buf[offset] = if is_plugin {
                CsLogic::Ignore.c_enum_val()
            } else {
                chip.cs_config.cs1_logic().c_enum_val()
            };
            offset += 1;
            buf[offset] = if is_plugin {
                CsLogic::Ignore.c_enum_val()
            } else {
                chip.cs_config.cs2_logic().map_or(2, |cs| cs.c_enum_val())
            };
            offset += 1;
            buf[offset] = if is_plugin {
                CsLogic::Ignore.c_enum_val()
            } else {
                chip.cs_config.cs3_logic().map_or(2, |cs| cs.c_enum_val())
            };
            offset += 1;

            // Add filename if required
            if include_filenames {
                let chip_filename_ptr = chip_filename_ptrs
                    .get(chip.index())
                    .copied()
                    .ok_or_else(|| Error::MissingPointer { id: chip.index() })?;
                buf[offset..offset + 4].copy_from_slice(&chip_filename_ptr.to_le_bytes());
                offset += 4;
            }
        }

        Ok(offset)
    }

    /// Writes the array of pointers to each chip metadata struct.  Must be
    /// called after [`Self::write_chip_metadata()`].
    pub fn write_chip_pointer_array(
        &self,
        buf: &mut [u8],
        chip_metadata_ptrs: &[u32],
    ) -> Result<usize> {
        let num_chips = self.chips.len();

        // Check enough buffer space
        let expected_len = 4 * num_chips;
        if buf.len() < expected_len {
            return Err(Error::BufferTooSmall {
                location: "write_chip_pointer_array",
                expected: expected_len,
                actual: buf.len(),
            });
        }

        // Check enough pointers
        if chip_metadata_ptrs.len() < num_chips {
            return Err(Error::MissingPointer {
                id: chip_metadata_ptrs.len(),
            });
        }

        let mut offset = 0;

        // Write the array of pointers
        for ii in chip_metadata_ptrs.iter() {
            buf[offset..offset + 4].copy_from_slice(&ii.to_le_bytes());
            offset += 4;
        }

        Ok(offset)
    }

    /// Writes the actual set metadata for this set.  This function must be
    /// called for each set one after the other, in order of set ID, as it
    /// must write an array of sets.
    #[allow(clippy::too_many_arguments)]
    pub fn write_set_metadata(
        &self,
        buf: &mut [u8],
        data_ptr: u32,
        chip_array_ptr: u32,
        board: &Board,
        version: &FirmwareVersion,
        serve_config_ptr: Option<u32>,
        firmware_overrides_ptr: Option<u32>,
    ) -> Result<usize> {
        // Check enough buffer space
        let expected_len = Self::chip_set_metadata_len(version);
        if buf.len() < expected_len {
            return Err(Error::BufferTooSmall {
                location: "write_set_metadata",
                expected: expected_len,
                actual: buf.len(),
            });
        }

        let mut offset = 0;

        // Write the chip image(s) data pointer
        buf[offset..offset + 4].copy_from_slice(&data_ptr.to_le_bytes());
        offset += 4;

        // Write the chip data size
        let data_size = self.image_size(board, version) as u32;
        buf[offset..offset + 4].copy_from_slice(&data_size.to_le_bytes());
        offset += 4;

        // Write the chip metadata pointer
        buf[offset..offset + 4].copy_from_slice(&chip_array_ptr.to_le_bytes());
        offset += 4;

        // Write the nubmer of chips in this set
        let num_chips = self.chips.len() as u8;
        buf[offset] = num_chips;
        offset += 1;

        // Write the serving algorithm
        let algorithm = self.serve_alg().c_enum_value();
        buf[offset] = algorithm;
        offset += 1;

        // Write the multi-chip CS state
        let multi_cs_state = self.multi_cs_logic()?.c_enum_val();
        buf[offset] = multi_cs_state;
        offset += 1;

        if version >= &MIN_FIRMWARE_OVERRIDES_VERSION {
            buf[offset] = 1; // extra_info = 1 for 0.6.0+
        } else {
            buf[offset] = PAD_METADATA_BYTE; // pad byte for pre-0.6.0
        }
        offset += 1;

        assert_eq!(offset, 16, "First 16 bytes should be written");

        // Write extended fields for 0.6.0+
        if version >= &MIN_FIRMWARE_OVERRIDES_VERSION {
            // Write serve_config pointer
            let serve_ptr = serve_config_ptr.unwrap_or(0xFFFFFFFF);
            buf[offset..offset + 4].copy_from_slice(&serve_ptr.to_le_bytes());
            offset += 4;

            // Write firmware_overrides pointer
            let fw_ptr = firmware_overrides_ptr.unwrap_or(0xFFFFFFFF);
            buf[offset..offset + 4].copy_from_slice(&fw_ptr.to_le_bytes());
            offset += 4;

            // Write padding to reach 64 bytes
            buf[offset..offset + 40].copy_from_slice(&[0u8; 40]);
            offset += 40;

            assert_eq!(
                offset, CHIP_SET_FIRMWARE_OVERRIDES_METADATA_LEN,
                "Total should be 64 bytes for 0.6.0+"
            );
        }

        assert_eq!(
            offset, expected_len,
            "Internal error: offset does not match expected length"
        );

        Ok(offset)
    }

    pub fn chip_set_metadata_len(version: &FirmwareVersion) -> usize {
        if *version >= MIN_FIRMWARE_OVERRIDES_VERSION {
            CHIP_SET_METADATA_LEN_EXTRA_INFO
        } else {
            CHIP_SET_METADATA_LEN
        }
    }

    pub fn serve_alg(&self) -> ServeAlg {
        self.serve_alg
    }
}

// Handle Chip Types which do not have a standard address layout.  Currently,
// the only know Chip type needing special handling is the 2732, which has
// swapped A11 and A12 lines.
//
// Also, now using this function to handle 28 pin chips that aren't the
// 231024.  In this case, we want to throw away the first two address lines,
// as these are CS lines, which aren't used as address lines, except for the
// 231024.
fn handle_snowflake_chip_types(
    board: &Board,
    phys_pin_to_addr_map: &[Option<usize>],
    chip_type: &ChipType,
) -> Vec<Option<usize>> {
    let mut modified_map = phys_pin_to_addr_map.to_vec();
    if board.chip_pins() == 24 && *chip_type == ChipType::Chip2732 {
        // Swap A11 and A12
        let a11_index = modified_map.iter().position(|&x| x == Some(11));
        let a12_index = modified_map.iter().position(|&x| x == Some(12));
        if let (Some(i11), Some(i12)) = (a11_index, a12_index) {
            modified_map[i11] = Some(12);
            modified_map[i12] = Some(11);
        } else {
            // Address lines not found as expected.  Panic, as this is an
            // internal error and implies a board has been added supporting
            // the 2732 but without pins A11 and/or A12.
            panic!(
                "Address lines A11 and/or A12 not found in phys_pin_to_addr_map for 2732 handling"
            );
        }
    } else if board.chip_pins() == 28 && *chip_type == ChipType::Chip2364 {
        // When serving a 2364 from a 28 pin board, A16 and /CE are transposed
        let cs_pin = if matches!(board, Board::Fire28A | Board::Fire28B) {
            // These boards have /CE in the A16 position
            board.bit_ce(ChipType::Chip2764) as usize
        } else {
            // This board has /OE in the A16 position
            board.bit_oe(ChipType::Chip2764) as usize
        };
        if let Some(i16) = modified_map.iter().position(|&x| x == Some(16)) {
            modified_map.swap(i16, cs_pin);
        } else {
            panic!(
                "Address line A16 not found in phys_pin_to_addr_map for 2364-in-28-pin handling"
            );
        }

        // And CS1 is A11, and A11 is A12.
        let cs1_pin = board.bit_cs1(ChipType::Chip231024) as usize;
        let i11 = modified_map
            .iter()
            .position(|&x| x == Some(11))
            .expect("A11 not found for 2364-in-28-pin handling");
        let i12 = modified_map
            .iter()
            .position(|&x| x == Some(12))
            .expect("A12 not found for 2364-in-28-pin handling");
        let old_cs1 = modified_map[cs1_pin];
        modified_map[cs1_pin] = Some(11);
        modified_map[i11] = Some(12);
        modified_map[i12] = old_cs1;
    } else if board.chip_pins() == 28
        && *chip_type != ChipType::Chip231024
        && *chip_type != ChipType::Chip2364
        && *chip_type != ChipType::Chip23QL512
        && *chip_type != ChipType::Chip23QL384
    {
        // Covers 27xx chips as well as 28 pin types

        // Remove first two entries, and add two Nones on the end.
        modified_map.remove(0);
        modified_map.remove(0);
        modified_map.push(None);
        modified_map.push(None);

        if *chip_type == ChipType::Chip28C256 {
            // Swap A15 and A14
            let a14_index = modified_map.iter().position(|&x| x == Some(14));
            let a15_index = modified_map.iter().position(|&x| x == Some(15));
            if let (Some(i14), Some(i15)) = (a14_index, a15_index) {
                modified_map[i14] = Some(15);
                modified_map[i15] = Some(14);
            } else {
                // Address lines not found as expected.  Panic, as this is an
                // internal error and implies a board has been added supporting
                // the 28C256 but without pins A14 and/or A15.
                panic!(
                    "Address lines A14 and/or A15 not found in phys_pin_to_addr_map for 28C256 handling"
                );
            }
        }
    } else if *chip_type == ChipType::Chip27C301 {
        // A16 is an alternate pin
        if let Some(a16_index) = modified_map.iter().position(|&x| x == Some(16)) {
            match board {
                Board::Fire32A => {
                    if a16_index == 0 {
                        // Remove this entry, and add Some(16) on the end instead.
                        modified_map.remove(0);
                        modified_map.push(Some(16));
                    } else {
                        panic!(
                            "Address line A16 found at unexpected position {} in phys_pin_to_addr_map for 27C301 handling",
                            a16_index
                        );
                    }
                },
                Board::Fire32B => {
                    if a16_index == 2 {
                        // Remove two entries of pin map
                        modified_map.remove(0);
                        modified_map.remove(0);

                        // Now add None on the end
                        modified_map.push(None);

                        // Now put A16 on the end
                        modified_map.push(Some(16));

                        // Finally, set the old A16 (now index 0) to None
                        modified_map[0] = None;
                    } else {
                        panic!(
                            "Address line A16 found at unexpected position {} in phys_pin_to_addr_map for 27C301 handling",
                            a16_index
                        );
                    }
                }
                _ => {
                    panic!("Unexpected 32 pin board type");
                }
            }
        } else {
            panic!("Address line A16 not found in phys_pin_to_addr_map for 27C301 handling");
        }
    } else if *chip_type == ChipType::Chip23QL512 || *chip_type == ChipType::Chip23QL384 {
        // A15 is actually the 27512's /CE line.  CS1 is actually OE.
        let oe_pin = board.bit_oe(ChipType::Chip27512) as usize;
        let ce_pin = board.bit_ce(ChipType::Chip27512) as usize;
        let i15 = modified_map
            .iter()
            .position(|&x| x == Some(15))
            .expect("A15 not found for 23QL512/384 handling");

        modified_map[ce_pin] = Some(15);
        modified_map[oe_pin] = None;
        modified_map[i15] = None;

        if matches!(board, Board::Fire28A | Board::Fire28B) {
            // On these boards we start indexing the image from the second address/cs pin, not the first.
            modified_map.remove(0);
            modified_map.push(None);
        }
    } else if *chip_type == ChipType::ChipSST39SF040 {
        if !matches!(board, Board::Fire32B) {
            panic!("SST39SF040 is not supported on fire-32-a - use 27C040 with a shim");
        }
        
        // On fire-32-b, regular A18 is the first address pin.  Remove it
        modified_map.remove(0);

        // Instead A18 is after the last address pin
        modified_map.push(Some(18));
    }
    modified_map
}
