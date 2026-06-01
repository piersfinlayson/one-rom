// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! sdrr-fw-parser
//!
//! Structures used to represent the parsed SDRR firmware information

use deku::prelude::*;

use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::{Board, Model};
use onerom_config::mcu::Variant as McuVariant;
use onerom_gen::firmware::FirmwareConfig;

use crate::types::{BitMode, FireServeMode, FireVreg, LimpMode};
use crate::{
    McuLine, McuStorage, SdrrAddress, SdrrCsState, SdrrLogicalAddress, SdrrMcuPort, SdrrRomType,
    SdrrServe,
};
use crate::{ParseError, Parser, Reader};

#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec, vec::Vec};

/// Container for both the parsed firmware information and runtime information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Sdrr {
    pub flash: Option<SdrrInfo>,
    pub ram: Option<SdrrRuntimeInfo>,
}

impl Sdrr {
    /// Method detects and returns whether the One ROM parsed is running.  This
    /// is based on the ability to parse the runtime information from RAM
    /// which is only present when the One ROM firmware is running.
    pub fn is_running(&self) -> bool {
        self.ram.is_some()
    }
}

/// Main SDRR runtime information data structure.  Contains all data parsed
/// from RAM.
///
/// Reflects `sdrr_runtime_info_t` from `sdrr/include/config_base.h`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SdrrRuntimeInfo {
    pub image_sel: u8,
    pub rom_set_index: u8,
    pub count_rom_access: u8,
    pub last_parsed_access_count: u32,
    pub account_count_address: u32,
    pub rom_table_address: u32,
    pub rom_table_size: u32,
    pub overclock_enabled: Option<bool>,
    pub status_led_enabled: Option<bool>,
    pub swd_enabled: Option<bool>,
    pub fire_vreg: Option<FireVreg>,
    pub ice_freq_mhz: Option<u16>,
    pub fire_freq_mhz: Option<u16>,
    pub sysclk_mhz: Option<u16>,
    pub fire_serve_mode: Option<FireServeMode>,
    pub bit_mode: Option<BitMode>,
    pub rom_dma_copy: Option<bool>,
    pub num_data_pins: Option<u8>,
    pub force_16_bit: Option<bool>,
    pub peri_en: Option<u8>,
    pub limp_mode: Option<LimpMode>,
}

/// Main SDRR firmware information data structure.  Contains all data parsed
/// from the firmware file.
///
/// Reflects `sdrr_info_t` from `sdrr/include/config_base.h`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SdrrInfo {
    // Core fields that are always present
    pub major_version: u16,
    pub minor_version: u16,
    pub patch_version: u16,
    pub build_number: u16,
    pub commit: [u8; 8],
    pub stm_line: McuLine,
    pub stm_storage: McuStorage,
    pub freq: u16,
    pub overclock: bool,
    pub swd_enabled: bool,
    pub preload_image_to_ram: bool,
    pub bootloader_capable: bool,
    pub status_led_enabled: bool,
    pub boot_logging_enabled: bool,
    pub mco_enabled: bool,
    pub rom_set_count: u8,
    pub count_rom_access: bool,
    pub boot_config: [u8; 4],

    // Fields that might fail to parse
    pub build_date: Option<String>,
    pub hw_rev: Option<String>,
    pub rom_sets: Vec<SdrrRomSet>, // Empty if failed
    pub pins: Option<SdrrPins>,

    /// Parse errors encountered during parsing
    pub parse_errors: Vec<ParseError>,

    /// Extra information
    pub extra_info: Option<SdrrExtraInfo>,

    /// Whether explicit metadata is included
    pub metadata_present: bool,

    /// Decoded hardware information
    pub version: FirmwareVersion,
    pub board: Option<Board>,
    pub model: Option<Model>,
    pub mcu_variant: Option<McuVariant>,

    // Located in extra info structure
    pub runtime_info_ptr: u32,
}

impl SdrrInfo {
    /// Returns whether this device is capable of being connected to via USB
    /// while running.
    ///
    /// This is currently true if the first plugin is a system plugin.
    pub fn is_usb_run_capable(&self) -> bool {
        // Get the first ROM set, if it exists
        let rom_set = match self.rom_sets.first() {
            Some(set) => set,
            None => return false,
        };

        // Get the first ROM in the set, if it exists
        let rom_info = match rom_set.roms.first() {
            Some(info) => info,
            None => return false,
        };

        // We should expand this to use the plugin header information which
        // contains supported capabilities - requires us to have read and held
        // the actual ROM image data (which in this case would be the plugin).
        matches!(rom_info.rom_type, SdrrRomType::SystemPlugin)
    }

