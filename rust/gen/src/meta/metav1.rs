// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Metadata generator for One ROM.
//!
//! Supports firmware before v0.7.0

use alloc::vec;
use alloc::vec::Vec;

use onerom_config::chip::ChipFunction;
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;

use crate::MetadataWriter;
use crate::image::{ChipSet, ChipSetType};
use crate::meta::fw_overrides_core;
use crate::{Error, FIRMWARE_SIZE, METADATA_VERSION, MIN_FIRMWARE_OVERRIDES_VERSION, Result};
use crate::{FireServeMode, FirmwareConfig, ServeAlgParams};

pub const PAD_METADATA_BYTE: u8 = 0xFF;

const HEADER_MAGIC: &[u8; 16] = b"ONEROM_METADATA\0";

// Metadata starts at 48KB from the start of flash.
const METADATA_START: u32 = FIRMWARE_SIZE as u32;

// ROM images start at 64KB from the start of flash.
const ROM_IMAGE_DATA_START: u32 = 65536;

/// Metadata max length
pub const MAX_METADATA_LEN: usize = 16384;

const METADATA_HEADER_LEN: usize = 256; // onerom_metadata_header_t

const METADATA_CHIP_SET_OFFSET: usize = 24; // Offset of chip_set pointer in header

pub(crate) const CHIP_SET_METADATA_LEN: usize = 16; // sdrr_rom_set_t
pub(crate) const CHIP_SET_METADATA_LEN_EXTRA_INFO: usize = 64; // sdrr_rom_set_t
pub(crate) const CHIP_SET_FIRMWARE_OVERRIDES_METADATA_LEN: usize = 64; // 0.6.0 onwards
pub(crate) const CHIP_SET_SERVE_CONFIG_METADATA_LEN: usize = 64; // 0.6.0 onwards

/// Metadata for One ROM firmware
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Metadata {
    board: Board,
    chip_sets: Vec<ChipSet>,
    filenames: bool,
    pio: bool,
    firmware_version: FirmwareVersion,
}

impl Metadata {
    pub fn new(
        board: Board,
        chip_sets: Vec<ChipSet>,
        filenames: bool,
        pio: bool,
        firmware_version: FirmwareVersion,
    ) -> Self {
        Self {
            board,
            chip_sets,
            filenames,
            pio,
            firmware_version,
        }
    }

    pub fn set_pio(&mut self) {
        self.pio = true;
    }

    pub fn pio(&self) -> bool {
        self.pio
    }

    const fn header_len(&self) -> usize {
        METADATA_HEADER_LEN
    }

    const fn abs_metadata_start(&self) -> u32 {
        self.board.mcu_family().get_flash_base() + METADATA_START
    }

    const fn abs_chip_image_start(&self) -> u32 {
        self.board.mcu_family().get_flash_base() + ROM_IMAGE_DATA_START
    }

    // Total number of ROMs across all sets
    fn total_chip_count(&self) -> usize {
        self.chip_sets.iter().map(|rs| rs.chips().len()).sum()
    }

    // Total length, including null terminators, of all filenames
    fn filenames_metadata_len(&self) -> usize {
        let len = if !self.filenames {
            0
        } else {
            self.chip_sets
                .iter()
                .flat_map(|rs| rs.chips())
                .map(|rom| rom.filename().len() + 1)
                .sum()
        };
        if len % 4 != 0 {
            // Align to 4 bytes
            len + (4 - (len % 4))
        } else {
            len
        }
    }

    // Get total length of sets:
    // - Pointer to array of ROM pointers
    // - All ROM structs
    //
    // Does not include filename lengths
    fn sets_len(&self) -> usize {
        let mut total = 0;
        for set in &self.chip_sets {
            total += set.chips_metadata_len(self.filenames);
            total += set.chips().len() * 4;
        }

        total += self.chip_sets.len() * ChipSet::chip_set_metadata_len(&self.firmware_version);

        total
    }

    // Writes all ROM filenames to provided buffer.
    fn write_filenames(&self, buf: &mut [u8], ptrs: &mut [u32]) -> Result<usize> {
        if !self.filenames {
            return Ok(0);
        }

        if buf.len() < self.filenames_metadata_len() {
            return Err(crate::Error::BufferTooSmall {
                location: "write_filenames1",
                expected: self.filenames_metadata_len(),
                actual: buf.len(),
            });
        }

        let mut offset = 0;

        // Set up array of filename pointers.
        let num_roms = self.total_chip_count();
        if ptrs.len() < num_roms {
            return Err(crate::Error::BufferTooSmall {
                location: "write_filenames2",
                expected: num_roms,
                actual: ptrs.len(),
            });
        }

        for (ii, rom) in self.chip_sets.iter().flat_map(|rs| rs.chips()).enumerate() {
            assert_eq!(ii, rom.index());

            // Get the filename and its length
            let name_bytes = rom.filename().as_bytes();
            let len = name_bytes.len();

            // Store off the pointer
            ptrs[ii] = offset as u32;

            // Store the null terminated filename
            buf[offset..offset + len].copy_from_slice(name_bytes);
            offset += len;
            buf[offset] = 0;
            offset += 1;
        }
        Ok(offset)
    }

