// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Metadata writer for One ROM firmware >= v0.7.0 (`onerom_metadata_header_t` v2).
//!
//! # Defaults
//!
//! The following algorithm parameters are defaulted pending per-slot
//! configuration support:
//!   cs_active_delay / cs_inactive_delay          = 0   (CS0/CS1/CS2 params)
//!   addr_delay_cycles                            = 0   (ALG_ADDR_0 params)
//!   clkdiv_int / clkdiv_frac                     = 1/0 (ALG_DATA_0 header)
//!
//! The following are defaulted due to missing `Board` accessors:
//!   gpio_from_phys_pin[][1]                      = 0xFF (second GPIO per physical pin;
//!                                                  unused on all current boards)
//!
//! # Not yet implemented (TODO)
//!
//!   - Multi-ROM / Banked `ChipSet` serving — `serve_cs_low_0` is hardcoded 0.
//!     Multi-ROM requires inverted CS logic and a different image encoding.
//!   - CS2 (address-qualified enable) algorithm — used by 23QL384 only.
//!   - Plugin chip slots — algorithm config is left as placeholder stubs.
//!   - /BYTE mode algorithms need more address read delays
//!   - Deduplication of algorithm configs and firmware overrides between slots
//!   - Add DMA config support
//!   - /BYTE being inverted is hardcoded
//!   - Should I allow address pins to be inverted?
//!   - X pins need pull-ups/downs.  Are these on CS lines or address lines?
//!   - Inversion and forcing need to be removed from these config structs
//!     and pull_config and override_config used instead.
//!   - Plugin type added to rom_info_t
//!   - Figure out how RBCP should deal with rom_types now not included, fix
//!     ora_get_ram_slot_info
//!   - Maybe add data slew rate, and drive strength as args
//!   - ROM vs RAM - fold RAM into ROM serving with more algs
//!   - Len in the alg structure should be param len
//!   - Plugin must update rom slot when changing it (switching)
//!   - set rom_set_type field

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use onerom_config::chip::{ChipFunction, ChipType};
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_config::pin_map::BoardPinMap;

use crate::MetadataWriter;
use crate::builder::FirmwareConfig;
use crate::image::{Chip, ChipSet, ChipSetType, CsConfig, CsLogic};
use crate::meta::fw_overrides_core;
use crate::{Error, FIRMWARE_SIZE, METADATA_VERSION, Result};

// ---------------------------------------------------------------------------
// C enum mirrors  (from enums.h)
// ---------------------------------------------------------------------------

/// Mirrors `onerom_alg_cs_t` — CS algorithm selector (1 byte).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AlgCs {
    Cs0 = 0,
    Cs1 = 1,
    #[allow(dead_code)]
    Cs2 = 2,
}

/// Mirrors `onerom_alg_addr_t` — address algorithm selector (1 byte).
#[repr(u8)]
#[derive(Clone, Copy, Debug)]
enum AlgAddr {
    Addr0 = 0,
}

/// Mirrors `onerom_alg_data_t` — data algorithm selector (1 byte).
#[repr(u8)]
#[derive(Clone, Copy, Debug)]
enum AlgData {
    Data0 = 0,
}

/// Mirrors `bit_modes_t` — DMA transfer width (1 byte).
#[repr(u8)]
#[derive(Clone, Copy, Debug)]
enum BitMode {
    Mode8 = 0x01,
    Mode16 = 0x02,
}

// ---------------------------------------------------------------------------
// C struct sizes (validated against C STATIC_ASSERT annotations)
// ---------------------------------------------------------------------------

const HEADER_LEN: usize = 256; // onerom_metadata_header_t
const HW_INFO_LEN: usize = 128; // onerom_hardware_info_t
const FW_CONFIG_LEN: usize = 8; // onerom_firmware_config_t  (2 × ptr)
const PIN_MAP_LEN: usize = 40; // onerom_rom_pin_map_t       (24 + 16)
const ROM_INFO_LEN: usize = 12; // onerom_rom_info_t          (3 × ptr)
const ROM_SLOT_LEN: usize = 32; // onerom_rom_slot_t
const ALG_CONFIG_LEN: usize = 32; // onerom_alg_config_t        (4 × ptr + 16 rsvd)
const DMA_CONFIG_LEN: usize = 4; // onerom_dma_config_t
const FW_OVERRIDES_LEN: usize = 32; // onerom_firmware_overrides_t (v2)
const FW_OVERRIDES_CORE_LEN: usize = 24; // shared payload with v1

/// CS / addr config header: `len (u8) + alg (u8)` = 2 bytes.
const ALG_CS_HDR: usize = 2;
const ALG_ADDR_HDR: usize = 2;
/// Data config header: `alg + param_len + clkdiv_int(u16) + clkdiv_frac + pad` = 6 bytes.
const ALG_DATA_HDR: usize = 6;

/// Algorithm parameter struct sizes (see `onerom_alg_csN_param_t` etc.).
const CS0_PARAMS: usize = 11;
const CS1_PARAMS: usize = 9;
const ADDR0_FIXED_PARAMS: usize = 5; // before variable force_list[]
const DATA0_PARAMS: usize = 3;

/// Byte offsets within the header where pointers live (for later patching).
const HDR_HW_PTR_OFF: usize = 20;
const HDR_FW_PTR_OFF: usize = 24;
const HDR_SLOTS_PTR_OFF: usize = 32;

// ---------------------------------------------------------------------------
// Defaults (see module-level doc)
// ---------------------------------------------------------------------------

