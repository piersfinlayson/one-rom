// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the GPIO plugin API (`ORA_ID_GPIO_QUERY`).
//!
//! `ora_gpio_set`'s register writes are compiled out under `TEST_BUILD`, so
//! only the query side — and in particular its `use` classification — is
//! testable here.
//!
//! # The oracle
//!
//! The classification lives in `pio_get_gpio_use()`
//! (`firmware/src/piodma/piorom2.c`) and duplicates knowledge the PIO setup
//! path also holds, so it can silently desync when slot layouts change.  The
//! oracle therefore has to be something other than a restatement of the same
//! derivation.  Two independent sources are used:
//!
//! 1. **The serving algorithm configuration in the generated firmware
//!    metadata**, read back through the same `onerom_gen` build the firmware
//!    under test was built from (see [`onerom_fw_tester::geometry`]).  This is
//!    a Rust reimplementation of the span arithmetic the C firmware does in
//!    `retrieve_gpio_init()`, over the same bytes, so an arithmetic slip in
//!    either shows up as a mismatch.  It covers every pin the serving
//!    algorithms name, including ones `retrieve_gpio_init()` does not itself
//!    collect (the `ALG_CS_2` qualifier pins and `ALG_DATA_1`'s A-1 pin), so a
//!    pin serving genuinely reads but the classifier reports free is a
//!    failure, not an invisible gap.
//!
//! 2. **The apio emulation's own record of what the serving setup configured**
//!    — `_apio_emulated_gpios.output_block[]`, written by
//!    `APIO_GPIO_INPUT_OUTPUT` in `setup_serving_gpios()`.  This is what
//!    serving actually did rather than what its configuration says, and it
//!    pins down the driven (data) pins exactly.
//!
//!    Note the companion `input_only` bit is *not* usable as an oracle for the
//!    read pins: `setup_initial_gpios()` (`firmware/src/rp235x.c`) applies
//!    `APIO_GPIO_INPUT_ONLY` to every GPIO at boot, so after boot every pin
//!    except the data pins has it set, whether serving reads it or not.  That
//!    is the same reason the firmware needs this API at all — an address or CS
//!    pin is indistinguishable from a free one by register inspection.

use std::path::Path;

use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_config::mcu::RpVariant;
use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_fw_tester::geometry;
use onerom_gen::Config;
use onerom_metadata::{
    GPIO_NONE, OneromAlgAddrConfig, OneromAlgCsConfig, OneromAlgDataConfig, OneromMetadataHeader,
    RomSlotType,
};

/// GPIOs on the running RP2350 variant, mirroring the firmware's `max_gpios[]`
/// (`firmware/src/constants.c`), which `MAX_GPIOS` indexes by variant.
///
/// A board with no RP variant boots the emulation as an RP235xA, matching
/// [`Emulator::set_rp_variant`].
fn max_gpios(board: Board) -> u8 {
    match board.rp_variant() {
        Some(RpVariant::Rp235xB) => 48,
        _ => 30,
    }
}

/// A bitmask of the `count` GPIOs starting at `base`, or 0 if there are none.
///
/// `base` is `GPIO_NONE` when the slot has no such span; the firmware's own
/// span tests are all guarded by `< MAX_GPIOS`, so an absent span contributes
/// nothing.
fn span(base: u8, count: u8) -> u64 {
    if base == GPIO_NONE || count == 0 || base as u32 + count as u32 > 64 {
        return 0;
    }
    let mask = if count >= 64 {
        u64::MAX
    } else {
        (1u64 << count) - 1
    };
    mask << base
}

/// A single GPIO as a bitmask, or 0 if it is absent.
fn pin(gpio: u8) -> u64 {
    if gpio == GPIO_NONE || gpio >= 64 {
        0
    } else {
        1u64 << gpio
    }
}

/// The pins the active slot's serving algorithms name, split by whether
/// serving drives them or only reads them.
struct ServingSet {
    /// Pins PIO drives — the data pins.
    driven: u64,
    /// Pins serving reads: the address span, the chip-select span, the /BYTE
    /// pin, `ALG_CS_2`'s qualifier pins and `ALG_DATA_1`'s A-1 pin.
    read: u64,
}

