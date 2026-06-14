// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Test execution.
//!
//! One firmware boot per chip set; one pass per bit mode per chip.
//!
//! Read protocol per address (mirrors the old C tester, `test_main.c`):
//!
//! ```text
//!   drive addr + CS inactive  →  step ADDR_BEFORE_CS cycles
//!   drive addr + CS active    →  step CS_TO_DATA cycles
//!   read pin states, extract byte, compare with oracle
//!   deassert CS               →  step AFTER_READ cycles
//! ```

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_fw_emulator::Emulator;
use onerom_gen::{ChipConfig, ChipSetConfig, ChipSetType, Config};

use crate::driver;
use crate::oracle;
use crate::pin_cache::PinCache;
use crate::report::{ChipResult, ModeResult, SetResult, TestReport};
use crate::timing;

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run_all(board: Board, config: &Config, base_dir: &std::path::Path, report: &mut TestReport) {
    info!("Running {} chip set(s)", config.chip_sets.len());
    for (set_idx, chip_set) in config.chip_sets.iter().enumerate() {
        let result = run_chip_set(board, chip_set, set_idx, base_dir);
        report.add_set_result(result);
    }
}

// ── Per chip set ──────────────────────────────────────────────────────────────

fn run_chip_set(board: Board, chip_set: &ChipSetConfig, set_idx: usize, base_dir: &std::path::Path) -> SetResult {
    // TODO: multi-ROM sets and bank-switched sets require additional orchestration.
    if chip_set.set_type != ChipSetType::Single {
        warn!("Set {}: skipping — multi-ROM and banked sets not yet supported", set_idx);
        return SetResult::skipped(set_idx, "multi-ROM and banked sets not yet supported");
    }

    // Image selection must happen before boot so the firmware sees the correct
    // sel GPIO state during initialisation.
    debug!("Set {}: selecting image", set_idx);
    Emulator::set_sel_image(set_idx as u8);

    debug!("Set {}: booting firmware", set_idx);
    let mut emulator = Emulator::boot();

    if emulator.limp_mode() {
        error!("Set {}: firmware entered limp mode", set_idx);
        return SetResult::boot_error(set_idx, "firmware entered limp mode");
    }
    if !emulator.pios_enabled() {
        error!("Set {}: PIO state machines not enabled after boot", set_idx);
        return SetResult::boot_error(set_idx, "PIO state machines not enabled after boot");
    }
    debug!("Set {}: PIOs enabled, setting up epio", set_idx);

    let word_size = word_size_for_set(chip_set);
    debug!("Set {}: word_size={}", set_idx, word_size);
    emulator.setup_epio(word_size);
    emulator.step_cycles(timing::CYCLES_BEFORE_START);

    let chip_results: Vec<ChipResult> = chip_set
        .chips
        .iter()
        .enumerate()
        .map(|(chip_idx, chip_config)| {
            run_chip(&emulator, board, chip_config, set_idx, chip_idx, base_dir)
        })
        .collect();

    SetResult::done(set_idx, chip_results)
    // `emulator` dropped here; Drop impl frees the epio handle.
}

// ── Per chip ──────────────────────────────────────────────────────────────────

fn run_chip(
    emulator: &Emulator,
    board: Board,
    chip_config: &ChipConfig,
    set_idx: usize,
    chip_idx: usize,
    base_dir: &std::path::Path,
) -> ChipResult {
    let chip_type = chip_config.chip_type;

    debug!(
        "Set {} chip {}: building pin cache for {} on board {}",
        set_idx, chip_idx, chip_type.name(), board.name()
    );
    let cache = PinCache::build(chip_type, chip_config, board);

    debug!(
        "Set {} chip {}: {} addr GPIOs, {} data GPIOs, {} control lines",
        set_idx, chip_idx,
        cache.addr_gpios.len(),
        cache.data_gpios.len(),
        cache.control_lines.len(),
    );
    for cl in &cache.control_lines {
        debug!(
            "  Control line '{}': GPIOs={:?} assert_high={}",
            cl.name, cl.gpios, cl.assert_high
        );
    }
    if cache.control_lines.is_empty() {
        warn!(
            "Set {} chip {}: no control lines — CS will never be driven",
            set_idx, chip_idx
        );
    }

    let oracle = oracle::load(chip_config, chip_type, base_dir);
    debug!(
        "Set {} chip {}: oracle loaded, {} bytes",
        set_idx, chip_idx, oracle.len()
    );

    let is_27c400_family =
        chip_type == ChipType::Chip27C400 || chip_type == ChipType::Chip27C200;

    let cycles_addr_before_cs = if is_27c400_family {
        timing::CYCLES_27C400_ADDR_BEFORE_CS
    } else {
        timing::CYCLES_ADDR_BEFORE_CS
    };

    let mut mode_results = Vec::new();
    for &mode in chip_type.bit_modes() {
        info!(
            "Testing set={} chip={} ({}) file={} mode={}bit ({} bytes)",
            set_idx, chip_idx, chip_type.name(), chip_config.file, mode, oracle.len(),
        );
        let result = run_mode(
            emulator,
            &cache,
            &oracle,
            chip_type,
            mode,
            cycles_addr_before_cs,
            set_idx,
            chip_idx,
        );
        mode_results.push(result);
    }

    ChipResult { set_idx, chip_idx, chip_type, filename: chip_config.file.clone(), mode_results }
}

// ── Per bit mode ──────────────────────────────────────────────────────────────

