// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! onerom-fw-parser
//!
//! Contains code and internal structures for parsing the SDRR firmware

use deku::prelude::*;
use onerom_config::fw::FirmwareVersion;
use onerom_gen::firmware::{FirmwareConfig, ServeAlgParams};
use static_assertions::const_assert_eq;

use crate::Reader;
use crate::types::{BitMode, FireServeMode, FireVreg, LimpMode};
use crate::{MAX_VERSION_MAJOR, MAX_VERSION_MINOR, MAX_VERSION_PATCH};
use crate::{McuLine, McuStorage, SdrrCsState, SdrrRomType, SdrrServe};
use crate::{SdrrExtraInfo, SdrrMcuPort, SdrrPins, SdrrRomInfo, SdrrRomSet};

#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec, vec::Vec};

// Maximum length of strings and bits of strings read from firmware
const MAX_STRING_LEN: usize = 1024;
const STRING_READ_CHUNK_SIZE: usize = 64;

/// How many bytes to read for the full runtime info buffer.
///
/// Covers all versioned extensions through V0.6.7 (57 bytes) with a small
/// margin.  Does not include `piorom_config` which starts at offset 60.
pub(crate) const SDRR_RUNTIME_BUF_SIZE: usize = 64;

#[derive(Debug, DekuRead, DekuWrite)]
#[deku(endian = "little", magic = b"sdrr")]
// Used internally to construct [`SdrrRuntimeInfo`]
pub(crate) struct SdrrRuntimeInfoHeader {
    pub runtime_info_size: u8,
    pub image_sel: u8,
    pub rom_set_index: u8,
    pub count_rom_access: u8,
    #[deku(endian = "little")]
    pub access_count: u32,
    #[deku(endian = "little")]
    pub rom_table_ptr: u32,
    #[deku(endian = "little")]
    pub rom_table_size: u32,
}

#[derive(Debug, DekuRead, DekuWrite)]
#[deku(endian = "little")]
// Used internally to construct [`SdrrRuntimeInfo`]
pub(crate) struct SdrrRuntimeInfoHeaderVunknown {
    #[deku(endian = "little")]
    pub stm32_bootloader_entry: u32,
}

#[derive(Debug, DekuRead, DekuWrite)]
// Used internally to construct [`SdrrRuntimeInfo`]
pub(crate) struct SdrrRuntimeInfoHeaderV0_6_0 {
    pub overclock_enabled: u8,
    pub status_led_enabled: u8,
    pub swd_enabled: u8,
    pub fire_vreg: FireVreg,
    #[deku(endian = "little")]
    pub ice_freq: u16,
    #[deku(endian = "little")]
    pub fire_freq: u16,
    #[deku(endian = "little")]
    pub sysclk_mhz: u16,
}

#[derive(Debug, DekuRead, DekuWrite)]
// Used internally to construct [`SdrrRuntimeInfo`]
pub(crate) struct SdrrRuntimeInfoHeaderV0_6_2 {
    pub fire_serve_mode: FireServeMode,
    pub bit_mode: BitMode,
}

#[derive(Debug, DekuRead, DekuWrite)]
#[deku(endian = "little")]
// Used internally to construct [`SdrrRuntimeInfo`]
pub(crate) struct SdrrRuntimeInfoHeaderV0_6_3 {
    pub rom_dma_copy: u8,
    pub num_data_pins: u8,
    pub force_16_bit: u8,
}

#[derive(Debug, DekuRead, DekuWrite)]
// Used internally to construct [`SdrrRuntimeInfo`]
pub(crate) struct SdrrRuntimeInfoHeaderV0_6_7 {
    pub peri_en: u8,
    #[deku(endian = "little")]
    pub system_plugin_context_ptr: u32,
    #[deku(endian = "little")]
    pub user_plugin_context_ptr: u32,
    #[deku(endian = "little")]
    pub timer0_irq_0_handler_ptr: u32,
    #[deku(endian = "little")]
    pub usbctrl_irq_handler_ptr: u32,
    pub limp_mode: LimpMode,
}

impl SdrrRuntimeInfoHeader {
    // Cannot assert this against SdrrInfoHeader size, as contains Vecs, which
    // increase its size.
    const SDRR_RUNTIME_INFO_HEADER_SIZE: usize = 20;

