// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for RAM slot reprogram and copy operations.
//!
//! RAM serving slots are passed in explicitly; the flash slot operated on is
//! the booted image (`set_idx` == `sel_image`), which also selects the oracle
//! and chip type for that image, chip 0.  The PIO verification tests set their
//! target slot active before serving, so they are independent of whichever slot
//! a previous test left active.

use std::path::Path;

use rand::{RngExt, SeedableRng, rngs::StdRng};

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_fw_emulator::{Emulator, ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS, OraResult};
use onerom_fw_tester::{
    oracle,
    pin_cache::PinCache,
    runner::{addr_before_cs_cycles, cs_to_data_cycles, run_mode},
};
use onerom_gen::{ChipConfig, Config};

const REPROGRAM_SEED: u64 = 0x1234_5678_90AB_CDEF;

// ── Private helpers ───────────────────────────────────────────────────────────

fn chip_type_from_config(config: &Config, set_idx: usize) -> Result<ChipType, String> {
    config
        .chip_sets
        .get(set_idx)
        .and_then(|s| s.chips.first())
        .map(|c| c.chip_type.resolved())
        .ok_or_else(|| format!("config has no chip set {} (or it has no chips)", set_idx))
}

/// Resolve chip 0 of the booted image's chip set.
fn chip_config_at(config: &Config, set_idx: usize) -> Result<&ChipConfig, String> {
    config
        .chip_sets
        .get(set_idx)
        .ok_or_else(|| format!("config has no chip set {}", set_idx))?
        .chips
        .first()
        .ok_or_else(|| format!("chip set {} has no chips", set_idx))
}

/// Whether the booted image's chip set forces 16-bit serving (`/BYTE`
/// ignored, the ROM always served as 16-bit).  When set, the PIO serves
/// only the 16-bit mode, so the PIO verification must not run an 8-bit
/// pass — mirrors the core tester's `get_force_16_bit` gate.
fn force_16_bit_for(config: &Config, set_idx: usize) -> bool {
    config
        .chip_sets
        .get(set_idx)
        .and_then(|s| s.firmware_overrides.as_ref())
        .and_then(|fw| fw.fire.as_ref())
        .map(|f| f.force_16_bit)
        .unwrap_or(false)
}

fn random_pattern(size: usize) -> Vec<u8> {
    let mut rng = StdRng::seed_from_u64(REPROGRAM_SEED);
    let mut buf = vec![0u8; size];
    rng.fill(&mut buf[..]);
    buf
}

fn verify(expected: &[u8], got: &[u8]) -> Result<usize, String> {
    let total = expected
        .iter()
        .zip(got.iter())
        .filter(|(e, g)| e != g)
        .count();
    if total == 0 {
        return Ok(expected.len());
    }
    let failures: Vec<String> = expected
        .iter()
        .zip(got.iter())
        .enumerate()
        .filter(|(_, (e, g))| e != g)
        .take(5)
        .map(|(addr, (e, g))| format!("addr=0x{:04X}: expected=0x{:02X} got=0x{:02X}", addr, e, g))
        .collect();
    Err(format!("{} failure(s): {}", total, failures.join("; ")))
}

fn read_and_verify(emu: &Emulator, slot: u8, expected: &[u8]) -> Result<usize, String> {
    let mut buf = vec![0u8; expected.len()];
    let result = emu.read_ram_rom_slot(slot, 0, &mut buf);
    if !result.is_ok() {
        return Err(format!(
            "read_ram_rom_slot(slot={}) failed: {:?}",
            slot, result
        ));
    }
    verify(expected, &buf)
}

/// Drive the PIO bus for every address and verify the served bytes match the
/// expected content.
fn pio_verify(
    emu: &Emulator,
    cache: &PinCache,
    oracle_bytes: &[u8],
    chip_type: ChipType,
    force_16_bit: bool,
) -> Result<usize, String> {
    let cycles_addr_before_cs = addr_before_cs_cycles(chip_type);

    let mut total_reads = 0u64;
    let mut total_failures = 0u64;
    let mut total_bus_failures = 0u64;
    let mut modes_run: Vec<u8> = Vec::new();

    for &mode in chip_type.bit_modes() {
        // force_16_bit serves only as 16-bit (/BYTE ignored), so an 8-bit
        // pass would compare against bytes the firmware never serves that
        // way.  Skip every non-16-bit mode, mirroring the core tester.
        if force_16_bit && mode != 16 {
            continue;
        }
        modes_run.push(mode);
        let cycles_cs_to_data = cs_to_data_cycles(chip_type, mode);
        // No forced-low gap drive here: the address-window override validation
        // is owned by the pio-tester's run loop.  Pass an empty gap set so this
        // reprogram-correctness pass is unchanged (the 4th return is always 0).
        let (reads, failures, bus_failures, _forced_low) = run_mode(
            emu,
            cache,
            oracle_bytes,
            mode,
            cycles_addr_before_cs,
            cycles_cs_to_data,
            0,
            0,
            (0u64, 0u64),
            &[],
        );
        total_reads += reads;
        total_failures += failures;
        total_bus_failures += bus_failures;
    }

    let modes_desc = modes_run
        .iter()
        .map(|m| format!("{}-bit", m))
        .collect::<Vec<_>>()
        .join(" + ");
    println!(
        "    PIO pass(es): {} ({})",
        modes_run.len(),
        if modes_desc.is_empty() {
            "none".to_string()
        } else {
            modes_desc
        }
    );

    if total_failures == 0 && total_bus_failures == 0 {
        Ok(total_reads as usize)
    } else {
        Err(format!(
            "{} data failure(s), {} bus violation(s)",
            total_failures, total_bus_failures
        ))
    }
}

