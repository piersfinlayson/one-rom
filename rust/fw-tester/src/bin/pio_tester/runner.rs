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

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::hw::Board;
use onerom_fw_emulator::Emulator;
use onerom_gen::{ChipConfig, ChipSetConfig, ChipSetType, Config, CsLogic};

use crate::report::{ChipResult, ModeResult, SetResult, TestReport};
use onerom_fw_tester::driver;
use onerom_fw_tester::oracle;
use onerom_fw_tester::pin_cache::{ControlLine, PinCache};
use onerom_fw_tester::runner::{addr_before_cs_cycles, cs_to_data_cycles, run_mode};
use onerom_fw_tester::timing;

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

        let mut result = run_chip_set(board, oracle_set, set_idx, set_idx as u8, base_dir);
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
            &config.chip_sets[0],
            num_sets,
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
    chip_set: &ChipSetConfig,
    set_idx: usize,
    sel_image: u8,
    base_dir: &std::path::Path,
) -> SetResult {
    match chip_set.set_type {
        ChipSetType::Single => run_single_set(board, chip_set, set_idx, sel_image, base_dir),
        ChipSetType::Multi => run_multi_set(board, chip_set, set_idx, sel_image, base_dir),
        ChipSetType::Banked => run_banked_set(board, chip_set, set_idx, sel_image, base_dir),
    }
}

// ── Single chip set ───────────────────────────────────────────────────────────

fn run_single_set(
    board: Board,
    chip_set: &ChipSetConfig,
    set_idx: usize,
    sel_image: u8,
    base_dir: &std::path::Path,
) -> SetResult {
    let emulator = match boot_set(board, chip_set, set_idx, sel_image) {
        Ok(e) => e,
        Err(r) => return r,
    };

    let force_16_bit = get_force_16_bit(chip_set);

    // Check ROM-serving GPIO pulls.  Build the PinCache for the first chip to
    // derive the active GPIO mask (data + addr + CS + byte).
    if let Some(chip_config) = chip_set.chips.first() {
        let chip_type =
            chip_substitution(board, chip_config.chip_type).unwrap_or(chip_config.chip_type);
        let cache = PinCache::build(chip_type, chip_config, board);
        if let Err(r) = check_rom_pin_pulls(&emulator, &cache, set_idx) {
            return r;
        }
    }

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
            )
        })
        .collect();

    SetResult::done(set_idx, chip_results)
    // `emulator` dropped here; Drop impl frees the epio handle.
}

// ── Multi-ROM chip set ────────────────────────────────────────────────────────

