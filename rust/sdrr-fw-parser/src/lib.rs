//! sdrr-fw-parser
//!
//! Parses [Software Defined Retro ROM](https://piers.rocks/u/sdrr) (SDRR)
//! firmware.
//!
//! This is a `no_std` compatible library, which can be used in both `std` and
//! `no_std` environments and can read and extract information from SDRR
//! firmware - either from
//! - a binary file
//! - an ELF file
//! - raw bytes, e.g. from bytes read directly from a device's flash or RAM
//!
//! This is used directly within the SDRR repository, by:
//! - [`sdrr-info`](https://github.com/piersfinlayson/software-defined-retro-rom/blob/main/rust/sdrr-info/README.md) -
//!   PC based tool to analyse SDRR firmware source code
//!
//! It can also be used by external tools.
//!
//! Typically used like this:
//!
//! ```rust ignore
//! use sdrr_fw_parser::SdrrInfo;
//! let sdrr_info = SdrrInfo::from_firmware_bytes(
//!     SdrrFileType::Elf,
//!     &sdrr_info, // Reference to sdrr_info_t from firmware file
//!     &full_fw,   // Reference to full firmware data
//!     file_size   // Size of the full firmware file in bytes
//! );
//! ```

#![cfg_attr(not(feature = "std"), no_std)]

// Get logging working when building on ESP32
#[cfg(feature = "esp32")]
use esp_println as _;

use airfrog_rpc::io::Reader;
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_config::mcu::Variant as McuVariant;

/// Maximum SDRR firmware versions supported by this version of`sdrr-fw-parser`
pub const MAX_VERSION_MAJOR: u16 = 0;
pub const MAX_VERSION_MINOR: u16 = 7;
pub const MAX_VERSION_PATCH: u16 = 999;

// lib.rs - Public API and core traits
pub mod info;
pub mod lab;
mod parsing;
pub mod readers;
pub mod types;

// Use alloc if no-std.
#[cfg(not(feature = "std"))]
extern crate alloc;
#[cfg(not(feature = "std"))]
use alloc::vec;

use core::fmt;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

pub use info::{Sdrr, SdrrExtraInfo, SdrrInfo, SdrrPins, SdrrRomInfo, SdrrRomSet, SdrrRuntimeInfo};
pub use lab::{LabFlash, LabParser, LabRam, OneRomLab};
pub use types::{
    McuLine, McuStorage, SdrrAddress, SdrrCsSet, SdrrCsState, SdrrLogicalAddress, SdrrMcuPort,
    SdrrRomType, SdrrServe, Source,
};

use crate::parsing::{
    SdrrInfoHeader, SdrrRuntimeInfoHeader, parse_and_validate_header,
    parse_and_validate_runtime_info,
};

/// Offset from start of the firmware where the SDRR info header is located.
///
/// The first 4 "magic" bytes are b"SDRR" (upper case).
pub const SDRR_INFO_FW_OFFSET: u32 = 0x200;

/// Offset from the start of RAM where the SDRR runtime info header is located.
///
/// The first 4 "magic" bytes are b"sdrr" (lower case).
pub const SDRR_RUNTIME_INFO_FW_OFFSET: u32 = 0x0;

// Use std/no-std String and Vec types
#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec::Vec};

// STM32F4 flash base address.  Required to find offset from pointers
pub(crate) const STM32F4_FLASH_BASE: u32 = 0x08000000;

// STM32F4 RAM base address.  Required to find offset from pointers
pub(crate) const STM32F4_RAM_BASE: u32 = 0x20000000;