    /// Demangles a byte from the physical pin representation to the logical
    /// representation which is served on D0-D7.  Use when looking up a byte
    /// from the ROM image data to get the "real" byte.
    pub fn demangle_byte(&self, byte: u8) -> Result<u8, String> {
        let pins = self
            .pins
            .as_ref()
            .ok_or("Pin configuration not available")?;

        assert!(pins.data.len() == 8, "Expected 8 data pins");
        let mut result = 0u8;
        for (logic_bit, &phys_pin) in pins.data.iter().enumerate() {
            assert!(phys_pin < 8, "Physical pin {} out of range", phys_pin);
            if (byte & (1 << phys_pin)) != 0 {
                result |= 1 << logic_bit;
            }
        }
        Ok(result)
    }

    /// Takes a logical address and all chip select line states, and produces
    /// a mangled address, as the firmware uses to lookup a byte in the ROM
    /// image stored in firmware.  Use to get the address to index into the
    /// ROM data stored in the firmware, and then use `demangle_byte()` to
    /// turn into a logical byte.
    #[allow(unused_variables)]
    #[allow(clippy::collapsible_if)]
    pub fn mangle_address(&self, addr: &SdrrLogicalAddress) -> Result<u32, String> {
        let cs1 = addr.cs_set().cs1();
        let cs2 = addr.cs_set().cs2();
        let cs3 = addr.cs_set().cs3();
        let x1 = addr.cs_set().x1();
        let x2 = addr.cs_set().x2();
        let addr = addr.addr();

        let pins = self
            .pins
            .as_ref()
            .ok_or("Pin configuration not available")?;

        // Remap the pins to start from 0, so they can be used as bit indeces
        let mut pins = pins.clone();
        pins.base_zero();

        if self.rom_sets.is_empty() {
            return Err("No ROM sets available".into());
        }

        let mut pin_to_addr_map = [None; 16];
        assert!(pins.addr.len() <= 16, "Expected up to 16 address pins");
        for (addr_bit, &phys_pin) in pins.addr.iter().enumerate() {
            if phys_pin < 16 {
                pin_to_addr_map[phys_pin as usize] = Some(addr_bit);
            }
        }

        let num_roms = self.rom_sets[0].rom_count as usize;
        if num_roms > 1 {
            assert!(
                pins.x1 < 16 && pins.x2 < 16,
                "X1 and X2 pins must be less than 16"
            );
            assert!(
                pin_to_addr_map[pins.x1 as usize].is_none()
                    && pin_to_addr_map[pins.x2 as usize].is_none(),
                "X1 and X2 pins must not overlap with other address pins"
            );
            pin_to_addr_map[pins.x1 as usize] = Some(14);
            pin_to_addr_map[pins.x2 as usize] = Some(15);
        }

        let rom_type = self.rom_sets[0].roms[0].rom_type;
        let addr_mask = match rom_type {
            SdrrRomType::Rom2704 | SdrrRomType::Rom2708 => {
                return Err(format!(
                    "ROM type {} not supported for address mangling",
                    rom_type
                ));
            }
            SdrrRomType::Rom2364 => {
                assert!(pins.cs1 < 16, "CS1 pin for 2364 must be less than 16");
                pin_to_addr_map[pins.cs1 as usize] = Some(13);
                0x1FFF // 13-bit address
            }
            SdrrRomType::Rom2332 => {
                assert!(pins.cs1 < 16, "CS1 pin for 2332 must be less than 16");
                assert!(pins.cs2 < 16, "CS2 pin for 2332 must be less than 16");
                pin_to_addr_map[pins.cs1 as usize] = Some(13);
                pin_to_addr_map[pins.cs2 as usize] = Some(12);
                0x0FFF // 12-bit address
            }
            SdrrRomType::Rom2316 => {
                assert!(pins.cs1 < 16, "CS1 pin for 2316 must be less than 16");
                assert!(pins.cs2 < 16, "CS2 pin for 2316 must be less than 16");
                assert!(pins.cs3 < 16, "CS3 pin for 2316 must be less than 16");
                pin_to_addr_map[pins.cs1 as usize] = Some(13);
                pin_to_addr_map[pins.cs2 as usize] = Some(11);
                pin_to_addr_map[pins.cs3 as usize] = Some(12);
                0x07FF // 11-bit address
            }
            SdrrRomType::Rom2716 => {
                assert!(pins.oe < 16, "OE pin for 2716 must be less than 16");
                assert!(pins.ce < 16, "CE pin for 2716 must be less than 16");
                pin_to_addr_map[pins.oe as usize] = Some(13);
                pin_to_addr_map[pins.ce as usize] = Some(11);
                0x07FF // 11-bit address
            }
            SdrrRomType::Rom28C16 => {
                assert!(pins.oe < 16, "OE pin for 28C16 must be less than 16");
                assert!(pins.ce < 16, "CE pin for 28C16 must be less than 16");
                pin_to_addr_map[pins.oe as usize] = Some(13);
                pin_to_addr_map[pins.ce as usize] = Some(11);
                0x07FF // 11-bit address
            }
            SdrrRomType::Rom2732 => {
                assert!(pins.oe < 16, "OE pin for 2732 must be less than 16");
                assert!(pins.ce < 16, "CE pin for 2732 must be less than 16");
                pin_to_addr_map[pins.oe as usize] = Some(13);
                pin_to_addr_map[pins.ce as usize] = Some(11);
                0x0FFF // 12-bit address
            }
            SdrrRomType::Rom2764 => {
                assert!(pins.oe < 18, "OE pin for 2764 must be less than 18");
                assert!(pins.ce < 18, "CE pin for 2764 must be less than 18");
                pin_to_addr_map[pins.oe as usize] = Some(16);
                pin_to_addr_map[pins.ce as usize] = Some(17);
                0x1FFF // 13-bit address
            }
            SdrrRomType::Rom28C64 => {
                assert!(pins.oe < 18, "OE pin for 28C64 must be less than 18");
                assert!(pins.ce < 18, "CE pin for 28C64 must be less than 18");
                pin_to_addr_map[pins.oe as usize] = Some(16);
                pin_to_addr_map[pins.ce as usize] = Some(17);
                0x1FFF // 13-bit address
            }
            SdrrRomType::Rom23128 => {
                assert!(pins.cs1 < 18, "CS1 pin for 23128 must be less than 18");
                assert!(pins.cs2 < 18, "CS2 pin for 23128 must be less than 18");
                assert!(pins.cs3 < 18, "CS3 pin for 23128 must be less than 18");
                pin_to_addr_map[pins.cs1 as usize] = Some(16);
                pin_to_addr_map[pins.cs2 as usize] = Some(17);
                pin_to_addr_map[pins.cs3 as usize] = Some(14);
                0x3FFF
            }
            SdrrRomType::Rom27128 => {
                assert!(pins.oe < 18, "OE pin for 27128 must be less than 18");
                assert!(pins.ce < 18, "CE pin for 27128 must be less than 18");
                pin_to_addr_map[pins.oe as usize] = Some(16);
                pin_to_addr_map[pins.ce as usize] = Some(17);
                0x3FFF
            }
            SdrrRomType::Rom23256 => {
                assert!(pins.cs1 < 18, "CS1 pin for 23256 must be less than 18");
                assert!(pins.cs2 < 18, "CS2 pin for 23256 must be less than 18");
                pin_to_addr_map[pins.cs1 as usize] = Some(16);
                pin_to_addr_map[pins.cs2 as usize] = Some(17);
                0x7FFF
            }
            SdrrRomType::Rom27256 => {
                assert!(pins.oe < 18, "OE pin for 27256 must be less than 18");
                assert!(pins.ce < 18, "CE pin for 27256 must be less than 18");
                pin_to_addr_map[pins.oe as usize] = Some(16);
                pin_to_addr_map[pins.ce as usize] = Some(17);
                0x7FFF
            }
            SdrrRomType::Rom28C256 => {
                assert!(pins.oe < 18, "OE pin for 28C256 must be less than 18");
                assert!(pins.ce < 18, "CE pin for 28C256 must be less than 18");
                pin_to_addr_map[pins.oe as usize] = Some(16);
                pin_to_addr_map[pins.ce as usize] = Some(17);
                0x7FFF
            }
            SdrrRomType::Rom23512 => {
                assert!(pins.cs1 < 18, "CS1 pin for 23512 must be less than 18");
                assert!(pins.cs2 < 18, "CS2 pin for 23512 must be less than 18");
                pin_to_addr_map[pins.cs1 as usize] = Some(16);
                pin_to_addr_map[pins.cs2 as usize] = Some(17);
                0xFFFF
            }
            SdrrRomType::Rom27512 => {
                assert!(pins.oe < 18, "OE pin for 27512 must be less than 18");
                assert!(pins.ce < 18, "CE pin for 27512 must be less than 18");
                pin_to_addr_map[pins.oe as usize] = Some(16);
                pin_to_addr_map[pins.ce as usize] = Some(17);
                0xFFFF
            }
            SdrrRomType::Rom231024 => {
                assert!(pins.cs1 < 18, "CS1 pin for 231024 must be less than 18");
                pin_to_addr_map[pins.cs1 as usize] = Some(17);
                0x1FFFF
            }
            SdrrRomType::RomSst39sf040
            | SdrrRomType::SystemPlugin
            | SdrrRomType::UserPlugin
            | SdrrRomType::PioPlugin
            | SdrrRomType::Rom28C512
            | SdrrRomType::Rom23QL512
            | SdrrRomType::Rom23QL384
            | SdrrRomType::Rom27C010
            | SdrrRomType::Rom27C020
            | SdrrRomType::Rom27C040
            | SdrrRomType::Rom27C080
            | SdrrRomType::Rom27C301
            | SdrrRomType::Rom23C1001
            | SdrrRomType::Rom27C200
            | SdrrRomType::Rom27C400 => {
                return Err(format!(
                    "ROM type {} not supported for address mangling",
                    rom_type
                ));
            }
            SdrrRomType::Ram6116 => {
                return Err("RAM 6116 not supported for address mangling".into());
            }
        };

        let overflow = addr & !addr_mask;
        if addr > addr_mask {
            return Err(format!(
                "Requested Address 0x{:08X} overflows the address space for ROM type {}",
                addr, rom_type
            ));
        }

        let mut input_addr = addr;

        // For 24 pin ROMs, CS is part of address space
        if rom_type.rom_pins() == 24 {
            match rom_type {
                SdrrRomType::Rom2364 => {
                    if cs1 {
                        input_addr |= 1 << 13;
                    }
                }
                SdrrRomType::Rom2332 => {
                    if cs1 {
                        input_addr |= 1 << 13;
                    }
                    if let Some(cs2) = cs2 {
                        if cs2 {
                            input_addr |= 1 << 12;
                        }
                    }
                }
                SdrrRomType::Rom2316 => {
                    if cs1 {
                        input_addr |= 1 << 13;
                    }
                    if let Some(cs2) = cs2 {
                        if cs2 {
                            input_addr |= 1 << 12;
                        }
                    }
                    if let Some(cs3) = cs3 {
                        if cs3 {
                            input_addr |= 1 << 11;
                        }
                    }
                }
                SdrrRomType::Rom2716 | SdrrRomType::Rom2732 => {
                    if cs1 {
                        input_addr |= 1 << 13;
                    }
                    if let Some(cs2) = cs2 {
                        if cs2 {
                            input_addr |= 1 << 12;
                        }
                    }
                }
                _ => unreachable!(),
            }
        }

        // For multi-ROM setups, X1 and X2 are part of address space
        if num_roms > 1 {
            if let Some(x1) = x1 {
                if x1 {
                    input_addr |= 1 << 14;
                }
            }
            if let Some(x2) = x2 {
                if x2 {
                    input_addr |= 1 << 15;
                }
            }
        }

        // Now actually do the mangling
        let mut result = 0;
        for (pin, item) in pin_to_addr_map.iter().enumerate() {
            if let Some(addr_bit) = item {
                if (input_addr & (1 << addr_bit)) != 0 {
                    result |= 1 << pin;
                }
            }
        }

        Ok(result)
    }

