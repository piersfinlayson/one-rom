// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `--pin` decoding.
//!
//! A pin selector names one MCU GPIO, either directly (`gpio23`) or through a
//! header pad that is wired to one (`sel_a`, `x1`). This module lives in the
//! library rather than the binary because more than one command takes a `--pin`,
//! and Studio is being moved onto the CLI library.
//!
//! ## Parsing and resolution are separate steps
//!
//! Clap's `value_parser` runs before any device is opened, so it cannot see the
//! board - and a pad name is meaningless without one, because which GPIO sits
//! behind `sel_a` is a fact about the board, not about the name. So
//! [`parse_pin`] is board-free and yields a [`Pin`], which is either an MCU GPIO
//! or a named [`Pad`]; the command handler then calls [`Pin::resolve`] once the
//! board is known and gets a [`ResolvedPin`], which is the only type that can
//! answer with a GPIO number.
//!
//! Resolution reads the board's *electrical* pin arrays
//! ([`Board::sel_pins`](onerom_config::hw::Board::sel_pins),
//! [`Board::pin_x1`](onerom_config::hw::Board::pin_x1),
//! [`Board::pin_x2`](onerom_config::hw::Board::pin_x2)), not its
//! [`jumper_header`](onerom_config::hw::Board::jumper_header). Those arrays
//! exist for every board, including the non-USB Ice boards whose header layout
//! is deliberately uncharacterised, and they are the real electrical facts: a
//! pad name must resolve on any board that has the pad.
//!
//! ## Why a bare number is rejected
//!
//! `--pin 23` cannot be resolved without guessing which namespace the user
//! meant. `23` is a plausible MCU GPIO, a plausible ROM socket leg, and - on a
//! board whose pads are silkscreened by role rather than by GPIO - a plausible
//! reading of neither. Guessing one and driving a pin is not a recoverable
//! mistake, so a bare number is an error whose message names the namespaces
//! instead. Accepting pad names does not remove that ambiguity; it sharpens it.
//!
//! ## Why the broken-out address pads are not accepted
//!
//! `--pin` addresses MCU GPIOs and the pads a wire can physically reach: the
//! image-select pads and X1/X2. A broken-out address line is a ROM signal rather
//! than one of those, and accepting `a17` would invite `a11` or `d3`, which have
//! no pad at all. The names are still recognised, so that a user who types one
//! is told why rather than told it is meaningless. The error deliberately makes
//! no claim either way about whether a later release might accept them - that
//! is not an error message's business.
//!
//! No syntax is reserved for the ROM socket legs. That namespace has not been
//! designed and this module must not pre-empt it.

use crate::Error;
use onerom_config::hw::Board;

/// Where to send a user who needs to know which GPIO is behind a header pad.
const HEADER_HINT: &str =
    "Run 'onerom inspect header' to see which GPIO is behind each header pad.";

/// What `--pin` accepts, as one sentence for an error message.
const NAMESPACE_HINT: &str = "--pin takes an MCU GPIO, written 'gpio<N>' - for example 'gpio23' - or a header pad name: 'sel_a'..'sel_e', 'x1' or 'x2'.";

/// The board pin arrays' "no such pin" sentinel. No real GPIO number reaches it.
const NO_PIN: u8 = 255;

/// The highest image-select pad this module knows how to spell, as an index from
/// `sel_a`. No board has more than five; a board with fewer is rejected at
/// resolution, naming the pads it does have.
const MAX_SELECT_PAD: u8 = 4;

/// A header pad a user can put a wire on.
///
/// Only pads that carry an MCU GPIO of their own appear here. Power, ground,
/// SWCLK, SWDIO, RUN and BOOTSEL do not, and the broken-out address pads are
/// deliberately excluded - see the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pad {
    /// An image-select pad, indexed from 0 for `sel_a`.
    Select(u8),
    /// The X1 pad.
    X1,
    /// The X2 pad.
    X2,
}

impl Pad {
    /// The MCU GPIO behind this pad on `board`, or `None` if the board has no
    /// such pad.
    fn gpio_on(&self, board: &Board) -> Option<u8> {
        let gpio = match self {
            Pad::Select(index) => board.sel_pins().get(*index as usize).copied()?,
            Pad::X1 => board.pin_x1(),
            Pad::X2 => board.pin_x2(),
        };
        (gpio != NO_PIN).then_some(gpio)
    }

    /// Every pad `board` has, in the spelling `--pin` accepts.
    fn all_on(board: &Board) -> Vec<String> {
        let mut pads: Vec<String> = (0..board.sel_pins().len())
            .map(|index| Pad::Select(index as u8).to_string())
            .collect();
        for pad in [Pad::X1, Pad::X2] {
            if pad.gpio_on(board).is_some() {
                pads.push(pad.to_string());
            }
        }
        pads
    }
}