/// Parser for Software Defined Retro ROM (SDRR) firmware images.
///
/// This parser extracts configuration and ROM data from SDRR firmware files,
/// which are used in devices that emulate vintage ROM chips (2316/2332/2364).
/// The parser is designed to work efficiently in both PC and embedded environments.
///
/// # Architecture
///
/// The parser uses a two-phase approach:
///
/// 1. **Metadata parsing** - Headers, pin configurations, and ROM set information
///    are parsed immediately into memory (typically just a few KB)
/// 2. **ROM data access** - ROM images (up to 64KB each) remain in the source
///    and are accessed lazily through reader callbacks
///
/// This design allows embedded devices with limited RAM to parse and work with
/// SDRR firmware without loading entire ROM images into memory.
///
/// # Usage
///
/// ```rust,no_run
/// # async fn test() -> Result<(), Box<dyn std::error::Error>> {
/// # use sdrr_fw_parser::{Parser, SdrrAddress};
/// # use airfrog_rpc::io::Reader;
/// # struct MyReader;
/// # impl MyReader {
/// #     fn new(_: &str) -> Self { MyReader }
/// # }
/// # impl Reader for MyReader {
/// #     type Error = std::io::Error;
/// #     async fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), Self::Error> { Ok(()) }
/// #     fn update_base_address(&mut self, base_address: u32) {}
/// # }
/// // Create a reader for your data source
/// let mut reader = MyReader::new("firmware.bin");
///
/// // Create parser and parse metadata
/// let mut parser = Parser::new(&mut reader);
/// let sdrr = parser.parse().await;
/// let mut info = sdrr.flash.unwrap();
///
/// // Access ROM data lazily
/// let byte = info.read_rom_byte_demangled(&mut parser, 0, SdrrAddress::Raw(0x1000)).await?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }
/// ```
///
/// # Firmware Structure
///
/// SDRR firmware contains:
/// - A header with "SDRR" magic bytes at offset 0x200 from base
/// - Version information and build metadata  
/// - Pin mapping configuration for the STM32F4 microcontroller
/// - One or more ROM sets, each containing up to 3 ROM images
/// - ROM data that has been pre-processed for efficient serving
///
/// # Address Translation
///
/// ROM addresses and data bytes are "mangled" in the firmware for efficient
/// real-time serving. The parser handles the translation between logical
/// addresses/data and their physical representation in the firmware.
pub struct Parser<'a, R: Reader> {
    reader: &'a mut R,
    base_flash_address: u32,
    base_ram_address: u32,
}

impl<'a, R: Reader> Parser<'a, R> {
    /// Create a new parser with the default STM32F4 base address (0x08000000).
    ///
    /// # Arguments
    ///
    /// * `reader` - Implementation of [`Reader`] trait that provides access to firmware bytes
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use sdrr_fw_parser::Parser;
    /// # use airfrog_rpc::io::Reader;
    /// # struct MyReader;
    /// # impl MyReader {
    /// #     fn new() -> Self { MyReader }
    /// # }
    /// # impl Reader for MyReader {
    /// #     type Error = std::io::Error;
    /// #     async fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), Self::Error> { Ok(()) }
    /// #     fn update_base_address(&mut self, base_address: u32) {}
    /// # }
    /// let mut reader = MyReader::new();
    /// let mut parser = Parser::new(&mut reader);
    /// ```
    pub fn new(reader: &'a mut R) -> Self {
        Self {
            reader,
            base_flash_address: STM32F4_FLASH_BASE,
            base_ram_address: STM32F4_RAM_BASE,
        }
    }

    /// Create a new parser with a custom base address.
    ///
    /// Use this when parsing firmware for devices with non-standard memory maps
    /// or when analyzing relocated firmware images.
    ///
    /// # Arguments
    ///
    /// * `reader` - Implementation of [`Reader`] trait that provides access to firmware bytes
    /// * `base_flash_address` - Base address where flash memory begins (e.g., 0x08000000 for STM32F4)
    /// * `base_ram_address` - Base address where RAM begins (e.g., 0x20000000 for STM32F4)
    pub fn with_base_flash_address(
        reader: &'a mut R,
        base_flash_address: u32,
        base_ram_address: u32,
    ) -> Self {
        Self {
            reader,
            base_flash_address,
            base_ram_address,
        }
    }