    /// Read a range of bytes from a ROM set.
    pub async fn read_rom_set_data<'a>(
        &mut self,
        parser: &mut Parser<'a, impl Reader>,
        set: u8,
        offset: u32,
        buf: &mut [u8],
    ) -> Result<(), String> {
        let rom_set = self
            .rom_sets
            .get(set as usize)
            .ok_or_else(|| format!("ROM set {} not found", set))?;

        if offset + buf.len() as u32 > rom_set.size {
            return Err(format!(
                "Read extends past ROM set data {offset}, {}",
                rom_set.size
            ));
        }

        let addr = rom_set.data_ptr + offset;
        parser
            .reader
            .read(addr, buf)
            .await
            .map_err(|_| "Failed to read ROM data".into())
    }

    /// Gets the size of a ROM set in bytes.
    pub fn get_rom_set_size(&self, set: u8) -> Result<usize, String> {
        let rom_set = self
            .rom_sets
            .get(set as usize)
            .ok_or_else(|| format!("ROM set {} not found", set))?;

        Ok(rom_set.size as usize)
    }

    /// Read a single byte from a ROM image at the specified logical address.
    pub async fn read_rom_byte_demangled<'a>(
        &mut self,
        parser: &mut Parser<'a, impl Reader>,
        set: u8,
        addr: SdrrAddress,
    ) -> Result<u8, String> {
        let byte = self.read_rom_byte_raw(parser, set, addr).await?;

        self.demangle_byte(byte)
    }

    pub async fn read_rom_byte_raw<'a>(
        &mut self,
        parser: &mut Parser<'a, impl Reader>,
        set: u8,
        addr: SdrrAddress,
    ) -> Result<u8, String> {
        let physical_addr = match addr {
            SdrrAddress::Raw(raw_addr) => raw_addr,
            SdrrAddress::Logical(logical_addr) => {
                // Mangle the logical address to get the physical address
                logical_addr.mangle(self)?
            }
        };

        // Read the byte
        let mut buf = [0u8; 1];
        self.read_rom_set_data(parser, set, physical_addr, &mut buf)
            .await?;

        Ok(buf[0])
    }
}

