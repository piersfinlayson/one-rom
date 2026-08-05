// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Test result types and human-readable stdout renderer.
//!
//! All results are accumulated into a [`TestReport`] during the run and
//! printed atomically at the end.  The internal representation is kept
//! separate from the rendering so alternative output formats (e.g. JUnit XML,
//! JSON) can be added later by swapping or supplementing the renderer without
//! touching the result types.

use onerom_config::chip::ChipType;

// ── Result leaf types ─────────────────────────────────────────────────────────

/// Results for one bit-mode pass of one chip.
pub struct ModeResult {
    /// Bit width under test (8 or 16).
    pub mode: u8,
    /// Total bytes compared across all combos (2 × word count in 16-bit mode,
    /// multiplied by `combos`).
    pub reads: u64,
    /// Bytes that did not match the oracle (across all combos).
    pub failures: u64,
    /// Data bus state violations across all combos (not driven when CS active,
    /// still driven after CS deassert, or driven for a non-active CS
    /// combination).
    pub bus_failures: u64,
    /// Forced-low override failures: a non-address GPIO inside the address
    /// window that a `GpioOverLow` override should hold low changed the served
    /// byte when toggled.  Zero on sets with no address-window gaps (the
    /// adversarial re-read is skipped entirely there).
    pub forced_low_failures: u64,
    /// CS-to-data timing checks run, and how many did not measure the expected
    /// latency.  Kept apart from `failures` because a timing regression is a
    /// change in how fast the PIO serving path runs, not a serving bug — the
    /// bytes are correct, they just arrive at a different time.  Zero checks
    /// when the pass could not run (see `timing_note`).
    pub timing_checks: u64,
    pub timing_failures: u64,
    /// Why the timing pass did not run, when `timing_checks` is 0.  Recorded
    /// rather than silently skipped: a check that quietly stops running looks
    /// exactly like one that passes.
    pub timing_note: Option<String>,
    /// Number of extra-address-bit combinations exercised.  1 for all single,
    /// banked, and equal-width multi sets.  > 1 for multi sets where the
    /// secondary chip has fewer address lines than the primary: each missing
    /// address line doubles the combo count (e.g. 2332 behind 2364 → 2 combos).
    pub combos: u32,
}

impl ModeResult {
    pub fn passed(&self) -> bool {
        self.failures == 0
            && self.bus_failures == 0
            && self.forced_low_failures == 0
            && self.timing_failures == 0
    }
}

/// Results for one chip within a chip set.
pub struct ChipResult {
    pub set_idx: usize,
    pub chip_idx: usize,
    pub chip_type: ChipType,
    pub filename: String,
    pub mode_results: Vec<ModeResult>,
}

impl ChipResult {
    pub fn passed(&self) -> bool {
        self.mode_results.iter().all(|m| m.passed())
    }
    // Retained for future structured output formats (e.g. JUnit XML, JSON).
    #[allow(dead_code)]
    pub fn total_reads(&self) -> u64 {
        self.mode_results.iter().map(|m| m.reads).sum()
    }
    #[allow(dead_code)]
    pub fn total_failures(&self) -> u64 {
        self.mode_results.iter().map(|m| m.failures).sum()
    }
    #[allow(dead_code)]
    pub fn total_bus_failures(&self) -> u64 {
        self.mode_results.iter().map(|m| m.bus_failures).sum()
    }
}

/// Results for one chip set (corresponding to one firmware boot).
pub struct SetResult {
    pub set_idx: usize,
    pub chip_results: Vec<ChipResult>,
    /// `true` → set was intentionally skipped (e.g. unsupported type).
    pub skipped: bool,
    pub skip_reason: Option<String>,
    /// `Some(msg)` → firmware did not boot correctly.
    pub boot_error: Option<String>,
    /// `Some(msg)` → the address-window gap check failed at setup: the
    /// tester-derived gap set and the firmware's declared `GpioOverLow` set
    /// disagree.  The set's read passes are not run when this is set.
    pub gap_error: Option<String>,
    /// Optional informational note shown in the report alongside this set's
    /// results (e.g. sel-wrap or one-beyond annotation).
    pub note: Option<String>,
}

impl SetResult {
    pub fn done(set_idx: usize, chip_results: Vec<ChipResult>) -> Self {
        Self {
            set_idx,
            chip_results,
            skipped: false,
            skip_reason: None,
            boot_error: None,
            gap_error: None,
            note: None,
        }
    }

    pub fn skipped(set_idx: usize, reason: &str) -> Self {
        Self {
            set_idx,
            chip_results: vec![],
            skipped: true,
            skip_reason: Some(reason.to_string()),
            boot_error: None,
            gap_error: None,
            note: None,
        }
    }

    pub fn boot_error(set_idx: usize, reason: &str) -> Self {
        Self {
            set_idx,
            chip_results: vec![],
            skipped: false,
            skip_reason: None,
            boot_error: Some(reason.to_string()),
            gap_error: None,
            note: None,
        }
    }

