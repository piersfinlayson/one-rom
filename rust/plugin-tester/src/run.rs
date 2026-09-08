// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Command line and reporting, shared by the plugin testers.
//!
//! Each tester owns its own scenario tables, because a scenario's signature is
//! made of that tester's own types — the RBCP tester hands its scenarios a ROM
//! bus, and a tester for another plugin would hand them something else.  What
//! is common is everything around them: the two filters, the verdict a scenario
//! can reach, and the shape of what is printed.
//!
//! Keeping the printing here is the point.  Two testers whose output differed
//! in spacing or wording would be two things for a reader of a CI log to learn,
//! and the difference would carry no information.

use std::process;

/// How a scenario ended.
///
/// A scenario that cannot run against the device in front of it is neither a
/// pass nor a failure.  Which scenarios those are is a property of the
/// configuration under test, so the judgement belongs to the scenario, at the
/// point it discovers it, rather than to a table written in advance.
pub enum Outcome {
    Pass,
    /// Not applicable to this device, for the stated reason.
    Skip(String),
}

/// What the caller asked to run.  Both default to everything.
pub struct Filters {
    pub suite: Option<String>,
    pub scenario: Option<String>,
}

impl Filters {
    /// Parse `--suite <name>` and `--scenario <substr>`.
    ///
    /// Exits with status 2 on an argument neither names, rather than running a
    /// different set of scenarios than the caller asked for.
    pub fn from_args() -> Self {
        let mut filters = Filters {
            suite: None,
            scenario: None,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--suite" => filters.suite = args.next(),
                "--scenario" => filters.scenario = args.next(),
                other => {
                    eprintln!("unknown argument '{other}' (expected --suite or --scenario)");
                    process::exit(2);
                }
            }
        }
        filters
    }

    /// Whether a suite of this name runs.  An unset filter matches everything.
    pub fn suite_matches(&self, name: &str) -> bool {
        self.suite.as_ref().is_none_or(|f| name == f)
    }

    /// Whether a scenario of this name runs.  Matched as a substring, so a
    /// group of related scenarios can be named by their common prefix.
    pub fn scenario_matches(&self, name: &str) -> bool {
        self.scenario
            .as_ref()
            .is_none_or(|f| name.contains(f.as_str()))
    }
}

/// Announce a suite that is about to run.
pub fn suite_header(name: &str, blurb: &str) {
    println!("\n== {name} — {blurb}");
}

/// Running counts, and the verdict lines as they are printed.
#[derive(Default)]
pub struct Tally {
    passed: u32,
    failed: u32,
    skipped: u32,
}

impl Tally {
    pub fn new() -> Self {
        Self::default()
    }

    /// Print one scenario's verdict and count it.
    ///
    /// `label` is the scenario name as the caller wants it read, which is not
    /// always the name it was filtered on — the RBCP tester appends the bit
    /// mode, since each scenario runs once per mode.  `spec_ref` is printed
    /// only on a failure, where it says what the scenario was asserting.
    pub fn record(&mut self, label: &str, spec_ref: &str, result: Result<Outcome, String>) {
        match result {
            Ok(Outcome::Pass) => {
                println!("PASS  {label}");
                self.passed += 1;
            }
            Ok(Outcome::Skip(why)) => {
                println!("SKIP  {label}\n        {why}");
                self.skipped += 1;
            }
            Err(e) => {
                println!("FAIL  {label}\n        [{spec_ref}]\n        {e}");
                self.failed += 1;
            }
        }
    }

    pub fn failed(&self) -> u32 {
        self.failed
    }

    /// Print the summary and exit: 0 if nothing failed, 1 otherwise.
    ///
    /// `tester` names the tester, and `context` says what it ran against.
    /// Skips are always reported, count included when zero: a suite that
    /// quietly dropped scenarios would otherwise read as full coverage.
    pub fn finish(&self, tester: &str, context: &str) -> ! {
        println!(
            "\n{tester}: {} passed, {} failed, {} skipped  [{context}]",
            self.passed, self.failed, self.skipped
        );
        process::exit(if self.failed == 0 { 0 } else { 1 });
    }
}
