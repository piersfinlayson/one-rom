// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::args::{BoardHeaderArgs, BoardListArgs, BoardSocketArgs};
use crate::board_view::{render_pin_header, render_rom_socket};
use crate::utils::{get_reference_boards, get_supported_boards, resolve_board};
use onerom_cli::{Error, Options};
use onerom_config::chip::ChipType;
use onerom_config::hw::Board;

pub async fn cmd_list(_options: &Options, _args: &BoardListArgs) -> Result<(), Error> {
    print_board_types();
    Ok(())
}

/// Print the board types, split by whether the CLI can act on them.
///
/// The two groups are listed separately rather than merged, because they are
/// not interchangeable: naming a Fire board gets you a device operation, while
/// naming an Ice board does not. Neither group says which commands accept
/// which - that is each command's own business to report, and a list here
/// would be wrong the moment one of them changed. Shared by `board list` and
/// `scan --list-boards`.
pub(crate) fn print_board_types() {
    println!("Supported One ROM board types:");
    println!("  {}", get_supported_boards());
    println!();
    println!("Recognised, but not supported by the CLI:");
    println!("  {}", get_reference_boards());
    println!("  These boards use an STM32, rather than the RP2350 the CLI works with.");
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
/// has no header descriptor yet. Shared by `board header` and `inspect header`.
pub(crate) fn show_pin_header(board: &Board) {
    match render_pin_header(board) {
        Some(diagram) => print!("{diagram}"),
        None => println!(
            "Board {} has no pin-header descriptor - command unsupported.",
            board.name()
        ),
    }
}

/// Print a board's ROM socket pinout. `chip_type`, when given, selects the
/// function view and must be a chip type the board accepts. Shared by
/// `board socket` and `inspect socket`.
pub(crate) fn show_rom_socket(
    board: &Board,
    chip_type: &Option<String>,
    gpio: bool,
) -> Result<(), Error> {
    // A board with no socket pin map has no GPIO behind any socket pin, so the
    // views built on one would draw a diagram with the GPIO column blank
    // throughout. Say so instead of handing back an emptier answer than asked
    // for.
    if view_needs_gpios(chip_type, gpio) && board.socket_pin_map().is_empty() {
        println!(
            "Board {} has no GPIO map - command unsupported.",
            board.name()
        );
        return Ok(());
    }

    let chip = match chip_type {
        Some(t) => Some(resolve_socket_chip(board, t)?),
        None => None,
    };
    print!("{}", render_rom_socket(board, chip, gpio));
    Ok(())
}

/// Whether a socket view is drawn from the board's GPIO map.
///
/// Without a chip type the diagram *is* the GPIO map, and `--gpio` overlays
/// GPIOs onto the function view. The function view on its own is drawn from the
/// chip's pinout and the board's ROM signal assignments, neither of which needs
/// the socket pin map.
fn view_needs_gpios(chip_type: &Option<String>, gpio: bool) -> bool {
    chip_type.is_none() || gpio
}

/// Resolve a `--chip-type` name to a [`ChipType`] this board can emulate,
/// erroring with the board's supported list otherwise.
///
/// A chip counts as emulatable if `onerom-gen`'s compatibility check places it
/// on the board, including the fly-lead cases documented in
/// `docs/COMPATIBILITY.md`. The socket renderer relies on that same geometry.
fn resolve_socket_chip(board: &Board, name: &str) -> Result<ChipType, Error> {
    let supported = onerom_cli::slot::emulatable_chip_names(board).join(", ");
    let chip = ChipType::try_from_str(name)
        .ok_or_else(|| Error::UnsupportedChipType(name.to_string(), supported.clone()))?;
    // Plugins (and any other 0-pin type) have no ROM socket to draw.
    if chip.chip_pins() == 0 {
        return Err(Error::UnsupportedChipType(name.to_string(), supported));
    }
    if onerom_gen::compat::check_chip_set_on_board(
        *board,
        chip,
        onerom_gen::ChipSetType::Single,
        1,
        onerom_gen::compat::default_cs_config(chip),
    )
    .is_err()
    {
        return Err(Error::UnsupportedChipType(name.to_string(), supported));
    }
    Ok(chip)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the views drawn from the GPIO map are gated on having one.
    #[test]
    fn only_the_gpio_views_need_the_socket_map() {
        assert!(view_needs_gpios(&None, false));
        assert!(view_needs_gpios(&None, true));
        assert!(view_needs_gpios(&Some("2364".to_string()), true));
        assert!(!view_needs_gpios(&Some("2364".to_string()), false));
    }

    /// The gate has to fire on a real board, or it is decoration: Ice boards
    /// carry no socket pin map, Fire boards do.
    #[test]
    fn boards_without_a_socket_map_are_the_ones_that_cannot_draw_gpios() {
        use onerom_config::hw::{BOARDS, Model};
        for board in BOARDS {
            assert_eq!(
                board.socket_pin_map().is_empty(),
                board.model() == Model::Ice,
                "{} socket map",
                board.name()
            );
        }
    }

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
