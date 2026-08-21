// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM documentation checker.
//!
//! A document that states a value someone else owns - a hold limit from the
//! metadata schema, the CLI's own version - states it inside a marker naming
//! that source:
//!
//! ```text
//! The device's own limit is <!--[const:GPIO_MAX_HOLD_MS:seconds]-->60 seconds<!--[/]-->.
//! ```
//!
//! This checks those spans and nothing else. **It never writes a document.**
//! Rewriting them would give a tool write access to thousands of lines of
//! hand-written prose, in exchange for saving a hand edit on the day a constant
//! changes - and these are values the firmware, its plugins and the host have
//! all agreed on, so that day is rare. The value being written twice is what
//! `CLAUDE.md` warns about only when nothing compares the copies. This compares
//! them, in the test gate and again before the PDFs are rendered.
//!
//! Run with:
//!
//! ```text
//! cargo run -p doc-gen -- --check docs
//! cargo run -p doc-gen -- --check docs/CLI-MANUAL.md
//! ```
//!
//! A path is taken relative to the repository root, so the same command works
//! from anywhere. A directory is walked; a document with no markers passes,
//! because marking one up is opt-in, one value at a time.

mod format;
mod marker;
mod source;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
One ROM documentation checker.

  doc-gen                        check every document under docs/
  doc-gen --check <path>         check one document, or a directory of them

Paths are relative to the repository root.  Nothing is ever written.
";

/// Checked when no path is named.
const DEFAULT_PATH: &str = "docs";

/// Levels up from `CARGO_MANIFEST_DIR` to the repository root.
/// `CARGO_MANIFEST_DIR` = `<repo>/rust/doc-gen`, so two pops reach `<repo>`.
const LEVELS_UP_TO_REPO_ROOT: usize = 2;

fn repo_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..LEVELS_UP_TO_REPO_ROOT {
        path.pop();
    }
    path
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    if flag("--help") || flag("-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    if flag("--check") && value("--check").is_none() {
        eprintln!("error: --check needs a path\n");
        print!("{USAGE}");
        return ExitCode::FAILURE;
    }

    let path = repo_root().join(value("--check").unwrap_or_else(|| DEFAULT_PATH.to_string()));

    let documents = match documents(&path) {
        Ok(documents) => documents,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut failures = 0;
    let mut checked = 0;
    let mut spans = 0;

    for document in documents {
        match check(&document) {
            Ok(found) => {
                if found > 0 {
                    checked += 1;
                    spans += found;
                }
            }
            Err(reports) => {
                checked += 1;
                failures += reports.len();
                let shown = document
                    .strip_prefix(repo_root())
                    .unwrap_or(&document)
                    .display()
                    .to_string();
                for report in reports {
                    println!("{shown}:{}: {}", report.line, report.detail);
                }
            }
        }
    }

    if failures > 0 {
        // Every failure is reported before this: a constant that moved and
        // reaches six documents is one edit round, not six.
        println!("\n{failures} problem(s) in {checked} document(s).");
        return ExitCode::FAILURE;
    }

    println!("Checked {spans} marked value(s) in {checked} document(s).");
    ExitCode::SUCCESS
}

/// One thing wrong with one document, at one line.
struct Report {
    line: usize,
    detail: String,
}

/// The markdown files under `path`, or `path` itself when it names a file.
fn documents(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        return Err(format!(
            "{} is neither a file nor a directory",
            path.display()
        ));
    }

    let mut found = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("could not read {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("could not read {}: {e}", dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Check one document, returning how many spans it checked.
fn check(path: &Path) -> Result<usize, Vec<Report>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            return Err(vec![Report {
                line: 0,
                detail: format!("could not read it: {e}"),
            }]);
        }
    };

    let scan = marker::scan(&text);
    if !scan.is_marked() {
        return Ok(0);
    }

    let mut reports: Vec<Report> = scan
        .problems
        .iter()
        .map(|problem| Report {
            line: problem.line,
            detail: problem.detail.clone(),
        })
        .collect();

    for span in &scan.spans {
        if let Err(detail) = check_span(span) {
            reports.push(Report {
                line: span.line,
                detail,
            });
        }
    }

    if reports.is_empty() {
        Ok(scan.spans.len())
    } else {
        reports.sort_by_key(|report| report.line);
        Err(reports)
    }
}

/// Check one span's text against what its source says today.
fn check_span(span: &marker::Span) -> Result<(), String> {
    let mut fields = span.spec.split(':');
    let source = fields.next().unwrap_or_default();
    let names = fields.next().unwrap_or_default();
    let format = fields.next();
    if fields.next().is_some() {
        return Err(format!("'{}' has more than source:name:format", span.spec));
    }
    if source.is_empty() || names.is_empty() {
        return Err(format!("'{}' is not source:name[:format]", span.spec));
    }

    // A span may name more than one constant where a document states a value
    // that several of them share - a table row covering two LED modes, say.
    // Every one of them has to agree with the text, so a value that moves apart
    // from its neighbour is caught rather than hidden by the one that did not.
    let mut expected: Option<String> = None;
    for name in names.split('+') {
        let value = source::resolve(source, name)?;
        let rendered = format::render(&value, format)?;
        match &expected {
            None => expected = Some(rendered),
            Some(first) if *first != rendered => {
                return Err(format!(
                    "'{}' names values that no longer agree: {first} and {rendered}",
                    span.spec
                ));
            }
            Some(_) => {}
        }
    }

    let expected = expected.unwrap_or_default();
    if span.text != expected {
        return Err(format!(
            "{} says '{}', {} says '{}'",
            span.spec, span.text, source, expected
        ));
    }

    Ok(())
}