    // Retrieve the SDRR info header from the firmware.
    async fn retrieve_header(&mut self) -> Result<SdrrInfoHeader, String> {
        // Try to find SDRR info at standard location
        let sdrr_info_addr = self.base_flash_address + SDRR_INFO_FW_OFFSET;

        // Read the header
        let mut header_buf = [0u8; SdrrInfoHeader::size()];
        self.reader
            .read(sdrr_info_addr, &mut header_buf)
            .await
            .map_err(|_| "Failed to read SDRR header")?;

        // Parse and validate header using the helper
        parse_and_validate_header(&header_buf)
    }

    async fn retrieve_runtime_header(&mut self) -> Result<SdrrRuntimeInfoHeader, String> {
        // Try to find SDRR runtime info at standard location
        let sdrr_runtime_info_addr = self.base_ram_address + SDRR_RUNTIME_INFO_FW_OFFSET;

        // Read the runtime info header
        let mut runtime_buf = [0u8; SdrrRuntimeInfoHeader::size()];
        self.reader
            .read(sdrr_runtime_info_addr, &mut runtime_buf)
            .await
            .map_err(|_| "Failed to read SDRR runtime info")?;
        // Parse and validate runtime info using the helper
        parse_and_validate_runtime_info(&runtime_buf)
    }

    async fn retrieve_runtime_header_from_info(
        &mut self,
        info: &SdrrInfo,
    ) -> Result<SdrrRuntimeInfoHeader, String> {
        let runtime_info_ptr = info.runtime_info_ptr;
        if runtime_info_ptr < self.base_ram_address && runtime_info_ptr != 0xFFFF_FFFF_u32 {
            return Err(format!(
                "Invalid runtime info pointer: 0x{:08X}",
                runtime_info_ptr
            ));
        }

        // Read the runtime info header
        let mut runtime_buf = [0u8; SdrrRuntimeInfoHeader::size()];
        self.reader
            .read(runtime_info_ptr, &mut runtime_buf)
            .await
            .map_err(|_| "Failed to read SDRR runtime info")?;
        // Parse and validate runtime info using the helper
        parse_and_validate_runtime_info(&runtime_buf)
    }

    /// Function to do a brief check whether this is an SDRR device.
    ///
    /// Returns:
    /// - `true` if the SDRR header was found and is valid
    /// - `false` if the header was not found (or an error occured)
    pub async fn detect(&mut self) -> bool {
        match self.retrieve_header().await {
            Ok(_header) => true,
            Err(_) => false,
        }
    }

    /// Parses both flash and RAM
    pub async fn parse(&mut self) -> Sdrr {
        let flash = match self.parse_flash().await {
            Ok(f) => Some(f),
            Err(e) => {
                debug!("Failed to parse flash: {}", e);
                None
            }
        };

        let ram = if let Some(flash) = &flash {
            self.parse_ram_from_info(flash).await
        } else {
            self.parse_ram().await
        };

        let ram = match ram {
            Ok(r) => Some(r),
            Err(e) => {
                debug!("Failed to parse RAM: {}", e);
                None
            }
        };

        Sdrr { flash, ram }
    }