    /// Setup-time address-window gap check failure.  Like a boot error, the
    /// set ran no read passes; reported distinctly from data/bus failures.
    pub fn gap_error(set_idx: usize, reason: &str) -> Self {
        Self {
            set_idx,
            chip_results: vec![],
            skipped: false,
            skip_reason: None,
            boot_error: None,
            gap_error: Some(reason.to_string()),
            note: None,
        }
    }

    pub fn set_note(&mut self, note: String) {
        self.note = Some(note);
    }

    /// `true` iff the set ran and every chip/mode passed.
    /// Boot errors and non-skipped sets with failures return `false`.
    /// Skipped sets return `false` but are excluded from [`TestReport::all_passed`].
    pub fn passed(&self) -> bool {
        if self.skipped || self.boot_error.is_some() || self.gap_error.is_some() {
            return false;
        }
        self.chip_results.iter().all(|c| c.passed())
    }
}

// ── Top-level report ──────────────────────────────────────────────────────────

/// Accumulated results for a complete test run.
pub struct TestReport {
    config_path: String,
    board_str: String,
    set_results: Vec<SetResult>,
}

impl TestReport {
    pub fn new(config_path: &str, board_str: &str) -> Self {
        Self {
            config_path: config_path.to_string(),
            board_str: board_str.to_string(),
            set_results: Vec::new(),
        }
    }

    pub fn add_set_result(&mut self, result: SetResult) {
        self.set_results.push(result);
    }

    /// `true` iff every non-skipped set passed (boot errors count as failures).
    pub fn all_passed(&self) -> bool {
        self.set_results
            .iter()
            .filter(|s| !s.skipped)
            .all(|s| s.passed())
    }

    /// Print a human-readable summary to stdout.
    pub fn print(&self) {
        println!("-----");
        println!("One ROM Firmware Tester");
        println!("Config : {}", self.config_path);
        println!("Board  : {}", self.board_str);
        println!("-----");

        let mut grand_reads = 0u64;
        let mut grand_failures = 0u64;
        let mut grand_bus_failures = 0u64;
        let mut grand_forced_low = 0u64;
        let mut grand_timing_checks = 0u64;
        let mut grand_timing_failures = 0u64;

        for set in &self.set_results {
            if let Some(ref note) = set.note {
                println!("Set {} : NOTE — {}", set.set_idx, note);
            }
            if let Some(ref msg) = set.boot_error {
                println!("Set {} : BOOT ERROR — {}", set.set_idx, msg);
                continue;
            }
            if let Some(ref msg) = set.gap_error {
                println!("Set {} : ADDRESS-WINDOW GAP CHECK — {}", set.set_idx, msg);
                continue;
            }
            if set.skipped {
                println!(
                    "Set {} : SKIPPED — {}",
                    set.set_idx,
                    set.skip_reason.as_deref().unwrap_or("")
                );
                continue;
            }

            for chip in &set.chip_results {
                for mode in &chip.mode_results {
                    grand_reads += mode.reads;
                    grand_failures += mode.failures;
                    grand_bus_failures += mode.bus_failures;
                    grand_forced_low += mode.forced_low_failures;
                    grand_timing_checks += mode.timing_checks;
                    grand_timing_failures += mode.timing_failures;

                    if let Some(ref note) = mode.timing_note {
                        println!(
                            "  [NOTE] set={} chip={} mode={}bit: CS timing pass \
                             did not run — {}",
                            chip.set_idx, chip.chip_idx, mode.mode, note,
                        );
                    }

                    // combos > 1 means the secondary chip had fewer address
                    // lines than the primary; show the count so the inflated
                    // reads total is self-evident (reads = combos × oracle_len).
                    let combo_str = if mode.combos > 1 {
                        format!(" combos={}", mode.combos)
                    } else {
                        String::new()
                    };

                    println!(
                        "  [{}] set={} chip={} ({}) file={} mode={}bit{} \
                         reads={} failures={} bus_failures={} forced_low_failures={} \
                         timing_checks={} timing_failures={}",
                        if mode.passed() { "PASS" } else { "FAIL" },
                        chip.set_idx,
                        chip.chip_idx,
                        chip.chip_type.name(),
                        chip.filename,
                        mode.mode,
                        combo_str,
                        mode.reads,
                        mode.failures,
                        mode.bus_failures,
                        mode.forced_low_failures,
                        mode.timing_checks,
                        mode.timing_failures,
                    );
                }
            }
        }

        println!("-----");
        println!(
            "Total: {} bytes read, {} data failures, {} bus violations, \
             {} forced-low failures, {} CS timing checks with {} failures — {}",
            grand_reads,
            grand_failures,
            grand_bus_failures,
            grand_forced_low,
            grand_timing_checks,
            grand_timing_failures,
            if self.all_passed() { "PASS" } else { "FAIL" },
        );
    }
}
