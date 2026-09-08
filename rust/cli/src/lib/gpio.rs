// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What an MCU GPIO *is* on a given One ROM board.
//!
//! Every answer here comes from the static board and chip metadata in
//! [`onerom_config`] - no device, no I/O - so the same questions are answerable
//! about a board that is not connected, and about the image a device is about to
//! be given rather than the one it is running.
//!
//! A device reports only a coarse use category for a GPIO and never a role name,
//! so naming is entirely the host's job. The CLI's `board` and `inspect`
//! renderers draw on these same lookups, so a diagram and a one-line answer
//! cannot disagree about what a pin is.

use onerom_config::chip::ChipType;
use onerom_config::hw::{Board, HeaderRole, HeaderSlot};
use onerom_gen::socket_pin_offset;

/// The short label shown on a header pad for a single role.
pub fn header_role_label(role: &HeaderRole) -> String {
    match role {
        HeaderRole::Power5V => "5V".to_string(),
        HeaderRole::Gnd => "GND".to_string(),
        HeaderRole::Run => "RUN".to_string(),
        HeaderRole::Bootsel => "BOOTSEL".to_string(),
        HeaderRole::Select(b) => format!("SEL_{}", (b'A' + *b) as char),
        HeaderRole::Swclk => "SWCLK".to_string(),
        HeaderRole::Swdio => "SWDIO".to_string(),
        HeaderRole::X1 => "X1".to_string(),
        HeaderRole::X2 => "X2".to_string(),
        HeaderRole::Addr(n) => format!("A{n}"),
    }
}

/// The MCU GPIO behind a role, where one exists (image-select, X and broken-out
/// address pads carry a GPIO; power/ground/SWD/control pads do not).
#[allow(clippy::wildcard_enum_match_arm)]
pub fn header_role_gpio(board: &Board, role: &HeaderRole) -> Option<u8> {
    match role {
        HeaderRole::Select(b) => board.sel_pins().get(*b as usize).copied(),
        HeaderRole::X1 => (board.pin_x1() != 255).then(|| board.pin_x1()),
        HeaderRole::X2 => (board.pin_x2() != 255).then(|| board.pin_x2()),
        HeaderRole::Addr(n) => board.addr_pins().get(*n as usize).copied(),
        _ => None,
    }
}
/// The ROM's function(s) for a socket pin, for a given chip type (e.g. `A12`,
/// `D3`, `CS1`, `CE`, `BYTE`, `VCC`, `GND`, `VPP`). `None` for a pin the chip
/// does not define.
///
/// A pin can carry more than one function on parts with a multiplexed pinout —
/// the 27C400's pin 29 is address A0 in byte mode and data D15 in word mode, for
/// example — in which case the functions are joined with `/` (e.g. `A0/D15`).
pub fn socket_function(chip: ChipType, pin: u8) -> Option<String> {
    let mut funcs: Vec<String> = Vec::new();
    for (i, &p) in chip.address_pins().iter().enumerate() {
        if p == pin {
            funcs.push(format!("A{i}"));
        }
    }
    for (i, &p) in chip.data_pins().iter().enumerate() {
        if p == pin {
            funcs.push(format!("D{i}"));
        }
    }
    for c in chip.control_lines().iter().filter(|c| c.pin == pin) {
        funcs.push(c.name.to_ascii_uppercase());
    }
    for p in chip.power_pins().iter().filter(|p| p.pin == pin) {
        funcs.push(p.name.to_ascii_uppercase());
    }
    if let Some(pins) = chip.programming_pins() {
        for p in pins.iter().filter(|p| p.pin == pin) {
            funcs.push(p.name.to_ascii_uppercase());
        }
    }
    (!funcs.is_empty()).then(|| funcs.join("/"))
}
// ===========================================================================
// GPIO naming
// ===========================================================================
//
// `inspect gpio` asks the board metadata the same questions the CLI's renderers
// ask, but one GPIO at a time. These are the per-GPIO form, built on the very
// same lookups (`header_role_gpio`, `socket_function`, `socket_pin_offset`) so a
// change to how a pad or a socket pin is named shows up in the diagram and the
// table together.

