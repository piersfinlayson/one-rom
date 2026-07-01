// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for RAM and flash slot introspection.
//!
//! Per-image tests (RAM slot count/info, read-initial) operate on the chip set
//! selected for the current boot (`set_idx` == `sel_image`), chip 0.  The flash
//! slot count/info tests enumerate every slot and so are image-independent.

use onerom_config::chip::ChipType;
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_fw_emulator::{
    Emulator, ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS, ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS,
};
use onerom_gen::Config;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn chip_type_from_config(config: &Config, set_idx: usize) -> Result<ChipType, String> {
    config
        .chip_sets
        .get(set_idx)
        .and_then(|s| s.chips.first())
        .map(|c| c.chip_type)
        .ok_or_else(|| format!("config has no chip set {} (or it has no chips)", set_idx))
}

/// Expected SRAM region size for the booted image (`set_idx`): the size of the
/// `set_idx`-th non-plugin slot in the gen-built metadata, matching the
/// firmware's flash slot enumeration under EXCLUDE_PLUGINS.
///
/// Thin caller over the shared `geometry` build→parse; chips with address-pin
/// gaps (e.g. 231024) therefore get the gen-computed region size rather than a
/// nominal address-line guess.
fn expected_rom_slot_size(
    config: &Config,
    board: Board,
    fw_version: FirmwareVersion,
    base_dir: &std::path::Path,
    set_idx: usize,
) -> Result<u32, String> {
    onerom_fw_tester::geometry::expected_rom_slot_size(config, board, fw_version, base_dir, set_idx)
}

// ── Flash slot tests ──────────────────────────────────────────────────────────

/// Verify flash slot counts against config.
///
/// - flags=0:                     should equal chip_sets.len()
/// - EXCLUDE_PLUGINS:             should equal chip_sets.len() (no plugins in config)
/// - EXCLUDE_NON_PLUGINS:         should be 0 (no plugins in config)
pub fn test_flash_slot_count(emu: &Emulator, config: &Config) -> Result<(), String> {
    let expected = config.chip_sets.len() as u8;

    let all = emu.get_flash_slot_count(0);
    if all != expected {
        return Err(format!(
            "get_flash_slot_count(0): expected {} got {}",
            expected, all
        ));
    }

    let non_plugin = emu.get_flash_slot_count(ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS);
    if non_plugin != expected {
        return Err(format!(
            "get_flash_slot_count(EXCLUDE_PLUGINS): expected {} got {}",
            expected, non_plugin
        ));
    }

    let plugin_only = emu.get_flash_slot_count(ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS);
    if plugin_only != 0 {
        return Err(format!(
            "get_flash_slot_count(EXCLUDE_NON_PLUGINS): expected 0 got {}",
            plugin_only
        ));
    }

    println!("  {} flash slot(s)", all);
    Ok(())
}