    // Offset of the access count field in the runtime info header
    const ACCESS_COUNT_OFFSET: usize = 8;

    pub(crate) const fn size() -> usize {
        // Rust struct size ignores the magic bytes
        const_assert_eq!(
            core::mem::size_of::<SdrrRuntimeInfoHeader>(),
            SdrrRuntimeInfoHeader::SDRR_RUNTIME_INFO_HEADER_SIZE - 4
        );
        Self::SDRR_RUNTIME_INFO_HEADER_SIZE
    }

    pub(crate) const fn access_count_offset() -> usize {
        Self::ACCESS_COUNT_OFFSET
    }
}

// ---------------------------------------------------------------------------
// Versioned runtime field offsets (from sdrr_runtime_info_t in config_base.h)
// ---------------------------------------------------------------------------
//
// Offset 0  - 19: base header (magic + fields)
// Offset 20 - 23: bootloader_entry (Vunknown)
// Offset 24 - 33: V0.6.0 fields (overclock, status_led, swd, fire_vreg, ice_freq, fire_freq, sysclk_mhz)
// Offset 34 - 35: V0.6.2 fields (fire_serve_mode, bit_mode)
// Offset 36 - 38: V0.6.3 fields (rom_dma_copy, num_data_pins, force_16_bit)
// Offset 39 - 56: V0.6.7 fields (peri_en, plugin ptrs x4, limp_mode)
// Offset 57 - 59: pad[3]
// Offset 60+    : piorom_config (not parsed here)

const V0_6_0_OFFSET: usize = 24;
const V0_6_0_END: usize = 34;
const V0_6_2_OFFSET: usize = 34;
const V0_6_2_END: usize = 36;
const V0_6_3_OFFSET: usize = 36;
const V0_6_3_END: usize = 39;
const V0_6_7_OFFSET: usize = 39;
const V0_6_7_END: usize = 57;