impl std::fmt::Display for Pad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Pad::Select(index) => write!(f, "sel_{}", (b'a' + index) as char),
            Pad::X1 => write!(f, "x1"),
            Pad::X2 => write!(f, "x2"),
        }
    }
}

/// A pin named on the command line, before any board is known.
///
/// [`Pin::resolve`] turns one of these into a [`ResolvedPin`], which is the only
/// type that carries a GPIO number. There is deliberately no way to get a GPIO
/// out of a `Pin`: a [`Pad`] does not have one until a board says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pin {
    /// An MCU GPIO, written `gpioN`.
    ///
    /// The number is not range-checked here - how many GPIOs a device has is
    /// read from it (`num_gpios` in its capabilities), never assumed.
    Gpio(u8),

    /// A header pad, written by name.
    Pad(Pad),
}

impl Pin {
    /// Resolve this pin to the MCU GPIO it names.
    ///
    /// `board` is what the caller could work out - from the connected device, or
    /// from a `--board` override - and may be `None`. A `gpioN` pin resolves
    /// without one; a pad does not, and says so.
    ///
    /// Fails when a pad was named and no board is known, or when the board has
    /// no such pad.
    pub fn resolve(&self, board: Option<&Board>) -> Result<ResolvedPin, Error> {
        let gpio = match self {
            Pin::Gpio(gpio) => *gpio,
            Pin::Pad(pad) => {
                let Some(board) = board else {
                    return Err(Error::InvalidPin(
                        pad.to_string(),
                        format!(
                            "'{pad}' is a header pad, and which GPIO sits behind a pad depends on the board.\n  \
                             This One ROM's board type could not be determined.\n  \
                             Pass --board <BOARD>, or name the MCU GPIO directly as 'gpio<N>'."
                        ),
                    ));
                };
                pad.gpio_on(board).ok_or_else(|| {
                    Error::InvalidPin(
                        pad.to_string(),
                        format!(
                            "Board {} has no '{pad}' pad.\n  Its header pads are: {}.\n  {HEADER_HINT}",
                            board.name(),
                            Pad::all_on(board).join(", "),
                        ),
                    )
                })?
            }
        };

        Ok(ResolvedPin { pin: *self, gpio })
    }
}

impl std::fmt::Display for Pin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Pin::Gpio(gpio) => write!(f, "gpio{gpio}"),
            Pin::Pad(pad) => write!(f, "{pad}"),
        }
    }
}

/// A [`Pin`] that has been resolved against a board.
///
/// This is what the commands that actually drive or query a pin take, so that
/// "which GPIO is this?" is answerable without a board, an `Option` or a panic:
/// the question was settled once, where the board was known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedPin {
    pin: Pin,
    gpio: u8,
}

impl ResolvedPin {
    /// The MCU GPIO this pin names.
    pub fn gpio(&self) -> u8 {
        self.gpio
    }

    /// The pin as the user named it.
    pub fn pin(&self) -> Pin {
        self.pin
    }
}

impl std::fmt::Display for ResolvedPin {
    /// `gpio9` for a GPIO named directly, `sel_a (gpio9)` for a pad - a user who
    /// named a pad still wants to see which GPIO it turned out to be.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.pin {
            Pin::Gpio(gpio) => write!(f, "gpio{gpio}"),
            Pin::Pad(pad) => write!(f, "{pad} (gpio{})", self.gpio),
        }
    }
}

/// An image-select pad name: `sel_a`, `sel-a` or `sela`, for `a`..`e`.
///
/// Bare `a`..`e` is deliberately not accepted: it is too terse for a canonical
/// name that is already five characters, and it reads badly next to the
/// broken-out address-line names.
fn parse_select_pad(name: &str) -> Option<Pad> {
    let rest = name.strip_prefix("sel")?;
    let letter = rest
        .strip_prefix('_')
        .or_else(|| rest.strip_prefix('-'))
        .unwrap_or(rest);
    let [letter] = letter.as_bytes() else {
        return None;
    };
    let index = letter.checked_sub(b'a')?;
    (index <= MAX_SELECT_PAD).then_some(Pad::Select(index))
}

/// A pad name that resolves to a GPIO.
fn parse_pad(name: &str) -> Option<Pad> {
    match name {
        "x1" => Some(Pad::X1),
        "x2" => Some(Pad::X2),
        _ => parse_select_pad(name),
    }
}

