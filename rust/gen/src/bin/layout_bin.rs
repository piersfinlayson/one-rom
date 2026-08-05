// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM board layout checker.
//!
//! Reports, for every chip type a board can emulate, how much flash the
//! image costs against the floor — the smallest table the chip's address
//! lines could ever be served from — and which of the board's pins are
//! responsible for the difference.
//!
//! The flash a chip's image occupies is `2^n` where `n` is the width of the
//! GPIO window spanning its address lines, so a board that maps an unrelated
//! signal into the middle of a chip's address range doubles that chip's image
//! for every such GPIO. Those GPIOs are what this reports: they are the
//! actionable output, because moving one is a board-layout decision.
//!
//! Banked sets are checked too. A banked set draws X1/X2 into the same
//! window, so an ordering that is optimal for single chips can still cost a
//! factor of two on every banked set of a small ROM — a case not visible in
//! `docs/COMPATIBILITY.md`, which lists single chips only.
//!
//! Run with:
//!
//! ```text
//! cargo run --bin layout -- [--board <name>] [--summary]
//! cargo run --bin layout -- --check
//! cargo run --bin layout -- --write-baseline
//! ```
//!
//! `ci/layout-baseline.txt` records how much flash each chip type costs on
//! each board. It is a checked-in generated file like `COMPATIBILITY.md`:
//! `ci/rust-tests.sh` rewrites it with `--write-baseline` and fails if that
//! produces a diff, so it cannot drift and there is no step to remember.
//!
//! `--check` is the human half of that. A diff in the baseline says the
//! numbers moved but not which way; `--check` reads it and says whether a
//! chip got more expensive (and by what factor) or cheaper.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use onerom_config::chip::{CHIP_TYPES, ChipType};
use onerom_config::hw::{Board, Model};
use onerom_gen::compat::{CompatResult, check_chip_set_on_board, default_cs_config, format_size};
use onerom_gen::{ChipSetType, socket_pin_offset};

// ── Repository paths ──────────────────────────────────────────────────────────

/// Levels up from `CARGO_MANIFEST_DIR` to the repository root.
/// `CARGO_MANIFEST_DIR` = `<repo>/rust/gen`, so two pops reach `<repo>`.
const LEVELS_UP_TO_REPO_ROOT: usize = 2;

/// Baseline path relative to the repository root.
const BASELINE_FILE: &str = "ci/layout-baseline.txt";

fn repo_root() -> PathBuf {
    let mut path = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    for _ in 0..LEVELS_UP_TO_REPO_ROOT {
        path.pop();
    }
    path
}

// ── Boards and set shapes ─────────────────────────────────────────────────────

/// Boards omitted from all output, matching the compatibility document.
const EXCLUDED_BOARDS: &[Board] = &[
    Board::Fire24Eadb01, // custom ADB01 variant, not for general release
];

/// The slot shapes each chip type is checked in.
///
/// Single, two banked depths, and the two multi sizes. ×4 banked and ×3 multi
/// each exercise X2 as well as X1, which is where an ordering that only ever
/// considered X1 shows up. There is no multi ×4: a multi set is the chip in
/// the socket plus one secondary per X pin, and there are only two X pins.
const SET_SHAPES: &[(&str, ChipSetType, usize)] = &[
    ("single", ChipSetType::Single, 1),
    ("banked2", ChipSetType::Banked, 2),
    ("banked4", ChipSetType::Banked, 4),
    ("multi2", ChipSetType::Multi, 2),
    ("multi3", ChipSetType::Multi, 3),
];

/// Column headings for `SET_SHAPES`, in the same order.
const SHAPE_HEADINGS: &[&str] = &["single", "bank x2", "bank x4", "mult x2", "mult x3"];

fn boards() -> Vec<Board> {
    let mut boards: Vec<Board> = Model::Fire
        .boards()
        .iter()
        .filter(|b| b.mcu_pio() && !EXCLUDED_BOARDS.contains(b))
        .copied()
        .collect();
    boards.sort_by_key(|b| b.name());
    boards
}