/// Verify flash slot info for every slot against config ground truth.
///
/// For each slot i: rom_type must match config chip type, rom_count must
/// match config chip count for that set.
pub fn test_flash_slot_info(emu: &Emulator, config: &Config) -> Result<(), String> {
    let mut errors = Vec::new();

    for (i, chip_set) in config.chip_sets.iter().enumerate() {
        let expected_chip_type = match chip_set.chips.first() {
            Some(c) => c.chip_type,
            None => {
                errors.push(format!("slot {}: config chip set has no chips", i));
                continue;
            }
        };
        let expected_rom_count = chip_set.chips.len() as u8;

        let (result, info) = emu.get_flash_slot_info(i as u8, ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS);
        if !result.is_ok() {
            errors.push(format!(
                "slot {}: get_flash_slot_info failed: {:?}",
                i, result
            ));
            continue;
        }
        let info = match info {
            Some(i) => i,
            None => {
                errors.push(format!("slot {}: get_flash_slot_info returned no info", i));
                continue;
            }
        };

        let api_chip_type = match ChipType::try_from_rbcp_u8(info.rom_type as u8) {
            Some(t) => t,
            None => {
                errors.push(format!(
                    "slot {}: rom_type {} is not a valid ChipType",
                    i, info.rom_type
                ));
                continue;
            }
        };
        if api_chip_type != expected_chip_type {
            errors.push(format!(
                "slot {}: chip type mismatch: API={} config={}",
                i,
                api_chip_type.name(),
                expected_chip_type.name()
            ));
        }

        if info.rom_count != expected_rom_count {
            errors.push(format!(
                "slot {}: rom_count mismatch: API={} config={}",
                i, info.rom_count, expected_rom_count
            ));
        }
    }

    if errors.is_empty() {
        println!("  {} flash slot(s) verified", config.chip_sets.len());
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

// ── RAM slot tests ────────────────────────────────────────────────────────────

/// Verify RAM slot count against the value derived from the booted image's
/// gen-computed region size.
///
/// The expected count mirrors ora_get_ram_slot_count in the firmware, keyed on
/// the SRAM region size:
///   region ≤ 64KB  → 7
///   region ≤ 128KB → 3
///   region ≤ 256KB → 2
///   region > 256KB → 1
///
/// The region size comes from the same onerom_gen pipeline the firmware uses
/// (via expected_rom_slot_size), so chips with address-pin gaps (e.g. 231024,
/// whose unused middle pin pushes a 128KB ROM into a 256KB region) are handled
/// correctly — the chip's nominal address-line count would give the wrong
/// answer for exactly those chips.
pub fn test_ram_slot_count(
    emu: &Emulator,
    config: &Config,
    board: Board,
    fw_version: FirmwareVersion,
    base_dir: &std::path::Path,
    set_idx: usize,
) -> Result<(), String> {
    let chip_type = chip_type_from_config(config, set_idx)?;
    let actual = emu.get_ram_slot_count();

    let region_size = expected_rom_slot_size(config, board, fw_version, base_dir, set_idx)?;
    let expected = if region_size <= 64 * 1024 {
        7u8
    } else if region_size <= 128 * 1024 {
        3
    } else if region_size <= 256 * 1024 {
        2
    } else {
        1
    };

    if actual != expected {
        return Err(format!(
            "expected {} slot(s) for {} (region size {}), got {}",
            expected,
            chip_type.name(),
            region_size,
            actual
        ));
    }

    println!("  {} RAM slot(s)", actual);
    Ok(())
}

/// Verify RAM slot info for all slots.
///
/// - Expected region size: derived from the same onerom_gen pipeline the
///   firmware uses for the current board + booted image (set_idx).
/// - Every slot: size must match expected region size, and rom_type must match
///   the configured chip type.  ROM type is fixed for the run — it is set once
///   and every RAM slot is of that type — so all slots report it, not just the
///   active one.
/// - Slot 0: addr must be non-zero (region base).
/// - Slots 1..: addr must be sequential (addr[i] = addr[0] + i * size).
/// - Slot >= count: must return ORA_RESULT_INVALID_SLOT.
pub fn test_ram_slot_info(
    emu: &Emulator,
    config: &Config,
    board: Board,
    fw_version: FirmwareVersion,
    base_dir: &std::path::Path,
    set_idx: usize,
) -> Result<(), String> {
    let chip_type = chip_type_from_config(config, set_idx)?;
    let expected_size = expected_rom_slot_size(config, board, fw_version, base_dir, set_idx)?;
    let slot_count = emu.get_ram_slot_count();
    let mut errors = Vec::new();

    let mut base_addr = 0u32;
    for slot in 0..slot_count {
        let (result, info) = emu.get_ram_slot_info(slot);
        if !result.is_ok() {
            errors.push(format!(
                "slot {}: get_ram_slot_info failed: {:?}",
                slot, result
            ));
            continue;
        }
        let info = match info {
            Some(i) => i,
            None => {
                errors.push(format!("slot {}: get_ram_slot_info returned no info", slot));
                continue;
            }
        };

        if info.size != expected_size {
            errors.push(format!(
                "slot {}: size mismatch: API={} expected={}",
                slot, info.size, expected_size
            ));
        }

        // ROM type is fixed for the run — every RAM slot is of the configured
        // chip type, so check it for all slots.
        match ChipType::try_from_rbcp_u8(info.rom_type as u8) {
            Some(t) if t == chip_type => {}
            Some(t) => errors.push(format!(
                "slot {}: rom_type mismatch: API={} expected={}",
                slot,
                t.name(),
                chip_type.name()
            )),
            None => errors.push(format!(
                "slot {}: rom_type 0x{:02X} is not a valid ChipType (expected {})",
                slot,
                info.rom_type,
                chip_type.name()
            )),
        }

        // Slot 0 anchors the address sequence; later slots must follow it.
        if slot == 0 {
            if info.addr == 0 {
                errors.push("slot 0: addr is zero".to_string());
            }
            base_addr = info.addr;
        } else {
            let expected_addr = base_addr + (slot as u32 * expected_size);
            if info.addr != expected_addr {
                errors.push(format!(
                    "slot {}: addr mismatch: API=0x{:08X} expected=0x{:08X}",
                    slot, info.addr, expected_addr
                ));
            }
        }
    }

    // One past the end must be invalid.
    let (result, _) = emu.get_ram_slot_info(slot_count);
    if result != onerom_fw_emulator::OraResult::InvalidSlot {
        errors.push(format!(
            "slot {}: expected InvalidSlot, got {:?}",
            slot_count, result
        ));
    }

    if errors.is_empty() {
        println!(
            "  {} RAM slot(s) verified (region size={})",
            slot_count, expected_size
        );
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Verify that the active RAM slot is `expected_slot` after boot.
pub fn test_active_ram_slot(emu: &Emulator, expected_slot: u8) -> Result<(), String> {
    let (result, slot) = emu.get_active_ram_slot();
    if !result.is_ok() {
        return Err(format!("get_active_ram_slot failed: {:?}", result));
    }
    match slot {
        Some(s) if s == expected_slot => {
            println!("  active slot: {}", s);
            Ok(())
        }
        Some(n) => Err(format!("expected active slot {}, got {}", expected_slot, n)),
        None => Err("get_active_ram_slot returned no slot".to_string()),
    }
}

/// Verify that the ROM image pre-populated into the boot slot matches the
/// oracle for the booted image (`set_idx`).
///
/// `boot_slot` must be the slot the firmware populates at boot (the active
/// slot on entry); only that slot holds valid content here.
pub fn test_read_initial_slot(
    emu: &Emulator,
    config: &Config,
    base_dir: &std::path::Path,
    boot_slot: u8,
    set_idx: usize,
) -> Result<(), String> {
    let chip_set = config
        .chip_sets
        .get(set_idx)
        .ok_or_else(|| format!("config has no chip set {}", set_idx))?;
    let chip_config = chip_set
        .chips
        .first()
        .ok_or_else(|| format!("chip set {} has no chips", set_idx))?;
    let chip_type = chip_config.chip_type;

    let expected = onerom_fw_tester::oracle::load(chip_config, chip_type, base_dir);
    let chip_size = expected.len();

    let mut buf = vec![0u8; chip_size];
    let result = emu.read_ram_rom_slot(boot_slot, 0, &mut buf);
    if !result.is_ok() {
        return Err(format!("read_ram_rom_slot failed: {:?}", result));
    }

    let total_failures = expected
        .iter()
        .zip(buf.iter())
        .filter(|(e, g)| e != g)
        .count();
    let failures: Vec<String> = expected
        .iter()
        .zip(buf.iter())
        .enumerate()
        .filter(|(_, (e, g))| e != g)
        .take(5)
        .map(|(addr, (e, g))| format!("addr=0x{:04X}: expected=0x{:02X} got=0x{:02X}", addr, e, g))
        .collect();

    if failures.is_empty() {
        println!(
            "  {} bytes verified against oracle (slot {})",
            chip_size, boot_slot
        );
        Ok(())
    } else {
        Err(format!(
            "{} failure(s): {}",
            total_failures,
            failures.join("; ")
        ))
    }
}
