// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for address and data mapping round-trips.
//!
//! Each test operates on the chip set selected for the current boot
//! (`set_idx` == `sel_image`), chip 0.

use std::path::Path;

use onerom_config::chip::ChipType;
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_fw_emulator::{Emulator, ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS, OraResult};
use onerom_fw_tester::geometry;
use onerom_gen::Config;
use onerom_metadata::{GPIO_NONE, OneromAlgDataConfig, OneromMetadataHeader, RomSlotType};

const MAX_FAILURES: usize = 5;

fn chip_type_from_config(config: &Config, set_idx: usize) -> Result<ChipType, String> {
    config
        .chip_sets
        .get(set_idx)
        .and_then(|s| s.chips.first())
        .map(|c| c.chip_type.resolved())
        .ok_or_else(|| format!("config has no chip set {} (or it has no chips)", set_idx))
}

/// Verify that the firmware's reported chip type and size match the config for
/// the booted image.
///
/// Steps:
/// 1. get_flash_slot_info(set_idx, EXCLUDE_PLUGINS) → rom_type
/// 2. ChipType::try_from_rbcp_u8(rom_type) → assert matches config chip type
/// 3. get_chip_size_from_type(rom_type) → assert matches config chip size
pub fn test_chip_size(emu: &Emulator, config: &Config, set_idx: usize) -> Result<(), String> {
    let expected_chip_type = chip_type_from_config(config, set_idx)?;
    let expected_size = expected_chip_type.size_bytes();

    let (result, info) =
        emu.get_flash_slot_info(set_idx as u8, ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS);
    if !result.is_ok() {
        return Err(format!("get_flash_slot_info failed: {:?}", result));
    }
    let info = info.ok_or_else(|| "get_flash_slot_info returned no info".to_string())?;

    let api_chip_type = ChipType::try_from_rbcp_u8(info.rom_type as u8)
        .ok_or_else(|| format!("rom_type {} is not a valid ChipType", info.rom_type))?;
    if api_chip_type != expected_chip_type {
        return Err(format!(
            "chip type mismatch: API={} config={}",
            api_chip_type.name(),
            expected_chip_type.name()
        ));
    }

    let api_size = emu.get_chip_size_from_type(info.rom_type);
    if api_size == 0 {
        return Err(format!(
            "get_chip_size_from_type returned 0 for rom_type {}",
            info.rom_type
        ));
    }
    if api_size != expected_size as u32 {
        return Err(format!(
            "chip size mismatch: API={} config={}",
            api_size, expected_size
        ));
    }

    println!(
        "  chip={} size={}",
        expected_chip_type.name(),
        expected_size
    );
    Ok(())
}

