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
//!
//! For multi-ROM sets a `background_mask` holds all non-active chip CS lines
//! deasserted, and any primary address GPIOs unused by the secondary chip at
//! zero, on every GPIO drive call so they cannot accidentally enable a chip
//! while another is under test, and so the firmware's address lookup stays
//! within the correct region of the padded ROM image.  For dynamically banked
//! sets the same mechanism holds all X pin GPIOs at the level corresponding to
//! the current bank throughout the test pass.
//!
//! For multi-ROM secondary chips with fewer address lines than the primary
//! (e.g. a 2332 behind a 2364), the extra address GPIO(s) are not connected
//! to the secondary chip and may be either HIGH or LOW on real hardware.  The
//! tester enumerates all 2^n level combinations for the n extra GPIOs, running
//! `run_mode` once per combination.  Results are accumulated into a single
//! `ModeResult`; `combos` records how many passes were made.

// Several helpers here return `Result<T, SetResult>`, where the `Err` variant
// is a fully-populated `SetResult` short-returned as the finished record for a
// set (boot error, skip, gap error) rather than a lightweight error.
// `SetResult` is meant to be large, this is an internal test-tool binary (not a
// public API), and the value is moved on a cold early-return path, so boxing it
// would add indirection and churn every call site for no real benefit.
#![allow(clippy::result_large_err)]

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_fw_emulator::Emulator;
use onerom_gen::{ChipConfig, ChipSetConfig, ChipSetType, Config, CsConfig, CsLogic};

use crate::report::{ChipResult, ModeResult, SetResult, TestReport};
use onerom_fw_tester::cs_timing;
use onerom_fw_tester::driver;
use onerom_fw_tester::geometry;
use onerom_fw_tester::geometry::chip_substitution;
use onerom_fw_tester::oracle;
use onerom_fw_tester::pin_cache::{ControlLine, PinCache};
use onerom_fw_tester::runner::{addr_before_cs_cycles, cs_to_data_cycles, run_mode};
use onerom_fw_tester::timing;

/// Config-derived serving-algorithm info for one chip of a set, for the CS
/// timing pass.  `None` (with the reason logged) when the combination does not
/// derive, in which case the pass is skipped rather than run against a guess.
fn alg_info_for(
    board: Board,
    chip_type: ChipType,
    chip_config: &ChipConfig,
    secondary: Option<&ChipConfig>,
    set_type: ChipSetType,
    num_chips: usize,
    force_16_bit: bool,
) -> Option<onerom_gen::compat::ServingAlgInfo> {
    let cs_config = CsConfig::from_chip_type(
        &chip_type,
        chip_config.cs1,
        chip_config.cs2,
        chip_config.cs3,
        chip_config.cs4,
        chip_config.ce,
        chip_config.oe,
    );
    let secondary_cs = secondary.map(|c| {
        let t = c.chip_type.resolved();
        CsConfig::from_chip_type(&t, c.cs1, c.cs2, c.cs3, c.cs4, c.ce, c.oe)
    });
    match onerom_gen::compat::serving_alg_info(
        board,
        chip_type,
        set_type,
        num_chips,
        cs_config,
        secondary_cs,
        force_16_bit,
    ) {
        Ok(i) => Some(i),
        Err(e) => {
            warn!(
                "CS timing: no serving alg info for {}: {e}",
                chip_type.name()
            );
            None
        }
    }
}

/// Run the CS timing pass, or record why it did not run.
// Every argument is an independent input the pass needs; bundling them into a
// struct would add a type whose only purpose is to be unpacked again.
#[allow(clippy::too_many_arguments)]
fn timing_pass(
    emulator: &Emulator,
    cache: &PinCache,
    mode: u8,
    addr_before_cs: u32,
    background: (u64, u64),
    info: Option<&onerom_gen::compat::ServingAlgInfo>,
    num_addrs: usize,
    gap_gpios: &[u8],
    label: &str,
) -> onerom_fw_tester::cs_timing::PassResult {
    match info {
        Some(i) => cs_timing::run_pass(
            emulator,
            cache,
            mode,
            addr_before_cs,
            background,
            i,
            num_addrs,
            gap_gpios,
            label,
        ),
        None => onerom_fw_tester::cs_timing::PassResult::skipped(
            "no serving algorithm info derives for this chip and board",
        ),
    }
}

// ── Capability helpers ────────────────────────────────────────────────────────

/// Returns `true` if `board` supports multi-ROM sets.
///
/// Requires X pins and excludes boards (Fire24A, Fire24B) that route their
/// X pins only to banked-switching logic, not secondary ROM socket CS lines.
fn board_supports_multi(board: Board) -> bool {
    !board.x_pin_map().is_empty() && !matches!(board, Board::Fire24A | Board::Fire24UsbB)
}