/// Extra information about this One ROM
///
/// Reflects `sdrr_extra_info` from `sdrr/include/config_base.h`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SdrrExtraInfo {
    /// Pointer to the RTT control block in RAM
    pub rtt_ptr: u32,

    /// Whether USB DFU support is compiled in
    pub usb_dfu: bool,

    /// Which GPIO Port the USB pins are located on
    pub usb_port: SdrrMcuPort,

    /// Pin number for VBUS detection
    pub vbus_pin: u8,

    /// Whether PIO is the default mode on Fire
    pub fire_pio_default: Option<bool>,

    /// Location of runtime info in RAM
    pub runtime_info_ptr: u32,
}

/// Information about a set of ROMs in an SDRR firmware
///
/// If individual ROMs are being servd, there is a set for each ROM image.
/// If multiple ROMs are being served, all ROMs served together are contained
/// in a single set.
///
/// Current maximum number of ROMs in a set is 3.
///
/// Reflects `sdrr_rom_set_t` from `sdrr/include/config_base.h`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SdrrRomSet {
    /// Pointer to the ROM image data in the firmware.
    pub data_ptr: u32,

    /// The size of the ROM image data in bytes.
    ///
    /// Currently 16KB for single ROM sets, 64KB for multi-ROM and banked
    /// switched ROM sets.
    pub size: u32,

    /// The ROMs in this set.
    pub roms: Vec<SdrrRomInfo>,

    /// The number of ROMs in this set.
    pub rom_count: u8,

    /// The serving algorithm used for this set.
    pub serve: SdrrServe,

    /// The state of the CS1 line for all ROMs in this set (active low/high/
    /// unused).  Only used for multi-ROM and bank switched sets.
    pub multi_rom_cs1_state: SdrrCsState,

    /// Firmware configuration overrides for this ROM set, if any.
    pub firmware_overrides: Option<FirmwareConfig>,
}