    fn write_header(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.len() < METADATA_HEADER_LEN {
            return Err(crate::Error::BufferTooSmall {
                location: "write_header",
                expected: METADATA_HEADER_LEN,
                actual: buf.len(),
            });
        }

        let mut offset = 0;
        let len = 16;
        buf[0..offset + len].copy_from_slice(HEADER_MAGIC);
        offset += len;

        let len = 4;
        buf[offset..offset + len].copy_from_slice(&METADATA_VERSION.to_le_bytes());
        offset += len;

        let len = 1;
        buf[offset..offset + len].copy_from_slice(&[self.chip_sets.len() as u8]);
        offset += len;

        let len = 3;
        buf[offset..offset + len].copy_from_slice(&[0u8; 3]);
        offset += len;

        // We'll need to update this later
        let len = 4;
        assert_eq!(offset, METADATA_CHIP_SET_OFFSET);
        buf[offset..offset + len].copy_from_slice(&0xFFFFFFFF_u32.to_le_bytes());
        offset += len;

        let len = 228;
        buf[offset..offset + len].copy_from_slice(&[0xFFu8; 228]);
        offset += len;

        // Final sanity check
        assert_eq!(offset, self.header_len());

        Ok(offset)
    }

    fn update_chip_set_ptr(&self, buf: &mut [u8], ptr: u32) -> Result<()> {
        if buf.len() < (METADATA_CHIP_SET_OFFSET + 4) {
            return Err(crate::Error::BufferTooSmall {
                location: "update_chip_set_ptr",
                expected: (METADATA_CHIP_SET_OFFSET + 4),
                actual: buf.len(),
            });
        }

        // Pointer is at offset 20
        buf[METADATA_CHIP_SET_OFFSET..METADATA_CHIP_SET_OFFSET + 4]
            .copy_from_slice(&ptr.to_le_bytes());
        Ok(())
    }

    /// Serialize FirmwareConfig into the 64-byte onerom_firmware_overrides_t structure (v1).
    #[allow(clippy::collapsible_if)]
    fn serialize_firmware_overrides(config: &FirmwareConfig, buf: &mut [u8]) -> Result<usize> {
        if buf.len() < CHIP_SET_FIRMWARE_OVERRIDES_METADATA_LEN {
            return Err(Error::BufferTooSmall {
                location: "serialize_firmware_overrides",
                expected: CHIP_SET_FIRMWARE_OVERRIDES_METADATA_LEN,
                actual: buf.len(),
            });
        }

        let core = fw_overrides_core(config);
        buf[..24].copy_from_slice(&core);

        // pad3: 40 bytes to reach the 64-byte v1 struct size
        buf[24..CHIP_SET_FIRMWARE_OVERRIDES_METADATA_LEN].fill(PAD_METADATA_BYTE);

        Ok(CHIP_SET_FIRMWARE_OVERRIDES_METADATA_LEN)
    }

    /// Serialize ServeAlgParams into the 64-byte onerom_serve_config_t structure
    fn serialize_serve_config(params: &ServeAlgParams, buf: &mut [u8]) -> Result<usize> {
        if buf.len() < CHIP_SET_SERVE_CONFIG_METADATA_LEN {
            return Err(Error::BufferTooSmall {
                location: "serialize_serve_config",
                expected: CHIP_SET_SERVE_CONFIG_METADATA_LEN,
                actual: buf.len(),
            });
        }

        buf[..64].fill(0xFF);

        // Copy params data, up to 64 bytes
        let len = params.params.len().min(CHIP_SET_SERVE_CONFIG_METADATA_LEN);
        buf[..len].copy_from_slice(&params.params[..len]);

        // Zero out any remaining bytes
        if len < CHIP_SET_SERVE_CONFIG_METADATA_LEN {
            buf[len..CHIP_SET_SERVE_CONFIG_METADATA_LEN].fill(PAD_METADATA_BYTE);
        }

        Ok(CHIP_SET_SERVE_CONFIG_METADATA_LEN)
    }