/// Returns `true` if `board` supports dynamically banked ROM sets.
///
/// Any board with X pins can perform dynamic bank switching.
fn board_supports_banked(board: Board) -> bool {
    !board.x_pin_map().is_empty()
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run_all(board: Board, config: &Config, base_dir: &std::path::Path, report: &mut TestReport) {
    let num_sets = config.chip_sets.len();
    let num_sel_pins = board.sel_pins().len();
    let max_images = 1usize << num_sel_pins;
    info!(
        "Running {} chip set(s); board has {} sel pin(s) (max {} images)",
        num_sets, num_sel_pins, max_images
    );

    for (set_idx, chip_set) in config.chip_sets.iter().enumerate() {
        let effective_idx = set_idx % max_images;

        let (oracle_set, note) = if effective_idx != set_idx {
            warn!(
                "Set {}: board has {} sel pin(s) (max {} images); \
                 sel wraps to set {} — oracle taken from set {}",
                set_idx, num_sel_pins, max_images, effective_idx, effective_idx,
            );
            (
                &config.chip_sets[effective_idx],
                Some(format!(
                    "sel wraps to set {} (board has {} sel pin(s), max {} images)",
                    effective_idx, num_sel_pins, max_images,
                )),
            )
        } else {
            (chip_set, None)
        };

        let mut result = run_chip_set(
            board,
            config,
            oracle_set,
            set_idx,
            effective_idx,
            set_idx as u8,
            base_dir,
        );
        if let Some(n) = note {
            result.set_note(n);
        }
        report.add_set_result(result);
    }

    // One-beyond test: verify the firmware wraps to set 0 when the sel value
    // is one past the last configured set, provided the board has enough sel
    // pins to express that value.
    if num_sets > 0 && num_sets < max_images {
        info!(
            "Running one-beyond test: sel={} expects set 0 to be served",
            num_sets
        );
        let note = format!(
            "one-beyond test: sel={} (one past {} configured set(s)), \
             firmware should wrap to set 0",
            num_sets, num_sets,
        );
        let mut result = run_chip_set(
            board,
            config,
            &config.chip_sets[0],
            num_sets,
            0,
            num_sets as u8,
            base_dir,
        );
        result.set_note(note);
        report.add_set_result(result);
    }
}

// ── Per chip set (dispatch) ───────────────────────────────────────────────────

fn run_chip_set(
    board: Board,
    config: &Config,
    chip_set: &ChipSetConfig,
    set_idx: usize,
    served_idx: usize,
    sel_image: u8,
    base_dir: &std::path::Path,
) -> SetResult {
    match chip_set.set_type {
        ChipSetType::Single => run_single_set(
            board, config, chip_set, set_idx, served_idx, sel_image, base_dir,
        ),
        ChipSetType::Multi => run_multi_set(
            board, config, chip_set, set_idx, served_idx, sel_image, base_dir,
        ),
        ChipSetType::Banked => run_banked_set(
            board, config, chip_set, set_idx, served_idx, sel_image, base_dir,
        ),
    }
}

// ── Single chip set ───────────────────────────────────────────────────────────

fn run_single_set(
    board: Board,
    config: &Config,
    chip_set: &ChipSetConfig,
    set_idx: usize,
    served_idx: usize,
    sel_image: u8,
    base_dir: &std::path::Path,
) -> SetResult {
    let (emulator, fw_version) = match boot_set(board, chip_set, set_idx, sel_image) {
        Ok(e) => e,
        Err(r) => return r,
    };

    let force_16_bit = get_force_16_bit(chip_set);

    // Check ROM-serving GPIO pulls, and compute the forced-low gap set.  Both
    // use the PinCache for the first chip (the slot's primary).  Single sets
    // use no X pins, so n_used_x = 0.
    let gap_gpios: Vec<u8> = if let Some(chip_config) = chip_set.chips.first() {
        let chip_type = chip_substitution(board, chip_config.chip_type.resolved())
            .unwrap_or(chip_config.chip_type.resolved());
        let cache = PinCache::build(chip_type, chip_config, board);
        if let Err(r) = check_rom_pin_pulls(&emulator, &cache, set_idx) {
            return r;
        }
        match gap_set_for_slot(
            board,
            config,
            base_dir,
            served_idx,
            fw_version,
            chip_config.chip_type.resolved(),
            &cache,
            0,
            set_idx,
        ) {
            Ok(g) => g,
            Err(r) => return r,
        }
    } else {
        Vec::new()
    };

    let chip_results: Vec<ChipResult> = chip_set
        .chips
        .iter()
        .enumerate()
        .map(|(chip_idx, chip_config)| {
            run_chip(
                &emulator,
                board,
                chip_config,
                set_idx,
                chip_idx,
                base_dir,
                force_16_bit,
                (0u64, 0u64),
                &gap_gpios,
            )
        })
        .collect();

    SetResult::done(set_idx, chip_results)
    // `emulator` dropped here; Drop impl frees the epio handle.
}

// ── Multi-ROM chip set ────────────────────────────────────────────────────────

fn run_multi_set(
    board: Board,
    config: &Config,
    chip_set: &ChipSetConfig,
    set_idx: usize,
    served_idx: usize,
    sel_image: u8,
    base_dir: &std::path::Path,
) -> SetResult {
    if !board_supports_multi(board) {
        warn!(
            "Set {}: skipping — multi-ROM sets not supported on board {}",
            set_idx,
            board.name()
        );
        return SetResult::skipped(set_idx, "multi-ROM sets not supported on this board");
    }

    // A multi-ROM set with only one chip is a config oddity; fall through to
    // the single-set path which handles it correctly.
    if chip_set.chips.len() <= 1 {
        warn!(
            "Set {}: multi-ROM set has {} chip(s) — treating as single",
            set_idx,
            chip_set.chips.len()
        );
        return run_single_set(
            board, config, chip_set, set_idx, served_idx, sel_image, base_dir,
        );
    }

    let n_secondary = chip_set.chips.len() - 1;
    let n_x_pins = board.x_pin_map().len();
    if n_secondary > n_x_pins {
        error!(
            "Set {}: {} secondary chip(s) but board {} has only {} X pin(s)",
            set_idx,
            n_secondary,
            board.name(),
            n_x_pins,
        );
        return SetResult::skipped(
            set_idx,
            &format!(
                "{} secondary chip(s) but board has only {} X pin(s)",
                n_secondary, n_x_pins,
            ),
        );
    }

    let (emulator, fw_version) = match boot_set(board, chip_set, set_idx, sel_image) {
        Ok(e) => e,
        Err(r) => return r,
    };

    let force_16_bit = get_force_16_bit(chip_set);

    // ── Primary chip (chips[0], in One ROM's socket) ──────────────────────────
    let primary_config = &chip_set.chips[0];
    let primary_requested = primary_config.chip_type.resolved();
    let primary_type = if let Some(sub) = chip_substitution(board, primary_requested) {
        warn!(
            "Set {} chip 0: {} on {} is not directly servable; \
             substituting {} (physical shim required)",
            set_idx,
            primary_requested.name(),
            board.name(),
            sub.name(),
        );
        sub
    } else {
        primary_requested
    };

    debug!(
        "Set {} chip 0: building pin cache for {} on board {}",
        set_idx,
        primary_type.name(),
        board.name()
    );
    let mut primary_cache = PinCache::build(primary_type, primary_config, board);
    debug!(
        "Set {} chip 0: {} addr GPIOs, {} data GPIOs, {} control line(s)",
        set_idx,
        primary_cache.addr_gpios.len(),
        primary_cache.data_gpios.len(),
        primary_cache.control_lines.len(),
    );

    // chip0's commoned lines: active CS lines that are not the per-chip select
    // (the line the secondaries drive via X). Held deasserted, not enumerated,
    // during the tristate sweep — they qualify a read, they don't select a chip.
    if let Some(select) = per_chip_select_name(&chip_set.chips[1]) {
        for cl in &mut primary_cache.control_lines {
            if cl.name != select {
                cl.commoned = true;
            }
        }
    }

    if let Err(r) = check_rom_pin_pulls(&emulator, &primary_cache, set_idx) {
        return r;
    }

    // Forced-low gap set for the slot: primary geometry + the first
    // `n_secondary` X pins (the secondary CS selectors) are the used X.
    let gap_gpios = match gap_set_for_slot(
        board,
        config,
        base_dir,
        served_idx,
        fw_version,
        primary_requested,
        &primary_cache,
        n_secondary,
        set_idx,
    ) {
        Ok(g) => g,
        Err(r) => return r,
    };

    // ── X pin CS info for secondary chips ─────────────────────────────────────
    // Use the nominal (non-substituted) chip type for polarity lookup since the
    // config author wrote cs1/cs2/cs3 against the nominal type.
    let x_pin_info: Vec<(Vec<u8>, bool)> = chip_set
        .chips
        .iter()
        .skip(1)
        .enumerate()
        .map(|(i, chip_config)| {
            let (_, gpios) = board.x_pin_map()[i];
            let assert_high =
                first_active_cs_polarity(chip_config, chip_config.chip_type.resolved());
            (gpios.to_vec(), assert_high)
        })
        .collect();

    // Deassert mask for each secondary chip's X pin CS, computed individually
    // so we can exclude each chip's own mask when building its background.
    let x_deassert_masks: Vec<(u64, u64)> = x_pin_info
        .iter()
        .map(|(gpios, assert_high)| {
            let line = ControlLine {
                name: "x_cs",
                gpios: gpios.clone(),
                assert_high: *assert_high,
                commoned: false,
            };
            driver::ctrl_mask(std::slice::from_ref(&line), false)
        })
        .collect();

    // chips[0] background: all X pin CSes held deasserted so secondary chips
    // cannot accidentally drive the bus while the primary is under test.
    let chips0_bg = x_deassert_masks
        .iter()
        .fold((0u64, 0u64), |acc, &m| driver::merge(acc, m));

    // Primary socket CS deassert mask, folded into every secondary chip's
    // background to prevent chips[0] from driving the bus during their tests.
    let primary_cs_deassert = driver::ctrl_mask(&primary_cache.control_lines, false);

    let mut chip_results = Vec::new();

    // ── Test primary chip ─────────────────────────────────────────────────────
    {
        let oracle = oracle::load(primary_config, primary_type, base_dir);
        debug!(
            "Set {} chip 0: oracle loaded, {} bytes",
            set_idx,
            oracle.len()
        );
        let cycles_addr_before_cs = addr_before_cs_cycles(primary_type);

        let mut mode_results = Vec::new();
        for &mode in primary_type.bit_modes() {
            if force_16_bit && mode != 16 {
                debug!(
                    "Set {} chip 0: skipping {}bit mode (force_16_bit)",
                    set_idx, mode
                );
                continue;
            }
            info!(
                "Testing set={} chip=0 ({}) file={} mode={}bit ({} bytes)",
                set_idx,
                primary_type.name(),
                primary_config.file,
                mode,
                oracle.len(),
            );
            let (reads, failures, bus_failures, forced_low_failures) = run_mode(
                &emulator,
                &primary_cache,
                &oracle,
                mode,
                cycles_addr_before_cs,
                timing::CYCLES_CS_TO_DATA_MULTI,
                set_idx,
                0,
                chips0_bg,
                &gap_gpios,
            );
            let t = timing_pass(
                &emulator,
                &primary_cache,
                mode,
                cycles_addr_before_cs,
                chips0_bg,
                alg_info_for(
                    board,
                    primary_type,
                    primary_config,
                    chip_set.chips.get(1),
                    ChipSetType::Multi,
                    chip_set.chips.len(),
                    force_16_bit,
                )
                .as_ref(),
                if mode == 16 {
                    oracle.len() / 2
                } else {
                    oracle.len()
                },
                &gap_gpios,
                &format!("set={set_idx} chip=0 mode={mode}bit"),
            );
            mode_results.push(ModeResult {
                mode,
                reads,
                failures,
                bus_failures,
                forced_low_failures,
                timing_checks: t.checks,
                timing_failures: t.failures,
                timing_note: t.note,
                combos: 1,
            });
        }
        chip_results.push(ChipResult {
            set_idx,
            chip_idx: 0,
            chip_type: primary_type,
            filename: primary_config.file.clone(),
            mode_results,
        });
    }

    // ── Test secondary chips (chips[1], chips[2], …) ──────────────────────────
    for (j, chip_config) in chip_set.chips.iter().skip(1).enumerate() {
        let chip_idx = j + 1;
        let requested_type = chip_config.chip_type.resolved();
        let chip_type = if let Some(sub) = chip_substitution(board, requested_type) {
            warn!(
                "Set {} chip {}: {} on {} is not directly servable; \
                 substituting {} (physical shim required)",
                set_idx,
                chip_idx,
                requested_type.name(),
                board.name(),
                sub.name(),
            );
            sub
        } else {
            requested_type
        };

        let (x_gpios, x_assert_high) = &x_pin_info[j];
        debug!(
            "Set {} chip {}: building secondary pin cache for {} on board {} \
             (X pin GPIOs={:?} assert_high={})",
            set_idx,
            chip_idx,
            chip_type.name(),
            board.name(),
            x_gpios,
            x_assert_high,
        );
        // Build the secondary cache before computing the background mask: the
        // cache's addr_gpios are needed to identify extra primary address GPIOs
        // that must be enumerated.
        let secondary_cache = PinCache::build_secondary(
            chip_type,
            &primary_cache,
            board,
            x_gpios.clone(),
            *x_assert_high,
        );
        debug!(
            "Set {} chip {}: {} addr GPIOs, {} data GPIOs",
            set_idx,
            chip_idx,
            secondary_cache.addr_gpios.len(),
            secondary_cache.data_gpios.len(),
        );

        // Extra address GPIOs: when the secondary has fewer address lines than
        // the primary (e.g. a 2332 secondary behind a 2364 primary), the
        // unshared GPIO(s) — A12 in that example — are not connected to the
        // secondary chip.  On real hardware these lines are driven by the host
        // and may be HIGH or LOW depending on which address the host is
        // accessing.  We enumerate all 2^n combinations so the test covers
        // every possible level rather than a single fixed value.
        //
        // When primary and secondary have the same address line count (e.g. two
        // 2364s) extra_mask=0, n_combos=1, and the loop degenerates to the
        // existing single-pass behaviour with no overhead.
        let extra_mask: u64 = {
            let secondary_addrs: std::collections::HashSet<u8> = secondary_cache
                .addr_gpios
                .iter()
                .flat_map(|v| v.iter().copied())
                .collect();
            let mut m = 0u64;
            for gpios in &primary_cache.addr_gpios {
                for &g in gpios {
                    if !secondary_addrs.contains(&g) {
                        m |= 1u64 << g;
                    }
                }
            }
            m
        };

        let extra_gpios: Vec<u8> = (0u8..64)
            .filter(|&g| extra_mask & (1u64 << g) != 0)
            .collect();
        let n_combos = 1usize << extra_gpios.len();

        if n_combos > 1 {
            debug!(
                "Set {} chip {}: {} extra addr GPIO(s) ({:?}) — {} combo(s)",
                set_idx,
                chip_idx,
                extra_gpios.len(),
                extra_gpios,
                n_combos,
            );
        }

        // Base background for this secondary chip: primary CS deasserted and
        // all other secondary X pin CSes deasserted.  The extra-bit levels are
        // merged in per combo inside the mode loop.
        let other_x_deassert = x_deassert_masks
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != j)
            .fold((0u64, 0u64), |acc, (_, &m)| driver::merge(acc, m));

        let base_bg = driver::merge(primary_cs_deassert, other_x_deassert);

        let oracle = oracle::load(chip_config, chip_type, base_dir);
        debug!(
            "Set {} chip {}: oracle loaded, {} bytes",
            set_idx,
            chip_idx,
            oracle.len()
        );

        let cycles_addr_before_cs = addr_before_cs_cycles(chip_type);

        let mut mode_results = Vec::new();
        for &mode in chip_type.bit_modes() {
            if force_16_bit && mode != 16 {
                debug!(
                    "Set {} chip {}: skipping {}bit mode (force_16_bit)",
                    set_idx, chip_idx, mode
                );
                continue;
            }
            info!(
                "Testing set={} chip={} ({}) file={} mode={}bit {} combo(s) ({} bytes)",
                set_idx,
                chip_idx,
                chip_type.name(),
                chip_config.file,
                mode,
                n_combos,
                oracle.len(),
            );

            let mut total_reads = 0u64;
            let mut total_failures = 0u64;
            let mut total_bus_failures = 0u64;
            let mut total_forced_low_failures = 0u64;

            for combo in 0..n_combos {
                // Build the level mask for the extra GPIOs for this combo.
                // Bit i of `combo` determines whether extra_gpios[i] is HIGH.
                let extra_levels: u64 =
                    extra_gpios.iter().enumerate().fold(0u64, |acc, (i, &g)| {
                        if (combo >> i) & 1 == 1 {
                            acc | (1u64 << g)
                        } else {
                            acc
                        }
                    });
                let bg = driver::merge(base_bg, (extra_mask, extra_levels));

                if n_combos > 1 {
                    debug!(
                        "Set {} chip {} mode {}bit combo {}/{}: \
                         extra_levels={:#018x}",
                        set_idx,
                        chip_idx,
                        mode,
                        combo + 1,
                        n_combos,
                        extra_levels,
                    );
                }

                let (reads, failures, bus_failures, forced_low_failures) = run_mode(
                    &emulator,
                    &secondary_cache,
                    &oracle,
                    mode,
                    cycles_addr_before_cs,
                    timing::CYCLES_CS_TO_DATA_MULTI,
                    set_idx,
                    chip_idx,
                    bg,
                    &gap_gpios,
                );
                total_reads += reads;
                total_failures += failures;
                total_bus_failures += bus_failures;
                total_forced_low_failures += forced_low_failures;
            }

            // Once per mode, not once per combo: the extra address bits a
            // combo varies do not change the serving path's latency, and the
            // pass writes to the image, so repeating it is cost without cover.
            // Combo 0's background is representative.
            let t = timing_pass(
                &emulator,
                &secondary_cache,
                mode,
                cycles_addr_before_cs,
                driver::merge(base_bg, (extra_mask, 0)),
                // The sampled window and the algorithms belong to the *slot*,
                // which derives from chips[0] — a secondary shares them, and
                // deriving from its own chip type asks a question the set does
                // not pose.  Its own control lines still decide whether it
                // lands inside that window.
                alg_info_for(
                    board,
                    primary_type,
                    &chip_set.chips[0],
                    chip_set.chips.get(1),
                    ChipSetType::Multi,
                    chip_set.chips.len(),
                    force_16_bit,
                )
                .as_ref(),
                if mode == 16 {
                    oracle.len() / 2
                } else {
                    oracle.len()
                },
                &gap_gpios,
                &format!("set={set_idx} chip={chip_idx} mode={mode}bit"),
            );

            mode_results.push(ModeResult {
                mode,
                reads: total_reads,
                failures: total_failures,
                bus_failures: total_bus_failures,
                forced_low_failures: total_forced_low_failures,
                timing_checks: t.checks,
                timing_failures: t.failures,
                timing_note: t.note,
                combos: n_combos as u32,
            });
        }
        chip_results.push(ChipResult {
            set_idx,
            chip_idx,
            chip_type,
            filename: chip_config.file.clone(),
            mode_results,
        });
    }

    SetResult::done(set_idx, chip_results)
    // `emulator` dropped here; Drop impl frees the epio handle.
}

