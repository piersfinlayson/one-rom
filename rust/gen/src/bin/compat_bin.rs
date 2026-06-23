// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM chip compatibility document generator.
//!
//! Writes docs/COMPATIBILITY.md at the repository root.
//!
//! Run with:
//!   cargo run --bin compat

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use onerom_config::chip::{CHIP_TYPE_NAMES, ChipType};
use onerom_config::hw::{Board, Model};
use onerom_gen::compat::{CompatResult, check_chip_on_board, is_v2_chip};

// ── Repository paths ──────────────────────────────────────────────────────────

/// Levels up from `CARGO_MANIFEST_DIR` to the repository root.
/// `CARGO_MANIFEST_DIR` = `<repo>/rust/gen`, so two pops reach `<repo>`.
const LEVELS_UP_TO_REPO_ROOT: usize = 2;

/// Output file path relative to the repository root.
const OUTPUT_FILE: &str = "docs/COMPATIBILITY.md";

/// Makefile path relative to the repository root (contains version numbers).
const VERSION_FILE: &str = "Makefile";

fn repo_root() -> PathBuf {
    let mut path = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    for _ in 0..LEVELS_UP_TO_REPO_ROOT {
        path.pop();
    }
    path
}

fn output_path() -> PathBuf {
    repo_root().join(OUTPUT_FILE)
}

// ── Version ───────────────────────────────────────────────────────────────────