/// Information about a single ROM in an SDRR firmware
///
/// Reflects `sdrr_rom_info_t` from `sdrr/include/config_base.h`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SdrrRomInfo {
    /// The type of the ROM
    pub rom_type: SdrrRomType,

    /// The state of the CS1 line (active low/high/unused)
    pub cs1_state: SdrrCsState,

    /// The state of the CS2 line (active low/high/unused)
    pub cs2_state: SdrrCsState,

    /// The state of the CS3 line (active low/high/unused)
    pub cs3_state: SdrrCsState,

    /// The filename used to create the ROM image (if present in the firmware)
    pub filename: Option<String>,
}

/// SDRR pin configuration
///
/// All pin fields refer to the physical STM32 port pin number.
/// Indexes of arrays/Vecs are the address/data lines (Ax/Dx).
///
/// A pin value of 255 is used to indicate that the pin is not used.
///
/// Reflects `sdrr_pins_t` from `sdrr/include/config_base.h`
#[derive(Debug, Clone, DekuRead, DekuWrite, serde::Serialize, serde::Deserialize)]
pub struct SdrrPins {
    pub data_port: SdrrMcuPort,
    pub addr_port: SdrrMcuPort,
    pub cs_port: SdrrMcuPort,
    pub sel_port: SdrrMcuPort,
    pub status_port: SdrrMcuPort,
    pub rom_pins: u8,
    #[deku(pad_bytes_before = "2")]
    #[deku(count = "8")]
    pub data: Vec<u8>,
    #[deku(count = "16")]
    pub addr: Vec<u8>,
    #[deku(pad_bytes_before = "4")]
    pub cs1: u8,
    pub cs2: u8,
    pub cs3: u8,
    pub reserved2a: u8,
    pub reserved2b: u8,
    pub reserved2c: u8,
    pub x1: u8,
    pub x2: u8,
    pub ce: u8,
    pub oe: u8,
    pub x_jumper_pull: u8,
    #[deku(pad_bytes_before = "3")]
    pub swclk_sel: u8,
    pub swdio_sel: u8,
    pub sel0: u8,
    pub sel1: u8,
    pub sel2: u8,
    pub sel3: u8,
    pub sel4: u8,
    pub sel5: u8,
    pub sel6: u8,
    pub sel_jumper_pull: u8,