    /// Parse SDRR metadata from the firmware.
    ///
    /// This method reads and parses all structural information from the firmware,
    /// including headers, version info, pin configurations, and ROM set descriptors.
    /// ROM image data is NOT loaded - only pointers to where it exists in the firmware.
    ///
    /// # What gets parsed
    ///
    /// - SDRR header with version and build information
    /// - Pin mapping configuration for STM32F4 GPIO
    /// - ROM set headers with serving algorithms
    /// - ROM information (type, CS line configuration)
    /// - String data (build date, hardware revision, ROM filenames)
    ///
    /// # Error handling
    ///
    /// The parser attempts to continue parsing even when encountering errors in
    /// non-critical sections. Failed sections are recorded in [`SdrrInfo::parse_errors`]
    /// while their fields are set to `None`.
    ///
    /// # Returns
    ///
    /// Returns `Ok(SdrrInfo)` if the header was found and core fields parsed successfully.
    /// Returns `Err` if:
    /// - SDRR magic bytes not found at expected location
    /// - Version is newer than this parser supports
    /// - Critical header fields are corrupted
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn test() -> Result<(), Box<dyn std::error::Error>> {
    /// # use sdrr_fw_parser::Parser;
    /// # use airfrog_rpc::io::Reader;
    /// # struct MyReader;
    /// # impl MyReader {
    /// #     fn new() -> Self { MyReader }
    /// # }
    /// # impl Reader for MyReader {
    /// #     type Error = std::io::Error;
    /// #     async fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), Self::Error> { Ok(()) }
    /// #     fn update_base_address(&mut self, base_address: u32) {}
    /// # }
    /// # let mut reader = MyReader::new();
    /// let mut parser = Parser::new(&mut reader);
    /// match parser.parse_flash().await {
    ///     Ok(info) => {
    ///         println!("Parsed SDRR v{}.{}.{}",
    ///                  info.major_version,
    ///                  info.minor_version,
    ///                  info.patch_version);
    ///         if !info.parse_errors.is_empty() {
    ///             println!("Encountered {} non-fatal errors", info.parse_errors.len());
    ///         }
    ///     }
    ///     Err(e) => eprintln!("Failed to parse: {}", e),
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// # }
    /// ```
    pub async fn parse_flash(&mut self) -> Result<SdrrInfo, String> {
        // Parse and validate header using the helper
        let mut header = self.retrieve_header().await?;

        // Get firmware version
        let version = FirmwareVersion::new(
            header.major_version,
            header.minor_version,
            header.patch_version,
            header.build_number,
        );

        // Update our base address based on the header - before this we don't
        // need to have the correct base_flash_address set.  Base RAM is the
        // same.
        if header.stm_line == McuLine::Rp2350 {
            self.base_flash_address = 0x10000000; // RP2350 flash base address
            self.reader.update_base_address(self.base_flash_address);
        }

        let mut parse_errors = Vec::new();

        // Parse strings with error collection
        let build_date = match self.read_string_at_ptr(header.build_date_ptr).await {
            Ok(s) => Some(s),
            Err(e) => {
                parse_errors.push(ParseError::new("Build Date", e));
                None
            }
        };

        let hw_rev = match self.read_string_at_ptr(header.hw_rev_ptr).await {
            Ok(s) => Some(s),
            Err(e) => {
                parse_errors.push(ParseError::new("Hardware Revision", e));
                None
            }
        };

        // Parse extra info
        let (extra_info, runtime_info_ptr) = match parsing::read_extra_info(
            self.reader,
            header.extra_ptr,
            self.base_flash_address,
            &version,
        )
        .await
        {
            Ok(info) => {
                let runtime_info_ptr = info.runtime_info_ptr;
                (Some(info), runtime_info_ptr)
            }
            Err(e) => {
                parse_errors.push(ParseError::new("Extra Info", e));
                (None, 0xFFFF_FFFF_u32)
            }
        };

        // If necessary, parse OneRomMetadataHeader
        let metadata_present = if header.major_version > 0 || header.minor_version > 4 {
            // OneRomMetadataHeader should be parsed for 0.5.0 and above.  Its
            // pointer is actually stored in rom_sets_ptr.
            // However, it is valid to build a firmware file without metadata even
            // beyond 0.5.0 - so we do not report that as an error.
            let metadata_ptr = header.rom_sets_ptr;
            header.rom_set_count = 0;
            header.rom_sets_ptr = 0;

            match parsing::read_one_rom_metadata_header_info(
                self.reader,
                metadata_ptr,
                self.base_flash_address,
            )
            .await
            {
                Ok(metadata) => {
                    if metadata.version == 1 {
                        if metadata.rom_set_count == 0 {
                            true
                        } else if metadata.rom_sets_ptr > 0 {
                            // Update main header's ROM set info.
                            header.rom_set_count = metadata.rom_set_count;
                            header.rom_sets_ptr = metadata.rom_sets_ptr;
                            true
                        } else {
                            parse_errors.push(ParseError::new(
                                "Metadata",
                                format!(
                                    "Metadata: Invalid ROM sets pointer {}",
                                    metadata.rom_sets_ptr
                                ),
                            ));
                            false
                        }
                    } else {
                        parse_errors.push(ParseError::new(
                            "Metadata",
                            format!("Metadata: Invalid version {}", metadata.version),
                        ));
                        false
                    }
                }
                Err(_) => {
                    // Set ROM set info to 0
                    false
                }
            }
        } else {
            // No metadata
            false
        };

        // Parse ROM sets with error collection
        let rom_sets =
            match parsing::read_rom_sets(self.reader, &header, self.base_flash_address, &version)
                .await
            {
                Ok(sets) => {
                    if sets.len() != header.rom_set_count as usize {
                        parse_errors.push(ParseError::new(
                            "Rom Sets",
                            format!(
                                "Incorrect number of ROM sets found: Found {}, expected {}",
                                sets.len(),
                                header.rom_set_count
                            ),
                        ));
                    }
                    sets
                }
                Err(e) => {
                    parse_errors.push(ParseError::new("ROM Sets", e));
                    Vec::new()
                }
            };

        // Parse pins
        let pins =
            match parsing::read_pins(self.reader, header.pins_ptr, self.base_flash_address).await {
                Ok(p) => Some(p),
                Err(e) => {
                    parse_errors.push(ParseError::new("Pins", e));
                    None
                }
            };

        // Try to decode board, model and MCU variant from hw_rev
        let board = hw_rev.as_ref().and_then(|s| Board::try_from_str(s));
        let model = board.as_ref().map(|b| b.model());
        let mcu_lookup_str = format!(
            "{}{}",
            header.stm_line.chip_suffix(),
            header.stm_storage.stm32_suffix()
        );
        let mcu_variant = McuVariant::try_from_str(&mcu_lookup_str);
        if board.is_none() {
            parse_errors.push(ParseError::new(
                "Board",
                format!(
                    "Could not decode board from hardware revision string: {:?}",
                    hw_rev
                ),
            ));
        }
        if mcu_variant.is_none() {
            parse_errors.push(ParseError::new(
                "MCU Variant",
                format!(
                    "Could not decode MCU variant from string: {}",
                    mcu_lookup_str
                ),
            ));
        }

        Ok(SdrrInfo {
            major_version: header.major_version,
            minor_version: header.minor_version,
            patch_version: header.patch_version,
            build_number: header.build_number,
            build_date,
            commit: header.commit,
            hw_rev,
            stm_line: header.stm_line,
            stm_storage: header.stm_storage,
            freq: header.freq,
            overclock: header.overclock != 0,
            swd_enabled: header.swd_enabled != 0,
            preload_image_to_ram: header.preload_image_to_ram != 0,
            bootloader_capable: header.bootloader_capable != 0,
            status_led_enabled: header.status_led_enabled != 0,
            boot_logging_enabled: header.boot_logging_enabled != 0,
            mco_enabled: header.mco_enabled != 0,
            rom_set_count: header.rom_set_count,
            count_rom_access: header.count_rom_access != 0,
            rom_sets,
            pins,
            boot_config: header.boot_config,
            parse_errors,
            extra_info,
            metadata_present,
            version,
            board,
            model,
            mcu_variant,
            runtime_info_ptr,
        })
    }

    async fn parse_ram_from_runtime_info(
        &mut self,
        runtime_info: SdrrRuntimeInfoHeader,
    ) -> Result<SdrrRuntimeInfo, String> {
        Ok(SdrrRuntimeInfo {
            image_sel: runtime_info.image_sel,
            rom_set_index: runtime_info.rom_set_index,
            count_rom_access: runtime_info.count_rom_access,
            last_parsed_access_count: runtime_info.access_count,
            account_count_address: self.base_ram_address
                + SdrrRuntimeInfoHeader::access_count_offset() as u32,
            rom_table_address: runtime_info.rom_table_ptr,
            rom_table_size: runtime_info.rom_table_size,
            overclock_enabled: None,
            status_led_enabled: None,
            swd_enabled: None,
            fire_vreg: None,
            ice_freq_mhz: None,
            fire_freq_mhz: None,
            sysclk_mhz: None,
            fire_serve_mode: None,
            bit_mode: None,
            rom_dma_copy: None,
            num_data_pins: None,
            force_16_bit: None,
            peri_en: None,
            limp_mode: None,
        })
    }

    async fn parse_ram(&mut self) -> Result<SdrrRuntimeInfo, String> {
        // Parse and validate runtime info using the helper
        let runtime_info = self.retrieve_runtime_header().await?;
        self.parse_ram_from_runtime_info(runtime_info).await
    }

    async fn parse_ram_from_info(&mut self, info: &SdrrInfo) -> Result<SdrrRuntimeInfo, String> {
        // Parse and validate runtime info using the helper
        let runtime_info = self.retrieve_runtime_header_from_info(info).await?;
        self.parse_ram_from_runtime_info(runtime_info).await
    }

    async fn read_string_at_ptr(&mut self, ptr: u32) -> Result<String, String> {
        if ptr < self.base_flash_address {
            return Err(format!("Invalid pointer: 0x{:08X}", ptr));
        }

        read_string_at_ptr(self.reader, ptr).await
    }
}

/// Error information for non-fatal parsing failures.
///
/// When parsing SDRR firmware, some sections may fail to parse due to corruption,
/// invalid pointers, or other issues. Rather than failing the entire parse operation,
/// these errors are collected and reported while the parser continues with other
/// sections.
///
/// # Examples
///
/// ```rust
/// # use sdrr_fw_parser::ParseError;
/// let error = ParseError {
///     field: "build_date".to_string(),
///     reason: "Invalid pointer: 0xFFFFFFFF".to_string(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParseError {
    /// The field or structure that failed to parse.
    ///
    /// Examples:
    /// - `"build_date"` - Build date string
    /// - `"hw_rev"` - Hardware revision string  
    /// - `"rom_set[0]"` - First ROM set
    /// - `"rom_set[1].roms[2]"` - Third ROM in second ROM set
    /// - `"pins"` - Pin configuration structure
    pub field: String,