    // Calculate total size needed for firmware overrides and serve config structures
    fn firmware_overrides_len(&self) -> usize {
        const MIN_EXTENDED_VERSION: FirmwareVersion = FirmwareVersion::new(0, 6, 0, 0);

        if self.firmware_version < MIN_EXTENDED_VERSION {
            return 0;
        }

        let mut total = 0;
        for chip_set in &self.chip_sets {
            if let Some(ref fw_config) = chip_set.firmware_overrides {
                // firmware_overrides structure is 64 bytes
                total += CHIP_SET_FIRMWARE_OVERRIDES_METADATA_LEN;

                // serve_alg_params structure is also 64 bytes if present
                if fw_config.serve_alg_params.is_some() {
                    total += CHIP_SET_FIRMWARE_OVERRIDES_METADATA_LEN;
                }
            }
        }
        total
    }
}

impl MetadataWriter for Metadata {
    /// Length of buffer required for metadata.
    fn metadata_len(&self) -> usize {
        // Size needs to include:
        // - Header (256 bytes) - onerom_metadata_header_t
        // - All ROM filenames - char[]
        // - Firmware overrides, if any
        // - All ROM set entries (16 bytes) - sdrr_chip_set_t
        // - Array of pointers to ROMs in each set (4 bytes per ROM)
        // - Each ROM entry (4-8 bytes) - sdrr_chip_info_t
        let len = self.header_len()
            + self.filenames_metadata_len()
            + self.firmware_overrides_len()
            + self.sets_len();

        if len > MAX_METADATA_LEN {
            panic!(
                "Metadata too large: {} bytes (max {})",
                len, MAX_METADATA_LEN
            );
        }

        len
    }

    fn total_set_count(&self) -> usize {
        self.chip_sets.len()
    }

    /// Writes all metadata to provided buffer.
    ///
    /// It is advisable to call [`Self::metadata_len`] first to ensure the
    /// buffer provided is large enough.  Also [`Self::total_set_count`] should
    /// be called to get the number of ROM sets, so the caller can allocate
    /// space for the returned ROM data pointers.
    ///
    /// The `rtn_chip_data_ptrs` slice provides offsets from the start of the ROM
    /// data location (flash_base + 64KB) for each ROM set.
    ///
    /// The caller should ensure that each ROM set data is written to the flash.
    fn write_all(&self, buf: &mut [u8], rtn_chip_data_ptrs: &mut [u32]) -> Result<usize> {
        // Check we have enough of a buffer.
        if self.metadata_len() > buf.len() {
            return Err(Error::BufferTooSmall {
                location: "write_all",
                expected: self.metadata_len(),
                actual: buf.len(),
            });
        }

        let mut offset = 0;

        // Write the header
        offset += self.write_header(&mut buf[offset..])?;

        // Write the filenames.
        let mut filename_ptrs = vec![0xFF_u32; self.total_chip_count()];
        if self.filenames {
            // Store off the offset where filenames start
            let filename_offset = offset;

            // write_filenames() fills in filename_ptrs, but starts at 0
            let filename_len = self.write_filenames(&mut buf[offset..], &mut filename_ptrs)?;
            offset += filename_len;

            // Need to correct filename pointers to be absolute addresses.
            // We need to add filename_offset plus the flash base
            for ptr in filename_ptrs.iter_mut() {
                *ptr += (filename_offset as u32) + self.abs_metadata_start();
            }

            if filename_len % 4 != 0 {
                // Align to 4 bytes
                let padding = 4 - (filename_len % 4);
                for _ in 0..padding {
                    buf[offset] = PAD_METADATA_BYTE;
                    offset += 1;
                }
            }

            assert_eq!(
                offset % 4,
                0,
                "Metadata offset not 4 byte aligned after writing filenames"
            );
        }

        let mut firmware_overrides_ptrs = vec![None; self.chip_sets.len()];
        let mut serve_config_ptrs = vec![None; self.chip_sets.len()];

        if self.firmware_version >= MIN_FIRMWARE_OVERRIDES_VERSION {
            for (ii, chip_set) in self.chip_sets.iter().enumerate() {
                // Serialize firmware overrides if present
                if let Some(ref fw_config) = chip_set.firmware_overrides {
                    firmware_overrides_ptrs[ii] = Some(offset as u32 + self.abs_metadata_start());
                    let len = Self::serialize_firmware_overrides(fw_config, &mut buf[offset..])?;
                    offset += len;

                    // Serialize serve_alg_params if present within firmware_overrides
                    if let Some(ref params) = fw_config.serve_alg_params {
                        serve_config_ptrs[ii] = Some(offset as u32 + self.abs_metadata_start());
                        let len = Self::serialize_serve_config(params, &mut buf[offset..])?;
                        offset += len;
                    }
                }
            }
        }

        // Pre-compute where the ROM set image data will live for each rom set
        // now, so we can fill in the pointers in each set.  This is from
        // the start of flash + 64KB.  We also set up a vec to hold offsets
        // from the start of the ROM image location to return from this
        // function.
        let mut rom_data_ptrs = vec![0u32; self.chip_sets.len()];
        let mut rom_data_ptr = self.abs_chip_image_start();
        let mut rtn_chip_data_ptr = 0;
        for (ii, set) in self.chip_sets.iter().enumerate() {
            if !set.has_data() && (set.chip_function() == ChipFunction::Ram) {
                // No ROM data for RAM chip sets
                rom_data_ptrs[ii] = 0xFFFF_FFFF;
                rtn_chip_data_ptrs[ii] = 0xFFFF_FFFF;
                continue;
            }

            // Either ROM or RAM has an image
            rom_data_ptrs[ii] = rom_data_ptr;
            rtn_chip_data_ptrs[ii] = rtn_chip_data_ptr;
            let rom_data_size = set.image_size(&self.board, &self.firmware_version);
            rom_data_ptr += rom_data_size as u32;
            rtn_chip_data_ptr += rom_data_size as u32;
        }

        // Write each set's ROM data, which need to return pointers to rom arrays.
        // This doesn't write the set itself - that comes last.
        let mut rom_array_ptrs = vec![Vec::new(); self.chip_sets.len()];
        for (ii, chip_set) in self.chip_sets.iter().enumerate() {
            // Each write_metadata() fills in rom_ptrs for that set
            let mut rom_metadata_ptrs = vec![0u32; chip_set.chips().len()];
            let len = chip_set.write_chip_metadata(
                &mut buf[offset..],
                &filename_ptrs,
                &mut rom_metadata_ptrs,
                self.filenames,
            )?;

            // Now update this set's array of ROM pointers
            for ptr in rom_metadata_ptrs.iter_mut() {
                *ptr += offset as u32 + self.abs_metadata_start();
            }
            rom_array_ptrs[ii] = rom_metadata_ptrs;

            // Advance the offset
            offset += len;
        }

        // Next, write each of the ROM pointer arrays creating a vec of
        // actual pointers to each array, to include in each set.
        let mut actual_chip_array_ptrs = vec![0u32; self.chip_sets.len()];
        for (ii, chip_set) in self.chip_sets.iter().enumerate() {
            let len = chip_set.write_chip_pointer_array(&mut buf[offset..], &rom_array_ptrs[ii])?;
            actual_chip_array_ptrs[ii] = offset as u32 + self.abs_metadata_start();
            offset += len;
        }

        // Write each set struct - this will become an array of set structs.
        let first_chip_set_ptr = offset as u32 + self.abs_metadata_start();
        for (ii, chip_set) in self.chip_sets.iter().enumerate() {
            offset += chip_set.write_set_metadata(
                &mut buf[offset..],
                rom_data_ptrs[ii],
                actual_chip_array_ptrs[ii],
                &self.board,
                &self.firmware_version,
                serve_config_ptrs[ii],
                firmware_overrides_ptrs[ii],
            )?;
        }

        // Finally, update the pointer to the first ROM set in the header.
        self.update_chip_set_ptr(&mut buf[..], first_chip_set_ptr)?;

        Ok(offset)
    }