/// Versioned fields parsed from the runtime info buffer beyond the base header.
pub(crate) struct RuntimeVersionedFields {
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

impl RuntimeVersionedFields {
    fn none() -> Self {
        Self {
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
        }
    }
}

/// Parse versioned runtime fields from the full raw runtime info buffer.
///
/// Uses `runtime_info_size` (byte 4 of the buffer) as the boundary rather
/// than the firmware version, so this works even when flash info is not
/// available.
pub(crate) fn parse_runtime_versioned_fields(data: &[u8]) -> RuntimeVersionedFields {
    let mut f = RuntimeVersionedFields::none();

    if data.len() < SdrrRuntimeInfoHeader::size() {
        return f;
    }

    // runtime_info_size is at offset 4 in the raw buffer (after the 4-byte magic).
    let runtime_info_size = data[4] as usize;

    // V0.6.0: overclock, status_led, swd, fire_vreg, ice_freq, fire_freq, sysclk_mhz
    #[allow(clippy::collapsible_if)]
    if runtime_info_size >= V0_6_0_END && data.len() >= V0_6_0_END {
        if let Ok((_, v)) = SdrrRuntimeInfoHeaderV0_6_0::from_bytes((&data[V0_6_0_OFFSET..], 0)) {
            f.overclock_enabled = Some(v.overclock_enabled != 0);
            f.status_led_enabled = Some(v.status_led_enabled != 0);
            f.swd_enabled = Some(v.swd_enabled != 0);
            f.fire_vreg = Some(v.fire_vreg);
            f.ice_freq_mhz = Some(v.ice_freq);
            f.fire_freq_mhz = Some(v.fire_freq);
            f.sysclk_mhz = Some(v.sysclk_mhz);
        }
    }

    // V0.6.2: fire_serve_mode, bit_mode
    #[allow(clippy::collapsible_if)]
    if runtime_info_size >= V0_6_2_END && data.len() >= V0_6_2_END {
        if let Ok((_, v)) = SdrrRuntimeInfoHeaderV0_6_2::from_bytes((&data[V0_6_2_OFFSET..], 0)) {
            f.fire_serve_mode = Some(v.fire_serve_mode);
            f.bit_mode = Some(v.bit_mode);
        }
    }

    // V0.6.3: rom_dma_copy, num_data_pins, force_16_bit
    #[allow(clippy::collapsible_if)]
    if runtime_info_size >= V0_6_3_END && data.len() >= V0_6_3_END {
        if let Ok((_, v)) = SdrrRuntimeInfoHeaderV0_6_3::from_bytes((&data[V0_6_3_OFFSET..], 0)) {
            f.rom_dma_copy = Some(v.rom_dma_copy != 0);
            f.num_data_pins = Some(v.num_data_pins);
            f.force_16_bit = Some(v.force_16_bit != 0);
        }
    }

    // V0.6.7: peri_en, plugin context ptrs (skipped), limp_mode
    #[allow(clippy::collapsible_if)]
    if runtime_info_size >= V0_6_7_END && data.len() >= V0_6_7_END {
        if let Ok((_, v)) = SdrrRuntimeInfoHeaderV0_6_7::from_bytes((&data[V0_6_7_OFFSET..], 0)) {
            f.peri_en = Some(v.peri_en);
            f.limp_mode = Some(v.limp_mode);
        }
    }

    f
}

#[derive(Debug, DekuRead, DekuWrite)]
#[deku(endian = "little", magic = b"SDRR")]
// Used internally to construct [`SdrrInfo`]
//
// Reflects `sdrr_info_t` from `sdrr/include/config_base.h`
pub(crate) struct SdrrInfoHeader {
    #[deku(endian = "little")]
    pub major_version: u16,
    #[deku(endian = "little")]
    pub minor_version: u16,
    #[deku(endian = "little")]
    pub patch_version: u16,
    #[deku(endian = "little")]
    pub build_number: u16,
    #[deku(endian = "little")]
    pub build_date_ptr: u32,
    #[deku(bytes = "8")]
    pub commit: [u8; 8],
    #[deku(endian = "little")]
    pub hw_rev_ptr: u32,
    pub stm_line: McuLine,
    pub stm_storage: McuStorage,
    #[deku(endian = "little")]
    pub freq: u16,
    pub overclock: u8,
    pub swd_enabled: u8,
    pub preload_image_to_ram: u8,
    pub bootloader_capable: u8,
    pub status_led_enabled: u8,
    pub boot_logging_enabled: u8,
    pub mco_enabled: u8,
    pub rom_set_count: u8, // Deprecated as of v0.5.0
    pub count_rom_access: u8,
    #[deku(pad_bytes_before = "1")]
    #[deku(endian = "little")]
    pub rom_sets_ptr: u32, // Changed as of v0.5.0 to be metadata pointer
    #[deku(endian = "little")]
    pub pins_ptr: u32,
    #[deku(bytes = "4")]
    pub boot_config: [u8; 4],
    pub extra_ptr: u32,
    pub _post: [u8; 4],
}

impl SdrrInfoHeader {
    // Cannot assert this against SdrrInfoHeader size, as contains Vecs, which
    // increase its size.
    const SDRR_INFO_HEADER_SIZE: usize = 64;

    pub(crate) const fn size() -> usize {
        Self::SDRR_INFO_HEADER_SIZE
    }

    // Indicates whether filenames present on ROM info structures
    // Always true for v0.5.1 and later
    // For earlier versions, depends on boot_logging_enabled flag.
    // Note however, even if included, they may be null pointers, or 0xFFFFFFFF
    // pointers
    pub(crate) fn filenames_enabled(&self) -> bool {
        if self.major_version > 0
            || self.minor_version > 5
            || (self.minor_version == 5 && self.patch_version >= 1)
        {
            true
        } else {
            self.boot_logging_enabled != 0
        }
    }
}

// Used internally to construct SdrrExtraInfo
//
// Reflects `sdrr_extra_info_t` from `sdrr/include/config_base.h`
#[derive(Debug, DekuRead, DekuWrite)]
pub(crate) struct SdrrExtraInfoHeader {
    #[deku(endian = "little")]
    pub rtt_ptr: u32,

    pub usb_dfu: u8,
    pub usb_port: u8,
    pub vbus_pin: u8,
    pub fire_pio_default: u8, // Changed from reserved in 0.6.0

    pub runtime_info_ptr: u32,

    pub _post: [u8; 244],
}

impl SdrrExtraInfoHeader {
    // Cannot assert this against SdrrExtraInfoHeader size, as contains Vecs, which
    // increase its size.
    const EXTRA_INFO_HEADER_SIZE: usize = 256;