/// Every chip type worth a row on this board, canonical names only.
///
/// Includes chips the board *cannot* serve: "this type is not possible, and
/// here is why" is the half of the report a board design turns on, and
/// omitting it silently is how a layout regression hides. Only chips whose
/// pin count is not a supported pairing with the socket at all are left out —
/// a 40-pin ROM in a 24-pin socket is not a layout problem, it is a different
/// board.
///
/// Deliberately not `supported_chips`, which lists a chip once per accepted
/// spelling so a user can find the part number stamped on their chip. Three
/// rows for 2316/9316/9316A is right for the compatibility document and wrong
/// here: they are one layout problem, not three.
fn chip_types_for(board: Board) -> Vec<ChipType> {
    let mut types: Vec<ChipType> = CHIP_TYPES
        .iter()
        .copied()
        .filter(|c| !c.is_plugin() && socket_pin_offset(c.chip_pins(), board.chip_pins()).is_some())
        .collect();
    types.sort_by_key(|c| (c.size_bytes(), c.name()));
    types
}

// ── Measurement ───────────────────────────────────────────────────────────────

/// One chip type on one board, across every slot shape it can be served in.
struct ChipRow {
    chip_type: ChipType,
    /// Indexed in `SET_SHAPES` order, in the chip's default CS configuration.
    /// `None` where the shape is not servable — a banked set the board cannot
    /// do, or one whose table would exceed `MAX_IMAGE_SIZE`.
    shapes: Vec<Option<CompatResult>>,

    /// Why the chip cannot be served alone on this board, if it cannot.
    /// `None` when the single slot derives.
    unsupported: Option<String>,
}

impl ChipRow {
    fn single(&self) -> Option<&CompatResult> {
        self.shapes[0].as_ref()
    }

    /// The shape this chip fares worst in, and its result. A chip is only
    /// "done" when no shape wastes a bit, and the shape that wastes most is
    /// the one whose blocking pins are worth reporting — frequently a banked
    /// set rather than the single.
    fn worst_shape(&self) -> Option<(&'static str, &CompatResult)> {
        self.shapes
            .iter()
            .enumerate()
            .filter_map(|(idx, r)| r.as_ref().map(|r| (SET_SHAPES[idx].0, r)))
            .max_by_key(|(_, r)| r.excess_addr_bits())
    }

    fn worst_excess(&self) -> u32 {
        self.worst_shape().map_or(0, |(_, r)| r.excess_addr_bits())
    }
}

fn measure(board: Board) -> Vec<ChipRow> {
    chip_types_for(board)
        .into_iter()
        .map(|chip_type| ChipRow {
            chip_type,
            shapes: SET_SHAPES
                .iter()
                .map(|(_, set_type, n)| {
                    check_chip_set_on_board(
                        board,
                        chip_type,
                        *set_type,
                        *n,
                        default_cs_config(chip_type),
                    )
                    .ok()
                })
                .collect(),
            unsupported: check_chip_set_on_board(
                board,
                chip_type,
                ChipSetType::Single,
                1,
                default_cs_config(chip_type),
            )
            .err()
            .map(|e| reason(&e)),
        })
        .collect()
}

/// Condense a build error into something that fits a table cell.
///
/// The full message is written for someone who hit it while building and
/// needs to know what to change; here the board and chip are already the row,
/// so only the cause is new information.
fn reason(error: &onerom_gen::Error) -> String {
    let text = error.to_string();
    match text.split_once(": ") {
        Some((_, rest)) => rest.to_string(),
        None => text,
    }
}

/// Name the pins sitting on a result's wasted table-index bits.
///
/// A hole GPIO usually maps to a socket pin — that pin's signal is what sits
/// in the middle of this chip's address range. Where it maps to nothing, the
/// window was widened past the board's mapped GPIOs (`MIN_ADDR_PINS`, or a
/// gap in the board's own numbering), which is not attributable to a pin.
fn blocking_pins(board: Board, result: &CompatResult) -> String {
    let mut parts: Vec<String> = Vec::new();
    for gpio in result.hole_gpio_list() {
        if let Some(pin) = board.socket_pin_for_gpio(gpio) {
            parts.push(format!("gpio{gpio}=pin{pin}"));
        } else if let Some(x) = board.x_pin_for_gpio(gpio) {
            parts.push(format!("gpio{gpio}=X{x}"));
        } else {
            parts.push(format!("gpio{gpio}=unmapped"));
        }
    }
    parts.join(" ")
}

// ── Reports ───────────────────────────────────────────────────────────────────

