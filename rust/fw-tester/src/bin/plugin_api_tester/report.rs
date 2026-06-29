// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Test result accumulator and summary printer.
//!
//! Results are grouped by flash slot.  Open a slot with [`ApiReport::begin_slot`]
//! (a Single slot under test) or record one skipped with
//! [`ApiReport::skip_slot`] (a Multi/Banked slot), then feed per-test results
//! with [`ApiReport::add`] — each lands in the most recently opened slot.

struct SlotReport {
    #[allow(dead_code)]
    idx: usize,
    #[allow(dead_code)]
    sel: u8,
    skipped: bool,
    // (name, passed, message, skipped)
    results: Vec<(String, bool, Option<String>, bool)>,
}

impl SlotReport {
    fn all_passed(&self) -> bool {
        self.results.iter().all(|(_, p, _, s)| *s || *p)
    }
}

pub struct ApiReport {
    board_str: String,
    config_path: String,
    slots: Vec<SlotReport>,
}

impl ApiReport {
    pub fn new(board_str: &str, config_path: &str) -> Self {
        Self {
            board_str: board_str.to_string(),
            config_path: config_path.to_string(),
            slots: Vec::new(),
        }
    }

    /// Open a flash slot that is under test, printing its header.  Subsequent
    /// [`Self::add`] calls record against this slot until the next slot opens.
    pub fn begin_slot(&mut self, idx: usize, sel: u8, label: &str) {
        println!("=== Flash slot {} (sel={}): {} ===", idx, sel, label);
        self.slots.push(SlotReport {
            idx,
            sel,
            skipped: false,
            results: Vec::new(),
        });
    }

    /// Record a flash slot that was skipped (e.g. Multi/Banked), printing a
    /// skip header.  No per-test results are expected for it.
    pub fn skip_slot(&mut self, idx: usize, sel: u8, reason: &str) {
        println!(
            "=== Flash slot {} (sel={}): SKIPPED ({}) ===",
            idx, sel, reason
        );
        self.slots.push(SlotReport {
            idx,
            sel,
            skipped: true,
            results: Vec::new(),
        });
    }

    pub fn add(&mut self, name: &str, result: Result<(), String>) {
        let passed = result.is_ok();
        let msg = result.err();
        if passed {
            println!("  [PASS] {}", name);
        } else {
            println!("  [FAIL] {} — {}", name, msg.as_deref().unwrap_or(""));
        }
        if let Some(slot) = self.slots.last_mut() {
            slot.results.push((name.to_string(), passed, msg, false));
        }
    }

    /// Record a test that was skipped because its precondition does not hold
    /// for this slot (e.g. a second RAM slot is required but the region size
    /// leaves only one).  Skipped tests count as neither pass nor fail and are
    /// excluded from the test tally.
    pub fn skip(&mut self, name: &str, reason: &str) {
        println!("  [SKIP] {} — {}", name, reason);
        if let Some(slot) = self.slots.last_mut() {
            slot.results
                .push((name.to_string(), true, Some(reason.to_string()), true));
        }
    }

    /// True if no test failed.  Skipped slots and slots with no results do not
    /// count as failures.
    pub fn all_passed(&self) -> bool {
        self.slots.iter().all(|s| s.all_passed())
    }

    pub fn print(&self) {
        let skipped = self.slots.iter().filter(|s| s.skipped).count();
        let ran: Vec<&SlotReport> = self.slots.iter().filter(|s| !s.skipped).collect();
        let ran_count = ran.len();
        let slots_passed = ran.iter().filter(|s| s.all_passed()).count();
        let total_tests: usize = ran
            .iter()
            .map(|s| s.results.iter().filter(|(_, _, _, skip)| !*skip).count())
            .sum();
        let passed_tests: usize = ran
            .iter()
            .map(|s| {
                s.results
                    .iter()
                    .filter(|(_, p, _, skip)| !*skip && *p)
                    .count()
            })
            .sum();

        println!("-----");
        println!("One ROM Plugin API Tester");
        println!("Board  : {}", self.board_str);
        println!("Config : {}", self.config_path);
        if skipped > 0 {
            println!(
                "Slots  : {}/{} passed, {} skipped",
                slots_passed, ran_count, skipped
            );
        } else {
            println!("Slots  : {}/{} passed", slots_passed, ran_count);
        }
        println!("Tests  : {}/{} passed", passed_tests, total_tests);
        println!("{}", if self.all_passed() { "PASS" } else { "FAIL" });
    }
}