/// `a<N>` - a broken-out address pad. Recognised so it can be refused with a
/// reason; see the [module documentation](self).
fn is_address_pad_name(name: &str) -> bool {
    name.strip_prefix('a')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Names of pins that are not GPIOs at all and never will be.
fn is_not_a_gpio_name(name: &str) -> bool {
    matches!(name, "run" | "bootsel" | "swclk" | "swdio")
}

/// Decode a `--pin` value.
///
/// Accepts `gpioN` and the header pad names `sel_a`..`sel_e`, `x1` and `x2`, all
/// case-insensitively; the image-select pads also accept `sel-a` and `sela`.
/// Everything else is an error whose message teaches the namespace rather than
/// guessing at what was meant.
///
/// Whether the board actually has a named pad is **not** decided here - see the
/// [module documentation](self) for why that is [`Pin::resolve`]'s job.
pub fn parse_pin(spec: &str) -> Result<Pin, Error> {
    let trimmed = spec.trim();
    let name = trimmed.to_ascii_lowercase();

    let invalid = |detail: String| Err(Error::InvalidPin(trimmed.to_string(), detail));

    if name.is_empty() {
        return invalid(format!(
            "No pin given.\n  {NAMESPACE_HINT}\n  {HEADER_HINT}"
        ));
    }

    if let Some(digits) = name.strip_prefix("gpio") {
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return invalid(
                "'gpio' must be followed by a GPIO number - for example 'gpio23'.".to_string(),
            );
        }
        // Parsed as a u8 only. The real upper bound is the device's num_gpios
        // (30 on an RP2350A, 48 on an RP2350B), which is read from the device,
        // so this must not second-guess it with a constant of its own.
        return match digits.parse::<u8>() {
            Ok(gpio) => Ok(Pin::Gpio(gpio)),
            Err(_) => invalid(format!(
                "GPIO number '{digits}' is out of range - GPIO numbers are 0 to 255."
            )),
        };
    }

    if let Some(pad) = parse_pad(&name) {
        return Ok(Pin::Pad(pad));
    }

    if name.bytes().all(|b| b.is_ascii_digit()) {
        return invalid(format!(
            "A bare number is ambiguous: it could be an MCU GPIO, an image-select pad, an X pad or a ROM socket pin.\n  Write an MCU GPIO as 'gpio{name}'.\n  {HEADER_HINT}"
        ));
    }

    if is_address_pad_name(&name) {
        return invalid(format!(
            "'{name}' is a broken-out address line, which --pin does not accept.\n  \
             --pin takes an MCU GPIO ('gpio<N>') or a header pad ('sel_a'..'sel_e', 'x1', 'x2').\n  \
             Use the MCU GPIO behind the pad, written 'gpio<N>'.\n  \
             {HEADER_HINT}"
        ));
    }

    if is_not_a_gpio_name(&name) {
        return invalid(format!(
            "'{name}' is not a GPIO - it is a dedicated MCU pin and cannot be driven.\n  {NAMESPACE_HINT}"
        ));
    }

    invalid(format!(
        "Unrecognised pin.\n  {NAMESPACE_HINT}\n  {HEADER_HINT}"
    ))
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
            Err(e) => e.to_string(),
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
    fn pad_names_parse_in_every_accepted_spelling() {
        for spec in ["sel_a", "sel-a", "sela", "SEL_A", "Sel-A", "  sela  "] {
            assert_eq!(
                parse_pin(spec).expect("parses"),
                Pin::Pad(Pad::Select(0)),
                "{spec}"
            );
        }
        assert_eq!(
            parse_pin("sel_e").expect("parses"),
            Pin::Pad(Pad::Select(4))
        );
        assert_eq!(parse_pin("x1").expect("parses"), Pin::Pad(Pad::X1));
        assert_eq!(parse_pin("X2").expect("parses"), Pin::Pad(Pad::X2));
    }

    #[test]
    fn a_bare_select_letter_is_not_a_pad_name() {
        // Too terse to be canonical, and it reads badly next to 'a17'.
        for spec in ["a", "b", "c", "d", "e"] {
            let msg = rejection(spec);
            assert!(msg.contains("Unrecognised pin"), "{spec}: {msg}");
        }
    }

    #[test]
    fn a_pin_displays_as_it_is_written() {
        assert_eq!(Pin::Gpio(23).to_string(), "gpio23");
        assert_eq!(Pin::Pad(Pad::Select(0)).to_string(), "sel_a");
        assert_eq!(Pin::Pad(Pad::Select(4)).to_string(), "sel_e");
        assert_eq!(Pin::Pad(Pad::X1).to_string(), "x1");
        assert_eq!(Pin::Pad(Pad::X2).to_string(), "x2");
    }

    // -- Resolution against real board metadata -----------------------------

    #[test]
    fn a_four_select_board_with_x_pads_resolves_every_pad() {
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
    fn a_board_without_x_pads_says_so_and_names_what_it_has() {
        // fire-32-a: sel = [38, 39, 36, 37], no X pads.
        let b = board("fire-32-a");
        assert_eq!(b.pin_x1(), NO_PIN);
        let msg = resolve_rejection("x1", Some(&b));
        assert!(msg.contains("has no 'x1' pad"), "{msg}");
        assert!(msg.contains("fire-32-a"), "{msg}");
        assert!(msg.contains("sel_a, sel_b, sel_c, sel_d"), "{msg}");
        assert!(!msg.contains("x1,"), "{msg}");
        assert!(msg.contains("onerom inspect header"), "{msg}");
        // The select pads it does have still resolve.
        assert_eq!(resolved("sel_d", &b), b.sel_pins()[3]);
    }

    #[test]
    fn a_board_with_fewer_select_pads_says_so() {
        // fire-28-a has two image-select pins and no X pads.
        let b = board("fire-28-a");
        assert_eq!(b.sel_pins().len(), 2);
        let msg = resolve_rejection("sel_c", Some(&b));
        assert!(msg.contains("has no 'sel_c' pad"), "{msg}");
        assert!(msg.contains("Its header pads are: sel_a, sel_b."), "{msg}");
        // sel_e on a four-select board is the same shape of answer.
        let four = board("fire-24-f");
        let msg = resolve_rejection("sel_e", Some(&four));
        assert!(msg.contains("has no 'sel_e' pad"), "{msg}");
        assert!(msg.contains("sel_a, sel_b, sel_c, sel_d, x1, x2"), "{msg}");
    }

    #[test]
    fn a_pad_without_a_board_points_at_the_board_option() {
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
    fn a_resolved_pad_shows_both_names() {
        let b = board("fire-24-f");
        let resolved = parse_pin("x1")
            .expect("parses")
            .resolve(Some(&b))
            .expect("resolves");
        assert_eq!(resolved.to_string(), format!("x1 (gpio{})", b.pin_x1()));
        assert_eq!(resolved.pin(), Pin::Pad(Pad::X1));
    }

    // -- Rejections ---------------------------------------------------------

    #[test]
    fn a_bare_number_names_the_namespaces_it_is_ambiguous_between() {
        let msg = rejection("23");
        assert!(msg.contains("ambiguous"), "{msg}");
        assert!(msg.contains("image-select pad"), "{msg}");
        assert!(msg.contains("X pad"), "{msg}");
        assert!(msg.contains("ROM socket pin"), "{msg}");
        // It must say what to type instead, using the number given.
        assert!(msg.contains("'gpio23'"), "{msg}");
        assert!(msg.contains("onerom inspect header"), "{msg}");
        // And it must not guess.
        assert!(!msg.contains("Assuming"), "{msg}");
    }

    #[test]
    fn address_pad_names_are_refused_with_a_reason_and_no_forecast() {
        for spec in ["a0", "a13", "A17"] {
            let msg = rejection(spec);
            assert!(msg.contains("broken-out address line"), "{spec}: {msg}");
            assert!(msg.contains("does not accept"), "{spec}: {msg}");
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
    fn unrecognised_names_teach_the_namespace() {
        for spec in [
            "banana", "pin23", "sel_f", "sel_", "gpio-1", "gpio 23", "d3", "cs1",
        ] {
            let msg = rejection(spec);
            assert!(
                msg.contains("gpio<N>") || msg.contains("gpio23"),
                "{spec}: {msg}"
            );
        }
        // The generic message names the pad namespace too, so 'sel_f' is told
        // how far the select pads go.
        assert!(rejection("sel_f").contains("'sel_a'..'sel_e'"));
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
        assert!(rejection("").contains("No pin given"));
        assert!(rejection("   ").contains("No pin given"));
    }

    #[test]
    fn every_rejection_quotes_what_was_typed() {
        for spec in ["23", "a17", "run", "banana", "gpio", "GPIO256"] {
            let msg = rejection(spec);
            assert!(msg.contains(spec), "{spec}: {msg}");
        }
    }

    #[test]
    fn every_board_resolves_every_pad_it_reports() {
        // The pad list an error message offers must itself be resolvable, on
        // every board this build knows - otherwise the advice is wrong
        // somewhere.
        for b in onerom_config::hw::BOARDS {
            for pad in Pad::all_on(&b) {
                let pin = parse_pin(&pad).unwrap_or_else(|e| panic!("{}: {pad}: {e}", b.name()));
                pin.resolve(Some(&b))
                    .unwrap_or_else(|e| panic!("{}: {pad}: {e}", b.name()));
            }
        }
    }
}