const DEFAULT_CS_ACTIVE_DELAY: u8 = 0;
const DEFAULT_CS_INACTIVE_DELAY: u8 = 0;
const DEFAULT_ADDR_DELAY_CYCLES: u8 = 2;
const DEFAULT_CLKDIV_INT: u16 = 1;
const DEFAULT_CLKDIV_FRAC: u8 = 0;

/// NULL pointer fields.
const NULL_PTR: u32 = 0;
/// Unused / unavailable GPIO pin.
const GPIO_NONE: u8 = 0xFF;
/// Pad byte written into reserved / alignment holes.
const PAD: u8 = 0xFF;

/// Flash byte offset where ROM image data begins (immediately after firmware).
const ROM_DATA_FLASH_OFFSET: u32 = 65536;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct AdditionalProps {
    instance_name: Option<String>,
    serial_override: Option<String>,
    boot_logging: bool,
    swd_enabled: bool,
    turbo_boot: bool,
}

impl From<&crate::builder::Config> for AdditionalProps {
    fn from(cfg: &crate::builder::Config) -> Self {
        Self {
            instance_name: cfg.instance_name.clone(),
            serial_override: cfg.serial_override.clone(),
            boot_logging: cfg.boot_logging,
            swd_enabled: cfg.swd_enabled,
            turbo_boot: cfg.turbo_boot,
        }
    }
}

/// Metadata writer for One ROM firmware >= v0.7.0.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MetadataV2 {
    board: Board,
    chip_sets: Vec<ChipSet>,
    filenames: bool,
    firmware_version: FirmwareVersion,
    additional_props: AdditionalProps,
}

impl MetadataV2 {
    pub(crate) fn new(
        board: Board,
        chip_sets: Vec<ChipSet>,
        filenames: bool,
        firmware_version: FirmwareVersion,
        additional_props: AdditionalProps,
    ) -> Self {
        Self {
            board,
            chip_sets,
            filenames,
            firmware_version,
            additional_props,
        }
    }

    fn abs_metadata_start(&self) -> u32 {
        self.board.mcu_family().get_flash_base() + FIRMWARE_SIZE as u32
    }

    fn abs_rom_data_start(&self) -> u32 {
        self.board.mcu_family().get_flash_base() + ROM_DATA_FLASH_OFFSET
    }
}

// ---------------------------------------------------------------------------
// MetadataWriter
// ---------------------------------------------------------------------------

impl MetadataWriter for MetadataV2 {
    fn metadata_len(&self) -> usize {
        let mut len = HEADER_LEN;

        // hw_rev string + hardware info struct
        len += align4(self.board.name().len() + 1);
        len += HW_INFO_LEN;

        // Optional additional properties:
        if let Some(ref n) = self.additional_props.instance_name {
            len += align4(n.len() + 1);
        }
        if let Some(ref s) = self.additional_props.serial_override {
            len += align4(s.len() + 1);
        }
        len += FW_CONFIG_LEN;

        // Per unique chip type: ROM-type string + pin map (deduplicated)
        for ct in unique_chip_types(&self.chip_sets) {
            len += align4(ct.name().len() + 1);
            len += PIN_MAP_LEN;
        }

        // Filenames (one per chip across all slots, if requested)
        if self.filenames {
            for cs in &self.chip_sets {
                for chip in cs.chips() {
                    len += align4(chip.filename().len() + 1);
                }
            }
        }

        // Per slot: algorithm configs + DMA + alg_config struct + fw overrides
        for cs in &self.chip_sets {
            let ct = *cs.chips()[0].chip_type();
            len += cs_alg_written_size(self.board, ct, cs);
            len += addr_alg_written_size(self.board, ct, cs);
            len += align4(ALG_DATA_HDR + DATA0_PARAMS);
            len += DMA_CONFIG_LEN;
            len += ALG_CONFIG_LEN;
            if cs.firmware_overrides.is_some() {
                len += FW_OVERRIDES_LEN;
            }
        }

        // Per chip per slot: ROM info struct (12 bytes) + slot pointer arrays (4 bytes/chip)
        for cs in &self.chip_sets {
            len += (ROM_INFO_LEN + 4) * cs.chips().len();
        }

        // ROM slot array
        len += ROM_SLOT_LEN * self.chip_sets.len();

        len
    }

    fn total_set_count(&self) -> usize {
        self.chip_sets.len()
    }

    fn rom_images_size(&self) -> usize {
        self.chip_sets
            .iter()
            .filter(|cs| cs.chip_function() != ChipFunction::Ram && cs.has_data())
            .map(|cs| chip_image_size(self.board, *cs.chips()[0].chip_type()))
            .sum()
    }