    /// Returns the total size needed for all ROM images
    fn rom_images_size(&self) -> usize {
        self.chip_sets
            .iter()
            .filter(|set| set.has_data())
            .map(|set| set.image_size(&self.board, &self.firmware_version))
            .sum()
    }

    /// Write all ROM images to buffer
    fn write_roms(&self, buf: &mut [u8]) -> Result<()> {
        // Validate buffer size
        if buf.len() < self.rom_images_size() {
            return Err(Error::BufferTooSmall {
                location: "write_roms",
                expected: self.rom_images_size(),
                actual: buf.len(),
            });
        }

        let mut offset = 0;
        for chip_set in &self.chip_sets {
            // Don't write a ROM image for RAM chip sets
            if !chip_set.has_data() && chip_set.chip_function() == ChipFunction::Ram {
                continue;
            }

            // For PIO based multi-ROM sets, we need to flip the sense of the
            // CS1/X1 and X2 (if applicable) lines, as the PIO algorithm is
            // implemented differently in this case, and the CS1/X1/X2 lines
            // are all flipped in hardware.  Without this image flipping, the
            // wrong bytes would be served.
            let mut pio = self.pio();
            if let Some(serve_mode) = chip_set
                .firmware_overrides
                .as_ref()
                .and_then(|o| o.fire.as_ref())
                .and_then(|f| f.serve_mode.as_ref())
            {
                pio = *serve_mode == FireServeMode::Pio;
            }
            let flip_cs1_x = if pio {
                chip_set.set_type == ChipSetType::Multi
            } else {
                false
            };

            let size = chip_set.image_size(&self.board, &self.firmware_version);

            // Fill buffer by calling get_byte for each address
            for addr in 0..size {
                buf[offset + addr] =
                    chip_set.get_byte(addr, &self.board, &self.firmware_version, flip_cs1_x);
            }

            offset += size;
        }

        Ok(())
    }
}