// ── Banked chip set ───────────────────────────────────────────────────────────

fn run_banked_set(
    board: Board,
    config: &Config,
    chip_set: &ChipSetConfig,
    set_idx: usize,
    served_idx: usize,
    sel_image: u8,
    base_dir: &std::path::Path,
) -> SetResult {
    if !board_supports_banked(board) {
        warn!(
            "Set {}: skipping — dynamic banked sets not supported on board {}",
            set_idx,
            board.name()
        );
        return SetResult::skipped(set_idx, "dynamic banked sets not supported on this board");
    }

    if chip_set.chips.is_empty() {
        warn!("Set {}: banked set has no chips", set_idx);
        return SetResult::skipped(set_idx, "banked set has no chips");
    }

    // All chips in a banked set must be the same type — they share the same
    // socket and the same PinCache; only the oracle and X pin state vary.
    let chip_type_0 = chip_set.chips[0].chip_type.resolved();
    if let Some(pos) = chip_set
        .chips
        .iter()
        .position(|c| c.chip_type.resolved() != chip_type_0)
    {
        error!(
            "Set {}: banked sets require a uniform chip type; \
             chip {} is {} but chip 0 is {}",
            set_idx,
            pos,
            chip_set.chips[pos].chip_type.resolved().name(),
            chip_type_0.name(),
        );
        return SetResult::skipped(
            set_idx,
            "banked sets require all chips to have the same type",
        );
    }

    // Verify the board has enough X pins to encode all banks in binary.
    // n banks require ceil(log2(n)) X pins; computed via leading_zeros.
    let n_banks = chip_set.chips.len();
    let x_pins_needed = (usize::BITS - n_banks.saturating_sub(1).leading_zeros()) as usize;
    let n_x_pins = board.x_pin_map().len();
    if x_pins_needed > n_x_pins {
        error!(
            "Set {}: {} bank(s) require {} X pin(s) to encode but board {} has only {}",
            set_idx,
            n_banks,
            x_pins_needed,
            board.name(),
            n_x_pins,
        );
        return SetResult::skipped(
            set_idx,
            &format!(
                "{} banks require {} X pin(s) but board has only {}",
                n_banks, x_pins_needed, n_x_pins,
            ),
        );
    }

    // Apply any board-specific chip substitution (same for all banks since
    // all banks share the chip type).
    let chip_type = if let Some(sub) = chip_substitution(board, chip_type_0) {
        warn!(
            "Set {}: {} on {} is not directly servable; \
             substituting {} (physical shim required) for all banks",
            set_idx,
            chip_type_0.name(),
            board.name(),
            sub.name(),
        );
        sub
    } else {
        chip_type_0
    };

    let (emulator, fw_version) = match boot_set(board, chip_set, set_idx, sel_image) {
        Ok(e) => e,
        Err(r) => return r,
    };

    let force_16_bit = get_force_16_bit(chip_set);

    // All banks share the same chip type → one PinCache covers every bank.
    debug!(
        "Set {}: building pin cache for {} on board {}",
        set_idx,
        chip_type.name(),
        board.name()
    );
    let cache = PinCache::build(chip_type, &chip_set.chips[0], board);
    debug!(
        "Set {}: {} addr GPIOs, {} data GPIOs, {} control line(s)",
        set_idx,
        cache.addr_gpios.len(),
        cache.data_gpios.len(),
        cache.control_lines.len(),
    );

    if let Err(r) = check_rom_pin_pulls(&emulator, &cache, set_idx) {
        return r;
    }
    if let Err(r) = check_x_pin_pulls(&emulator, board, set_idx, x_pins_needed) {
        return r;
    }

    // Forced-low gap set for the slot: primary geometry + the first
    // `x_pins_needed` X pins (the bank-select lines) are the used X.
    let gap_gpios = match gap_set_for_slot(
        board,
        config,
        base_dir,
        served_idx,
        fw_version,
        chip_type_0,
        &cache,
        x_pins_needed,
        set_idx,
    ) {
        Ok(g) => g,
        Err(r) => return r,
    };

    let cycles_addr_before_cs = addr_before_cs_cycles(chip_type);

    let mut chip_results = Vec::new();

    for (bank, chip_config) in chip_set.chips.iter().enumerate() {
        // Drive X pins to select this bank.  Because bank switching is dynamic,
        // no reboot is needed between banks: the firmware reads the X pin state
        // on every access.  The mask is held throughout the entire test pass for
        // this bank via background_mask in run_mode().
        let bg = banked_x_mask(board, bank);
        debug!(
            "Set {} bank {}: X pin background mask=({:#018x}, {:#018x})",
            set_idx, bank, bg.0, bg.1,
        );

        let oracle = oracle::load(chip_config, chip_type, base_dir);
        debug!(
            "Set {} bank {}: oracle loaded, {} bytes",
            set_idx,
            bank,
            oracle.len()
        );

        let mut mode_results = Vec::new();
        for &mode in chip_type.bit_modes() {
            if force_16_bit && mode != 16 {
                debug!(
                    "Set {} bank {}: skipping {}bit mode (force_16_bit)",
                    set_idx, bank, mode
                );
                continue;
            }
            let cycles_cs_to_data = cs_to_data_cycles(chip_type, mode);
            info!(
                "Testing set={} bank={} ({}) file={} mode={}bit ({} bytes)",
                set_idx,
                bank,
                chip_type.name(),
                chip_config.file,
                mode,
                oracle.len(),
            );
            let (reads, failures, bus_failures, forced_low_failures) = run_mode(
                &emulator,
                &cache,
                &oracle,
                mode,
                cycles_addr_before_cs,
                cycles_cs_to_data,
                set_idx,
                bank,
                bg,
                &gap_gpios,
            );
            let t = timing_pass(
                &emulator,
                &cache,
                mode,
                cycles_addr_before_cs,
                bg,
                alg_info_for(
                    board,
                    chip_type,
                    chip_config,
                    None,
                    ChipSetType::Banked,
                    chip_set.chips.len(),
                    force_16_bit,
                )
                .as_ref(),
                if mode == 16 {
                    oracle.len() / 2
                } else {
                    oracle.len()
                },
                &gap_gpios,
                &format!("set={set_idx} bank={bank} mode={mode}bit"),
            );
            mode_results.push(ModeResult {
                mode,
                reads,
                failures,
                bus_failures,
                forced_low_failures,
                timing_checks: t.checks,
                timing_failures: t.failures,
                timing_note: t.note,
                combos: 1,
            });
        }
        chip_results.push(ChipResult {
            set_idx,
            chip_idx: bank,
            chip_type,
            filename: chip_config.file.clone(),
            mode_results,
        });
    }

    SetResult::done(set_idx, chip_results)
    // `emulator` dropped here; Drop impl frees the epio handle.
}