fn run_mode(
    emulator: &Emulator,
    cache: &PinCache,
    oracle: &[u8],
    chip_type: ChipType,
    mode: u8,
    cycles_addr_before_cs: u32,
    set_idx: usize,
    chip_idx: usize,
) -> ModeResult {
    let is_27c400_family =
        chip_type == ChipType::Chip27C400 || chip_type == ChipType::Chip27C200;

    // Set BYTE# once for the whole mode pass.
    if let Some(gpio) = cache.byte_n_gpio {
        let (mask, levels) = driver::byte_n_mask(gpio, mode);
        debug!("BYTE# gpio={} mode={} mask={:#018x} levels={:#018x}", gpio, mode, mask, levels);
        emulator.drive_gpios(mask, levels);
    }

    let (iter_count, addr_shift, cycles_cs_to_data) = if mode == 16 {
        (oracle.len() / 2, 1usize, timing::CYCLES_CS_TO_DATA)
    } else {
        let cs_to_data = if is_27c400_family {
            timing::CYCLES_27C400_CS_TO_DATA_BYTE
        } else {
            timing::CYCLES_CS_TO_DATA
        };
        (oracle.len(), 0usize, cs_to_data)
    };

    debug!(
        "Mode {}bit: {} iterations, addr_shift={}, cycles_addr_before_cs={}, cycles_cs_to_data={}",
        mode, iter_count, addr_shift, cycles_addr_before_cs, cycles_cs_to_data
    );

    // Pre-compute the deasserted control mask — reused on every iteration.
    let ctrl_deasserted = driver::ctrl_mask(&cache.control_lines, false);
    let ctrl_active    = driver::ctrl_mask(&cache.control_lines, true);

    debug!(
        "ctrl_deasserted: mask={:#018x} levels={:#018x}",
        ctrl_deasserted.0, ctrl_deasserted.1
    );
    debug!(
        "ctrl_active:     mask={:#018x} levels={:#018x}",
        ctrl_active.0, ctrl_active.1
    );

    let mut reads = 0u64;
    let mut failures = 0u64;

    for addr_idx in 0..iter_count {
        let phys_addr = addr_idx << addr_shift;

        // ── Phase 1: address valid, CS inactive ──────────────────────────────
        let phase1 = driver::merge(
            driver::addr_mask(phys_addr, &cache.addr_gpios),
            ctrl_deasserted,
        );
        if addr_idx == 0 {
            debug!(
                "addr=0 phase1: mask={:#018x} levels={:#018x}",
                phase1.0, phase1.1
            );
        }
        emulator.drive_gpios(phase1.0, phase1.1);
        emulator.step_cycles(cycles_addr_before_cs);

        // ── Phase 2: CS asserted ─────────────────────────────────────────────
        let phase2 = driver::merge(
            driver::addr_mask(phys_addr, &cache.addr_gpios),
            ctrl_active,
        );
        if addr_idx == 0 {
            debug!(
                "addr=0 phase2: mask={:#018x} levels={:#018x}",
                phase2.0, phase2.1
            );
        }
        emulator.drive_gpios(phase2.0, phase2.1);
        emulator.step_cycles(cycles_cs_to_data);

        // ── Phase 3: read and compare ─────────────────────────────────────────
        let pin_states = emulator.read_pin_states();
        let driven_pins = emulator.read_driven_pins();
        if addr_idx == 0 {
            debug!("addr=0 pin_states={:#018x}", pin_states);
            debug!("addr=0 driven_pins={:#018x}", driven_pins);
        }

        if mode == 16 {
            let lo = driver::extract_byte(pin_states, &cache.data_gpios[..8]);
            let hi = driver::extract_byte(pin_states, &cache.data_gpios[8..16]);

            reads += 2;
            let exp_lo = oracle[addr_idx * 2];
            let exp_hi = oracle[addr_idx * 2 + 1];

            if lo != exp_lo {
                failures += 1;
                log_mismatch(set_idx, chip_idx, addr_idx * 2, lo, exp_lo, driven_pins, &cache.data_gpios[..8], failures);
            }
            if hi != exp_hi {
                failures += 1;
                log_mismatch(set_idx, chip_idx, addr_idx * 2 + 1, hi, exp_hi, driven_pins, &cache.data_gpios[8..16], failures);
            }
        } else {
            let byte = driver::extract_byte(pin_states, &cache.data_gpios);
            reads += 1;
            let expected = oracle[addr_idx];
            if byte != expected {
                failures += 1;
                log_mismatch(set_idx, chip_idx, addr_idx, byte, expected, driven_pins, &cache.data_gpios, failures);
            }
        }

        // ── Phase 4: deassert CS and settle ──────────────────────────────────
        emulator.drive_gpios(ctrl_deasserted.0, ctrl_deasserted.1);
        emulator.step_cycles(timing::CYCLES_AFTER_READ);
    }

    ModeResult { mode, reads, failures }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn word_size_for_set(chip_set: &ChipSetConfig) -> u8 {
    chip_set
        .chips
        .first()
        .map(|c| {
            if c.chip_type == ChipType::Chip27C400 || c.chip_type == ChipType::Chip27C200 {
                16
            } else {
                8
            }
        })
        .unwrap_or(8)
}

/// Log a byte mismatch, capped at 5 per mode pass to avoid flooding the log
/// for systematic failures.
fn log_mismatch(set: usize, chip: usize, addr: usize, got: u8, expected: u8,
                driven_pins: u64, data_gpios: &[u8], count: u64) {
    if count <= 5 {
        let drive_state: String = data_gpios.iter().map(|&g| {
            if driven_pins & (1u64 << g) != 0 { 'y' } else { 'n' }
        }).collect();
        error!(
            "MISMATCH set={} chip={} addr=0x{:04X}: got=0x{:02X} expected=0x{:02X} driven=[{}]",
            set, chip, addr, got, expected, drive_state,
        );
    } else if count == 6 {
        error!("(further mismatches suppressed for this mode pass)");
    }
}