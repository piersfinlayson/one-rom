// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The chip fit listing, run for real.
//!
//! The figures are `onerom-gen`'s compatibility pass over a board, and the
//! names are `onerom-config`'s chip types, so a chip type added to the tree
//! appears here with nobody typing anything.
//!
//! The sections are the CLI's, and so is what decides them: `supported_chips`
//! orders its entries so chips that sit in the socket the same way are
//! consecutive, and the same `CompatResult` that fills a row's `Fit` cell says
//! which way that is.

use onerom_cli::Error;
use onerom_config::chip::{CHIP_TYPES, ChipType};
use onerom_config::hw::{BOARDS, Board};
use onerom_gen::ChipSetType;
use onerom_gen::compat::{
    ChipCompat, check_chip_set_on_board, default_cs_config, format_size, supported_chips,
};

use super::{Inputs, Need, Runner, Takes};
use crate::stub::{Body, Output, Section};

/// One chip on one board, the whole board, or every chip type there is.
pub const FIT: Runner = Runner {
    needs: &[
        Need {
            long: "board",
            takes: Takes::Words,
        },
        Need {
            long: "all",
            takes: Takes::Switch,
        },
        Need {
            long: "chip-type",
            takes: Takes::Words,
        },
    ],
    does: "the chip fit figures in onerom-gen and onerom-config",
    work: fit,
};

/// The columns a fit listing has, in the CLI's order.
const COLUMNS: [&str; 4] = ["Chip", "ROM size", "Image size", "Fit"];

/// A per-chip figure is one ROM with a slot to itself.
const ALONE: ChipSetType = ChipSetType::Single;

/// How many chips share that slot.
const ONE: usize = 1;

fn fit(inputs: &Inputs) -> Result<Output, Error> {
    let named = inputs.text("board");
    let chip = inputs.text("chip-type");

    if inputs.switch("all") {
        return Ok(everything());
    }

    // The CLI falls back to the board of a connected One ROM.  Nothing here
    // touches USB, so with no board named there is nothing to answer with.
    if named.is_empty() {
        return Err(Error::NoBoardOrDevice);
    }

    let board = Board::try_from_str(named).ok_or_else(|| {
        let known: Vec<&str> = BOARDS.iter().map(Board::name).collect();
        Error::InvalidBoard(named.to_owned(), known.join(", "))
    })?;

    if chip.is_empty() {
        Ok(on_board(board))
    } else {
        one_chip(board, chip)
    }
}

/// Every chip type this build knows, whatever board it needs.
///
/// A section per pin count, as the CLI lists them.  There is no board here to
/// fit a chip against, so a chip's own pin count is all there is to group by.
fn everything() -> Output {
    let servable = || CHIP_TYPES.iter().filter(|chip| !chip.is_plugin());

    let mut sizes: Vec<u8> = servable().map(|chip| chip.chip_pins()).collect();
    sizes.sort_unstable();
    sizes.dedup();

    let sections = sizes
        .into_iter()
        .map(|pins| Section {
            heading: format!("{pins}-pin chips"),
            rows: servable()
                .filter(|chip| chip.chip_pins() == pins)
                .map(|chip| vec![chip.to_string(), format_size(chip.size_bytes() as u32)])
                .collect(),
        })
        .collect();

    Output::Table {
        headers: ["Chip", "ROM size"].map(str::to_owned).to_vec(),
        body: Body::Sections(sections),
    }
}

/// What one board can serve, and what each chip costs it.
fn on_board(board: Board) -> Output {
    let entries = supported_chips(board, ALONE, ONE);

    // A board `onerom-gen` cannot size has no per-chip figures, so its own
    // list of names is all there is to say.  Naming them beats leaving a
    // recognised type unaccounted for.
    if entries.is_empty() {
        return Output::Table {
            headers: vec![COLUMNS[0].to_owned()],
            body: Body::Rows(
                board
                    .supported_chip_type_names()
                    .iter()
                    .map(|name| vec![(*name).to_owned()])
                    .collect(),
            ),
        };
    }

    Output::Table {
        headers: COLUMNS.map(str::to_owned).to_vec(),
        body: Body::Sections(fit_sections(board, &entries)),
    }
}

/// The entries grouped the way they arrived, a section per way of fitting.
///
/// `supported_chips` sorts by fit class, so a change of pin offset ends a
/// section — the CLI splits the same listing on the same change.  The `Fit`
/// column stays, because one heading covers several: a fly-lead section holds
/// chips wanting X1, chips wanting X1 and X2, and chips wanting neither.
fn fit_sections(board: Board, entries: &[ChipCompat]) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut offset: Option<i16> = None;

    for entry in entries {
        if offset != Some(entry.result.pin_offset) {
            offset = Some(entry.result.pin_offset);
            sections.push(Section {
                heading: fit_heading(board, entry),
                rows: Vec::new(),
            });
        }

        // The first entry pushed a section, so there is always one to add to.
        if let Some(section) = sections.last_mut() {
            section.rows.push(vec![
                entry.alias.to_owned(),
                format_size(entry.rom_size_bytes),
                format_size(entry.result.slot_size_bytes),
                entry.result.fit_description(),
            ]);
        }
    }

    sections
}

/// What a run of chips that fit the board the same way is headed with.
///
/// A native chip is named by the board's pin count and the rest by their own,
/// because that is the number a user is comparing against the socket in front
/// of them.
fn fit_heading(board: Board, entry: &ChipCompat) -> String {
    let pins = entry.chip_type.chip_pins();

    if entry.result.is_native() {
        format!("{}-pin chips (native)", board.chip_pins())
    } else if entry.result.requires_fly_leads() {
        format!("{pins}-pin chips (with fly-leads)")
    } else {
        format!("{pins}-pin chips (with overhang)")
    }
}

/// One chip on one board, as a table of a single row.
fn one_chip(board: Board, name: &str) -> Result<Output, Error> {
    let unsupported = || {
        Error::UnsupportedChipType(
            name.to_owned(),
            onerom_cli::slot::emulatable_chip_names(&board).join(", "),
        )
    };

    let chip = ChipType::try_from_str(name).ok_or_else(unsupported)?;
    let result = check_chip_set_on_board(board, chip, ALONE, ONE, default_cs_config(chip))
        .map_err(|_| unsupported())?;

    Ok(Output::Table {
        headers: COLUMNS.map(str::to_owned).to_vec(),
        body: Body::Rows(vec![vec![
            name.to_owned(),
            format_size(chip.size_bytes() as u32),
            format_size(result.slot_size_bytes),
            result.fit_description(),
        ]]),
    })
}
