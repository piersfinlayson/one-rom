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
//!   read driven pins, check data lines released
//! ```
//!
//! After the per-address loop, every non-active combination of the control
//! lines is tested at address 0 to confirm the data bus is tristated for all
//! combinations other than the fully-asserted (valid read) state.

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

fn run_chip_set(
    board: Board,
    chip_set: &ChipSetConfig,
    set_idx: usize,
    base_dir: &std::path::Path,
) -> SetResult {
    // TODO: multi-ROM sets and bank-switched sets require additional orchestration.
    if chip_set.set_type != ChipSetType::Single {
        warn!(
            "Set {}: skipping — multi-ROM and banked sets not yet supported",
            set_idx
        );
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
        set_idx,
        chip_idx,
        chip_type.name(),
        board.name()
    );
    let cache = PinCache::build(chip_type, chip_config, board);

    debug!(
        "Set {} chip {}: {} addr GPIOs, {} data GPIOs, {} control lines",
        set_idx,
        chip_idx,
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
        set_idx,
        chip_idx,
        oracle.len()
    );

    let is_27c400_family = chip_type == ChipType::Chip27C400 || chip_type == ChipType::Chip27C200;

    let cycles_addr_before_cs = if is_27c400_family {
        timing::CYCLES_27C400_ADDR_BEFORE_CS
    } else {
        timing::CYCLES_ADDR_BEFORE_CS
    };

    let mut mode_results = Vec::new();
    for &mode in chip_type.bit_modes() {
        info!(
            "Testing set={} chip={} ({}) file={} mode={}bit ({} bytes)",
            set_idx,
            chip_idx,
            chip_type.name(),
            chip_config.file,
            mode,
            oracle.len(),
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

    ChipResult {
        set_idx,
        chip_idx,
        chip_type,
        filename: chip_config.file.clone(),
        mode_results,
    }
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
    let is_27c400_family = chip_type == ChipType::Chip27C400 || chip_type == ChipType::Chip27C200;

    // Set BYTE# once for the whole mode pass.
    if let Some(gpio) = cache.byte_n_gpio {
        let (mask, levels) = driver::byte_n_mask(gpio, mode);
        debug!(
            "BYTE# gpio={} mode={} mask={:#018x} levels={:#018x}",
            gpio, mode, mask, levels
        );
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
    let ctrl_active = driver::ctrl_mask(&cache.control_lines, true);

    debug!(
        "ctrl_deasserted: mask={:#018x} levels={:#018x}",
        ctrl_deasserted.0, ctrl_deasserted.1
    );
    debug!(
        "ctrl_active:     mask={:#018x} levels={:#018x}",
        ctrl_active.0, ctrl_active.1
    );

    // The data GPIO slice used for bus-state checks.  For 16-bit mode all 16
    // pins are live.  For 8-bit mode only the low byte lane is driven by the
    // chip (BYTE# keeps D8-D15 tristated on 27C400-family devices), so we
    // limit the check to the first 8 GPIOs in the cache.
    let driven_check_gpios: &[u8] = if mode == 16 {
        &cache.data_gpios[..16]
    } else {
        &cache.data_gpios[..8.min(cache.data_gpios.len())]
    };

    let mut reads = 0u64;
    let mut failures = 0u64;
    let mut bus_failures = 0u64;

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
        let phase2 = driver::merge(driver::addr_mask(phys_addr, &cache.addr_gpios), ctrl_active);
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

        // Data lines must be driven while CS is active.
        if !driven_check_gpios
            .iter()
            .all(|&g| driven_pins & (1u64 << g) != 0)
        {
            bus_failures += 1;
            log_bus_violation(
                set_idx,
                chip_idx,
                Some(phys_addr),
                "not all driven (CS active)",
                driven_pins,
                driven_check_gpios,
                bus_failures,
            );
        }

        if mode == 16 {
            let lo = driver::extract_byte(pin_states, &cache.data_gpios[..8]);
            let hi = driver::extract_byte(pin_states, &cache.data_gpios[8..16]);

            reads += 2;
            let exp_lo = oracle[addr_idx * 2];
            let exp_hi = oracle[addr_idx * 2 + 1];

            if lo != exp_lo {
                failures += 1;
                log_mismatch(
                    set_idx,
                    chip_idx,
                    addr_idx * 2,
                    lo,
                    exp_lo,
                    driven_pins,
                    &cache.data_gpios[..8],
                    failures,
                );
            }
            if hi != exp_hi {
                failures += 1;
                log_mismatch(
                    set_idx,
                    chip_idx,
                    addr_idx * 2 + 1,
                    hi,
                    exp_hi,
                    driven_pins,
                    &cache.data_gpios[8..16],
                    failures,
                );
            }
        } else {
            let byte = driver::extract_byte(pin_states, &cache.data_gpios[..8]);
            reads += 1;
            let expected = oracle[addr_idx];
            if byte != expected {
                failures += 1;
                log_mismatch(
                    set_idx,
                    chip_idx,
                    addr_idx,
                    byte,
                    expected,
                    driven_pins,
                    &cache.data_gpios,
                    failures,
                );
            }
        }

        // ── Phase 4: deassert CS and settle ──────────────────────────────────
        emulator.drive_gpios(ctrl_deasserted.0, ctrl_deasserted.1);
        emulator.step_cycles(timing::CYCLES_AFTER_READ);

        // Data lines must have released after CS deassert.
        let driven_after = emulator.read_driven_pins();
        if driven_check_gpios
            .iter()
            .any(|&g| driven_after & (1u64 << g) != 0)
        {
            bus_failures += 1;
            log_bus_violation(
                set_idx,
                chip_idx,
                Some(phys_addr),
                "still driven (CS deasserted)",
                driven_after,
                driven_check_gpios,
                bus_failures,
            );
        }
    }

    // ── CS combination tests ──────────────────────────────────────────────────
    // Walk every non-active combination of the control lines and confirm the
    // data bus is tristated.  The all-active combination (combo == all_asserted)
    // is the only state that should drive the bus; the exclusive upper bound of
    // the range naturally excludes it.
    // Address 0 is used throughout — tristate behaviour is address-independent.
    let n = cache.control_lines.len();
    if n > 0 {
        let all_asserted: u64 = (1u64 << n) - 1;
        debug!(
            "Mode {}bit combo test: {} control line(s), {} non-active combinations",
            mode, n, all_asserted
        );

        for combo in 0u64..all_asserted {
            let ctrl = ctrl_combo_mask(cache, combo);
            let phase = driver::merge(driver::addr_mask(0, &cache.addr_gpios), ctrl);
            emulator.drive_gpios(phase.0, phase.1);
            emulator.step_cycles(cycles_cs_to_data);

            let driven_combo = emulator.read_driven_pins();
            if driven_check_gpios
                .iter()
                .any(|&g| driven_combo & (1u64 << g) != 0)
            {
                bus_failures += 1;
                log_bus_violation(
                    set_idx,
                    chip_idx,
                    None,
                    &format!("data driven for non-active CS combo {:#b}", combo),
                    driven_combo,
                    driven_check_gpios,
                    bus_failures,
                );
            }
        }

        // Leave control lines deasserted.
        emulator.drive_gpios(ctrl_deasserted.0, ctrl_deasserted.1);
        emulator.step_cycles(timing::CYCLES_AFTER_READ);
    }

    ModeResult {
        mode,
        reads,
        failures,
        bus_failures,
    }
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

/// Build a GPIO (mask, levels) pair for an arbitrary combination of control
/// lines.  `combo` is a bitmask over `cache.control_lines`: bit i set means
/// control line i is logically *asserted*; bit i clear means deasserted.
fn ctrl_combo_mask(cache: &PinCache, combo: u64) -> (u64, u64) {
    cache
        .control_lines
        .iter()
        .enumerate()
        .map(|(i, cl)| driver::ctrl_mask(std::slice::from_ref(cl), (combo >> i) & 1 == 1))
        .fold((0u64, 0u64), |acc, m| driver::merge(acc, m))
}

/// Log a data bus violation (lines unexpectedly driven or unexpectedly
/// released), capped at 5 per mode pass to avoid flooding the log for
/// systematic failures.
fn log_bus_violation(
    set: usize,
    chip: usize,
    addr: Option<usize>,
    desc: &str,
    driven_pins: u64,
    data_gpios: &[u8],
    count: u64,
) {
    if count <= 5 {
        let drive_state: String = data_gpios
            .iter()
            .rev()
            .map(|&g| {
                if driven_pins & (1u64 << g) != 0 {
                    'y'
                } else {
                    'n'
                }
            })
            .collect();
        match addr {
            Some(a) => error!(
                "BUS set={} chip={} addr=0x{:04X}: {} driven=[{}]",
                set, chip, a, desc, drive_state
            ),
            None => error!(
                "BUS set={} chip={}: {} driven=[{}]",
                set, chip, desc, drive_state
            ),
        }
    } else if count == 6 {
        error!("(further bus violations suppressed for this mode pass)");
    }
}

/// Log a byte mismatch, capped at 5 per mode pass to avoid flooding the log
/// for systematic failures.
fn log_mismatch(
    set: usize,
    chip: usize,
    addr: usize,
    got: u8,
    expected: u8,
    driven_pins: u64,
    data_gpios: &[u8],
    count: u64,
) {
    if count <= 5 {
        let drive_state: String = data_gpios
            .iter()
            .map(|&g| {
                if driven_pins & (1u64 << g) != 0 {
                    'y'
                } else {
                    'n'
                }
            })
            .collect();
        error!(
            "MISMATCH set={} chip={} addr=0x{:04X}: got=0x{:02X} expected=0x{:02X} driven=[{}]",
            set, chip, addr, got, expected, drive_state,
        );
    } else if count == 6 {
        error!("(further mismatches suppressed for this mode pass)");
    }
}