/// How many times larger the image is than the smallest table that could
/// serve this slot, as `1x`, `2x`, `32x`.
///
/// Derived from the wasted bits rather than from the chip's own size, because
/// the two differ for a chip whose capacity is not a power of two: a 23QL384
/// holds 48KB but its 16 address lines need a 64KB table however the board is
/// laid out, so 64KB is the floor and dividing by 48KB would claim a waste
/// the board cannot remove.
fn ratio(result: &CompatResult) -> String {
    format!("{}x", 1u64 << result.excess_addr_bits())
}

fn cell(row: &ChipRow, idx: usize) -> String {
    match &row.shapes[idx] {
        None => "-".to_string(),
        Some(r) => format!("{} {}", format_size(r.slot_size_bytes), ratio(r)),
    }
}

/// A fit class short enough to tabulate.
///
/// [`CompatResult::fit_description`] is prose for the compatibility document
/// and the CLI; at 28 characters `larger socket (no fly-leads)` would set the
/// column width for every row here.
fn fit_short(result: &CompatResult) -> &'static str {
    if result.is_native() {
        "native"
    } else if result.is_overhang() {
        "overhang"
    } else {
        match result.num_fly_lead_pins {
            0 => "larger",
            1 => "fly-X1",
            _ => "fly-X1X2",
        }
    }
}

fn report_board(board: Board, out: &mut String) {
    let rows = measure(board);
    let single_at_floor = rows
        .iter()
        .filter(|r| r.single().is_some_and(|s| s.excess_addr_bits() == 0))
        .count();
    let all_at_floor = rows.iter().filter(|r| r.worst_excess() == 0).count();

    let _ = writeln!(out, "\n{} — {}\n", board.name(), board.description());
    let _ = writeln!(
        out,
        "{:<11}{:>7}  {:<9}{}  {:>4} {:<8} blocked by",
        "chip",
        "ROM",
        "fit",
        SHAPE_HEADINGS
            .iter()
            .map(|h| format!("{h:>11}"))
            .collect::<Vec<_>>()
            .join(""),
        "bits",
        "worst in"
    );

    for row in &rows {
        // The bits/worst-in/blocked-by columns all describe the same shape -
        // the one that wastes most - so the pins named are the pins costing
        // the figure alongside them. A chip the board cannot serve at all has
        // no shape to describe, and gives its reason in their place.
        let worst = row.worst_shape();
        let waste = worst.map_or(0, |(_, r)| r.excess_addr_bits());
        let trailing = match (&row.unsupported, worst) {
            (Some(why), _) => why.clone(),
            (None, Some((_, r))) => blocking_pins(board, r),
            (None, None) => String::new(),
        };
        let _ = writeln!(
            out,
            "{:<11}{:>7}  {:<9}{}  {:>4} {:<8} {}",
            row.chip_type.name(),
            format_size(chip_size(row.chip_type)),
            row.single().map_or("-", fit_short),
            (0..SET_SHAPES.len())
                .map(|idx| format!("{:>11}", cell(row, idx)))
                .collect::<Vec<_>>()
                .join(""),
            if waste == 0 {
                "-".to_string()
            } else {
                waste.to_string()
            },
            match worst {
                Some((shape, _)) if waste > 0 => shape,
                _ => "-",
            },
            trailing,
        );
    }

    let total = rows.len();
    let _ = writeln!(
        out,
        "\n{single_at_floor} of {total} chip types at floor as singles, \
         {all_at_floor} of {total} in every slot shape"
    );
}

fn report_summary(out: &mut String) {
    let _ = writeln!(
        out,
        "{:<16}{:>7}{:>10}{:>8}  worst chip",
        "board", "chips", "at floor", "worst"
    );
    for board in boards() {
        let rows = measure(board);
        let at_floor = rows.iter().filter(|r| r.worst_excess() == 0).count();
        let worst = rows.iter().max_by_key(|r| r.worst_excess());
        let (worst_bits, worst_name) = match worst {
            Some(r) if r.worst_excess() > 0 => {
                (format!("{}x", 1u64 << r.worst_excess()), r.chip_type.name())
            }
            _ => ("1x".to_string(), "-"),
        };
        let _ = writeln!(
            out,
            "{:<16}{:>7}{:>10}{:>8}  {}",
            board.name(),
            rows.len(),
            at_floor,
            worst_bits,
            worst_name
        );
    }
}