/// Assemble the serving set for the `set_idx`-th non-plugin ROM slot from the
/// generated metadata.
///
/// Every GPIO field in the algorithm configuration is relative to that
/// algorithm's `gpio_base` (the PIO block's `GPIOBASE`), so each is offset by
/// its own base — which is exactly the arithmetic `retrieve_gpio_init()` does,
/// including only offsetting the /BYTE pin when one is present.
fn serving_set(header: &OneromMetadataHeader, set_idx: usize) -> Result<ServingSet, String> {
    let is_plugin = |t: RomSlotType| {
        matches!(
            t,
            RomSlotType::RomSlotTypePluginSystem
                | RomSlotType::RomSlotTypePluginUser
                | RomSlotType::RomSlotTypePluginPio
        )
    };

    let slot = header
        .rom_slots
        .iter()
        .filter(|s| !is_plugin(s.slot_type))
        .nth(set_idx)
        .ok_or_else(|| format!("no non-plugin ROM slot {set_idx} in metadata"))?;
    let alg = slot
        .alg
        .as_ref()
        .ok_or_else(|| format!("ROM slot {set_idx} has no alg config"))?;

    // Chip select and data.  The common fields are repeated per variant
    // because each variant is a distinct enum shape; the extras differ.
    let (cs_base, cs_pins, data_base, data_pins, cs_extra) = match alg.alg_cs {
        OneromAlgCsConfig::AlgCs0 {
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            byte_pin,
            ..
        } => (
            gpio_base + base_cs_pin,
            num_cs_pins,
            gpio_base + base_data_pin,
            num_data_pins,
            // The /BYTE pin sits outside the CS span, so it has to be named
            // specifically or it would fall through to free.
            if byte_pin == GPIO_NONE {
                0
            } else {
                pin(gpio_base + byte_pin)
            },
        ),
        OneromAlgCsConfig::AlgCs1 {
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            ..
        } => (
            gpio_base + base_cs_pin,
            num_cs_pins,
            gpio_base + base_data_pin,
            num_data_pins,
            // cs_ignore_index names a position the select field masks out; it
            // is still inside the CS span and still sampled, so it needs no
            // separate handling.
            0,
        ),
        OneromAlgCsConfig::AlgCs2 {
            gpio_base,
            base_cs_pin,
            num_cs_pins,
            base_data_pin,
            num_data_pins,
            base_qualifier_pin,
            num_qualifier_pins,
            ..
        } => (
            gpio_base + base_cs_pin,
            num_cs_pins,
            gpio_base + base_data_pin,
            num_data_pins,
            // The qualifier pins are address lines the CS state machine
            // samples to decide whether this bank is selected.
            span(gpio_base + base_qualifier_pin, num_qualifier_pins),
        ),
    };

    let OneromAlgAddrConfig::AlgAddr0 {
        gpio_base,
        base_addr_pin,
        num_addr_pins,
        ..
    } = alg.alg_addr;
    let addr = span(gpio_base + base_addr_pin, num_addr_pins);

    // The data algorithm names the /BYTE pin again, plus the A-1 pin the
    // 16-bit data state machine reads to pick a half-word.
    let data_extra = match alg.alg_data {
        OneromAlgDataConfig::AlgData0 { .. } => 0,
        OneromAlgDataConfig::AlgData1 {
            gpio_base,
            byte_pin,
            a_minus_1_pin,
            ..
        } => pin(gpio_base + byte_pin) | pin(gpio_base + a_minus_1_pin),
    };

    let driven = span(data_base, data_pins);
    let read = (addr | span(cs_base, cs_pins) | cs_extra | data_extra) & !driven;

    Ok(ServingSet { driven, read })
}

/// GPIOs the apio emulation records as PIO-driven outputs, i.e. those
/// `setup_serving_gpios()` passed to `APIO_GPIO_INPUT_OUTPUT`.
fn apio_driven_pins(max_gpios: u8) -> u64 {
    // SAFETY: `_apio_emulated_gpios` is a plain C global written by the
    // firmware under emulation; this reads it through a raw pointer without
    // forming a reference to the `static mut`.
    let output_block = unsafe { (*core::ptr::addr_of!(ffi::_apio_emulated_gpios)).output_block };
    let mut mask = 0u64;
    for gpio in 0..max_gpios {
        if output_block[gpio as usize] >= 0 {
            mask |= 1u64 << gpio;
        }
    }
    mask
}

/// The board's own system GPIOs, read back through the metadata getters.
///
/// These are the pins `ora_gpio_query` reports as `ORA_GPIO_USE_SYSTEM` when
/// serving is not using them.  Each is `GPIO_NONE` on a board without it.
fn system_pins(emu: &Emulator) -> Result<u64, String> {
    let keys: &[(ffi::ora_metadata_key_t, &str)] = &[
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_STATUS,
            "GPIO_STATUS",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_NEOPIXEL,
            "GPIO_NEOPIXEL",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_VBUS,
            "GPIO_VBUS",
        ),
        (
            ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_EXT_FLASH_CS,
            "GPIO_EXT_FLASH_CS",
        ),
    ];

    let mut mask = 0u64;
    for (key, label) in keys {
        let (result, value) = emu.get_metadata_uint(*key);
        if !result.is_ok() {
            return Err(format!("{label}: expected OK, got {result:?}"));
        }
        let value = value.ok_or_else(|| format!("{label}: OK but no value"))?;
        mask |= pin(value as u8);
    }
    Ok(mask)
}