    pub(crate) const fn size() -> usize {
        const_assert_eq!(
            core::mem::size_of::<SdrrExtraInfoHeader>(),
            SdrrExtraInfoHeader::EXTRA_INFO_HEADER_SIZE
        );
        Self::EXTRA_INFO_HEADER_SIZE
    }
}

#[derive(Debug, DekuRead, DekuWrite)]
#[deku(endian = "little", magic = b"ONEROM_METADATA\0")]
// Used internally to construct OneRomMetadataHeader
//
// Reflects onerom_metadata_header_t from `sdrr/include/config_base.h`
pub(crate) struct OneRomMetadataHeaderInternal {
    #[deku(endian = "little")]
    pub version: u32,
    #[deku(pad_bytes_after = "3")]
    pub rom_set_count: u8,
    #[deku(endian = "little")]
    pub rom_sets_ptr: u32,
    pub _reserved: [u8; 228],
}

impl OneRomMetadataHeaderInternal {
    // Cannot assert this against SdrrInfoHeader size, as contains Vecs, which
    // increase its size.
    const ONE_ROM_METADATA_HEADER_SIZE: usize = 256;

    pub(crate) const fn size() -> usize {
        Self::ONE_ROM_METADATA_HEADER_SIZE
    }
}

// Information about a specific ROM set
//
// Reflects `sdrr_rom_set_info_t` from `sdrr/include/config_base.h`
//
// Used internally to construct [`SdrrRomSet`]
#[derive(Debug, DekuRead, DekuWrite)]
pub(crate) struct SdrrRomSetHeader {
    #[deku(endian = "little")]
    pub data_ptr: u32,
    #[deku(endian = "little")]
    pub size: u32,
    #[deku(endian = "little")]
    pub roms_ptr: u32,
    pub rom_count: u8,
    pub serve: SdrrServe,
    pub multi_rom_cs1_state: SdrrCsState,
    pub extra_info: u8,
    // These fields only exist when extra_info == 1, from 0.6.0 onwards
    #[deku(cond = "*extra_info == 1", endian = "little")]
    pub serve_config_ptr: Option<u32>,
    #[deku(cond = "*extra_info == 1", endian = "little")]
    pub firmware_overrides_ptr: Option<u32>,
    #[deku(cond = "*extra_info == 1", endian = "little")]
    pub pad2: Option<[u8; 40]>,
}

impl SdrrRomSetHeader {
    const BASE_SIZE: usize = 16; // size when extra_info == 0
    const EXTRA_SIZE: usize = 48; // extra size when extra_info == 1

    pub(crate) const fn base_size() -> usize {
        Self::BASE_SIZE
    }

