// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `--pin` decoding.
//!
//! [`Pin`], [`HeaderPin`] and [`ResolvedPin`] are defined in [`onerom_config::pin`].
//! This module converts that module's errors into CLI error messages.

use crate::Error;
use onerom_config::hw::Board;
use onerom_config::pin::{PinError, ResolveError};
use onerom_fw_parser::ParsedDevice;
use onerom_metadata::GPIO_NONE;

pub use onerom_config::pin::{HeaderPin, Pin, ResolvedPin};

const HEADER_HINT: &str = "Use 'onerom inspect header' to see each header pin's GPIO.";

/// What `--pin` accepts, as one sentence for an error message.
///
/// The number of image select pins differs between boards, so they are written
/// `sel_<letter>` rather than as a range.
const NAMESPACE_HINT: &str = "--pin is a header pin: 'sel_<letter>', 'x1' or 'x2'.";

fn header_pins_on(board: &Board) -> String {
    HeaderPin::all_on(board)
        .map(|pin| pin.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl From<ResolveError> for Error {
    fn from(error: ResolveError) -> Self {
        match error {
            ResolveError::NoBoard { pin } => Error::InvalidPin(
                pin.to_string(),
                format!(
                    "'{pin}' is a header pin. Its GPIO depends on the board.\n  \
                     This One ROM's board type could not be determined.\n  \
                     Pass --board <BOARD> or write the MCU GPIO as 'gpio<N>'."
                ),
            ),
            ResolveError::NoSuchPin { pin, board } => Error::InvalidPin(
                pin.to_string(),
                format!(
                    "Board {} has no '{pin}' pin.\n  Its header pins are: {}.\n  {HEADER_HINT}",
                    board.name(),
                    header_pins_on(&board),
                ),
            ),
            // ResolveError is non_exhaustive.
            other => Error::InvalidPin(String::new(), other.to_string()),
        }
    }
}

/// Decode a `--pin` value.
///
/// The accepted values are listed at [`onerom_config::pin::parse_pin`]. The
/// pin is checked against the board by [`Pin::resolve`].
pub fn parse_pin(spec: &str) -> Result<Pin, Error> {
    let trimmed = spec.trim();
    onerom_config::pin::parse_pin(trimmed)
        .map_err(|error| Error::InvalidPin(trimmed.to_string(), pin_error_detail(error, trimmed)))
}

/// Decode a `--reserve-pin` value.
///
/// The pin is checked against the board when the image is built.
pub fn parse_reserve_pin(spec: &str) -> Result<Pin, Error> {
    let trimmed = spec.trim();
    onerom_config::pin::parse_pin(trimmed)
        .map_err(|_| Error::InvalidReservePin(trimmed.to_string()))
}

/// The image select, X1 or X2 pin wired to `gpio`, from `image`'s metadata.
///
/// Read from the metadata so it is correct for a board this build doesn't
/// recognise.
pub fn metadata_header_pin(image: &ParsedDevice, gpio: u8) -> Option<HeaderPin> {
    // Metadata older than reserved pins isn't read.
    image.reserved_pins()?;
    let hw = &image.as_schema()?.metadata()?.hw;
    if gpio == GPIO_NONE {
        return None;
    }
    if let Some(index) = hw.gpio_sel.iter().position(|&g| g == gpio) {
        return Some(HeaderPin::Select(index as u8));
    }
    if hw.gpio_x1.contains(&gpio) {
        return Some(HeaderPin::X1);
    }
    hw.gpio_x2.contains(&gpio).then_some(HeaderPin::X2)
}

/// A mask of the GPIOs wired to pins reserved in `image`'s metadata, bit N
/// for GPIO N.
pub fn reserved_gpios(image: &ParsedDevice) -> u64 {
    let Some(reserved) = image.reserved_pins() else {
        return 0;
    };
    (0..64u8)
        .filter(|&gpio| metadata_header_pin(image, gpio).is_some_and(|pin| reserved.contains(pin)))
        .fold(0, |mask, gpio| mask | (1 << gpio))
}

fn pin_error_detail(error: PinError, trimmed: &str) -> String {
    let name = trimmed.to_ascii_lowercase();
    match error {
        PinError::Empty => format!("No pin was provided.\n  {NAMESPACE_HINT}\n  {HEADER_HINT}"),
        PinError::MissingGpioNumber => {
            "'gpio' must be followed by a GPIO number - for example 'gpio23'.".to_string()
        }
        PinError::GpioOutOfRange => {
            let digits = name.strip_prefix("gpio").unwrap_or(&name);
            format!("GPIO number '{digits}' is out of range - GPIO numbers are 0 to 255.")
        }
        PinError::BareNumber => format!(
            "A bare number is ambiguous: it could be an MCU GPIO, an image select pin, an X pin or a ROM socket pin.\n  Write an MCU GPIO as 'gpio{name}'.\n  {HEADER_HINT}"
        ),
        PinError::AddressLine => format!(
            "'{name}' is an address line, not a header pin.\n  \
             To use an address line, use its MCU GPIO, written 'gpio<N>'.\n  {HEADER_HINT}"
        ),
        PinError::NotAGpio => format!(
            "'{name}' is not a GPIO - it is a dedicated MCU pin and cannot be driven.\n  {NAMESPACE_HINT}"
        ),
        // PinError is non_exhaustive.
        PinError::Unrecognised | _ => {
            format!("Unrecognised pin.\n  {NAMESPACE_HINT}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rendered error for a spec that must not parse.
    fn rejection(spec: &str) -> String {
        match parse_pin(spec) {
            Ok(pin) => panic!("'{spec}' should not parse, but gave {pin}"),
            Err(e) => e.to_string(),
        }
    }

    /// The rendered error for a pin that parses but must not resolve.
    fn resolve_rejection(spec: &str, board: Option<&Board>) -> String {
        let pin = parse_pin(spec).expect("parses");
        match pin.resolve(board) {
            Ok(resolved) => panic!("'{spec}' should not resolve, but gave {resolved}"),
            Err(e) => Error::from(e).to_string(),
        }
    }

    /// The GPIO `spec` resolves to on `board`.
    fn resolved(spec: &str, board: &Board) -> u8 {
        parse_pin(spec)
            .expect("parses")
            .resolve(Some(board))
            .expect("resolves")
            .gpio()
    }

    fn board(name: &str) -> Board {
        Board::try_from_str(name).unwrap_or_else(|| panic!("{name} is a known board"))
    }

    #[test]
    fn gpio_names_parse() {
        assert_eq!(parse_pin("gpio0").expect("parses"), Pin::Gpio(0));
        assert_eq!(parse_pin("gpio23").expect("parses"), Pin::Gpio(23));
        assert_eq!(parse_pin("gpio47").expect("parses"), Pin::Gpio(47));
        // A GPIO number no device has: the device's num_gpios is the authority
        // on the bound, not this parser.
        assert_eq!(parse_pin("gpio255").expect("parses"), Pin::Gpio(255));
    }

    #[test]
    fn gpio_names_are_case_and_whitespace_insensitive() {
        assert_eq!(parse_pin("GPIO23").expect("parses"), Pin::Gpio(23));
        assert_eq!(parse_pin("Gpio23").expect("parses"), Pin::Gpio(23));
        assert_eq!(parse_pin("  gpio23  ").expect("parses"), Pin::Gpio(23));
    }

    #[test]
    fn a_gpio_needs_no_board() {
        let resolved = parse_pin("gpio23")
            .expect("parses")
            .resolve(None)
            .expect("resolves without a board");
        assert_eq!(resolved.gpio(), 23);
        assert_eq!(resolved.to_string(), "gpio23");
        assert_eq!(resolved.pin(), Pin::Gpio(23));
    }

    #[test]
    fn pins_parse_in_every_accepted_spelling() {
        for spec in ["sel_a", "sel-a", "sela", "SEL_A", "Sel-A", "  sela  "] {
            assert_eq!(
                parse_pin(spec).expect("parses"),
                Pin::Header(HeaderPin::Select(0)),
                "{spec}"
            );
        }
        assert_eq!(
            parse_pin("sel_e").expect("parses"),
            Pin::Header(HeaderPin::Select(4))
        );
        assert_eq!(parse_pin("x1").expect("parses"), Pin::Header(HeaderPin::X1));
        assert_eq!(parse_pin("X2").expect("parses"), Pin::Header(HeaderPin::X2));
    }

    #[test]
    fn select_pins_go_up_to_sel_g() {
        assert_eq!(
            parse_pin("sel_f").expect("parses"),
            Pin::Header(HeaderPin::Select(5))
        );
        assert_eq!(
            parse_pin("SEL-G").expect("parses"),
            Pin::Header(HeaderPin::Select(6))
        );
        assert!(parse_pin("sel_h").is_err());
    }

    #[test]
    fn a_bare_select_letter_is_not_a_pin() {
        // Too terse to be canonical, and it reads badly next to 'a17'.
        for spec in ["a", "b", "c", "d", "e"] {
            let msg = rejection(spec);
            assert!(msg.contains("Unrecognised pin"), "{spec}: {msg}");
        }
    }

    #[test]
    fn a_pin_displays_as_it_is_written() {
        assert_eq!(Pin::Gpio(23).to_string(), "gpio23");
        assert_eq!(Pin::Header(HeaderPin::Select(0)).to_string(), "sel_a");
        assert_eq!(Pin::Header(HeaderPin::Select(4)).to_string(), "sel_e");
        assert_eq!(Pin::Header(HeaderPin::X1).to_string(), "x1");
        assert_eq!(Pin::Header(HeaderPin::X2).to_string(), "x2");
    }

    // -- Resolution against real board metadata -----------------------------

    #[test]
    fn a_four_select_board_with_x_pins_resolves_every_pin() {
        // fire-24-f: sel = [26, 27, 25, 24], x1 = 9, x2 = 8.
        let b = board("fire-24-f");
        assert_eq!(resolved("sel_a", &b), b.sel_pins()[0]);
        assert_eq!(resolved("sel_b", &b), b.sel_pins()[1]);
        assert_eq!(resolved("sel_c", &b), b.sel_pins()[2]);
        assert_eq!(resolved("sel_d", &b), b.sel_pins()[3]);
        assert_eq!(resolved("x1", &b), b.pin_x1());
        assert_eq!(resolved("x2", &b), b.pin_x2());
        assert_eq!(b.sel_pins().len(), 4);
    }

    #[test]
    fn a_five_select_board_resolves_sel_e() {
        // ice-24-g is the only shape with five image-select pins.
        let b = board("ice-24-g");
        assert_eq!(b.sel_pins().len(), 5);
        assert_eq!(resolved("sel_e", &b), b.sel_pins()[4]);
        // And it is an Ice board with no characterised jumper header, which is
        // exactly why resolution reads the electrical arrays instead.
        assert!(b.jumper_header().is_none());
        assert_eq!(resolved("x1", &b), b.pin_x1());
    }

    #[test]
    fn a_board_without_x_pins_says_so_and_names_what_it_has() {
        // fire-32-a: sel = [38, 39, 36, 37], no X pins.
        let b = board("fire-32-a");
        assert_eq!(b.pin_x1(), 255);
        let msg = resolve_rejection("x1", Some(&b));
        assert!(msg.contains("has no 'x1' pin"), "{msg}");
        assert!(msg.contains("fire-32-a"), "{msg}");
        assert!(msg.contains("sel_a, sel_b, sel_c, sel_d"), "{msg}");
        assert!(!msg.contains("x1,"), "{msg}");
        assert!(msg.contains("onerom inspect header"), "{msg}");
        // The select pins it does have still resolve.
        assert_eq!(resolved("sel_d", &b), b.sel_pins()[3]);
    }

    #[test]
    fn a_board_with_fewer_select_pins_says_so() {
        // fire-28-a has two image-select pins and no X pins.
        let b = board("fire-28-a");
        assert_eq!(b.sel_pins().len(), 2);
        let msg = resolve_rejection("sel_c", Some(&b));
        assert!(msg.contains("has no 'sel_c' pin"), "{msg}");
        assert!(msg.contains("Its header pins are: sel_a, sel_b."), "{msg}");
        // sel_e on a four-select board is the same shape of answer.
        let four = board("fire-24-f");
        let msg = resolve_rejection("sel_e", Some(&four));
        assert!(msg.contains("has no 'sel_e' pin"), "{msg}");
        assert!(msg.contains("sel_a, sel_b, sel_c, sel_d, x1, x2"), "{msg}");
    }

    #[test]
    fn a_pin_without_a_board_points_at_the_board_option() {
        for spec in ["sel_a", "x1"] {
            let msg = resolve_rejection(spec, None);
            assert!(msg.contains("depends on"), "{spec}: {msg}");
            assert!(
                msg.contains("board type could not be determined"),
                "{spec}: {msg}"
            );
            assert!(msg.contains("--board"), "{spec}: {msg}");
            assert!(msg.contains("'gpio<N>'"), "{spec}: {msg}");
        }
    }

    #[test]
    fn a_resolved_pin_shows_both_names() {
        let b = board("fire-24-f");
        let resolved = parse_pin("x1")
            .expect("parses")
            .resolve(Some(&b))
            .expect("resolves");
        assert_eq!(resolved.to_string(), format!("x1 (gpio{})", b.pin_x1()));
        assert_eq!(resolved.pin(), Pin::Header(HeaderPin::X1));
    }

    // -- Rejections ---------------------------------------------------------

    #[test]
    fn a_bare_number_names_the_namespaces_it_is_ambiguous_between() {
        let msg = rejection("23");
        assert!(msg.contains("ambiguous"), "{msg}");
        assert!(msg.contains("image select pin"), "{msg}");
        assert!(msg.contains("X pin"), "{msg}");
        assert!(msg.contains("ROM socket pin"), "{msg}");
        // It must say what to type instead, using the number given.
        assert!(msg.contains("'gpio23'"), "{msg}");
        assert!(msg.contains("onerom inspect header"), "{msg}");
        // And it must not guess.
        assert!(!msg.contains("Assuming"), "{msg}");
    }

    #[test]
    fn address_lines_are_refused_with_a_reason_and_no_forecast() {
        for spec in ["a0", "a13", "A17"] {
            let msg = rejection(spec);
            assert!(msg.contains("address line"), "{spec}: {msg}");
            assert!(msg.contains("onerom inspect header"), "{spec}: {msg}");
            assert!(msg.contains("'gpio<N>'"), "{spec}: {msg}");
            // The message says what --pin takes and what to type instead. It
            // deliberately promises nothing about later releases in either
            // direction: whether these are ever accepted is not a decision an
            // error message gets to announce.
            for forecast in ["not yet", "yet supported", "now or", "later", "never"] {
                assert!(!msg.contains(forecast), "{spec} says '{forecast}': {msg}");
            }
        }
    }

    #[test]
    fn dedicated_pins_say_they_are_not_gpios() {
        for spec in ["run", "bootsel", "swclk", "swdio", "RUN", "BootSel"] {
            let msg = rejection(spec);
            assert!(msg.contains("is not a GPIO"), "{spec}: {msg}");
            // These will never resolve, so they must not be described as
            // merely unimplemented.
            assert!(!msg.contains("not yet supported"), "{spec}: {msg}");
        }
    }

    #[test]
    fn unrecognised_pins_list_the_header_pins() {
        for spec in ["banana", "pin23", "sel_h", "sel_", "d3", "cs1"] {
            let msg = rejection(spec);
            assert!(msg.contains("'x1'"), "{spec}: {msg}");
        }
        assert!(rejection("sel_h").contains("'sel_<letter>'"));
        assert!(!rejection("sel_h").contains("sel_e"));
    }

    #[test]
    fn a_malformed_gpio_name_says_what_is_missing() {
        for spec in ["gpio", "gpiox", "gpio1a", "gpio-1", "gpio 1", "gpio0x10"] {
            let msg = rejection(spec);
            assert!(msg.contains("gpio"), "{spec}: {msg}");
        }
        assert!(
            rejection("gpio").contains("must be followed by a GPIO number"),
            "{}",
            rejection("gpio")
        );
    }

    #[test]
    fn a_gpio_number_too_large_for_a_u8_is_rejected() {
        let msg = rejection("gpio256");
        assert!(msg.contains("out of range"), "{msg}");
        assert!(msg.contains("0 to 255"), "{msg}");
    }

    #[test]
    fn an_empty_pin_is_rejected() {
        assert!(rejection("").contains("No pin was provided"));
        assert!(rejection("   ").contains("No pin was provided"));
    }

    #[test]
    fn every_rejection_quotes_what_was_typed() {
        for spec in ["23", "a17", "run", "banana", "gpio", "GPIO256"] {
            let msg = rejection(spec);
            assert!(msg.contains(spec), "{spec}: {msg}");
        }
    }

    #[test]
    fn every_board_resolves_every_pin_it_reports() {
        // The pin list an error message offers must itself be resolvable, on
        // every board this build knows - otherwise the advice is wrong
        // somewhere.
        for b in onerom_config::hw::BOARDS {
            for header_pin in HeaderPin::all_on(&b) {
                let pin = parse_pin(&header_pin.to_string())
                    .unwrap_or_else(|e| panic!("{}: {header_pin}: {e}", b.name()));
                pin.resolve(Some(&b))
                    .unwrap_or_else(|e| panic!("{}: {header_pin}: {e}", b.name()));
            }
        }
    }
}
