// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Slot geometry derived from the firmware metadata.
//!
//! The firmware metadata is the source of truth for the address-read PIO
//! window, the per-ROM address/data GPIO maps, the board X-pin GPIOs, and the
//! `GpioOverLow` input-override list.  Rather than re-deriving any of this from
//! chip/board definitions, this module runs the same `onerom_gen` build the
//! production firmware uses, parses the resulting blob, and returns the flat
//! geometry fields a consumer needs.
//!
//! It deliberately does no set algebra: it surfaces what gen emitted and lets
//! the caller assemble the used set and compute gaps, because only the caller
//! knows which X pins a given set actually uses (banked vs multi vs single) and
//! which chip-select GPIOs apply.  Two consumers (pio-tester and
//! plugin-api-tester) share the build→parse cost via [`build_header`]; the
//! former additionally uses [`slot_geometry`] for the forced-low gap check, and
//! [`expected_rom_slot_size`] is the thin region-size caller the RAM-slot tests
//! use.
//!
//! Running that gen build needs `onerom-fw` to fetch the ROM files, which does
//! not compile for wasm, so this module stays here rather than joining the pure
//! geometry in `onerom-fw-geometry`.  [`chip_substitution`] does live there —
//! it is pure — and is re-exported below so `geometry::chip_substitution` still
//! resolves.

use std::path::Path;

use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::Board;
use onerom_config::mcu::{Family, Variant as McuVariant};
use onerom_gen::{Builder, Config};
use onerom_metadata::{
    DeviceMemoryView, GpioOverride, METADATA_BASE, OneromAlgAddrConfig, OneromMetadataHeader,
    RomSlotType,
};

pub use onerom_fw_geometry::substitution::chip_substitution;

/// `GpioOverLow` discriminant (top two bits of an override-config byte).
const OVERRIDE_LOW: u8 = GpioOverride::GpioOverLow as u8;
/// `GpioOverInvert` discriminant.  Banked/multi sets emit their *used* bank
/// select / secondary-CS X pins as inverted overrides; these are the only
/// place X-pin identity survives into the metadata blob (there is no flat
/// hardware/X section).
const OVERRIDE_INVERT: u8 = GpioOverride::GpioOverInvert as u8;

/// Flat per-slot geometry read back from the firmware metadata.
///
/// All fields are taken verbatim from what `onerom_gen` emitted; no set algebra
/// is performed here.  GPIO maps have `GPIO_NONE` sentinels removed so the
/// vectors contain only real GPIO numbers.
#[derive(Debug, Clone)]
pub struct SlotGeometry {
    /// Address-read PIO window base GPIO.
    ///
    /// The emitted `alg_addr.gpio_base` is the PIO `GPIOBASE` (0 or 16) and
    /// `alg_addr.base_addr_pin` is the window's offset from it, so the absolute
    /// window base is `gpio_base + base_addr_pin`.
    pub addr_window_base: u8,
    /// Address-read PIO window length in GPIOs (`alg_addr.num_addr_pins`).
    /// The window is the contiguous range
    /// `[addr_window_base, addr_window_base + addr_window_len)`.
    pub addr_window_len: u8,
    /// Address GPIOs for the slot's primary ROM (`roms[0].pin_map.addr`,
    /// `GPIO_NONE` removed).
    pub addr_pins: Vec<u8>,
    /// Data GPIOs for the slot's primary ROM (`roms[0].pin_map.data`,
    /// `GPIO_NONE` removed).
    pub data_pins: Vec<u8>,
    /// Board X-pin GPIOs from `hw.gpio_x1`/`hw.gpio_x2`, indexed `[0] = X1`,
    /// `[1] = X2` (`GPIO_NONE` removed).  Entries are empty when an X pin is
    /// absent.  These are *all* board X pins; which a given slot actually uses
    /// (and so are part of the used set rather than gaps) depends on the set
    /// type and is the caller's decision: banked uses the first
    /// `x_pins_needed`, multi the first `n_secondary`, single none.
    pub x_pin_gpios: Vec<Vec<u8>>,
    /// Absolute GPIOs gen flagged `GpioOverInvert` for this slot: the *used*
    /// bank-select / secondary-CS X pins (empty for single sets).  Exposed as a
    /// cross-reference for the X pins the caller selects from `x_pin_gpios`.
    pub inverted_gpios: Vec<u8>,
    /// Absolute GPIOs gen flagged `GpioOverLow` for this slot — the forced-low
    /// gaps.  Empty when no override config is present.
    pub forced_low_gpios: Vec<u8>,
    /// Served region size in bytes (`slot.size`).
    pub region_size: u32,
}

/// `true` if a slot is a plugin slot (excluded from the flash-slot enumeration
/// the firmware exposes under EXCLUDE_PLUGINS).
fn is_plugin(slot_type: RomSlotType) -> bool {
    matches!(
        slot_type,
        RomSlotType::RomSlotTypePluginSystem
            | RomSlotType::RomSlotTypePluginUser
            | RomSlotType::RomSlotTypePluginPio
    )
}