// ── Boot helper ───────────────────────────────────────────────────────────────

/// Boot the firmware for a chip set, returning the ready `Emulator` and the
/// parsed firmware version, or an error `SetResult` if the firmware failed to
/// start correctly.
///
/// Sets the RP variant and sel image before booting, then verifies that the
/// firmware is not in limp mode and that the PIO state machines are enabled.
/// The firmware version is read back (and `'v'`-stripped before parsing) so
/// callers can rebuild the gen metadata that matches the running firmware.
/// Shared by all three set types.
fn boot_set(
    board: Board,
    chip_set: &ChipSetConfig,
    set_idx: usize,
    sel_image: u8,
) -> Result<(Emulator, FirmwareVersion), SetResult> {
    // Both the RP variant and image selection must be set before boot so the
    // firmware sees the correct state during initialisation.
    Emulator::set_rp_variant(board.rp_variant());
    debug!("Set {}: selecting image {}", set_idx, sel_image);
    Emulator::set_sel_image(sel_image);

    debug!("Set {}: booting firmware", set_idx);
    let mut emulator = Emulator::boot();

    // Confirm the firmware selected the image this set drove the sel pins for.
    // A sel value beyond the board's pin count wraps, by design — run_all
    // accounts for that (oracle substitution, one-beyond test), so compare
    // against the wrapped value rather than the raw request.  Any other
    // discrepancy means the set would silently have tested a different ROM.
    let max_images = 1usize << board.sel_pins().len();
    let expected_image = (sel_image as usize % max_images) as u8;
    if emulator.sel_image() != expected_image {
        error!(
            "Set {}: firmware selected image {}, not {}",
            set_idx,
            emulator.sel_image(),
            expected_image
        );
        return Err(SetResult::boot_error(
            set_idx,
            "firmware selected a different image — the set would have tested the wrong ROM",
        ));
    }
    if emulator.limp_mode() {
        error!("Set {}: firmware entered limp mode", set_idx);
        return Err(SetResult::boot_error(set_idx, "firmware entered limp mode"));
    }
    if !emulator.pios_enabled() {
        error!("Set {}: PIO state machines not enabled after boot", set_idx);
        return Err(SetResult::boot_error(
            set_idx,
            "PIO state machines not enabled after boot",
        ));
    }

    // Read and parse the firmware version (same protocol as setup.rs), so the
    // gap check can rebuild metadata matching the running firmware.
    let (result, version_str) = emulator.get_device_version(64);
    if !result.is_ok() {
        error!(
            "Set {}: failed to get device version: {:?}",
            set_idx, result
        );
        return Err(SetResult::boot_error(
            set_idx,
            "failed to get device version",
        ));
    }
    let version_str = match version_str {
        Some(s) => s,
        None => {
            error!(
                "Set {}: get_device_version returned OK but no string",
                set_idx
            );
            return Err(SetResult::boot_error(
                set_idx,
                "get_device_version returned no string",
            ));
        }
    };
    let stripped = version_str
        .strip_prefix('v')
        .unwrap_or(&version_str)
        .to_string();
    let fw_version = match FirmwareVersion::try_from_str(&stripped) {
        Ok(v) => v,
        Err(e) => {
            error!(
                "Set {}: failed to parse firmware version '{}': {}",
                set_idx, version_str, e
            );
            return Err(SetResult::boot_error(
                set_idx,
                "failed to parse firmware version",
            ));
        }
    };

    debug!("Set {}: PIOs enabled, setting up epio", set_idx);

    let word_size = word_size_for_set(chip_set);
    debug!("Set {}: word_size={}", set_idx, word_size);
    emulator.setup_epio(word_size);
    emulator.step_cycles(timing::CYCLES_BEFORE_START);

    Ok((emulator, fw_version))
}