/// The header pad role(s) an MCU GPIO *is*, e.g. `SEL_A`, `X1`, `A12`.
///
/// Only roles that have a GPIO behind them are named. A pad may carry more than
/// one role — on a Fire 24/28 board the SEL_C and SEL_D pads sit on the SWCLK
/// and SWDIO nets — but SWCLK and SWDIO are dedicated RP2350 pins, not GPIOs, so
/// naming GPIO 25 `SEL_C/SWCLK` would assert something untrue of the GPIO. That
/// a pad shares a net with a debug probe is a fact about the pad; this answer is
/// indexed by GPIO. (The header diagram is indexed by pad and does show every
/// role — see the CLI's header view.) Where a GPIO genuinely is more than one
/// role, the roles are joined with `/`.
///
/// `jumper_header` is populated for the Fire 24/28/32 boards but not yet for
/// Fire 40 or any Ice board, so an uncharacterised board falls back to the
/// electrical pin arrays, which name the image-select and X pads without
/// claiming to know where on the header they sit. Callers that show the physical
/// header layout must still check
/// [`Board::jumper_header`](onerom_config::hw::Board::jumper_header) themselves;
/// this function degrades to naming rather than to nothing.
///
/// `None` means no pad carries this GPIO.
#[allow(clippy::wildcard_enum_match_arm)]
pub fn header_role(board: &Board, gpio: u8) -> Option<String> {
    // The board pin arrays use 255 for "no such pin", and no real GPIO number
    // reaches it, so it must never match one of those sentinels.
    if gpio == 255 {
        return None;
    }

    if let Some(header) = board.jumper_header() {
        let pad = header
            .columns
            .iter()
            .flat_map(|c| [Some(&c.row1), Some(&c.row2), c.row3.as_ref()])
            .flatten()
            .find_map(|slot| match slot {
                HeaderSlot::Roles(roles) => {
                    // Roles with no GPIO of their own (SWCLK, SWDIO, RUN,
                    // BOOTSEL, power, ground) are dropped: they belong to the
                    // pad, not to this GPIO.
                    let named: Vec<String> = roles
                        .iter()
                        .filter(|r| header_role_gpio(board, r) == Some(gpio))
                        .map(header_role_label)
                        .collect();
                    (!named.is_empty()).then(|| named.join("/"))
                }
                _ => None,
            });
        if pad.is_some() {
            return pad;
        }
    }

    // Uncharacterised header, or a GPIO on no pad of a characterised one. The
    // pin arrays are electrical facts and hold either way.
    if let Some(bit) = board.sel_pins().iter().position(|&p| p == gpio) {
        return Some(header_role_label(&HeaderRole::Select(bit as u8)));
    }
    if board.pin_x1() == gpio {
        return Some(header_role_label(&HeaderRole::X1));
    }
    if board.pin_x2() == gpio {
        return Some(header_role_label(&HeaderRole::X2));
    }
    None
}

/// The ROM function of an MCU GPIO under `chip`, e.g. `A5`, `D3`, `CS1`, `BYTE`.
///
/// The board's socket pin numbering and the chip's own differ whenever the two
/// pin counts differ, so the chip pin is recovered through
/// [`socket_pin_offset`] — the same geometry the CLI's socket view draws with.
/// `None` for a GPIO that is not on the ROM socket at all, for a socket pin
/// outside a smaller chip's body, and for a board/chip pin-count combination
/// that has no defined placement.
pub fn rom_function(board: &Board, chip: ChipType, gpio: u8) -> Option<String> {
    let socket_pin = board.socket_pin_for_gpio(gpio)?;
    let offset = socket_pin_offset(chip.chip_pins(), board.chip_pins())?;
    // socket_pin = chip_pin + offset.
    let chip_pin = i16::from(socket_pin) - offset;
    if chip_pin < 1 || chip_pin > i16::from(chip.chip_pins()) {
        return None;
    }
    socket_function(chip, chip_pin as u8)
}

