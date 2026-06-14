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
    /// Total bytes compared (2 × word count in 16-bit mode).
    pub reads: u64,
    /// Bytes that did not match the oracle.
    pub failures: u64,
}

impl ModeResult {
    pub fn passed(&self) -> bool {
        self.failures == 0
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
}

impl SetResult {
    pub fn done(set_idx: usize, chip_results: Vec<ChipResult>) -> Self {
        Self {
            set_idx,
            chip_results,
            skipped: false,
            skip_reason: None,
            boot_error: None,
        }
    }

    pub fn skipped(set_idx: usize, reason: &str) -> Self {
        Self {
            set_idx,
            chip_results: vec![],
            skipped: true,
            skip_reason: Some(reason.to_string()),
            boot_error: None,
        }
    }

    pub fn boot_error(set_idx: usize, reason: &str) -> Self {
        Self {
            set_idx,
            chip_results: vec![],
            skipped: false,
            skip_reason: None,
            boot_error: Some(reason.to_string()),
        }
    }

    /// `true` iff the set ran and every chip/mode passed.
    /// Boot errors and non-skipped sets with failures return `false`.
    /// Skipped sets return `false` but are excluded from [`TestReport::all_passed`].
    pub fn passed(&self) -> bool {
        if self.skipped || self.boot_error.is_some() {
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

        for set in &self.set_results {
            if let Some(ref msg) = set.boot_error {
                println!("Set {} : BOOT ERROR — {}", set.set_idx, msg);
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
                    println!(
                        "  [{}] set={} chip={} ({}) file={} mode={}bit \
                         reads={} failures={}",
                        if mode.passed() { "PASS" } else { "FAIL" },
                        chip.set_idx,
                        chip.chip_idx,
                        chip.chip_type.name(),
                        chip.filename,
                        mode.mode,
                        mode.reads,
                        mode.failures,
                    );
                }
            }
        }

        println!("-----");
        println!(
            "Total: {} bytes read, {} failures — {}",
            grand_reads,
            grand_failures,
            if self.all_passed() { "PASS" } else { "FAIL" },
        );
    }
}