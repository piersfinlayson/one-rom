// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::args::{BoardArgs, BoardHeaderArgs, BoardSocketArgs};
use crate::board_view::{render_pin_header, render_rom_socket};
use crate::utils::resolve_board;
use onerom_cli::{Error, Options};
use onerom_config::chip::ChipType;
use onerom_config::hw::{BOARDS, Board};

pub async fn cmd_boards(_options: &Options, _args: &BoardArgs) -> Result<(), Error> {
    println!("Supported One ROM board types:");
    // Comma separate them
    let boards = BOARDS
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    println!("  {boards}");
    Ok(())
}

pub async fn cmd_header(options: &Options, args: &BoardHeaderArgs) -> Result<(), Error> {
    let board = resolve_board(options, &args.board)?.ok_or(Error::NoBoardOrDevice)?;
    show_pin_header(&board);
    Ok(())
}

pub async fn cmd_socket(options: &Options, args: &BoardSocketArgs) -> Result<(), Error> {
    let board = resolve_board(options, &args.board)?.ok_or(Error::NoBoardOrDevice)?;
    show_rom_socket(&board, &args.chip_type, args.gpio)
}

/// Print a board's pin (jumper / programming) header, or a notice if the board
/// has no header descriptor yet. Shared by `boards header` and `inspect header`.
pub(crate) fn show_pin_header(board: &Board) {
    match render_pin_header(board) {
        Some(diagram) => print!("{diagram}"),
        None => println!(
            "Board {} has no pin-header descriptor yet - nothing to draw.",
            board.name()
        ),
    }
}

/// Print a board's ROM socket pinout. `chip_type`, when given, selects the
/// function view and must be a chip type the board accepts. Shared by
/// `boards socket` and `inspect socket`.
pub(crate) fn show_rom_socket(
    board: &Board,
    chip_type: &Option<String>,
    gpio: bool,
) -> Result<(), Error> {
    let chip = match chip_type {
        Some(t) => Some(resolve_socket_chip(board, t)?),
        None => None,
    };
    print!("{}", render_rom_socket(board, chip, gpio));
    Ok(())
}

/// Resolve a `--chip-type` name to a [`ChipType`] this board can emulate,
/// erroring with the board's supported list otherwise.
///
/// A chip counts as emulatable if the board natively/overhang-accepts it
/// (`allows_chip_type`) or `onerom-gen`'s compatibility check places it,
/// including the fly-lead cases documented in `docs/COMPATIBILITY.md`. The
/// socket renderer relies on that same geometry.
fn resolve_socket_chip(board: &Board, name: &str) -> Result<ChipType, Error> {
    let supported = board.supported_chip_type_names().join(", ");
    let chip = ChipType::try_from_str(name)
        .ok_or_else(|| Error::UnsupportedChipType(name.to_string(), supported.clone()))?;
    // Plugins (and any other 0-pin type) have no ROM socket to draw.
    if chip.chip_pins() == 0 {
        return Err(Error::UnsupportedChipType(name.to_string(), supported));
    }
    let emulatable = board.allows_chip_type(chip)
        || onerom_gen::compat::check_chip_on_board(*board, chip).is_some();
    if !emulatable {
        return Err(Error::UnsupportedChipType(name.to_string(), supported));
    }
    Ok(chip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_chip_rejects_plugins_and_zero_pin_types() {
        let board = Board::try_from_str("fire-24-f").unwrap();
        // Plugins parse as ChipType but have no ROM socket.
        assert!(resolve_socket_chip(&board, "SystemPlugin").is_err());
        assert!(resolve_socket_chip(&board, "UserPlugin").is_err());
        // Real ROM types still resolve: native, and a larger (fly-lead) type.
        assert!(resolve_socket_chip(&board, "2364").is_ok());
        assert!(resolve_socket_chip(&board, "2764").is_ok());
    }
}