/// Build the firmware metadata for `config` on `board` and parse it.
///
/// Runs the exact `onerom_gen` pipeline the production generator uses: real ROM
/// files are loaded raw via `get_rom_files` and the builder applies
/// `size_handling` itself, once, so the resulting layout matches the firmware
/// under test.
///
/// `get_rom_files` resolves `spec.source` against the current working
/// directory, but the tester does not run from the project root.  Each chip's
/// relative `file` path is therefore rewritten to an absolute path under
/// `base_dir` before building (the same base the oracle resolves against), so
/// loading is independent of cwd.  Absolute and http(s) sources are left
/// untouched.
pub fn build_header(
    config: &Config,
    board: Board,
    fw_version: FirmwareVersion,
    base_dir: &Path,
) -> Result<OneromMetadataHeader, String> {
    let mut abs_config = config.clone();
    for set in &mut abs_config.chip_sets {
        for chip in &mut set.chips {
            if chip.file.is_empty()
                || chip.file.starts_with("http://")
                || chip.file.starts_with("https://")
            {
                continue;
            }
            let p = Path::new(&chip.file);
            if p.is_relative() {
                chip.file = base_dir.join(p).to_string_lossy().into_owned();
            }
        }
    }

    let config_json =
        serde_json::to_string(&abs_config).map_err(|e| format!("reserialize config: {e}"))?;

    let mut builder = Builder::from_json(fw_version, Family::Rp2350, &config_json)
        .map_err(|e| format!("Builder::from_json: {e}"))?;

    onerom_fw::get_rom_files(&mut builder).map_err(|e| format!("get_rom_files: {e}"))?;

    let props = FirmwareProperties::new(
        fw_version,
        board,
        McuVariant::RP2350,
        ServeAlg::default(),
        false,
    )
    .map_err(|e| format!("FirmwareProperties::new: {e}"))?;

    let (metadata_buf, _) = builder
        .build(props)
        .map_err(|e| format!("builder.build: {e}"))?;

    let view = DeviceMemoryView::new(&metadata_buf, METADATA_BASE);
    OneromMetadataHeader::parse(&view, METADATA_BASE).map_err(|e| format!("metadata parse: {e:?}"))
}

/// Parse the metadata and return the [`SlotGeometry`] for the `set_idx`-th
/// non-plugin ROM slot (matching the firmware's EXCLUDE_PLUGINS enumeration,
/// which is what `sel_image` selects).
pub fn slot_geometry(
    config: &Config,
    board: Board,
    fw_version: FirmwareVersion,
    base_dir: &Path,
    set_idx: usize,
) -> Result<SlotGeometry, String> {
    let header = build_header(config, board, fw_version, base_dir)?;

    let slot = header
        .rom_slots
        .iter()
        .filter(|s| !is_plugin(s.slot_type))
        .nth(set_idx)
        .ok_or_else(|| format!("no non-plugin ROM slot {set_idx} in metadata"))?;

    let alg = slot
        .alg
        .as_ref()
        .ok_or_else(|| format!("ROM slot {set_idx} has no alg config (plugin?)"))?;

    let (addr_window_base, addr_window_len) = match alg.alg_addr {
        OneromAlgAddrConfig::AlgAddr0 {
            gpio_base,
            base_addr_pin,
            num_addr_pins,
            ..
        } => (gpio_base + base_addr_pin, num_addr_pins),
    };

    let pin_map = slot
        .roms
        .first()
        .and_then(|r| r.pin_map.as_ref())
        .ok_or_else(|| format!("ROM slot {set_idx} primary ROM has no pin_map"))?;

    let addr_pins = pin_map
        .addr
        .iter()
        .copied()
        .filter(|&g| g != onerom_metadata::GPIO_NONE)
        .collect();
    let data_pins = pin_map
        .data
        .iter()
        .copied()
        .filter(|&g| g != onerom_metadata::GPIO_NONE)
        .collect();

    // Partition the override config into inverted (used X) and forced-low (gap)
    // GPIOs.  The low 6 bits of each byte are the absolute GPIO; the top two are
    // the override class.
    let collect_overrides = |class: u8| -> Vec<u8> {
        alg.gpio_override_config
            .as_ref()
            .map(|o| {
                o.params
                    .iter()
                    .filter(|&&b| (b >> 6) == class)
                    .map(|&b| b & 0x3F)
                    .collect()
            })
            .unwrap_or_default()
    };
    let inverted_gpios = collect_overrides(OVERRIDE_INVERT);
    let forced_low_gpios = collect_overrides(OVERRIDE_LOW);

    let strip = |raw: &[u8]| -> Vec<u8> {
        raw.iter()
            .copied()
            .filter(|&g| g != onerom_metadata::GPIO_NONE)
            .collect()
    };
    let x_pin_gpios = vec![strip(&header.hw.gpio_x1), strip(&header.hw.gpio_x2)];

    Ok(SlotGeometry {
        addr_window_base,
        addr_window_len,
        addr_pins,
        data_pins,
        x_pin_gpios,
        inverted_gpios,
        forced_low_gpios,
        region_size: slot.size,
    })
}

/// Served region size in bytes for the `set_idx`-th non-plugin slot.
///
/// Thin caller over [`slot_geometry`]; retained as the entry point the RAM-slot
/// count/info tests use so chips with address-pin gaps (e.g. 231024) get the
/// gen-computed region size rather than a nominal address-line guess.
pub fn expected_rom_slot_size(
    config: &Config,
    board: Board,
    fw_version: FirmwareVersion,
    base_dir: &Path,
    set_idx: usize,
) -> Result<u32, String> {
    slot_geometry(config, board, fw_version, base_dir, set_idx).map(|g| g.region_size)
}