/// Set `slot` active so the PIO serves it, returning a descriptive error on
/// failure.  Used by the PIO verification tests to make themselves independent
/// of whichever slot a previous test left active.
fn make_active(emu: &Emulator, slot: u8) -> Result<(), String> {
    let result = emu.set_active_ram_slot(slot);
    if !result.is_ok() {
        return Err(format!(
            "set_active_ram_slot({}) failed: {:?}",
            slot, result
        ));
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Verify the PIO serves the correct bytes for every address before any
/// reprogram or slot switch has occurred.
///
/// The boot slot is the active slot and contains the booted image's oracle.
/// If this test fails the PIO path itself is broken, independently of any
/// reprogram logic.
pub fn test_initial_pio_verify(
    emu: &Emulator,
    config: &Config,
    board: Board,
    base_dir: &Path,
    set_idx: usize,
) -> Result<(), String> {
    let chip_config = chip_config_at(config, set_idx)?;
    let chip_type = chip_config.chip_type.resolved();
    let oracle_bytes = oracle::load(chip_config, chip_type, base_dir);

    let cache = PinCache::build(chip_type, chip_config, board);

    pio_verify(
        emu,
        &cache,
        &oracle_bytes,
        chip_type,
        force_16_bit_for(config, set_idx),
    )
    .map(|n| println!("  {} bytes verified via PIO (initial boot slot)", n))
}

/// Switch to the slot that is *already* active and verify the PIO still serves
/// the booted image correctly.
///
/// This isolates the apio→epio pre-instruction apply path: the served region,
/// the X value pushed, and the buffer contents are all unchanged, so the only
/// thing exercised is one replay of the pio_switch_rom_region pre-instructions
/// against the live, enabled address SM (the in-flight delay must survive it).
pub fn test_noop_switch_pio_verify(
    emu: &Emulator,
    config: &Config,
    board: Board,
    base_dir: &Path,
    active_slot: u8,
    set_idx: usize,
) -> Result<(), String> {
    let chip_config = chip_config_at(config, set_idx)?;
    let chip_type = chip_config.chip_type.resolved();
    let oracle_bytes = oracle::load(chip_config, chip_type, base_dir);

    // No reprogram, no content change — switch to the already-active slot.
    make_active(emu, active_slot)?;

    let cache = PinCache::build(chip_type, chip_config, board);
    pio_verify(
        emu,
        &cache,
        &oracle_bytes,
        chip_type,
        force_16_bit_for(config, set_idx),
    )
    .map(|n| {
        println!(
            "  {} bytes verified via PIO (no-op switch → slot {})",
            n, active_slot
        )
    })
}

/// Verify that reprogram_ram_rom_slot returns SlotActive when allow_active=false
/// and `active_slot` is the active slot.
pub fn test_reprogram_reject_active(emu: &Emulator, active_slot: u8) -> Result<(), String> {
    let result = emu.reprogram_ram_rom_slot(active_slot, 0, &[0u8], false);
    if result == OraResult::SlotActive {
        Ok(())
    } else {
        Err(format!("expected SlotActive, got {:?}", result))
    }
}

/// Write a random pattern to non-active `slot`, read it back, verify
/// byte-for-byte.  Pattern size is the booted image's served size.
pub fn test_reprogram_round_trip(
    emu: &Emulator,
    config: &Config,
    slot: u8,
    set_idx: usize,
) -> Result<(), String> {
    let chip_type = chip_type_from_config(config, set_idx)?;
    let chip_size = oracle::served_size(chip_type);
    let pattern = random_pattern(chip_size);

    println!("  seed=0x{:016X}", REPROGRAM_SEED);

    let result = emu.reprogram_ram_rom_slot(slot, 0, &pattern, false);
    if !result.is_ok() {
        return Err(format!("reprogram_ram_rom_slot failed: {:?}", result));
    }

    read_and_verify(emu, slot, &pattern).map(|n| println!("  {} bytes verified (slot {})", n, slot))
}

/// Write a random pattern to the active `active_slot` (allow_active=true), read
/// it back, verify byte-for-byte.  Pattern size is the booted image's served
/// size.
pub fn test_reprogram_active_round_trip(
    emu: &Emulator,
    config: &Config,
    active_slot: u8,
    set_idx: usize,
) -> Result<(), String> {
    let chip_type = chip_type_from_config(config, set_idx)?;
    let chip_size = oracle::served_size(chip_type);
    let pattern = random_pattern(chip_size);

    println!("  seed=0x{:016X}", REPROGRAM_SEED);

    let result = emu.reprogram_ram_rom_slot(active_slot, 0, &pattern, true);
    if !result.is_ok() {
        return Err(format!("reprogram_ram_rom_slot failed: {:?}", result));
    }

    read_and_verify(emu, active_slot, &pattern)
        .map(|n| println!("  {} bytes verified (slot {}, active)", n, active_slot))
}

/// Copy the booted image's flash slot (`set_idx`) to `dst_ram`, read it back,
/// verify against oracle.
pub fn test_copy_flash_to_ram(
    emu: &Emulator,
    config: &Config,
    base_dir: &Path,
    set_idx: usize,
    dst_ram: u8,
) -> Result<(), String> {
    let chip_config = chip_config_at(config, set_idx)?;
    let chip_type = chip_config.chip_type.resolved();
    let expected = oracle::load(chip_config, chip_type, base_dir);

    let result = emu.copy_flash_slot_to_ram_slot(
        set_idx as u8,
        ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS,
        dst_ram,
        0,
    );
    if !result.is_ok() {
        return Err(format!("copy_flash_slot_to_ram_slot failed: {:?}", result));
    }

    read_and_verify(emu, dst_ram, &expected).map(|n| {
        println!(
            "  {} bytes verified (flash slot {} → ram slot {})",
            n, set_idx, dst_ram
        )
    })
}

/// Switch the active RAM slot to `slot` and verify get_active_ram_slot returns
/// it.
///
/// `slot` should refer to a populated slot so it can be safely served (the
/// caller copies content into it beforehand).
pub fn test_switch_active_slot(emu: &Emulator, slot: u8) -> Result<(), String> {
    make_active(emu, slot)?;

    let (result, active) = emu.get_active_ram_slot();
    if !result.is_ok() {
        return Err(format!("get_active_ram_slot failed: {:?}", result));
    }
    match active {
        Some(s) if s == slot => {
            println!("  active slot: {}", s);
            Ok(())
        }
        Some(n) => Err(format!("expected active slot {}, got {}", slot, n)),
        None => Err("get_active_ram_slot returned no slot".to_string()),
    }
}

/// Reprogram `slot` with the booted image's oracle, make it the active slot,
/// and verify the PIO serves the correct bytes for every address.
///
/// Setting the slot active makes this test order-independent: it serves what it
/// just reprogrammed regardless of which slot was active on entry.
pub fn test_reprogram_pio_verify(
    emu: &Emulator,
    config: &Config,
    board: Board,
    base_dir: &Path,
    slot: u8,
    set_idx: usize,
) -> Result<(), String> {
    let chip_config = chip_config_at(config, set_idx)?;
    let chip_type = chip_config.chip_type.resolved();
    let oracle_bytes = oracle::load(chip_config, chip_type, base_dir);

    let result = emu.reprogram_ram_rom_slot(slot, 0, &oracle_bytes, true);
    if !result.is_ok() {
        return Err(format!("reprogram_ram_rom_slot failed: {:?}", result));
    }

    make_active(emu, slot)?;

    let cache = PinCache::build(chip_type, chip_config, board);

    pio_verify(
        emu,
        &cache,
        &oracle_bytes,
        chip_type,
        force_16_bit_for(config, set_idx),
    )
    .map(|n| println!("  {} bytes verified via PIO (reprogram → slot {})", n, slot))
}

/// Copy the booted image's flash slot (`set_idx`) into `ram_slot`, make it the
/// active slot, and verify the PIO serves the correct bytes for every address.
///
/// Setting the slot active makes this test order-independent: it serves what it
/// just copied regardless of which slot was active on entry.
pub fn test_copy_flash_pio_verify(
    emu: &Emulator,
    config: &Config,
    board: Board,
    base_dir: &Path,
    set_idx: usize,
    ram_slot: u8,
) -> Result<(), String> {
    let chip_config = chip_config_at(config, set_idx)?;
    let chip_type = chip_config.chip_type.resolved();
    let oracle_bytes = oracle::load(chip_config, chip_type, base_dir);

    let result = emu.copy_flash_slot_to_ram_slot(
        set_idx as u8,
        ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS,
        ram_slot,
        0,
    );
    if !result.is_ok() {
        return Err(format!("copy_flash_slot_to_ram_slot failed: {:?}", result));
    }

    make_active(emu, ram_slot)?;

    let cache = PinCache::build(chip_type, chip_config, board);

    pio_verify(
        emu,
        &cache,
        &oracle_bytes,
        chip_type,
        force_16_bit_for(config, set_idx),
    )
    .map(|n| {
        println!(
            "  {} bytes verified via PIO (flash {} → ram slot {})",
            n, set_idx, ram_slot
        )
    })
}