    fn write_all(&self, buf: &mut [u8], rtn_chip_data_ptrs: &mut [u32]) -> Result<usize> {
        // Check we have enough of a buffer.
        if self.metadata_len() > buf.len() {
            return Err(Error::BufferTooSmall {
                location: "write_all",
                expected: self.metadata_len(),
                actual: buf.len(),
            });
        }

        let abs_base = self.abs_metadata_start();
        let mut w = BufWriter::new(buf, abs_base);

        // ── 1. Header (placeholder pointers, patched at the end) ─────────────
        write_header(&mut w, self.chip_sets.len() as u8, &self.additional_props);
        debug_assert_eq!(w.offset, HEADER_LEN);

        // ── 2. hw_rev string → hardware info struct ───────────────────────────
        let hw_rev_ptr = w.write_str(self.board.name());
        let hw_info_ptr = write_hw_info(&mut w, self.board, hw_rev_ptr);
        w.patch_u32(HDR_HW_PTR_OFF, hw_info_ptr);

        // ── 3. Optional additional properties → firmware config struct ────────
        let name_ptr = self
            .additional_props
            .instance_name
            .as_ref()
            .map(|s| w.write_str(s))
            .unwrap_or(NULL_PTR);
        let serial_ptr = self
            .additional_props
            .serial_override
            .as_ref()
            .map(|s| w.write_str(s))
            .unwrap_or(NULL_PTR);
        let fw_config_ptr = w.abs_addr();
        w.write_u32(name_ptr);
        w.write_u32(serial_ptr);
        w.patch_u32(HDR_FW_PTR_OFF, fw_config_ptr);

        // ── 4. ROM-type strings (one per unique ChipType, deduplicated) ────────
        let unique = unique_chip_types(&self.chip_sets);
        let mut type_str_ptrs: Vec<(ChipType, u32)> = Vec::new();
        for ct in &unique {
            let ptr = w.write_str(ct.name());
            type_str_ptrs.push((*ct, ptr));
        }

        // ── 5. Filenames (one per chip, if requested) ─────────────────────────
        let mut filename_ptrs: Vec<u32> = Vec::new();
        if self.filenames {
            for cs in &self.chip_sets {
                for chip in cs.chips() {
                    filename_ptrs.push(w.write_str(chip.filename()));
                }
            }
        }

        // ── 6. Pin maps (one per unique ChipType, deduplicated) ───────────────
        let mut pin_map_ptrs: Vec<(ChipType, u32)> = Vec::new();
        for ct in &unique {
            let ptr = write_pin_map(&mut w, self.board, *ct)?;
            pin_map_ptrs.push((*ct, ptr));
        }

        // ── 7. Per-slot: algorithm sub-configs, DMA, alg_config, fw overrides ─
        struct SlotAlg {
            alg_config_ptr: u32,
            fw_overrides_ptr: u32,
        }
        let mut slot_algs: Vec<SlotAlg> = Vec::new();

        for cs in &self.chip_sets {
            let ct = *cs.chips()[0].chip_type();

            let cs_ptr = write_alg_cs_config(&mut w, self.board, ct, cs)?;
            let addr_ptr = write_alg_addr_config(&mut w, self.board, ct, cs)?;
            let data_ptr = write_alg_data_config(&mut w, self.board, ct);
            let dma_ptr = write_dma_config(&mut w, ct);
            let alg_ptr = write_alg_config_struct(&mut w, cs_ptr, addr_ptr, data_ptr, dma_ptr);

            let fw_ptr = match &cs.firmware_overrides {
                Some(fo) => {
                    let ptr = w.abs_addr();
                    write_fw_overrides_v2(&mut w, fo);
                    ptr
                }
                None => NULL_PTR,
            };

            slot_algs.push(SlotAlg {
                alg_config_ptr: alg_ptr,
                fw_overrides_ptr: fw_ptr,
            });
        }

        // ── 8. Per-slot: ROM info structs + pointer arrays ────────────────────
        struct SlotRom {
            roms_array_ptr: u32,
            image_size: usize,
        }
        let mut slot_roms: Vec<SlotRom> = Vec::new();
        let mut global_chip = 0usize;

        for cs in &self.chip_sets {
            let mut info_ptrs: Vec<u32> = Vec::new();

            for chip in cs.chips() {
                let ct = *chip.chip_type();

                let type_ptr = type_str_ptrs
                    .iter()
                    .find(|(c, _)| *c == ct)
                    .map(|(_, p)| *p)
                    .unwrap_or(NULL_PTR);

                let fn_ptr = if self.filenames {
                    filename_ptrs.get(global_chip).copied().unwrap_or(NULL_PTR)
                } else {
                    NULL_PTR
                };

                let pm_ptr = pin_map_ptrs
                    .iter()
                    .find(|(c, _)| *c == ct)
                    .map(|(_, p)| *p)
                    .unwrap_or(NULL_PTR);

                // onerom_rom_info_t: rom_type ptr, filename ptr, pin_map ptr
                let info_ptr = w.abs_addr();
                w.write_u32(type_ptr);
                w.write_u32(fn_ptr);
                w.write_u32(pm_ptr);
                info_ptrs.push(info_ptr);

                global_chip += 1;
            }

            // Pointer array (rom_info* [rom_count])
            let arr_ptr = w.abs_addr();
            for p in &info_ptrs {
                w.write_u32(*p);
            }

            let ct = *cs.chips()[0].chip_type();
            slot_roms.push(SlotRom {
                roms_array_ptr: arr_ptr,
                image_size: chip_image_size(self.board, ct),
            });
        }

        // ── 9. Compute ROM data flash addresses ────────────────────────────────
        let mut rom_data_ptrs: Vec<u32> = Vec::new();
        {
            let mut abs = self.abs_rom_data_start();
            let mut rel: u32 = 0;
            for (ii, cs) in self.chip_sets.iter().enumerate() {
                if cs.chip_function() == ChipFunction::Ram && !cs.has_data() {
                    rom_data_ptrs.push(NULL_PTR);
                    rtn_chip_data_ptrs[ii] = NULL_PTR;
                } else {
                    rom_data_ptrs.push(abs);
                    rtn_chip_data_ptrs[ii] = rel;
                    let sz = slot_roms[ii].image_size as u32;
                    abs += sz;
                    rel += sz;
                }
            }
        }

        // ── 10. ROM slot array ─────────────────────────────────────────────────
        let slots_ptr = w.abs_addr();
        for (ii, cs) in self.chip_sets.iter().enumerate() {
            let image_size = if cs.chip_function() == ChipFunction::Ram && !cs.has_data() {
                0u32
            } else {
                slot_roms[ii].image_size as u32
            };
            write_rom_slot(
                &mut w,
                rom_data_ptrs[ii],
                image_size,
                slot_roms[ii].roms_array_ptr,
                cs.chips().len() as u8,
                slot_algs[ii].alg_config_ptr,
                slot_algs[ii].fw_overrides_ptr,
            );
        }

        // Patch header with rom_slots pointer
        w.patch_u32(HDR_SLOTS_PTR_OFF, slots_ptr);

        Ok(w.offset)
    }