fn run_multi_set(
    board: Board,
    chip_set: &ChipSetConfig,
    set_idx: usize,
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
        return run_single_set(board, chip_set, set_idx, sel_image, base_dir);
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

    let emulator = match boot_set(board, chip_set, set_idx, sel_image) {
        Ok(e) => e,
        Err(r) => return r,
    };

    let force_16_bit = get_force_16_bit(chip_set);

    // ── Primary chip (chips[0], in One ROM's socket) ──────────────────────────
    let primary_config = &chip_set.chips[0];
    let primary_requested = primary_config.chip_type;
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
    let primary_cache = PinCache::build(primary_type, primary_config, board);
    debug!(
        "Set {} chip 0: {} addr GPIOs, {} data GPIOs, {} control line(s)",
        set_idx,
        primary_cache.addr_gpios.len(),
        primary_cache.data_gpios.len(),
        primary_cache.control_lines.len(),
    );

    if let Err(r) = check_rom_pin_pulls(&emulator, &primary_cache, set_idx) {
        return r;
    }

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
            let assert_high = first_active_cs_polarity(chip_config, chip_config.chip_type);
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
            let (reads, failures, bus_failures) = run_mode(
                &emulator,
                &primary_cache,
                &oracle,
                mode,
                cycles_addr_before_cs,
                timing::CYCLES_CS_TO_DATA_MULTI,
                set_idx,
                0,
                chips0_bg,
            );
            mode_results.push(ModeResult {
                mode,
                reads,
                failures,
                bus_failures,
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
        let requested_type = chip_config.chip_type;
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

                let (reads, failures, bus_failures) = run_mode(
                    &emulator,
                    &secondary_cache,
                    &oracle,
                    mode,
                    cycles_addr_before_cs,
                    timing::CYCLES_CS_TO_DATA_MULTI,
                    set_idx,
                    chip_idx,
                    bg,
                );
                total_reads += reads;
                total_failures += failures;
                total_bus_failures += bus_failures;
            }

            mode_results.push(ModeResult {
                mode,
                reads: total_reads,
                failures: total_failures,
                bus_failures: total_bus_failures,
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
    chip_set: &ChipSetConfig,
    set_idx: usize,
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
    let chip_type_0 = chip_set.chips[0].chip_type;
    if let Some(pos) = chip_set
        .chips
        .iter()
        .position(|c| c.chip_type != chip_type_0)
    {
        error!(
            "Set {}: banked sets require a uniform chip type; \
             chip {} is {} but chip 0 is {}",
            set_idx,
            pos,
            chip_set.chips[pos].chip_type.name(),
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

    let emulator = match boot_set(board, chip_set, set_idx, sel_image) {
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
            let (reads, failures, bus_failures) = run_mode(
                &emulator,
                &cache,
                &oracle,
                mode,
                cycles_addr_before_cs,
                cycles_cs_to_data,
                set_idx,
                bank,
                bg,
            );
            mode_results.push(ModeResult {
                mode,
                reads,
                failures,
                bus_failures,
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

/// Boot the firmware for a chip set, returning the ready `Emulator` or an
/// error `SetResult` if the firmware failed to start correctly.
///
/// Sets the RP variant and sel image before booting, then verifies that the
/// firmware is not in limp mode and that the PIO state machines are enabled.
/// Shared by all three set types.
fn boot_set(
    board: Board,
    chip_set: &ChipSetConfig,
    set_idx: usize,
    sel_image: u8,
) -> Result<Emulator, SetResult> {
    // Both the RP variant and image selection must be set before boot so the
    // firmware sees the correct state during initialisation.
    Emulator::set_rp_variant(board.rp_variant());
    debug!("Set {}: selecting image {}", set_idx, sel_image);
    Emulator::set_sel_image(sel_image);

    debug!("Set {}: booting firmware", set_idx);
    let mut emulator = Emulator::boot();

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
    debug!("Set {}: PIOs enabled, setting up epio", set_idx);

    let word_size = word_size_for_set(chip_set);
    debug!("Set {}: word_size={}", set_idx, word_size);
    emulator.setup_epio(word_size);
    emulator.step_cycles(timing::CYCLES_BEFORE_START);

    Ok(emulator)
}

// ── Per chip ──────────────────────────────────────────────────────────────────

fn run_chip(
    emulator: &Emulator,
    board: Board,
    chip_config: &ChipConfig,
    set_idx: usize,
    chip_idx: usize,
    base_dir: &std::path::Path,
    force_16_bit: bool,
    background_mask: (u64, u64),
) -> ChipResult {
    let requested_chip_type = chip_config.chip_type;

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
        let (reads, failures, bus_failures) = run_mode(
            emulator,
            &cache,
            &oracle,
            mode,
            cycles_addr_before_cs,
            cycles_cs_to_data,
            set_idx,
            chip_idx,
            background_mask,
        );
        mode_results.push(ModeResult {
            mode,
            reads,
            failures,
            bus_failures,
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
fn chip_substitution(board: Board, chip_type: ChipType) -> Option<ChipType> {
    match (board, chip_type) {
        // fire-32-a cannot drive SST39SF040 directly; a pin-remap shim allows
        // it to serve the image as a 27C040 instead.
        (Board::Fire32A, ChipType::ChipSST39SF040) => Some(ChipType::Chip27C040),
        _ => None,
    }
}

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