    #[deku(pad_bytes_after = "2")]
    pub status: u8,

    pub extended: u8,

    #[deku(cond = "*extended == 1", endian = "little")]
    #[deku(count = "8")]
    pub data2: Option<Vec<u8>>,

    #[deku(cond = "*extended == 1", endian = "little")]
    #[deku(count = "16")]
    pub addr2: Option<Vec<u8>>,
}

impl SdrrPins {
    const BASE_SIZE: usize = 64;
    const EXTRA_SIZE: usize = 192;

    pub(crate) const fn base_size() -> usize {
        Self::BASE_SIZE
    }

    pub(crate) const fn extra_size() -> usize {
        Self::EXTRA_SIZE
    }

    /// Rebases all address lines and, for 24 pin roms, ce/oe, cs and x lines
    /// to start from zero.  Modifies in place.
    pub fn base_zero(&mut self) {
        let mut min_pin = 255u8;
        for &pin in self.addr.iter() {
            if pin < min_pin {
                min_pin = pin;
            }
        }

        if self.rom_pins == 24 {
            if self.cs1 < min_pin {
                min_pin = self.cs1;
            }
            if self.cs2 < min_pin {
                min_pin = self.cs2;
            }
            if self.cs3 < min_pin {
                min_pin = self.cs3;
            }
            if self.x1 < min_pin {
                min_pin = self.x1;
            }
            if self.x2 < min_pin {
                min_pin = self.x2;
            }
            if self.ce < min_pin {
                min_pin = self.ce;
            }
            if self.oe < min_pin {
                min_pin = self.oe;
            }
        }

        for pin in self.addr.iter_mut() {
            if *pin != 255 {
                *pin -= min_pin;
            }
        }

        if self.rom_pins == 24 {
            if self.cs1 != 255 {
                self.cs1 -= min_pin;
            }
            if self.cs2 != 255 {
                self.cs2 -= min_pin;
            }
            if self.cs3 != 255 {
                self.cs3 -= min_pin;
            }
            if self.x1 != 255 {
                self.x1 -= min_pin;
            }
            if self.x2 != 255 {
                self.x2 -= min_pin;
            }
            if self.ce != 255 {
                self.ce -= min_pin;
            }
            if self.oe != 255 {
                self.oe -= min_pin;
            }
        }
    }
}