fn read_version() -> String {
    let content = match std::fs::read_to_string(repo_root().join(VERSION_FILE)) {
        Ok(c) => c,
        Err(_) => return "unknown".to_string(),
    };

    let mut major = None;
    let mut minor = None;
    let mut patch = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("VERSION_MAJOR :=") {
            major = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("VERSION_MINOR :=") {
            minor = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("VERSION_PATCH :=") {
            patch = Some(v.trim().to_string());
        }
    }

    match (major, minor, patch) {
        (Some(ma), Some(mi), Some(pa)) => format!("{}.{}.{}", ma, mi, pa),
        _ => "unknown".to_string(),
    }
}

// ── Excluded boards ───────────────────────────────────────────────────────────

/// Boards omitted from all output sections.
const EXCLUDED_BOARDS: &[Board] = &[
    Board::Fire24Eadb01, // custom ADB01 variant, not for general release
];

// ── Board abbreviations ───────────────────────────────────────────────────────

/// Generate a short display name from the board's own name and chip_pins().
///
/// Algorithm: strip the "fire-" prefix, take the last dash-separated
/// component, uppercase its first character, prepend the pin count.
///
///   "fire-24-a"     →  "24A"
///   "fire-24-usb-b" →  "24B"   (last component of "24-usb-b")
///   "fire-28-c"     →  "28C"
fn board_short(board: Board) -> String {
    let suffix = board.name().strip_prefix("fire-").unwrap_or(board.name());
    let rev = suffix
        .rsplit('-')
        .next()
        .and_then(|p| p.chars().next())
        .unwrap_or('?')
        .to_ascii_uppercase();
    format!("{}{}", board.chip_pins(), rev)
}

// ── Formatting helpers ────────────────────────────────────────────────────────

fn format_size(bytes: u32) -> String {
    if bytes >= 1024 * 1024 {
        format!("{}MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{}B", bytes)
    }
}

/// Format a compat result as a Markdown table cell.
///
/// `-` = not supported; `64KB` = native fit; `64KB*` = One ROM overhangs
/// the socket; `64KB†` = fly-leads required to X1/X2 header pins.
fn format_result(result: &Option<CompatResult>) -> String {
    match result {
        None => "-".to_string(),
        Some(r) => {
            let size = format_size(r.slot_size_bytes);
            if r.is_native() {
                size
            } else if r.requires_fly_leads() {
                format!("{}†", size)
            } else {
                format!("{}*", size)
            }
        }
    }
}

/// Human-readable socket fit description for per-board tables.
fn format_socket(result: &CompatResult) -> String {
    if result.is_native() {
        "native".to_string()
    } else if result.requires_fly_leads() {
        match result.num_fly_lead_pins {
            0 => "no fly-leads required".to_string(),
            1 => "fly-lead to X1".to_string(),
            2 => "fly-lead to X1 and X2".to_string(),
            n => format!("fly-lead ({} pins)", n),
        }
    } else {
        "overhang".to_string()
    }
}

// ── Sort key ──────────────────────────────────────────────────────────────────

/// Sort key for entries: native first, then overhang (ascending by chip size
/// difference), then fly-lead (ascending by chip size difference).
fn pin_offset_order(pin_offset: i16) -> i32 {
    if pin_offset == 0 {
        0
    } else if pin_offset > 0 {
        1
    } else {
        2
    }
}

// ── Data structures ───────────────────────────────────────────────────────────

/// One chip row in the compatibility matrix.
struct MatrixEntry {
    pin_offset: i16,
    alias: &'static str,
    rom_size: u32,
    results: Vec<Option<CompatResult>>,
}

/// A group of same-offset chips in a matrix section.
struct MatrixGroup {
    pin_offset: i16,
    entries: Vec<MatrixEntry>,
}

/// One supported chip in a per-board table.
struct BoardEntry {
    alias: &'static str,
    rom_size: u32,
    chip_pins: u8,
    result: CompatResult,
}

/// A group of same-offset chips in a per-board table.
struct BoardGroup {
    pin_offset: i16,
    chip_pins: u8,
    entries: Vec<BoardEntry>,
}

// ── Matrix section ────────────────────────────────────────────────────────────

fn write_matrix_section(w: &mut impl Write, title: &str, boards: &[Board]) -> io::Result<()> {
    if boards.is_empty() {
        return Ok(());
    }

    let board_pins = boards[0].chip_pins();

    writeln!(w, "## {}", title)?;
    writeln!(w)?;

    let mut entries: Vec<MatrixEntry> = CHIP_TYPE_NAMES
        .iter()
        .filter_map(|alias| {
            let chip_type = ChipType::try_from_str(alias)?;
            if !is_v2_chip(chip_type) {
                return None;
            }
            let results: Vec<Option<CompatResult>> = boards
                .iter()
                .map(|b| check_chip_on_board(*b, chip_type))
                .collect();
            if results.iter().all(|r| r.is_none()) {
                return None;
            }
            let pin_offset = results
                .iter()
                .find_map(|r| r.as_ref())
                .map(|r| r.pin_offset)
                .unwrap_or(0);
            Some(MatrixEntry {
                pin_offset,
                alias,
                rom_size: chip_type.size_bytes() as u32,
                results,
            })
        })
        .collect();

    entries.sort_by_key(|e| {
        (
            pin_offset_order(e.pin_offset),
            e.pin_offset.abs(),
            e.rom_size,
            e.alias,
        )
    });

    let mut groups: Vec<MatrixGroup> = Vec::new();
    for entry in entries {
        match groups.last_mut() {
            Some(g) if g.pin_offset == entry.pin_offset => g.entries.push(entry),
            _ => groups.push(MatrixGroup {
                pin_offset: entry.pin_offset,
                entries: vec![entry],
            }),
        }
    }

    let show_headers = groups.len() > 1;

    for group in &groups {
        if show_headers {
            let chip_pins = board_pins as i16 - 2 * group.pin_offset;
            if group.pin_offset == 0 {
                writeln!(w, "*{}-pin chips (native)*", board_pins)?;
            } else if group.pin_offset > 0 {
                writeln!(w, "*{}-pin chips (with overhang)*", chip_pins)?;
            } else {
                writeln!(w, "*{}-pin chips (with fly-leads)*", chip_pins)?;
            }
            writeln!(w)?;
        }

        write!(w, "| Chip | ROM size |")?;
        for board in boards {
            write!(w, " {} |", board_short(*board))?;
        }
        writeln!(w)?;
        write!(w, "|:---|---:|")?;
        for _ in boards {
            write!(w, "---:|")?;
        }
        writeln!(w)?;

        for entry in &group.entries {
            write!(w, "| {} | {} |", entry.alias, format_size(entry.rom_size))?;
            for result in &entry.results {
                write!(w, " {} |", format_result(result))?;
            }
            writeln!(w)?;
        }
        writeln!(w)?;
    }

    Ok(())
}

// ── Per-board table ───────────────────────────────────────────────────────────

fn write_board_table(w: &mut impl Write, board: Board) -> io::Result<()> {
    writeln!(w, "## {} — {}", board.description(), board.name())?;
    writeln!(w)?;

    let mut entries: Vec<BoardEntry> = CHIP_TYPE_NAMES
        .iter()
        .filter_map(|alias| {
            let chip_type = ChipType::try_from_str(alias)?;
            let result = check_chip_on_board(board, chip_type)?;
            Some(BoardEntry {
                alias,
                rom_size: chip_type.size_bytes() as u32,
                chip_pins: chip_type.chip_pins(),
                result,
            })
        })
        .collect();

    if entries.is_empty() {
        writeln!(w, "*(no supported chips)*")?;
        writeln!(w)?;
        return Ok(());
    }

    entries.sort_by_key(|e| {
        (
            pin_offset_order(e.result.pin_offset),
            e.result.pin_offset.abs(),
            e.rom_size,
            e.alias,
        )
    });

    let mut groups: Vec<BoardGroup> = Vec::new();
    for entry in entries {
        match groups.last_mut() {
            Some(g) if g.pin_offset == entry.result.pin_offset => g.entries.push(entry),
            _ => groups.push(BoardGroup {
                pin_offset: entry.result.pin_offset,
                chip_pins: entry.chip_pins,
                entries: vec![entry],
            }),
        }
    }

    let show_headers = groups.len() > 1;

    for group in &groups {
        if show_headers {
            if group.pin_offset == 0 {
                writeln!(w, "*{}-pin chips (native)*", board.chip_pins())?;
            } else if group.pin_offset > 0 {
                writeln!(w, "*{}-pin chips (with overhang)*", group.chip_pins)?;
            } else {
                writeln!(w, "*{}-pin chips (with fly-leads)*", group.chip_pins)?;
            }
            writeln!(w)?;
        }

        writeln!(w, "| Chip | ROM size | Image size | Fit |")?;
        writeln!(w, "|:---|---:|---:|:---|")?;

        for entry in &group.entries {
            writeln!(
                w,
                "| {} | {} | {} | {} |",
                entry.alias,
                format_size(entry.rom_size),
                format_size(entry.result.slot_size_bytes),
                format_socket(&entry.result),
            )?;
        }
        writeln!(w)?;
    }

    Ok(())
}

// ── Document generation ───────────────────────────────────────────────────────

fn generate_document(w: &mut impl Write) -> io::Result<()> {
    let version = read_version();

    let mut fire_boards: Vec<Board> = Model::Fire
        .boards()
        .iter()
        .filter(|b| b.mcu_pio() && !EXCLUDED_BOARDS.contains(b))
        .copied()
        .collect();
    fire_boards.sort_by_key(|b| board_short(*b));

    let boards_24: Vec<Board> = fire_boards
        .iter()
        .filter(|b| b.chip_pins() == 24)
        .copied()
        .collect();
    let boards_28: Vec<Board> = fire_boards
        .iter()
        .filter(|b| b.chip_pins() == 28)
        .copied()
        .collect();
    let boards_32: Vec<Board> = fire_boards
        .iter()
        .filter(|b| b.chip_pins() == 32)
        .copied()
        .collect();
    let boards_40: Vec<Board> = fire_boards
        .iter()
        .filter(|b| b.chip_pins() == 40)
        .copied()
        .collect();

    writeln!(w, "# One ROM Chip Compatibility — firmware v{}", version)?;
    writeln!(w)?;
    writeln!(
        w,
        "This document shows which chips each One ROM Fire hardware variant emulates."
    )?;
    writeln!(w)?;
    writeln!(w, "**ROM size** is the chip's actual storage capacity.")?;
    writeln!(w)?;
    writeln!(
        w,
        "**Image size** is the space used on One ROM device's internal \
                 flash to emulate that chip. This can be larger than the original \
                 ROM itself, due to the way One ROM works."
    )?;
    writeln!(w)?;
    writeln!(
        w,
        "One ROM typically has a 2MB flash, with 64KB reserved for the firmware \
                 and metadata. The remainder is available for ROM images. The total number \
                 of images that can be supported is based on the image size of each ROM."
    )?;
    writeln!(w)?;
    writeln!(
        w,
        "Some lower pin count ROMs can \
                 be emulated by larger One ROMs, by inserting the larger One ROM in the \
                 smaller socket, with the top pins (1, 2, ...) hanging out (and it is not \
                 necessary to solder these pins to One ROM if using like this). If doing \
                 this, it is _extremely_ important that power is rerouted to One ROM's VCC \
                 pin, or to the 5V header pin, or One ROM may be damaged."
    )?;
    writeln!(w)?;
    writeln!(
        w,
        "Some greater pin count ROMs can be emulated by a smaller One ROM, provided \
                 the ROM's extra address pins fall on socket positions that One ROM does not \
                 use. In this case, the smaller One ROM gets installed to the bottom \
                 of the larger socket, with the top pins of the socket unpopulated. \
                 A short fly-lead must be run from each additional address pin of those \
                 socket pins to the X1 (and, if two are needed, X2) header pin on One ROM."
    )?;
    writeln!(w)?;
    writeln!(w, "| Cell | Meaning |")?;
    writeln!(w, "|:---|:---|")?;
    writeln!(
        w,
        "| `64KB` | Chip is supported on this board; shows the image size |"
    )?;
    writeln!(
        w,
        "| `64KB*` | Supported with One ROM overhanging the socket (top pins exposed — power reroute required) |"
    )?;
    writeln!(
        w,
        "| `64KB†` | Supported with fly-lead wire(s) from the chip socket's address pin(s) to One ROM's X1 (and X2) header pin |"
    )?;
    writeln!(w, "| `-` | Not supported on this board |")?;
    writeln!(w)?;

    write_matrix_section(w, "24-pin boards", &boards_24)?;
    write_matrix_section(w, "28-pin boards", &boards_28)?;
    write_matrix_section(w, "32-pin boards", &boards_32)?;
    write_matrix_section(w, "40-pin boards", &boards_40)?;

    writeln!(w, "---")?;
    writeln!(w)?;
    writeln!(w, "# Per-board details")?;
    writeln!(w)?;
    writeln!(
        w,
        "Full chip list for each board. Where a particular ROM type goes \
                 by multiple identifiers (for example 27512, 27C512, 27SF512), each \
                 type appears as a separate row."
    )?;
    writeln!(w)?;

    for board in &fire_boards {
        write_board_table(w, *board)?;
    }

    Ok(())
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() -> io::Result<()> {
    let output = output_path();
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut w = BufWriter::new(File::create(&output)?);
    generate_document(&mut w)?;
    w.flush()?;
    eprintln!("Written to {}", output.display());
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Command to regenerate the compatibility document.
    const REGEN_CMD: &str = "cargo run -p onerom-gen --bin compat";

    #[test]
    fn compatibility_doc_is_current() {
        let mut generated = Vec::new();
        generate_document(&mut generated).expect("document generation failed");
        let generated =
            String::from_utf8(generated).expect("generated document is not valid UTF-8");

        let checked_in = std::fs::read_to_string(output_path()).unwrap_or_else(|_| {
            panic!(
                "docs/COMPATIBILITY.md is missing — regenerate with: {}",
                REGEN_CMD
            )
        });

        assert!(
            checked_in == generated,
            "docs/COMPATIBILITY.md is out of date — regenerate with: {}",
            REGEN_CMD
        );
    }
}