// ── Per chip ──────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn run_chip(
    emulator: &Emulator,
    board: Board,
    chip_config: &ChipConfig,
    set_idx: usize,
    chip_idx: usize,
    base_dir: &std::path::Path,
    force_16_bit: bool,
    background_mask: (u64, u64),
    gap_gpios: &[u8],
) -> ChipResult {
    let requested_chip_type = chip_config.chip_type.resolved();

    // Apply any board-specific chip substitutions.  Some boards cannot serve
    // a chip in its native mode but can do so with a physical shim that
    // remaps pins; in those cases the firmware actually serves a different
    // chip type.  We warn loudly and test against the effective type.
    let chip_type = if let Some(sub) = chip_substitution(board, requested_chip_type) {
        warn!(
            "Set {} chip {}: {} on {} is not directly servable; \
             substituting {} (physical shim required) for this test",
            set_idx,
            chip_idx,
            requested_chip_type.name(),
            board.name(),
            sub.name(),
        );
        sub
    } else {
        requested_chip_type
    };

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

    let cycles_addr_before_cs = addr_before_cs_cycles(chip_type);

    let mut mode_results = Vec::new();
    for &mode in chip_type.bit_modes() {
        // In force_16_bit mode the firmware uses AlgData0 (word_size=16) and
        // ignores BYTE# entirely, so only the 16-bit pass is meaningful.
        if force_16_bit && mode != 16 {
            debug!(
                "Set {} chip {}: skipping {}bit mode (force_16_bit)",
                set_idx, chip_idx, mode
            );
            continue;
        }
        let cycles_cs_to_data = cs_to_data_cycles(chip_type, mode);
        info!(
            "Testing set={} chip={} ({}) file={} mode={}bit ({} bytes)",
            set_idx,
            chip_idx,
            chip_type.name(),
            chip_config.file,
            mode,
            oracle.len(),
        );
        let (reads, failures, bus_failures, forced_low_failures) = run_mode(
            emulator,
            &cache,
            &oracle,
            mode,
            cycles_addr_before_cs,
            cycles_cs_to_data,
            set_idx,
            chip_idx,
            background_mask,
            gap_gpios,
        );
        let t = timing_pass(
            emulator,
            &cache,
            mode,
            cycles_addr_before_cs,
            background_mask,
            alg_info_for(
                board,
                chip_type,
                chip_config,
                None,
                ChipSetType::Single,
                1,
                force_16_bit,
            )
            .as_ref(),
            if mode == 16 {
                oracle.len() / 2
            } else {
                oracle.len()
            },
            gap_gpios,
            &format!("set={set_idx} chip={chip_idx} mode={mode}bit"),
        );
        mode_results.push(ModeResult {
            mode,
            reads,
            failures,
            bus_failures,
            forced_low_failures,
            timing_checks: t.checks,
            timing_failures: t.failures,
            timing_note: t.note,
            combos: 1,
        });
    }

    ChipResult {
        set_idx,
        chip_idx,
        chip_type,
        filename: chip_config.file.clone(),
        mode_results,
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Compute the forced-low gap set for a slot and cross-check it against the
/// firmware's declared `GpioOverLow` overrides.
///
/// The used set is assembled from gen's emitted metadata for address and data
/// (`geom.addr_pins`, `geom.data_pins`) and the first `n_used_x` board X pins
/// (`geom.x_pin_gpios`, the bank-select / secondary-CS lines this set actually
/// uses), unioned with the tester's independent `PinCache` view of the chip
/// selects and `/BYTE` pin.  Gaps are the address-window GPIOs not in that set.
///
/// On success the derived gaps are returned for use as the adversarial drive
/// set.  On disagreement with `geom.forced_low_gpios` a `gap_error` `SetResult`
/// is returned, which aborts the set's read passes (mirroring a boot error) and
/// names the under-/over-declared pins.
///
/// Substituted sets are skipped: the rebuilt metadata reflects the requested
/// chip type while the drive uses the substituted type, so the two geometries
/// can legitimately diverge.  An empty gap set is returned (no drive, no check).
#[allow(clippy::too_many_arguments)]
fn gap_set_for_slot(
    board: Board,
    config: &Config,
    base_dir: &std::path::Path,
    served_idx: usize,
    fw_version: FirmwareVersion,
    requested_primary_type: ChipType,
    primary_cache: &PinCache,
    n_used_x: usize,
    set_idx: usize,
) -> Result<Vec<u8>, SetResult> {
    if chip_substitution(board, requested_primary_type).is_some() {
        warn!(
            "Set {}: skipping forced-low gap check — chip substitution in effect",
            set_idx
        );
        return Ok(Vec::new());
    }

    let geom = match geometry::slot_geometry(config, board, fw_version, base_dir, served_idx) {
        Ok(g) => g,
        Err(e) => {
            error!("Set {}: metadata geometry unavailable: {}", set_idx, e);
            return Err(SetResult::gap_error(
                set_idx,
                &format!("metadata geometry unavailable: {e}"),
            ));
        }
    };

    // Used set: addr/data from gen's metadata; CS + /BYTE from the tester's
    // independent PinCache; the first n_used_x board X pins from metadata.
    let mut used = 0u64;
    for &g in &geom.addr_pins {
        used |= 1u64 << g;
    }
    for &g in &geom.data_pins {
        used |= 1u64 << g;
    }
    for cl in &primary_cache.control_lines {
        for &g in &cl.gpios {
            used |= 1u64 << g;
        }
    }
    if let Some(g) = primary_cache.byte_n_gpio {
        used |= 1u64 << g;
    }
    for x in geom.x_pin_gpios.iter().take(n_used_x) {
        for &g in x {
            used |= 1u64 << g;
        }
    }

    let base = geom.addr_window_base;
    let end = geom.addr_window_base + geom.addr_window_len;
    let mut gaps: Vec<u8> = (base..end).filter(|&g| used & (1u64 << g) == 0).collect();
    gaps.sort_unstable();

    let mut declared = geom.forced_low_gpios.clone();
    declared.sort_unstable();
    declared.dedup();

    if gaps != declared {
        let derived: std::collections::HashSet<u8> = gaps.iter().copied().collect();
        let decl: std::collections::HashSet<u8> = declared.iter().copied().collect();
        let under: Vec<u8> = gaps.iter().copied().filter(|g| !decl.contains(g)).collect();
        let over: Vec<u8> = declared
            .iter()
            .copied()
            .filter(|g| !derived.contains(g))
            .collect();
        error!(
            "Set {}: address-window gap check failed — derived {:?} vs declared {:?}",
            set_idx, gaps, declared
        );
        return Err(SetResult::gap_error(
            set_idx,
            &format!(
                "tester-derived gaps {gaps:?} != firmware GpioOverLow {declared:?}; \
                 under-declared (gap not forced low) {under:?}; \
                 over-declared (forced low but not a gap) {over:?}"
            ),
        ));
    }

    Ok(gaps)
}

/// Name of the per-chip select line for a Multi set: the single active
/// (non-Ignore) configurable CS line on a secondary chip. `None` if none
/// (only configurable-CS chips are supported as secondaries, so this matches
/// the polarity lookup in `first_active_cs_polarity`).
fn per_chip_select_name(secondary: &ChipConfig) -> Option<&'static str> {
    secondary
        .chip_type
        .resolved()
        .control_lines()
        .iter()
        .filter(|spec| matches!(spec.line_type, ControlLineType::Configurable))
        .find_map(|spec| {
            let logic = match spec.name {
                "cs1" => secondary.cs1,
                "cs2" => secondary.cs2,
                "cs3" => secondary.cs3,
                _ => None,
            };
            matches!(logic, Some(CsLogic::ActiveHigh) | Some(CsLogic::ActiveLow))
                .then_some(spec.name)
        })
}

/// Verify that no ROM-serving GPIO has a pull resistor configured.
///
/// Builds a mask of the active pins — data, address, CS, and byte — from the
/// PinCache and checks that none of them carry a pull-up or pull-down.  X pins
/// (used for bank selection) are intentionally pulled and are not in the cache,
/// so they are not checked.
///
/// A pull on a ROM-serving pin means the firmware is missing
/// `APIO_GPIO_PULL_NONE` for that pin, which would corrupt emulated reads when
/// `epio_drive_gpios_ext` restores undriven pins to their pull state.
fn check_rom_pin_pulls(
    emulator: &Emulator,
    cache: &PinCache,
    set_idx: usize,
) -> Result<(), SetResult> {
    let mut mask = 0u64;
    for &g in &cache.data_gpios {
        mask |= 1u64 << g;
    }
    for gpios in &cache.addr_gpios {
        for &g in gpios {
            mask |= 1u64 << g;
        }
    }
    for cl in &cache.control_lines {
        for &g in &cl.gpios {
            mask |= 1u64 << g;
        }
    }
    if let Some(g) = cache.byte_n_gpio {
        mask |= 1u64 << g;
    }

    let bad = (emulator.read_pull_up_pins() | emulator.read_pull_down_pins()) & mask;
    debug!(
        "Set {}: checking ROM-serving GPIO pulls with mask {:#018x}",
        set_idx, mask
    );
    if bad != 0 {
        error!(
            "Set {}: ROM-serving GPIOs have unexpected pull — {:#018x} \
             (firmware missing APIO_GPIO_PULL_NONE)",
            set_idx, bad
        );
        return Err(SetResult::boot_error(
            set_idx,
            "unexpected pull on ROM-serving GPIO",
        ));
    }
    Ok(())
}

/// Verify that X pins have the correct pull direction configured for a banked
/// set.
///
/// Open jumpers rely on the MCU pull resistor to hold a defined level.  The
/// required direction is the opposite of `board.x_jumper_pull()`:
///
/// * closed = HIGH (`x_jumper_pull() == 1`) → open must read LOW → pull-down
/// * closed = LOW  (`x_jumper_pull() == 0`) → open must read HIGH → pull-up
///
/// A wrong or missing pull means an open jumper would float to the float-mode
/// value rather than the firmware-intended level, causing the wrong bank to be
/// selected.
fn check_x_pin_pulls(
    emulator: &Emulator,
    board: Board,
    set_idx: usize,
    n_pins_used: usize,
) -> Result<(), SetResult> {
    let x_pin_map = board.x_pin_map();
    if x_pin_map.is_empty() || n_pins_used == 0 {
        return Ok(());
    }

    let mut x_mask = 0u64;
    for (_, gpios) in x_pin_map.iter().take(n_pins_used) {
        for &g in *gpios {
            x_mask |= 1u64 << g;
        }
    }

    debug!(
        "Set {}: checking X pin pulls with mask {:#018x}",
        set_idx, x_mask
    );

    let pull_up = emulator.read_pull_up_pins();
    let pull_down = emulator.read_pull_down_pins();

    if board.x_jumper_pull() == 1 {
        // Jumper closed = HIGH → open pin must read LOW → pull-down required
        let missing = x_mask & !pull_down;
        let wrong = x_mask & pull_up;
        if missing != 0 || wrong != 0 {
            error!(
                "Set {}: X pins have wrong pull — expected pull-down; \
                 missing={:#018x} wrong_pull_up={:#018x}",
                set_idx, missing, wrong
            );
            return Err(SetResult::boot_error(
                set_idx,
                "X pins missing required pull-down",
            ));
        }
    } else {
        // Jumper closed = LOW → open pin must read HIGH → pull-up required
        let missing = x_mask & !pull_up;
        let wrong = x_mask & pull_down;
        if missing != 0 || wrong != 0 {
            error!(
                "Set {}: X pins have wrong pull — expected pull-up; \
                 missing={:#018x} wrong_pull_down={:#018x}",
                set_idx, missing, wrong
            );
            return Err(SetResult::boot_error(
                set_idx,
                "X pins missing required pull-up",
            ));
        }
    }

    Ok(())
}

/// Return the effective `ChipType` to test when a board/chip combination
/// requires a physical shim and the firmware therefore serves a different chip
/// type than the one nominally installed.  Returns `None` when no substitution
/// is needed.
///
/// Add new entries here as further board/chip shim combinations are discovered.
fn word_size_for_set(chip_set: &ChipSetConfig) -> u8 {
    chip_set
        .chips
        .first()
        .map(|c| {
            if c.chip_type.resolved() == ChipType::Chip27C400
                || c.chip_type.resolved() == ChipType::Chip27C200
            {
                16
            } else {
                8
            }
        })
        .unwrap_or(8)
}

/// Extract the `force_16_bit` flag from a chip set's firmware overrides.
fn get_force_16_bit(chip_set: &ChipSetConfig) -> bool {
    chip_set
        .firmware_overrides
        .as_ref()
        .and_then(|fw| fw.fire.as_ref())
        .map(|f| f.force_16_bit)
        .unwrap_or(false)
}

/// Find the assertion polarity for the first active (non-Ignore) configurable
/// CS line on a secondary chip in a multi-ROM set.
///
/// Returns `true` if the corresponding X pin must be driven HIGH to assert CS.
///
/// # Panics
/// Panics if `chip_type` has no active configurable CS line.  Only chips with
/// at least one configurable CS are currently supported as secondary chips;
/// chips with only fixed CE/OE lines require future config extensions to
/// specify which CE/OE pin connects to the X pin.
fn first_active_cs_polarity(chip_config: &ChipConfig, chip_type: ChipType) -> bool {
    chip_type
        .control_lines()
        .iter()
        .filter(|spec| matches!(spec.line_type, ControlLineType::Configurable))
        .find_map(|spec| {
            let logic = match spec.name {
                "cs1" => chip_config.cs1,
                "cs2" => chip_config.cs2,
                "cs3" => chip_config.cs3,
                _ => None,
            };
            match logic {
                Some(CsLogic::ActiveHigh) => Some(true),
                Some(CsLogic::ActiveLow) => Some(false),
                Some(CsLogic::Ignore) | None => None,
            }
        })
        .unwrap_or_else(|| {
            panic!(
                "Multi-ROM secondary chip {} has no active (non-Ignore) configurable CS \
                 line — only chips with a configurable CS are currently supported as \
                 secondary chips; fixed CE/OE chips require future config extensions",
                chip_type.name()
            )
        })
}

/// Build the GPIO background mask for X pins in a dynamically banked set.
///
/// Bit k (0-indexed) of `bank_idx` is the logical value of X pin k+1:
/// `1` = jumper closed → drive to `x_jumper_pull()` level.
/// `0` = jumper open  → leave undriven; epio_drive_gpios_ext restores the
///                       pin to its configured pull state (pull-none →
///                       float mode value) on every drive call.
///
/// Only closed pins are included in the mask.  Open pins are left out so
/// epio_drive_gpios_ext can restore them correctly on every call.
///
/// If an X pin maps to multiple MCU GPIOs all are driven to the same level.
fn banked_x_mask(board: Board, bank_idx: usize) -> (u64, u64) {
    let closed_high = board.x_jumper_pull() == 1;
    board
        .x_pin_map()
        .iter()
        .enumerate()
        .fold((0u64, 0u64), |acc, (k, pin_entry)| {
            let gpios: &[u8] = pin_entry.1;
            if (bank_idx >> k) & 1 == 1 {
                // Jumper closed: drive to x_jumper_pull level.
                let gpio_mask = gpios.iter().fold((0u64, 0u64), |a, &g| {
                    driver::merge(a, (1u64 << g, if closed_high { 1u64 << g } else { 0 }))
                });
                driver::merge(acc, gpio_mask)
            } else {
                // Jumper open: omit from mask so epio_drive_gpios_ext restores
                // to pull state on every call.
                acc
            }
        })
}