/// Every One ROM system function of an MCU GPIO; empty if it has none.
///
/// These are the pins the firmware reports as `SYSTEM`: the status LED, the
/// NeoPixel, the USB VBUS sense line and the external flash chip-select. The
/// board data uses 255 as "no such pin", which no real GPIO number reaches.
///
/// A GPIO can carry more than one of them — on `fire-24-f` the status LED and
/// the NeoPixel are both GPIO 29, which is exactly why the RGB plugin reflects
/// the status-LED state on that shared LED — so this answers with all of them.
/// Stopping at the first would report half the truth about the pin most likely
/// to be driven by accident.
pub fn system_functions(board: &Board, gpio: u8) -> Vec<&'static str> {
    let mut functions = Vec::new();
    if gpio == 255 {
        return functions;
    }
    if board.pin_status() == gpio {
        functions.push("Status LED");
    }
    if board.pin_neo() == Some(gpio) {
        functions.push("RGB LED");
    }
    if board.usb_vbus_pin() == Some(gpio) {
        functions.push("USB VBUS");
    }
    if board.external_flash_cs_pin() == Some(gpio) {
        functions.push("ext flash CS");
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;
    use onerom_config::chip::CHIP_TYPES;

    /// fire-24-f: a characterised jumper header, and select pads behind the
    /// RP2350A's ADC pins.
    fn board() -> Board {
        Board::try_from_str("fire-24-f").unwrap()
    }

    #[test]
    fn header_role_names_only_what_the_gpio_is() {
        let b = board();
        assert_eq!(header_role(&b, 26).as_deref(), Some("SEL_A"));
        assert_eq!(header_role(&b, 9).as_deref(), Some("X1"));
        assert_eq!(header_role(&b, 8).as_deref(), Some("X2"));
        // GPIO25/24 sit on the pads that also carry the SWCLK/SWDIO nets, but
        // those are dedicated RP2350 pins rather than GPIOs, so the GPIO is
        // named SEL_C/SEL_D alone. The pad-indexed header diagram still shows
        // both roles - which the CLI's header view asserts for itself.
        assert_eq!(header_role(&b, 25).as_deref(), Some("SEL_C"));
        assert_eq!(header_role(&b, 24).as_deref(), Some("SEL_D"));
        // A data pin is on no header pad.
        assert_eq!(header_role(&b, 0), None);
    }

    #[test]
    fn header_role_degrades_without_a_jumper_header() {
        // Some Ice boards still have no jumper_header descriptor, so the pad
        // names come from the electrical pin arrays instead of nothing.
        let b = Board::try_from_str("ice-24-d").unwrap();
        assert!(b.jumper_header().is_none());
        let sel_a = b.sel_pins()[0];
        assert_eq!(header_role(&b, sel_a).as_deref(), Some("SEL_A"));
        // ice-24-d has no X pins, so nothing invents one.
        assert_eq!(b.pin_x1(), 255);
        assert_eq!(header_role(&b, 255), None);
    }

    #[test]
    fn rom_function_matches_the_socket_diagram() {
        let b = board();
        // The same facts the socket view asserts: A7 on GPIO16, CS1 on GPIO10.
        assert_eq!(
            rom_function(&b, ChipType::Chip2364, 16).as_deref(),
            Some("A7")
        );
        assert_eq!(
            rom_function(&b, ChipType::Chip2364, 10).as_deref(),
            Some("CS1")
        );
        // A GPIO that is not on the socket at all.
        assert_eq!(rom_function(&b, ChipType::Chip2364, 29), None);
    }

    #[test]
    fn rom_function_follows_the_socket_pin_offset() {
        // A 24-pin 2364 on a 28-pin board sits at socket pins 3-26, so the
        // chip's pin 1 (A7) is two pins along from the board's.
        let b = Board::try_from_str("fire-28-c").unwrap();
        let socket_pin_3_gpio = b.gpios_for_socket_pin(3)[0];
        assert_eq!(
            rom_function(&b, ChipType::Chip2364, socket_pin_3_gpio).as_deref(),
            Some("A7")
        );
        // Socket pin 1 is outside the 24-pin chip - One ROM overhangs it.
        let socket_pin_1_gpio = b.gpios_for_socket_pin(1)[0];
        assert_eq!(
            rom_function(&b, ChipType::Chip2364, socket_pin_1_gpio),
            None
        );
    }

    #[test]
    fn system_functions_name_the_firmwares_system_pins() {
        let b = board();
        // fire-24-f puts the status LED and the NeoPixel on the same GPIO, so
        // both must be named - reporting only the first hides half of what
        // driving GPIO 29 disturbs.
        assert_eq!(b.pin_status(), 29);
        assert_eq!(b.pin_neo(), Some(29));
        assert_eq!(system_functions(&b, 29), ["Status LED", "RGB LED"]);
        assert!(system_functions(&b, 0).is_empty());

        // A board with an external flash and a NeoPixel on distinct GPIOs.
        let b32 = Board::try_from_str("fire-32-b").unwrap();
        assert_eq!(system_functions(&b32, 44), ["RGB LED"]);
        assert_eq!(system_functions(&b32, 47), ["ext flash CS"]);
        assert_eq!(system_functions(&b32, 45), ["Status LED"]);

        // Ice boards report 255 for "no status LED"; 255 is not a GPIO.
        let ice = Board::try_from_str("ice-24-d").unwrap();
        assert_eq!(ice.pin_status(), 255);
        assert!(system_functions(&ice, 255).is_empty());
    }

    /// Naming every GPIO of every board, under every chip type that board
    /// accepts, must not panic. `inspect gpio` walks the whole device, so a
    /// board/chip combination no hand-written test covers still gets asked.
    #[test]
    fn every_gpio_of_every_board_can_be_named() {
        use onerom_config::hw::BOARDS;
        for board in BOARDS {
            let board = &board;
            for gpio in 0u8..48 {
                let _ = header_role(board, gpio);
                let _ = system_functions(board, gpio);
                for &chip in CHIP_TYPES {
                    let _ = rom_function(board, chip, gpio);
                }
            }
        }
    }
}
