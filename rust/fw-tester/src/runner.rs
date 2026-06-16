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
//! deasserted on every GPIO drive call so they cannot accidentally enable a
//! chip while another is under test.  For dynamically banked sets the same
//! mechanism holds all X pin GPIOs at the level corresponding to the current
//! bank throughout the test pass.

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::hw::Board;
use onerom_fw_emulator::Emulator;
use onerom_gen::{ChipConfig, ChipSetConfig, ChipSetType, Config, CsLogic};

use crate::driver;
use crate::oracle;
use crate::pin_cache::{ControlLine, PinCache};
use crate::report::{ChipResult, ModeResult, SetResult, TestReport};
use crate::timing;

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
        let mut result =
            run_chip_set(board, &config.chip_sets[0], num_sets, num_sets as u8, base_dir);
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
            set_idx, n_secondary, board.name(), n_x_pins,
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
        let cycles_addr_before_cs = if is_27c400_family(primary_type) {
            timing::CYCLES_27C400_ADDR_BEFORE_CS
        } else {
            timing::CYCLES_ADDR_BEFORE_CS
        };

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
            mode_results.push(run_mode(
                &emulator,
                &primary_cache,
                &oracle,
                primary_type,
                mode,
                cycles_addr_before_cs,
                set_idx,
                0,
                chips0_bg,
            ));
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

        // Background for this secondary chip:
        //   - primary socket CSes deasserted (prevent chips[0] driving the bus)
        //   - all OTHER secondary X pin CSes deasserted
        let other_x_deassert = x_deassert_masks
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != j)
            .fold((0u64, 0u64), |acc, (_, &m)| driver::merge(acc, m));
        let bg = driver::merge(primary_cs_deassert, other_x_deassert);

        let (x_gpios, x_assert_high) = &x_pin_info[j];
        debug!(
            "Set {} chip {}: building secondary pin cache for {} on board {} \
             (X pin GPIOs={:?} assert_high={})",
            set_idx, chip_idx,
            chip_type.name(), board.name(),
            x_gpios, x_assert_high,
        );
        let secondary_cache = PinCache::build_secondary(
            chip_type,
            &primary_cache,
            board,
            x_gpios.clone(),
            *x_assert_high,
        );
        debug!(
            "Set {} chip {}: {} addr GPIOs, {} data GPIOs",
            set_idx, chip_idx,
            secondary_cache.addr_gpios.len(),
            secondary_cache.data_gpios.len(),
        );

        let oracle = oracle::load(chip_config, chip_type, base_dir);
        debug!(
            "Set {} chip {}: oracle loaded, {} bytes",
            set_idx, chip_idx, oracle.len()
        );

        let cycles_addr_before_cs = if is_27c400_family(chip_type) {
            timing::CYCLES_27C400_ADDR_BEFORE_CS
        } else {
            timing::CYCLES_ADDR_BEFORE_CS
        };

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
                "Testing set={} chip={} ({}) file={} mode={}bit ({} bytes)",
                set_idx,
                chip_idx,
                chip_type.name(),
                chip_config.file,
                mode,
                oracle.len(),
            );
            mode_results.push(run_mode(
                &emulator,
                &secondary_cache,
                &oracle,
                chip_type,
                mode,
                cycles_addr_before_cs,
                set_idx,
                chip_idx,
                bg,
            ));
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
        return SetResult::skipped(
            set_idx,
            "dynamic banked sets not supported on this board",
        );
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
    let x_pins_needed =
        (usize::BITS - n_banks.saturating_sub(1).leading_zeros()) as usize;
    let n_x_pins = board.x_pin_map().len();
    if x_pins_needed > n_x_pins {
        error!(
            "Set {}: {} bank(s) require {} X pin(s) to encode but board {} has only {}",
            set_idx, n_banks, x_pins_needed, board.name(), n_x_pins,
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

    let cycles_addr_before_cs = if is_27c400_family(chip_type) {
        timing::CYCLES_27C400_ADDR_BEFORE_CS
    } else {
        timing::CYCLES_ADDR_BEFORE_CS
    };

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
            set_idx, bank, oracle.len()
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
            info!(
                "Testing set={} bank={} ({}) file={} mode={}bit ({} bytes)",
                set_idx,
                bank,
                chip_type.name(),
                chip_config.file,
                mode,
                oracle.len(),
            );
            mode_results.push(run_mode(
                &emulator,
                &cache,
                &oracle,
                chip_type,
                mode,
                cycles_addr_before_cs,
                set_idx,
                bank,
                bg,
            ));
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

    let cycles_addr_before_cs = if is_27c400_family(chip_type) {
        timing::CYCLES_27C400_ADDR_BEFORE_CS
    } else {
        timing::CYCLES_ADDR_BEFORE_CS
    };

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
            background_mask,
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
    background_mask: (u64, u64),
) -> ModeResult {
    let is_27c400_family = is_27c400_family(chip_type);

    // Pre-compute the BYTE# mask so it can be merged into every drive_gpios
    // call.  epio_drive_gpios_ext resets every GPIO that is *not* in the
    // supplied mask to the pull-up state (1) on each call, so a one-shot
    // drive before the loop is immediately overwritten by the first
    // phase1/phase2 call.  Merging into every call holds the level correctly
    // throughout the pass.
    let byte_mask: (u64, u64) = if let Some(gpio) = cache.byte_n_gpio {
        let bm = driver::byte_n_mask(gpio, mode);
        debug!(
            "BYTE# gpio={} mode={} mask={:#018x} levels={:#018x}",
            gpio, mode, bm.0, bm.1
        );
        bm
    } else {
        (0u64, 0u64)
    };

    // Merge BYTE# with the caller-supplied background mask to produce the
    // single constant mask applied on every GPIO drive call.
    //
    // background_mask holds GPIOs that must be kept at a fixed level:
    //   - Single chip sets:  (0, 0) — nothing extra to hold.
    //   - Multi-ROM sets:    deasserted CS lines of all non-active chips.
    //   - Banked sets:       all X pins driven to the level selecting the bank.
    let const_mask = driver::merge(byte_mask, background_mask);

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

    // In 16-bit mode, addr_gpios[0] is A-1, which is also D15 — a data
    // output pin in word mode.  Driving it as an address pin would interfere
    // with the data bus and cause false bus violations.  Skip it and use only
    // A0-A17 (indices [1..]) with the word index (addr_idx) as the drive
    // address, so bit 0 of addr_idx maps to A0, bit 1 to A1, etc.
    //
    // In 8-bit mode, use all address GPIOs including A-1 at index 0 (bit 0
    // of the byte address becomes the low/high byte select).
    //
    // addr_shift is retained solely for computing phys_addr for log messages,
    // which uses byte addresses in both modes.
    let addr_gpios: &[Vec<u8>] = if mode == 16 {
        &cache.addr_gpios[1..]
    } else {
        &cache.addr_gpios
    };

    debug!(
        "Mode {}bit: {} iterations, addr_shift={}, \
         cycles_addr_before_cs={}, cycles_cs_to_data={}",
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

    // The data GPIO slice for byte extraction and mismatch logging in 8-bit
    // mode.  Even on 16-bit-capable chips the cache has 16 data GPIOs, but
    // in byte mode only D0-D7 are driven.
    let data_gpios_8 = &cache.data_gpios[..8.min(cache.data_gpios.len())];

    // CS-deasserted drive merged with const_mask so all constant levels are
    // held between reads.
    let deassert_drive = driver::merge(ctrl_deasserted, const_mask);

    let mut reads = 0u64;
    let mut failures = 0u64;
    let mut bus_failures = 0u64;

    for addr_idx in 0..iter_count {
        // phys_addr is the byte address, used for log messages only.
        // In 16-bit mode this is addr_idx*2 (byte offset of the word).
        // In 8-bit mode addr_shift==0 so it equals addr_idx.
        let phys_addr = addr_idx << addr_shift;

        // The GPIO drive address is always addr_idx:
        // - 16-bit: addr_gpios is [1..] so bit 0 of addr_idx maps to A0. ✓
        // - 8-bit:  addr_gpios is full slice, addr_idx is the byte address. ✓
        let drive_addr = addr_idx;

        // ── Phase 1: address valid, CS inactive ──────────────────────────────
        let phase1 = driver::merge(
            driver::merge(driver::addr_mask(drive_addr, addr_gpios), ctrl_deasserted),
            const_mask,
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
            driver::merge(driver::addr_mask(drive_addr, addr_gpios), ctrl_active),
            const_mask,
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
            // 8-bit mode: only D0-D7 are active (D8-D15 are tristated by
            // BYTE# on 27C400-family devices).
            let byte = driver::extract_byte(pin_states, data_gpios_8);
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
                    data_gpios_8,
                    failures,
                );
            }
        }

        // ── Phase 4: deassert CS and settle ──────────────────────────────────
        emulator.drive_gpios(deassert_drive.0, deassert_drive.1);
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
    //
    // const_mask is merged into every drive here too, keeping background GPIOs
    // (other chips' CS lines, bank-select X pins) at their required levels
    // throughout the combo sweep.
    let n = cache.control_lines.len();
    if n > 0 {
        let all_asserted: u64 = (1u64 << n) - 1;
        debug!(
            "Mode {}bit combo test: {} control line(s), {} non-active combinations",
            mode, n, all_asserted
        );

        for combo in 0u64..all_asserted {
            let ctrl = ctrl_combo_mask(cache, combo);
            // Use the mode-appropriate addr_gpios slice (no A-1 in 16-bit
            // mode) and hold const_mask via merge.
            let phase = driver::merge(
                driver::merge(driver::addr_mask(0, addr_gpios), ctrl),
                const_mask,
            );
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

        // Leave control lines deasserted, const_mask held.
        emulator.drive_gpios(deassert_drive.0, deassert_drive.1);
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

/// Returns `true` for the 27C400/27C200 chip family, which requires special
/// timing and BYTE# handling.
fn is_27c400_family(chip_type: ChipType) -> bool {
    chip_type == ChipType::Chip27C400 || chip_type == ChipType::Chip27C200
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
/// `0` = jumper open  → leave undriven; the emulator's simulated firmware
///                       pull holds the pin at the correct level.
///
/// Only closed pins are included in the mask.  Driving open pins explicitly
/// is no longer necessary now that the emulator models the firmware's
/// internal pull configuration.
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
                    driver::merge(
                        a,
                        (1u64 << g, if closed_high { 1u64 << g } else { 0 }),
                    )
                });
                driver::merge(acc, gpio_mask)
            } else {
                // Jumper open: omit from mask; emulator pull takes over.
                acc
            }
        })
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
            .rev()
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