/// Verify that map_addr_to_phys → demangle_addr recovers every logical
/// address in [0, chip_size) for the booted image.
pub fn test_addr_mapping(emu: &Emulator, config: &Config, set_idx: usize) -> Result<(), String> {
    let chip_type = chip_type_from_config(config, set_idx)?;
    // Iterate the served address range, not the chip's full size.  For most
    // chips these are equal; for the 27C080 only the lower half is served (A19
    // is the chip-select line), so the upper half has no address-bit round-trip
    // to verify here — its CS-inactive / bus-tristate behaviour is covered by
    // the PIO serving tests.
    let chip_size = onerom_fw_tester::oracle::served_size(chip_type) as u32;

    let mut failures = Vec::new();
    for addr in 0..chip_size {
        let phys = emu.map_addr_to_phys(addr);
        let (result, recovered) = emu.demangle_addr(phys, false);
        if !result.is_ok() {
            failures.push(format!(
                "addr=0x{:04X}: demangle_addr failed: {:?}",
                addr, result
            ));
        } else if recovered != addr {
            failures.push(format!(
                "addr=0x{:04X}: round-trip gave 0x{:04X}",
                addr, recovered
            ));
        }
        if failures.len() >= MAX_FAILURES {
            failures.push("(further failures suppressed)".to_string());
            break;
        }
    }

    if failures.is_empty() {
        println!("  {} addresses verified", chip_size);
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// Verify that map_data_to_phys → demangle_data recovers every byte 0..=255.
///
/// Data pin mapping is board-fixed (independent of the booted image), so this
/// takes no `set_idx`.
pub fn test_data_mapping(emu: &Emulator, _config: &Config) -> Result<(), String> {
    let mut failures = Vec::new();
    for byte in 0u8..=255 {
        let phys = emu.map_data_to_phys(byte);
        let (result, recovered) = emu.demangle_data(phys);
        if !result.is_ok() {
            failures.push(format!(
                "byte=0x{:02X}: demangle_data failed: {:?}",
                byte, result
            ));
        } else if recovered != byte {
            failures.push(format!(
                "byte=0x{:02X}: round-trip gave 0x{:02X}",
                byte, recovered
            ));
        }
        if failures.len() >= MAX_FAILURES {
            failures.push("(further failures suppressed)".to_string());
            break;
        }
    }

    // A NULL out pointer is refused rather than written through — the guard
    // that stops a plugin's mistake becoming a fault on a device.
    let result = emu.demangle_data_null_out(0);
    if result != OraResult::InvalidArg {
        failures.push(format!(
            "demangle_data with NULL out: expected InvalidArg, got {result:?}"
        ));
    }

    if failures.is_empty() {
        println!("  256 bytes verified");
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// The data GPIOs of the booted slot, in D0..Dn order, and the bus width the
/// serving configuration was built for.
///
/// `pin_map.data[n]` holds D_n's absolute GPIO — that is how
/// `pio_map_data_to_phys()` reads it, subtracting the data base to get a bit
/// position — and `alg_data.word_size` is the width serving was configured
/// for.  Both come from the metadata blob the firmware under test was built
/// from, via the same `onerom_gen` build.
fn data_pin_expectation(
    header: &OneromMetadataHeader,
    set_idx: usize,
) -> Result<(Vec<u8>, u8), String> {
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

    let word_size = match alg.alg_data {
        OneromAlgDataConfig::AlgData0 { word_size, .. }
        | OneromAlgDataConfig::AlgData1 { word_size, .. } => word_size,
    };
    if word_size != 8 && word_size != 16 {
        return Err(format!("alg_data word_size is {word_size}, want 8 or 16"));
    }

    let pin_map = slot
        .roms
        .first()
        .and_then(|r| r.pin_map.as_ref())
        .ok_or_else(|| format!("ROM slot {set_idx} primary ROM has no pin_map"))?;

    let pins: Vec<u8> = pin_map.data[..word_size as usize].to_vec();
    if let Some(i) = pins.iter().position(|&g| g == GPIO_NONE) {
        return Err(format!(
            "pin_map has no GPIO for D{i}, but the slot serves {word_size} bits"
        ));
    }

    Ok((pins, word_size))
}

/// `ora_get_data_pin_nums` reports the booted slot's data GPIOs, in order, and
/// writes no more of them than it was asked for.
///
/// Arm from two sources that are not this call: the metadata's own
/// `pin_map.data`, which is where D_n's GPIO is recorded, and the apio
/// emulation's record of which GPIOs serving actually configured as PIO
/// outputs.  Stimulate at four caps and fence each with a canary byte after
/// the pins asked for, so a loop that runs one too far shows up as a
/// overwritten canary rather than as a count that happens to look right.
///
/// The bus width is the discriminating part.  The firmware branches on
/// `RUNTIME->bit_mode` to decide whether there are 8 pins or 16, while the
/// expectation here comes from `alg_data.word_size` — the width the serving
/// PIO was configured for.  A slot whose runtime mode and configured width
/// disagree fails here rather than handing a plugin half a bus.
pub fn test_data_pin_nums(
    emu: &Emulator,
    config: &Config,
    board: Board,
    fw_version: FirmwareVersion,
    base_dir: &Path,
    set_idx: usize,
) -> Result<(), String> {
    /// Written into every unasked-for byte of the buffer before each call.
    const CANARY: u8 = 0xA5;

    let header = geometry::build_header(config, board, fw_version, base_dir)?;
    let (expected, word_size) = data_pin_expectation(&header, set_idx)?;

    // Cross-check the metadata-derived pins against what serving handed the
    // PIO.  If these disagree the expectation is untrustworthy, so say so
    // before reporting anything about the call itself.
    let mut expected_mask = 0u64;
    for &gpio in &expected {
        if gpio >= 64 {
            return Err(format!("pin_map names GPIO {gpio}, past this device"));
        }
        expected_mask |= 1u64 << gpio;
    }
    let driven_by_apio = super::gpio::apio_driven_pins(super::gpio::max_gpios(board));
    if driven_by_apio != expected_mask {
        return Err(format!(
            "data pins disagree: metadata says 0x{expected_mask:012X}, apio recorded serving configuring 0x{driven_by_apio:012X}"
        ));
    }

    // Big enough for a 16-bit bus plus room to see an overrun.
    let mut buf = [CANARY; 24];
    let mut errors = Vec::new();

    // 0 asks for nothing and must write nothing; 1 stops after D0; the exact
    // width returns the lot; more than the width still returns just the lot.
    for cap in [0u8, 1, word_size, word_size + 4] {
        buf.fill(CANARY);
        let got = emu.get_data_pin_nums(&mut buf, cap);
        let want = cap.min(word_size);
        if got != want {
            errors.push(format!("cap {cap}: returned {got} pins, want {want}"));
            continue;
        }
        if buf[..want as usize] != expected[..want as usize] {
            errors.push(format!(
                "cap {cap}: pins {:?}, want {:?}",
                &buf[..want as usize],
                &expected[..want as usize]
            ));
        }
        if let Some(i) = buf[want as usize..].iter().position(|&b| b != CANARY) {
            errors.push(format!(
                "cap {cap}: wrote 0x{:02X} at index {}, past the {want} pins it reported",
                buf[want as usize + i],
                want as usize + i
            ));
        }
    }

    if errors.is_empty() {
        println!("  {word_size}-bit bus on GPIOs {expected:?}");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