    /// Human-readable description of why parsing failed.
    ///
    /// Examples:
    /// - `"Invalid pointer: 0xFFFFFFFF"`
    /// - `"String not null-terminated within bounds"`
    /// - `"ROM data extends past end of firmware"`
    /// - `"Unsupported ROM type value: 255"`
    pub reason: String,
}

impl ParseError {
    /// Create a new parse error.
    pub fn new(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}

async fn read_string_at_ptr<R: Reader>(reader: &mut R, ptr: u32) -> Result<String, String> {
    // Read in chunks to find null terminator
    let mut result = Vec::new();
    let mut addr = ptr;
    let mut buf = [0u8; 64];

    loop {
        let chunk_size = buf.len().min(1024 - result.len()); // Limit total size
        reader
            .read(addr, &mut buf[..chunk_size])
            .await
            .map_err(|_| format!("Failed to read string at 0x{ptr:08X}"))?;

        if let Some(null_pos) = buf[..chunk_size].iter().position(|&b| b == 0) {
            result.extend_from_slice(&buf[..null_pos]);
            break;
        }

        result.extend_from_slice(&buf[..chunk_size]);
        addr += chunk_size as u32;

        if result.len() >= 1024 {
            return Err("String too long (>1KB)".into());
        }
    }

    String::from_utf8(result).map_err(|_| "Invalid UTF-8 string".into())
}

async fn read_str_at_ptr<R: Reader>(reader: &mut R, len: u32, ptr: u32) -> Result<String, String> {
    if len > 1024 {
        return Err("String too long (>1KB)".into());
    } else if len == 0 {
        return Ok(String::new());
    }

    let mut buf = vec![0u8; len as usize];
    reader
        .read(ptr, &mut buf)
        .await
        .map_err(|_| format!("Failed to read string at 0x{ptr:08X}"))?;

    String::from_utf8(buf).map_err(|_| "Invalid UTF-8 string".into())
}

pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