/// `chip_type.size_bytes()` as the `u32` the formatter takes.
fn chip_size(chip_type: ChipType) -> u32 {
    chip_type.size_bytes() as u32
}

// ── Baseline ──────────────────────────────────────────────────────────────────

/// The baseline: excess bits per (board, chip, slot shape).
///
/// A sorted plain-text record per line, so a regression shows up in review as
/// one readable diff line rather than a reshuffled JSON blob.
fn baseline_text() -> String {
    let mut out = String::new();
    out.push_str("# One ROM board layout baseline — excess address bits.\n");
    out.push_str("# Regenerate deliberately with: cargo run -p onerom-gen --bin layout -- --write-baseline\n");
    out.push_str("# <board> <chip> <shape> <excess bits>\n");
    for board in boards() {
        for row in measure(board) {
            for (idx, (shape, _, _)) in SET_SHAPES.iter().enumerate() {
                if let Some(result) = &row.shapes[idx] {
                    let _ = writeln!(
                        out,
                        "{} {} {} {}",
                        board.name(),
                        row.chip_type.name(),
                        shape,
                        result.excess_addr_bits()
                    );
                }
            }
        }
    }
    out
}

fn parse_baseline(text: &str) -> BTreeMap<(String, String, String), u32> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let key = (
                f.next()?.to_string(),
                f.next()?.to_string(),
                f.next()?.to_string(),
            );
            Some((key, f.next()?.parse().ok()?))
        })
        .collect()
}

/// Compare against the committed baseline. Only growth fails: an improvement
/// is reported so the baseline gets regenerated, but does not break the build.
fn check(out: &mut String) -> bool {
    let path = repo_root().join(BASELINE_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        let _ = writeln!(
            out,
            "no baseline at {}; create it with --write-baseline",
            path.display()
        );
        return false;
    };

    let old = parse_baseline(&text);
    let new = parse_baseline(&baseline_text());
    let mut regressions = 0;
    let mut improvements = 0;
    let mut added = 0;

    for (key, &bits) in &new {
        match old.get(key) {
            None => added += 1,
            Some(&was) if bits > was => {
                regressions += 1;
                let _ = writeln!(
                    out,
                    "REGRESSED {} {} {}: {was} -> {bits} excess bits ({}x more flash)",
                    key.0,
                    key.1,
                    key.2,
                    1u64 << (bits - was)
                );
            }
            Some(&was) if bits < was => improvements += 1,
            Some(_) => {}
        }
    }

    for key in old.keys() {
        if !new.contains_key(key) {
            regressions += 1;
            let _ = writeln!(
                out,
                "LOST {} {} {}: no longer servable",
                key.0, key.1, key.2
            );
        }
    }

    if improvements > 0 || added > 0 {
        let _ = writeln!(
            out,
            "{improvements} improved, {added} new entries — rerun with --write-baseline to record them"
        );
    }
    if regressions == 0 {
        let _ = writeln!(out, "layout check passed ({} entries)", new.len());
    }
    regressions == 0
}

// ── Entry point ───────────────────────────────────────────────────────────────

const USAGE: &str = "\
One ROM board layout checker.

  layout                     report every board
  layout --board <name>      report one board
  layout --summary           one line per board
  layout --check             fail if any board regressed against the baseline
  layout --write-baseline    record the current state as the baseline
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = String::new();

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

    if flag("--write-baseline") {
        let path = repo_root().join(BASELINE_FILE);
        if let Err(e) = std::fs::write(&path, baseline_text()) {
            eprintln!("failed to write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
        println!("Written to {}", path.display());
        return ExitCode::SUCCESS;
    }

    if flag("--check") {
        let ok = check(&mut out);
        print!("{out}");
        return if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }

    if flag("--summary") {
        report_summary(&mut out);
        print!("{out}");
        return ExitCode::SUCCESS;
    }

    match value("--board") {
        Some(name) => match boards().into_iter().find(|b| b.name() == name) {
            Some(board) => report_board(board, &mut out),
            None => {
                eprintln!("unknown board: {name}");
                return ExitCode::FAILURE;
            }
        },
        None => {
            for board in boards() {
                report_board(board, &mut out);
            }
        }
    }

    print!("{out}");
    ExitCode::SUCCESS
}