    fn write_roms(&self, buf: &mut [u8]) -> Result<()> {
        // Validate buffer size
        if buf.len() < self.rom_images_size() {
            return Err(Error::BufferTooSmall {
                location: "write_roms",
                expected: self.rom_images_size(),
                actual: buf.len(),
            });
        }

        let mut offset = 0usize;

        for cs in &self.chip_sets {
            // RAM chips with no data: skip entirely
            if cs.chip_function() == ChipFunction::Ram && !cs.has_data() {
                continue;
            }

            let chip = &cs.chips()[0];
            let ct = *chip.chip_type();

            // Plugin: raw executable bytes, no address remapping
            if ct.chip_function() == ChipFunction::Plugin {
                let size = chip_image_size(self.board, ct);
                for addr in 0..size {
                    buf[offset + addr] = chip.data_byte_at(addr).unwrap_or(0xFF);
                }
                offset += size;
                continue;
            }

            // Multi-ROM and banked sets not yet implemented in v2
            if cs.set_type != ChipSetType::Single {
                return Err(Error::InvalidConfig {
                    error: format!(
                        "Multi-ROM / banked slot ROM writing not yet implemented \
                         in v2 metadata (chip type: {})",
                        ct.name()
                    ),
                });
            }

            // ── Compute address GPIO assignments for this chip on this board ──
            let bpm = BoardPinMap::new(self.board);
            let num_addr = ct.num_addr_lines();

            // addr_map[i] = (address_line_n, gpio)
            // Dynamically derived from BoardPinMap + chip_type.address_pins().
            // No snowflake handling needed: the BoardPinMap naturally handles
            // cases like the 2732 (A11 at physical pin 21 → correct GPIO).
            let addr_map: Vec<(usize, u8)> = ct.address_pins()[..num_addr]
                .iter()
                .enumerate()
                .filter_map(|(n, &pin)| bpm.gpio_for_chip_pin(pin).map(|g| (n, g)))
                .collect();

            if addr_map.len() != num_addr {
                return Err(Error::InvalidConfig {
                    error: format!(
                        "Not all {} address pins mapped for {} on {}",
                        num_addr,
                        ct.name(),
                        self.board.name()
                    ),
                });
            }

            let base = addr_map.iter().map(|(_, g)| *g).min().unwrap();
            let max = addr_map.iter().map(|(_, g)| *g).max().unwrap();
            let span = (max - base + 1) as usize;
            let image_size = 1usize << span;

            for image_addr in 0..image_size {
                // Compute the logical chip address from the image address bits.
                // Each bit at position (gpio - base) in image_addr encodes one
                // address line; bit position corresponds to addr_line n.
                let mut logical = 0usize;
                for (addr_line, gpio) in &addr_map {
                    let bit = (*gpio - base) as usize;
                    if (image_addr >> bit) & 1 != 0 {
                        logical |= 1 << addr_line;
                    }
                }

                // Bounds guard: chip data may be smaller than image space
                // (e.g. 23QL384 with non-power-of-2 size, or 27C080 half-image).
                let raw = chip.data_byte_at(logical).unwrap_or(0xFF);
                buf[offset + image_addr] = Chip::byte_mangled(raw, &self.board);
            }

            offset += image_size;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// BufWriter — thin cursor over the metadata buffer
// ---------------------------------------------------------------------------

struct BufWriter<'a> {
    buf: &'a mut [u8],
    offset: usize,
    abs_base: u32,
}

impl<'a> BufWriter<'a> {
    fn new(buf: &'a mut [u8], abs_base: u32) -> Self {
        Self {
            buf,
            offset: 0,
            abs_base,
        }
    }

    /// Absolute flash address of the current write position.
    #[inline]
    fn abs_addr(&self) -> u32 {
        self.abs_base + self.offset as u32
    }

    fn write_bytes(&mut self, data: &[u8]) {
        let end = self.offset + data.len();
        self.buf[self.offset..end].copy_from_slice(data);
        self.offset = end;
    }

    #[inline]
    fn write_u8(&mut self, v: u8) {
        self.buf[self.offset] = v;
        self.offset += 1;
    }
    #[inline]
    fn write_u16(&mut self, v: u16) {
        self.write_bytes(&v.to_le_bytes());
    }
    #[inline]
    fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }

    fn pad(&mut self, n: usize) {
        for _ in 0..n {
            self.buf[self.offset] = PAD;
            self.offset += 1;
        }
    }

    fn align4(&mut self) {
        while !self.offset.is_multiple_of(4) {
            self.buf[self.offset] = PAD;
            self.offset += 1;
        }
    }

    /// Write a null-terminated string and align to the next 4-byte boundary.
    /// Returns the absolute flash address of the first byte.
    fn write_str(&mut self, s: &str) -> u32 {
        let addr = self.abs_addr();
        self.write_bytes(s.as_bytes());
        self.write_u8(0);
        self.align4();
        addr
    }

    /// Overwrite 4 bytes at a buffer-relative offset (used to patch header
    /// pointer fields after the target structures have been written).
    fn patch_u32(&mut self, buf_off: usize, val: u32) {
        self.buf[buf_off..buf_off + 4].copy_from_slice(&val.to_le_bytes());
    }
}

// ---------------------------------------------------------------------------
// Structure writers
// ---------------------------------------------------------------------------

fn write_header(w: &mut BufWriter, slot_count: u8, additional_props: &AdditionalProps) {
    let start = w.offset;
    // magic[16]
    w.write_bytes(b"ONEROM_METADATA\0");
    // version
    w.write_u32(METADATA_VERSION);
    // hw ptr  (patched later at HDR_HW_PTR_OFF = 20)
    w.write_u32(NULL_PTR);
    // fw ptr  (patched later at HDR_FW_PTR_OFF = 24)
    w.write_u32(NULL_PTR);
    // rom_slot_count + pad1[3]
    w.write_u8(slot_count);

    // Additional properties.
    w.write_u8(additional_props.boot_logging as u8);
    w.write_u8(additional_props.swd_enabled as u8);
    w.write_u8(additional_props.turbo_boot as u8);

    // rom_slots ptr  (patched later at HDR_SLOTS_PTR_OFF = 32)
    w.write_u32(NULL_PTR);
    // reserved[220]
    w.pad(220);
    debug_assert_eq!(w.offset - start, HEADER_LEN);
}

fn write_hw_info(w: &mut BufWriter, board: Board, hw_rev_ptr: u32) -> u32 {
    let addr = w.abs_addr();
    let start = w.offset;

    // +0  hw_rev pointer
    w.write_u32(hw_rev_ptr);

    // +4  rp235x_variant, num_phys_pins, usb_capable, gpio_vbus
    w.write_u8(board.rp_variant().expect("RP235X variant not found") as u8);
    w.write_u8(board.chip_pins());
    w.write_u8(board.has_usb() as u8);
    w.write_u8(board.usb_vbus_pin().unwrap_or(GPIO_NONE));

    // +8  gpio_ext_flash_cs, gpio_status, gpio_neopixel, gpio_swdio
    w.write_u8(board.external_flash_cs_pin().unwrap_or(GPIO_NONE));
    w.write_u8(board.pin_status());
    w.write_u8(board.pin_neo().unwrap_or(GPIO_NONE));
    w.write_u8(board.swdio_sel_pin());

    // +12 gpio_swclk, gpio_x1, gpio_x2, x_jumper_pull
    w.write_u8(board.swclk_sel_pin());
    w.write_u8(board.pin_x1());
    w.write_u8(board.pin_x2());
    w.write_u8(board.x_jumper_pull());

    // +16 gpio_sel[7]
    let sel = board.sel_pins();
    for i in 0..7usize {
        w.write_u8(sel.get(i).copied().unwrap_or(GPIO_NONE));
    }

    // +23 sel_jumper_pull — one-hot bitmask derived from sel_jumper_pulls()
    let pulls = board.sel_jumper_pulls();
    let mut mask: u8 = 0;
    for (i, &p) in pulls.iter().enumerate() {
        if i < 8 && p != 0 {
            mask |= 1 << i;
        }
    }
    w.write_u8(mask);

    // +24 gpio_from_phys_pin[40][2]
    // gpio_from_phys_pin[pin][0] = primary GPIO  (from BoardPinMap)
    // gpio_from_phys_pin[pin][1] = 0xFF          (second GPIO unused on current boards)
    let bpm = BoardPinMap::new(board);
    for phys in 1u8..=40 {
        w.write_u8(bpm.gpio_for_chip_pin(phys).unwrap_or(GPIO_NONE));
        w.write_u8(GPIO_NONE);
    }

    // +104 reserved[24]
    w.pad(24);

    debug_assert_eq!(w.offset - start, HW_INFO_LEN);
    addr
}

/// Write `onerom_rom_pin_map_t` (40 bytes) for the given chip type on the board.
///
/// `addr[n]` = bit position of address line An within the image address
///           = GPIO_for_An − base_addr_gpio
///
/// `data[n]` = bit position of data line Dn within the data output register
///           = GPIO_for_Dn − base_data_gpio
fn write_pin_map(w: &mut BufWriter, board: Board, ct: ChipType) -> Result<u32> {
    // ── address map ──────────────────────────────────────────────────────────
    let mut addr_map = [GPIO_NONE; 24];

    if !ct.address_pins().is_empty() && ct.num_addr_lines() > 0 {
        let bpm = BoardPinMap::new(board);
        let num_addr = ct.num_addr_lines();
        let pins = ct.address_pins();

        let addr_gpios: Vec<u8> = pins[..num_addr]
            .iter()
            .map(|&p| bpm.gpio_for_chip_pin(p).unwrap_or(GPIO_NONE))
            .collect();

        if addr_gpios.contains(&GPIO_NONE) {
            return Err(Error::InvalidConfig {
                error: format!(
                    "Not all address pins mapped for {} on {}",
                    ct.name(),
                    board.name()
                ),
            });
        }

        let base = *addr_gpios.iter().min().unwrap();
        for (n, &gpio) in addr_gpios.iter().enumerate() {
            addr_map[n] = gpio - base;
        }
    }

    // ── data map ─────────────────────────────────────────────────────────────
    let mut data_map = [GPIO_NONE; 16];
    let board_data = board.data_pins();
    let num_data = ct.data_pins().len().min(16).min(board_data.len());

    if num_data > 0 {
        let base = *board_data[..num_data].iter().min().unwrap();
        for n in 0..num_data {
            data_map[n] = board_data[n] - base;
        }
    }

    // ── write 40 bytes ───────────────────────────────────────────────────────
    debug_assert_eq!(PIN_MAP_LEN, 24 + 16);
    let ptr = w.abs_addr();
    w.write_bytes(&addr_map);
    w.write_bytes(&data_map);
    Ok(ptr)
}

// ---------------------------------------------------------------------------
// Algorithm config writers
// ---------------------------------------------------------------------------

fn write_alg_cs_config(w: &mut BufWriter, board: Board, ct: ChipType, cs: &ChipSet) -> Result<u32> {
    // Plugins: emit a minimal stub CS0 config so the slot struct is valid.
    // TODO: firmware detection of plugins may not require a real CS config.
    if ct.chip_function() == ChipFunction::Plugin {
        let data_base = board.data_pins().iter().copied().min().unwrap_or(0);
        let params = [
            0u8,
            0,
            1,
            0,
            data_base,
            8,
            0,
            DEFAULT_CS_ACTIVE_DELAY,
            DEFAULT_CS_INACTIVE_DELAY,
            GPIO_NONE,
            0,
        ];
        return Ok(write_cs_block(w, AlgCs::Cs0, &params));
    }

    // Multi-ROM / banked sets not yet implemented
    if cs.set_type != ChipSetType::Single {
        return Err(Error::InvalidConfig {
            error: format!(
                "Multi-ROM / banked CS algorithm not yet implemented in v2 \
                 metadata (chip type: {})",
                ct.name()
            ),
        });
    }

    // CS2 algorithm (address-qualified enable) — 23QL384 only
    if ct.deselect_when_address_all_high().is_some() {
        return Err(Error::InvalidConfig {
            error: format!(
                "CS2 (address-qualified enable) algorithm not yet implemented \
                 for {}",
                ct.name()
            ),
        });
    }

    let sorted = cs_check_gpios(board, ct, cs);
    if sorted.is_empty() {
        return Err(Error::InvalidConfig {
            error: format!(
                "No CS / CE / OE pins found for {} on {}",
                ct.name(),
                board.name()
            ),
        });
    }

    let base = sorted[0];
    let span = *sorted.last().unwrap() - base + 1;
    let num = sorted.len() as u8;
    let inv = cs_pin_inversion(board, ct, cs, &sorted);
    let data_pins = board.data_pins();
    let num_data = ct.data_pins().len().min(data_pins.len());
    let data_base = *data_pins[..num_data].iter().min().unwrap_or(&0);
    let byte_pin = board.pin_byte();

    if span == num {
        // Contiguous CS/CE/OE pins → CS0
        let params = [
            0u8,            // gpio_base
            base,           // base_cs_pin
            span,           // num_cs_pins
            inv,            // cs_pin_inversion
            data_base,      // base_data_pin
            num_data as u8, // num_data_pins
            0u8,            // serve_cs_low_0 (0; multi-ROM TODO)
            DEFAULT_CS_ACTIVE_DELAY,
            DEFAULT_CS_INACTIVE_DELAY,
            byte_pin, // byte_pin (/BYTE for 16-bit wide chips)
            1,        // Always invert // BYTE for now
        ];
        Ok(write_cs_block(w, AlgCs::Cs0, &params))
    } else if span - num == 1 {
        // Single gap → CS1
        let gap = (0..span).find(|&i| !sorted.contains(&(base + i))).unwrap();
        let params = [
            0u8,            // gpio_base
            base,           // base_cs_pin
            span,           // num_cs_pins (includes gap)
            gap,            // cs_ignore_index
            inv,            // cs_pin_inversion
            data_base,      // base_data_pin
            num_data as u8, // num_data_pins
            DEFAULT_CS_ACTIVE_DELAY,
            DEFAULT_CS_INACTIVE_DELAY,
        ];
        Ok(write_cs_block(w, AlgCs::Cs1, &params))
    } else {
        Err(Error::InvalidConfig {
            error: format!(
                "CS span has more than one gap for {} on {} \
                 (span={}, count={})",
                ct.name(),
                board.name(),
                span,
                num
            ),
        })
    }
}

fn write_cs_block(w: &mut BufWriter, alg: AlgCs, params: &[u8]) -> u32 {
    let addr = w.abs_addr();
    w.write_u8((ALG_CS_HDR + params.len()) as u8);
    w.write_u8(alg as u8);
    w.write_bytes(params);
    w.align4();
    addr
}

fn write_alg_addr_config(
    w: &mut BufWriter,
    board: Board,
    ct: ChipType,
    cs: &ChipSet,
) -> Result<u32> {
    // Plugins / chips with no address lines
    if ct.address_pins().is_empty() || ct.num_addr_lines() == 0 {
        let addr = w.abs_addr();
        w.write_u8((ALG_ADDR_HDR + ADDR0_FIXED_PARAMS) as u8);
        w.write_u8(AlgAddr::Addr0 as u8);
        w.write_u8(0); // gpio_base
        w.write_u8(DEFAULT_ADDR_DELAY_CYCLES); // num_delay_cycles
        w.pad(3); // base_addr_pin, num_addr_pins, num_rom_table_bits
        w.align4();
        return Ok(addr);
    }

    let bpm = BoardPinMap::new(board);
    let num_addr = ct.num_addr_lines();

    let addr_gpios: Vec<u8> = ct.address_pins()[..num_addr]
        .iter()
        .map(|&p| bpm.gpio_for_chip_pin(p).unwrap_or(GPIO_NONE))
        .collect();

    if addr_gpios.contains(&GPIO_NONE) {
        return Err(Error::InvalidConfig {
            error: format!(
                "Not all address pins mapped for {} on {}",
                ct.name(),
                board.name()
            ),
        });
    }

    let base = *addr_gpios.iter().min().unwrap();
    let max = *addr_gpios.iter().max().unwrap();
    let span = max - base + 1;

    // Force-list: bit positions i in [0, span) where GPIO (base+i) is neither
    // an address line for this chip NOR an active CS/CE/OE pin for this slot.
    // Active CS/OE pins must NOT be forced — when the firmware is serving, they
    // are already in their active state, which naturally produces the correct
    // address bit value.
    let active_cs = cs_check_gpios(board, ct, cs);
    let mut force_list: Vec<u8> = Vec::new();
    for i in 0..span {
        let gpio = base + i;
        if !addr_gpios.contains(&gpio) && !active_cs.contains(&gpio) {
            force_list.push(i); // MSB=0 → force this GPIO to 0
        }
    }

    let total = ALG_ADDR_HDR + ADDR0_FIXED_PARAMS + force_list.len();
    let addr = w.abs_addr();
    w.write_u8(total as u8);
    w.write_u8(AlgAddr::Addr0 as u8);
    w.write_u8(0); // gpio_base
    w.write_u8(DEFAULT_ADDR_DELAY_CYCLES); // num_delay_cycles
    w.write_u8(base); // base_addr_pin
    w.write_u8(span); // num_addr_pins
    w.write_u8(span); // num_rom_table_bits (= span; image_size = 2^span)
    w.write_bytes(&force_list);
    w.align4();
    Ok(addr)
}

fn write_alg_data_config(w: &mut BufWriter, board: Board, ct: ChipType) -> u32 {
    let data_pins = board.data_pins();
    let num_data = ct.data_pins().len().min(data_pins.len());
    let data_base = if num_data > 0 {
        *data_pins[..num_data].iter().min().unwrap()
    } else {
        0
    };
    let word_size: u8 = if num_data == 16 { 16 } else { 8 };

    let addr = w.abs_addr();
    // onerom_alg_data_config_t header (6 bytes)
    w.write_u8(AlgData::Data0 as u8);
    w.write_u8(DATA0_PARAMS as u8); // param_len
    w.write_u16(DEFAULT_CLKDIV_INT); // clkdiv_int
    w.write_u8(DEFAULT_CLKDIV_FRAC); // clkdiv_frac
    w.write_u8(PAD); // pad
    // onerom_alg_data0_param_t (3 bytes)
    w.write_u8(0); // gpio_base
    w.write_u8(data_base); // base_data_pin
    w.write_u8(word_size); // word_size
    w.align4();
    addr
}

fn write_dma_config(w: &mut BufWriter, ct: ChipType) -> u32 {
    let bit_mode = if ct.data_pins().len() == 16 {
        BitMode::Mode16 as u8
    } else {
        BitMode::Mode8 as u8
    };
    let addr = w.abs_addr();
    w.write_u8(bit_mode); // bit_mode
    w.write_u8(0); // continuous = 0 (single-shot)
    w.write_u8(PAD); // reserved[0]
    w.write_u8(PAD); // reserved[1]
    addr
}

fn write_alg_config_struct(
    w: &mut BufWriter,
    cs_ptr: u32,
    addr_ptr: u32,
    data_ptr: u32,
    dma_ptr: u32,
) -> u32 {
    let base = w.abs_addr();
    w.write_u32(cs_ptr);
    w.write_u32(addr_ptr);
    w.write_u32(data_ptr);
    w.write_u32(dma_ptr);
    w.pad(16); // reserved[4 × 4]
    debug_assert_eq!((w.abs_addr() - base) as usize, ALG_CONFIG_LEN);
    base
}

fn write_fw_overrides_v2(w: &mut BufWriter, config: &FirmwareConfig) {
    let core = fw_overrides_core(config);
    w.write_bytes(&core);
    // Pad from 24-byte core to 32-byte v2 struct
    w.pad(FW_OVERRIDES_LEN - FW_OVERRIDES_CORE_LEN);
}

fn write_rom_slot(
    w: &mut BufWriter,
    data_ptr: u32,
    image_size: u32,
    roms_array_ptr: u32,
    rom_count: u8,
    alg_ptr: u32,
    fw_ptr: u32,
) {
    let start = w.abs_addr();
    w.write_u32(data_ptr); // data
    w.write_u32(image_size); // size
    w.write_u32(roms_array_ptr); // roms
    w.write_u8(rom_count); // rom_count
    w.pad(3); // reserved1[3]
    w.write_u32(alg_ptr); // alg
    w.write_u32(fw_ptr); // firmware_overrides
    w.pad(8); // reserved2[8]
    debug_assert_eq!((w.abs_addr() - start) as usize, ROM_SLOT_LEN);
}

// ---------------------------------------------------------------------------
// CS helpers
// ---------------------------------------------------------------------------

/// Returns the sorted set of GPIO pins actively checked by the CS algorithm
/// for this slot (CE/OE for EPROM-type chips, CS1/CS2/CS3 for mask ROM types).
///
/// These GPIOs must NOT appear in the addr algorithm force_list: when the
/// firmware is serving, these pins are in their active state, which naturally
/// produces the correct contribution to the image address.
fn cs_check_gpios(board: Board, ct: ChipType, cs: &ChipSet) -> Vec<u8> {
    let chip = &cs.chips()[0];
    let mut gpios = Vec::new();

    #[allow(clippy::collapsible_if)]
    match chip.cs_config() {
        CsConfig::CeOe => {
            let ce = board.pin_ce(ct);
            let oe = board.pin_oe(ct);
            if ce != GPIO_NONE {
                gpios.push(ce);
            }
            if oe != GPIO_NONE {
                gpios.push(oe);
            }
        }
        CsConfig::ChipSelect { cs2, cs3, .. } => {
            let p1 = board.pin_cs1(ct);
            if p1 != GPIO_NONE {
                gpios.push(p1);
            }

            if let Some(l) = cs2 {
                if *l != CsLogic::Ignore {
                    let p = board.pin_cs2(ct);
                    if p != GPIO_NONE {
                        gpios.push(p);
                    }
                }
            }
            if let Some(l) = cs3 {
                if *l != CsLogic::Ignore {
                    let p = board.pin_cs3(ct);
                    if p != GPIO_NONE {
                        gpios.push(p);
                    }
                }
            }
        }
    }

    gpios.sort_unstable();
    gpios
}

/// Compute the `cs_pin_inversion` bitmask.  Bit n = 1 means the nth CS pin
/// in `sorted` is active-high (and should be inverted before the CS check).
/// CE/OE lines (CeOe chips) are always active-low → inversion = 0.
fn cs_pin_inversion(board: Board, ct: ChipType, cs: &ChipSet, sorted: &[u8]) -> u8 {
    let chip = &cs.chips()[0];
    match chip.cs_config() {
        CsConfig::CeOe => 0,

        CsConfig::ChipSelect { cs1, cs2, cs3 } => {
            // Map each active GPIO to its CsLogic polarity
            let cs1_pin = board.pin_cs1(ct);
            let cs2_pin = board.pin_cs2(ct);
            let cs3_pin = board.pin_cs3(ct);

            let lookup: [(u8, CsLogic); 3] = [
                (cs1_pin, *cs1),
                (cs2_pin, cs2.unwrap_or(CsLogic::Ignore)),
                (cs3_pin, cs3.unwrap_or(CsLogic::Ignore)),
            ];

            let mut inv: u8 = 0;
            for (bit, &gpio) in sorted.iter().enumerate() {
                #[allow(clippy::collapsible_if)]
                if let Some(&(_, logic)) = lookup.iter().find(|(g, _)| *g == gpio) {
                    if logic == CsLogic::ActiveHigh {
                        inv |= 1 << bit;
                    }
                }
            }
            inv
        }
    }
}

// ---------------------------------------------------------------------------
// Image size
// ---------------------------------------------------------------------------

/// Compute the ROM image size for `ct` on `board`.
///
/// For ROM chips: image_size = 2^span, where span = max(addr_gpio) − min(addr_gpio) + 1.
/// Address GPIOs are derived dynamically from `BoardPinMap` and `ct.address_pins()`.
/// This produces chip-specific, board-specific sizes that are often much smaller
/// than the fixed 64 KB used by firmware < v0.7.0.
///
/// Example: 2364 on Fire24E → span 13 (GPIOs 11–23) → 8 KB.
///          2316 on Fire24E → span 11 (GPIOs 13–23) → 2 KB.
///          2364 on Fire24A → span 16 (GPIOs 0–15)  → 64 KB (suboptimal pin layout).
fn chip_image_size(board: Board, ct: ChipType) -> usize {
    if ct.chip_function() == ChipFunction::Plugin {
        return 65536; // plugins use a fixed 64 KB flash region
    }
    let num_addr = ct.num_addr_lines();
    if num_addr == 0 {
        return 0;
    }

    let bpm = BoardPinMap::new(board);
    let gpios: Vec<u8> = ct.address_pins()[..num_addr]
        .iter()
        .filter_map(|&p| bpm.gpio_for_chip_pin(p))
        .collect();

    if gpios.is_empty() {
        return 0;
    }

    let base = *gpios.iter().min().unwrap();
    let max = *gpios.iter().max().unwrap();
    1usize << (max - base + 1)
}

// ---------------------------------------------------------------------------
// metadata_len helpers (mirror the writing logic without touching a buffer)
// ---------------------------------------------------------------------------

fn cs_alg_written_size(board: Board, ct: ChipType, cs: &ChipSet) -> usize {
    if ct.chip_function() == ChipFunction::Plugin {
        return align4(ALG_CS_HDR + CS0_PARAMS);
    }
    let sorted = cs_check_gpios(board, ct, cs);
    if sorted.is_empty() {
        return align4(ALG_CS_HDR + CS0_PARAMS);
    }
    let span = *sorted.last().unwrap() - sorted[0] + 1;
    if span as usize == sorted.len() {
        align4(ALG_CS_HDR + CS0_PARAMS)
    } else {
        align4(ALG_CS_HDR + CS1_PARAMS)
    }
}

fn addr_alg_written_size(board: Board, ct: ChipType, cs: &ChipSet) -> usize {
    if ct.address_pins().is_empty() || ct.num_addr_lines() == 0 {
        return align4(ALG_ADDR_HDR + ADDR0_FIXED_PARAMS);
    }
    let bpm = BoardPinMap::new(board);
    let num_addr = ct.num_addr_lines();
    let gpios: Vec<u8> = ct.address_pins()[..num_addr]
        .iter()
        .filter_map(|&p| bpm.gpio_for_chip_pin(p))
        .collect();
    if gpios.is_empty() {
        return align4(ALG_ADDR_HDR + ADDR0_FIXED_PARAMS);
    }

    let base = *gpios.iter().min().unwrap();
    let max = *gpios.iter().max().unwrap();
    let span = (max - base + 1) as usize;

    let active_cs = cs_check_gpios(board, ct, cs);
    let force_count = (0..span as u8)
        .filter(|&i| {
            let g = base + i;
            !gpios.contains(&g) && !active_cs.contains(&g)
        })
        .count();

    align4(ALG_ADDR_HDR + ADDR0_FIXED_PARAMS + force_count)
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

#[inline]
fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn unique_chip_types(chip_sets: &[ChipSet]) -> Vec<ChipType> {
    let mut seen: Vec<ChipType> = Vec::new();
    for cs in chip_sets {
        for chip in cs.chips() {
            let ct = *chip.chip_type();
            if !seen.contains(&ct) {
                seen.push(ct);
            }
        }
    }
    seen
}