    pub(crate) const fn extra_size() -> usize {
        Self::EXTRA_SIZE
    }
}

// Contains information about a specific ROM image
//
// This version is used when the BOOT_LOGGING is not defined in the C code
//
// Reflects `sdrr_rom_info_t` from `sdrr/include/config_base.h`
//
// Only used internally
#[derive(Debug, DekuRead, DekuWrite)]
struct SdrrRomInfoBasic {
    pub rom_type: SdrrRomType,
    pub cs1_state: SdrrCsState,
    pub cs2_state: SdrrCsState,
    pub cs3_state: SdrrCsState,
}

impl SdrrRomInfoBasic {
    const ROM_INFO_BASIC_SIZE: usize = 4;
    pub(crate) fn size() -> usize {
        const_assert_eq!(
            core::mem::size_of::<SdrrRomInfoBasic>(),
            SdrrRomInfoBasic::ROM_INFO_BASIC_SIZE
        );
        Self::ROM_INFO_BASIC_SIZE
    }
}

// Contains information about a specific ROM image
//
// This version is used when the BOOT_LOGGING is defined in the C code
//
// Reflects `sdrr_rom_info_t` from `sdrr/include/config_base.h`
//
// Only used internally
#[derive(Debug, DekuRead, DekuWrite)]
struct SdrrRomInfoWithLogging {
    pub rom_type: SdrrRomType,
    pub cs1_state: SdrrCsState,
    pub cs2_state: SdrrCsState,
    pub cs3_state: SdrrCsState,
    #[deku(endian = "little")]
    pub filename_ptr: u32,
}

impl SdrrRomInfoWithLogging {
    const ROM_INFO_WITH_LOGGING_SIZE: usize = 8;
    pub(crate) fn size() -> usize {
        const_assert_eq!(
            core::mem::size_of::<SdrrRomInfoWithLogging>(),
            SdrrRomInfoWithLogging::ROM_INFO_WITH_LOGGING_SIZE
        );
        Self::ROM_INFO_WITH_LOGGING_SIZE
    }
}

/// Parse and validate runtime information from buffer
pub(crate) fn parse_and_validate_runtime_info(
    data: &[u8],
) -> Result<SdrrRuntimeInfoHeader, String> {
    if data.len() < SdrrRuntimeInfoHeader::size() {
        return Err("Runtime info data too small".into());
    }

    let (_, header) = SdrrRuntimeInfoHeader::from_bytes((data, 0))
        .map_err(|e| format!("Failed to parse runtime info header: {}", e))?;

    if header.runtime_info_size < SdrrRuntimeInfoHeader::size() as u8 {
        return Err(format!(
            "Invalid runtime info size: {} < {}",
            header.runtime_info_size,
            SdrrRuntimeInfoHeader::size()
        ));
    }

    Ok(header)
}

/// Parse and validate SDRR header from buffer
pub(crate) fn parse_and_validate_header(data: &[u8]) -> Result<SdrrInfoHeader, String> {
    if data.len() < SdrrInfoHeader::size() {
        return Err("Header data too small".into());
    }

    let (_, mut header) = SdrrInfoHeader::from_bytes((data, 0))
        .map_err(|e| format!("Failed to parse header: {}", e))?;

    // Validate version
    if header.major_version > MAX_VERSION_MAJOR
        || (header.major_version == MAX_VERSION_MAJOR && header.minor_version > MAX_VERSION_MINOR)
        || (header.major_version == MAX_VERSION_MAJOR
            && header.minor_version == MAX_VERSION_MINOR
            && header.patch_version > MAX_VERSION_PATCH)
    {
        return Err(format!(
            "One ROM firmware version v{}.{}.{} unsupported - max version v{}.{}.{}",
            header.major_version,
            header.minor_version,
            header.patch_version,
            MAX_VERSION_MAJOR,
            MAX_VERSION_MINOR,
            MAX_VERSION_PATCH
        ));
    }

    if header.major_version == 0 && header.minor_version < 4 {
        // Extra info and _post fields are invalid - re-initialize them.
        header.extra_ptr = 0xFFFFFFFF;
        header._post = [0xFF; 4];
    }

    Ok(header)
}

/// Read a null-terminated string from the given pointer
pub(crate) async fn read_string_at_ptr<R: Reader>(
    reader: &mut R,
    ptr: u32,
    base_addr: u32,
) -> Result<String, String> {
    if ptr < base_addr {
        return Err(format!("Invalid pointer: 0x{:08X}", ptr));
    }

    let mut result = Vec::new();
    let mut addr = ptr;
    let mut buf = [0u8; STRING_READ_CHUNK_SIZE];

    loop {
        let chunk_size = buf.len().min(MAX_STRING_LEN - result.len());
        reader
            .read(addr, &mut buf[..chunk_size])
            .await
            .map_err(|_| format!("Failed to read string at 0x{:08X}", ptr))?;

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

pub(crate) async fn read_extra_info<R: Reader>(
    reader: &mut R,
    ptr: u32,
    base_addr: u32,
    version: &FirmwareVersion,
) -> Result<SdrrExtraInfo, String> {
    if ptr < base_addr {
        return Err(format!("ROM Extra Info invalid pointer: {ptr:#010X}"));
    }

    let mut buf = [0u8; SdrrExtraInfoHeader::size()];
    reader
        .read(ptr, &mut buf)
        .await
        .map_err(|_| format!("Failed to read extra info at 0x{:08X}", ptr))?;

    let (_, header) = SdrrExtraInfoHeader::from_bytes((&buf, 0))
        .map_err(|e| format!("Failed to parse extra info: {}", e))?;

    let usb_port = SdrrMcuPort::from(header.usb_port);
    let vbus_pin = header.vbus_pin;

    const MIN_FIRE_PIO_DEFAULT_VERSION: FirmwareVersion = FirmwareVersion::new(0, 6, 0, 0);
    let fire_pio_default = if *version >= MIN_FIRE_PIO_DEFAULT_VERSION {
        Some(header.fire_pio_default != 0)
    } else {
        None
    };

    Ok(SdrrExtraInfo {
        rtt_ptr: header.rtt_ptr,
        usb_dfu: header.usb_dfu != 0,
        usb_port,
        vbus_pin,
        fire_pio_default,
        runtime_info_ptr: header.runtime_info_ptr,
    })
}

/// Read OneRomMetadataHeaderInfo
pub(crate) async fn read_one_rom_metadata_header_info<R: Reader>(
    reader: &mut R,
    ptr: u32,
    base_addr: u32,
) -> Result<OneRomMetadataHeaderInternal, String> {
    if ptr < base_addr {
        return Err(format!(
            "Failed to read One ROM metadata header as pointer invalid {ptr:#010X}"
        ));
    }

    let mut buf = [0u8; OneRomMetadataHeaderInternal::size()];
    reader
        .read(ptr, &mut buf)
        .await
        .map_err(|_| format!("Failed to read One ROM metadata header at {ptr:#010X}"))?;

    let (_, metadata) = OneRomMetadataHeaderInternal::from_bytes((&buf, 0))
        .map_err(|e| format!("Failed to parse One ROM metadata header: {e}"))?;

    Ok(metadata)
}

/// Read ROM sets from firmware
pub(crate) async fn read_rom_sets<R: Reader>(
    reader: &mut R,
    info_header: &SdrrInfoHeader,
    base_addr: u32,
    version: &FirmwareVersion,
) -> Result<Vec<SdrrRomSet>, String> {
    let ptr = info_header.rom_sets_ptr;
    let count = info_header.rom_set_count;

    if count == 0 {
        return Ok(Vec::new());
    }

    if ptr < base_addr {
        return Err(format!(
            "ROM set pointer {ptr:#010X} is below base address {base_addr:#010X}"
        ));
    }

    let mut rom_sets = Vec::with_capacity(count as usize);

    let mut current_offset = 0u32;
    const MAX_HEADER_SIZE: usize = SdrrRomSetHeader::base_size() + SdrrRomSetHeader::extra_size();

    for i in 0..count {
        let header_addr = ptr + current_offset;

        // Read ROM set header
        let mut header_buf = [0u8; MAX_HEADER_SIZE];
        reader
            .read(header_addr, &mut header_buf)
            .await
            .map_err(|_| format!("Failed to read ROM set header {i}"))?;

        let (_, header) = SdrrRomSetHeader::from_bytes((&header_buf, 0))
            .map_err(|e| format!("Failed to parse ROM set header {i}: {e}"))?;
        current_offset += SdrrRomSetHeader::base_size() as u32;
        if *version >= FirmwareVersion::new(0, 6, 0, 0) {
            // There are three scenarios here:
            // - FW built with Studio v0.1.8 or later - extra_info == 1, rom_set size includes extra fields
            // - FW built with Studio v0.1.7 or earlier - extra_info == 0, rom_set size excludes extra fields
            // - FW built from source - extra_info == 0, rom_set size _includes_ extra fields
            // It is not possible to distinguish between the last two cases, so we assume the first,
            // as very few people build from source.
            if header.extra_info == 1 {
                current_offset += SdrrRomSetHeader::extra_size() as u32;
            }
        } else if *version >= FirmwareVersion::new(0, 6, 1, 0) {
            current_offset += SdrrRomSetHeader::extra_size() as u32;
        }

        // Read serve_config if present
        let serve_config = if let Some(serve_config_ptr) = header.serve_config_ptr
            && serve_config_ptr != 0
            && serve_config_ptr != 0xFFFF_FFFF
        {
            let mut buf = [0u8; 64];
            reader
                .read(serve_config_ptr, &mut buf)
                .await
                .map_err(|_| format!("Failed to read serve_config for ROM set {}", i))?;
            Some(buf.to_vec())
        } else {
            None
        };

        // Read and deserialize firmware_overrides if present
        let firmware_overrides = if let Some(fw_ptr) = header.firmware_overrides_ptr
            && fw_ptr != 0
            && fw_ptr != 0xFFFF_FFFF
        {
            let mut buf = [0u8; 64];
            reader
                .read(fw_ptr, &mut buf)
                .await
                .map_err(|_| format!("Failed to read firmware_overrides for ROM set {}", i))?;

            let mut fw_config = FirmwareConfig::from_bytes(&buf).map_err(|e| {
                format!(
                    "Failed to parse firmware_overrides for ROM set {}: {}",
                    i, e
                )
            })?;
            fw_config.serve_alg_params = Some(ServeAlgParams {
                params: serve_config.clone().unwrap_or_default(),
            });
            Some(fw_config)
        } else {
            None
        };

        // Read ROM infos
        let roms = read_rom_infos(reader, info_header, &header, base_addr).await?;

        // Note: We don't read the ROM data itself - just store where it is
        rom_sets.push(SdrrRomSet {
            data_ptr: header.data_ptr, // Store pointer, not data
            size: header.size,
            roms,
            rom_count: header.rom_count,
            serve: header.serve,
            multi_rom_cs1_state: header.multi_rom_cs1_state,
            firmware_overrides,
        });
    }

    Ok(rom_sets)
}

// Read ROM info structures
async fn read_rom_infos<R: Reader>(
    reader: &mut R,
    info_header: &SdrrInfoHeader,
    rom_set_header: &SdrrRomSetHeader,
    base_addr: u32,
) -> Result<Vec<SdrrRomInfo>, String> {
    let ptr = rom_set_header.roms_ptr;
    let count = rom_set_header.rom_count;

    if count == 0 {
        return Ok(Vec::new());
    }

    if ptr < base_addr {
        return Err(format!(
            "ROM infos pointer {ptr:#010X} is below base address {base_addr:#010X}"
        ));
    }

    let mut rom_infos = Vec::with_capacity(count as usize);

    for i in 0..count {
        // Read pointer to ROM info
        let ptr_addr = ptr + (i as u32 * core::mem::size_of::<u32>() as u32);
        let mut ptr_buf = [0u8; core::mem::size_of::<u32>()];
        reader
            .read(ptr_addr, &mut ptr_buf)
            .await
            .map_err(|_| format!("Failed to read ROM info pointer {}", i))?;

        let rom_info_ptr = u32::from_le_bytes(ptr_buf);

        // Read the ROM info itself
        let info_size = if info_header.filenames_enabled() {
            SdrrRomInfoWithLogging::size()
        } else {
            SdrrRomInfoBasic::size()
        };
        let mut info_buf = vec![0u8; info_size];
        reader
            .read(rom_info_ptr, &mut info_buf)
            .await
            .map_err(|_| format!("Failed to read ROM info {}", i))?;

        let rom_info = if info_header.filenames_enabled() {
            let (_, info) = SdrrRomInfoWithLogging::from_bytes((&info_buf, 0))
                .map_err(|e| format!("Failed to parse ROM info with logging {}: {}", i, e))?;

            // This test sets filename to None if pointer invalid (for example 0)
            let filename = if info.filename_ptr >= base_addr && info.filename_ptr != 0xFFFFFFFF {
                read_string_at_ptr(reader, info.filename_ptr, base_addr)
                    .await
                    .ok()
            } else {
                None
            };

            SdrrRomInfo {
                rom_type: info.rom_type,
                cs1_state: info.cs1_state,
                cs2_state: info.cs2_state,
                cs3_state: info.cs3_state,
                filename,
            }
        } else {
            let (_, info) = SdrrRomInfoBasic::from_bytes((&info_buf, 0))
                .map_err(|e| format!("Failed to parse ROM info basic {}: {}", i, e))?;

            SdrrRomInfo {
                rom_type: info.rom_type,
                cs1_state: info.cs1_state,
                cs2_state: info.cs2_state,
                cs3_state: info.cs3_state,
                filename: None,
            }
        };

        rom_infos.push(rom_info);
    }

    Ok(rom_infos)
}

/// Read pin configuration
pub(crate) async fn read_pins<R: Reader>(
    reader: &mut R,
    ptr: u32,
    base_addr: u32,
) -> Result<SdrrPins, String> {
    if ptr < base_addr {
        return Err(format!("Invalid pins pointer: 0x{:08X}", ptr));
    }

    const MAX_PINS_SIZE: usize = SdrrPins::base_size() + SdrrPins::extra_size();
    let mut pins_buf = [0u8; MAX_PINS_SIZE];
    reader
        .read(ptr, &mut pins_buf)
        .await
        .map_err(|_| "Failed to read pins data")?;

    SdrrPins::from_bytes((&pins_buf, 0))
        .map_err(|e| format!("Failed to parse pins: {}", e))
        .map(|(_, pins)| pins)
}