fn use_name(value: u8) -> String {
    match value as ffi::ora_gpio_use_t {
        ffi::ora_gpio_use_t_ORA_GPIO_USE_FREE => "FREE".to_string(),
        ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_READ => "SERVING_READ".to_string(),
        ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_DRIVEN => "SERVING_DRIVEN".to_string(),
        ffi::ora_gpio_use_t_ORA_GPIO_USE_SYSTEM => "SYSTEM".to_string(),
        other => format!("<unknown {other}>"),
    }
}

/// Verify `ora_gpio_query`'s `use` field for every GPIO on the device against
/// the serving set the firmware was built from, cross-checked against what the
/// apio emulation recorded serving doing.
pub fn test_gpio_use(
    emu: &Emulator,
    config: &Config,
    board: Board,
    fw_version: FirmwareVersion,
    base_dir: &Path,
    set_idx: usize,
) -> Result<(), String> {
    let max_gpios = max_gpios(board);

    let header = geometry::build_header(config, board, fw_version, base_dir)?;
    let serving = serving_set(&header, set_idx)?;
    let system = system_pins(emu)?;

    // Cross-check the metadata-derived data pins against what serving actually
    // handed to the PIO.  If these disagree the metadata-derived expectation
    // below is untrustworthy, so say so before reporting per-pin mismatches.
    let driven_by_apio = apio_driven_pins(max_gpios);
    if driven_by_apio != serving.driven {
        return Err(format!(
            "data pins disagree: metadata says 0x{:012X}, apio recorded serving configuring 0x{:012X}",
            serving.driven, driven_by_apio
        ));
    }

    let mut errors = Vec::new();
    let mut counts = [0usize; 4];

    for gpio in 0..max_gpios {
        let bit = 1u64 << gpio;
        let expected: u8 = if serving.driven & bit != 0 {
            ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_DRIVEN as u8
        } else if serving.read & bit != 0 {
            ffi::ora_gpio_use_t_ORA_GPIO_USE_SERVING_READ as u8
        } else if system & bit != 0 {
            // Serving takes precedence: a system pin the active slot also uses
            // is reported as what serving is using it for.
            ffi::ora_gpio_use_t_ORA_GPIO_USE_SYSTEM as u8
        } else {
            ffi::ora_gpio_use_t_ORA_GPIO_USE_FREE as u8
        };

        let (result, info) = emu.gpio_query(gpio);
        if !result.is_ok() {
            errors.push(format!("gpio {gpio}: query failed: {result:?}"));
            continue;
        }
        if info.size as usize != size_of::<ffi::ora_gpio_info_t>() {
            errors.push(format!(
                "gpio {gpio}: wrote {} bytes, expected {}",
                info.size,
                size_of::<ffi::ora_gpio_info_t>()
            ));
        }
        if info.gpio_use != expected {
            errors.push(format!(
                "gpio {gpio}: use {} expected {}",
                use_name(info.gpio_use),
                use_name(expected)
            ));
        }
        if (info.gpio_use as usize) < counts.len() {
            counts[info.gpio_use as usize] += 1;
        }
    }

    // An out-of-range GPIO is rejected rather than silently classified, which
    // is what makes the loop above a whole-device sweep.
    for gpio in [max_gpios, 63, 255] {
        let (result, _) = emu.gpio_query(gpio);
        if result != OraResult::InvalidArg {
            errors.push(format!(
                "gpio {gpio} (out of range): expected InvalidArg, got {result:?}"
            ));
        }
    }

    // The forward-compatibility contract: the firmware writes no more than the
    // caller's own sizeof and reports how much it wrote.  0xFF is the sentinel
    // the wrapper pre-fills unwritten fields with.
    let (result, info) = emu.gpio_query_sized(0, 2);
    if !result.is_ok() {
        errors.push(format!("short query: expected OK, got {result:?}"));
    } else if info.size != 2 || info.level != 0xFF || info.is_output != 0xFF {
        errors.push(format!(
            "short query: wrote {} bytes and touched level={} is_output={}",
            info.size, info.level, info.is_output
        ));
    }
    let (result, _) = emu.gpio_query_sized(0, 0);
    if result != OraResult::InvalidSize {
        errors.push(format!(
            "zero-size query: expected InvalidSize, got {result:?}"
        ));
    }

    if errors.is_empty() {
        println!(
            "  {} GPIOs: {} free, {} serving-read, {} serving-driven, {} system",
            max_gpios, counts[0], counts[1], counts[2], counts[3]
        );
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
