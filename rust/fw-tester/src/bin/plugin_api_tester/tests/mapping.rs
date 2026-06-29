// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for address and data mapping round-trips.
//!
//! Each test operates on the chip set selected for the current boot
//! (`set_idx` == `sel_image`), chip 0.

use onerom_config::chip::ChipType;
use onerom_fw_emulator::{Emulator, ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS};
use onerom_gen::Config;

const MAX_FAILURES: usize = 5;

fn chip_type_from_config(config: &Config, set_idx: usize) -> Result<ChipType, String> {
    config
        .chip_sets
        .get(set_idx)
        .and_then(|s| s.chips.first())
        .map(|c| c.chip_type)
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

    if failures.is_empty() {
        println!("  256 bytes verified");
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}
