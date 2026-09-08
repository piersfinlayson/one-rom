// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The checker over real files: what it walks, what it passes, what it reports.

use std::path::Path;
use std::process::Command;

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

fn check(dir: &Path) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_doc-gen"))
        .args(["--check", dir.to_str().unwrap()])
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

#[test]
fn a_document_whose_values_match_passes() {
    let dir = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        "good.md",
        "The pulse is <!--[const:GPIO_RESET_DEFAULT_HOLD_MS:ms]-->100ms<!--[/]-->.\n\
         The limit is <!--[const:GPIO_MAX_HOLD_MS:seconds]-->60 seconds<!--[/]-->.\n",
    );

    let (ok, out) = check(dir.path());
    assert!(ok, "{out}");
    assert!(out.contains("2 marked value(s)"), "{out}");
}

#[test]
fn a_stale_value_is_reported_against_its_file_and_line() {
    let dir = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        "stale.md",
        "intro\nThe limit is <!--[const:GPIO_MAX_HOLD_MS:seconds]-->30 seconds<!--[/]-->.\n",
    );

    let (ok, out) = check(dir.path());
    assert!(!ok, "{out}");
    assert!(out.contains("stale.md:2:"), "{out}");
    assert!(out.contains("30 seconds"), "{out}");
    assert!(out.contains("60 seconds"), "{out}");
}

#[test]
fn every_problem_in_a_run_is_reported_not_just_the_first() {
    // A constant that moves reaches several documents at once, and fixing them
    // should be one edit round.
    let dir = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        "one.md",
        "<!--[const:GPIO_MAX_HOLD_MS:seconds]-->30 seconds<!--[/]-->\n",
    );
    write(
        dir.path(),
        "two.md",
        "<!--[const:LED_MAX_HOLD_MS]-->1<!--[/]-->\n\
         <!--[const:NOT_A_CONSTANT]-->2<!--[/]-->\n",
    );

    let (ok, out) = check(dir.path());
    assert!(!ok, "{out}");
    assert!(out.contains("one.md:1:"), "{out}");
    assert!(out.contains("two.md:1:"), "{out}");
    assert!(out.contains("two.md:2:"), "{out}");
    assert!(out.contains("3 problem(s)"), "{out}");
}

#[test]
fn a_document_with_no_markers_is_not_a_failure() {
    let dir = tempfile::TempDir::new().unwrap();
    write(
        dir.path(),
        "prose.md",
        "# Title\n\nNothing owned elsewhere.\n",
    );

    let (ok, out) = check(dir.path());
    assert!(ok, "{out}");
    assert!(out.contains("0 document(s)"), "{out}");
}

#[test]
fn a_span_naming_several_constants_checks_every_one() {
    let dir = tempfile::TempDir::new().unwrap();
    // A table row covering two LED modes states one value for both.
    write(
        dir.path(),
        "both.md",
        "| cycle, breathe | <!--[const:LED_CYCLE_MIN_PERIOD_MS+LED_BREATHE_MIN_PERIOD_MS:ms]-->1000ms<!--[/]--> |\n",
    );
    let (ok, out) = check(dir.path());
    assert!(ok, "{out}");

    // Naming a second constant that does not agree is the failure the '+' form
    // exists to catch - one of the pair moving apart from the other.
    write(
        dir.path(),
        "both.md",
        "| cycle, flame | <!--[const:LED_CYCLE_MIN_PERIOD_MS+LED_FLAME_MIN_PERIOD_MS:ms]-->1000ms<!--[/]--> |\n",
    );
    let (ok, out) = check(dir.path());
    assert!(!ok, "{out}");
    assert!(out.contains("no longer agree"), "{out}");
}

#[test]
fn the_repositorys_own_documents_are_checked_by_default() {
    // No --check, so it walks docs/ at the repository root: the invocation the
    // gate uses, and the one that proves the default path is right.
    let out = Command::new(env!("CARGO_BIN_EXE_doc-gen"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{text}");
    assert!(text.contains("marked value(s)"), "{text}");
}
