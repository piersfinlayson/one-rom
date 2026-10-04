// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! MCU GPIOs and header pins.
//!
//! A pin is written as an MCU GPIO (`gpio23`) or a header pin (`sel_a`,
//! `x1`). The CLI's `--pin` and the config key `reserved_pins` use this syntax.
//!
//! ## Parsing and resolution are separate steps
//!
//! The GPIO wired to a header pin depends on the board. [`parse_pin`] doesn't
//! take a board and returns a [`Pin`], either an MCU GPIO or a [`Pad`].
//! [`Pin::resolve`] turns it into a [`ResolvedPin`] once the board is known.
//! That is the only type with a GPIO number.
//!
//! Resolution reads [`Board::sel_pins`], [`Board::pin_x1`] and
//! [`Board::pin_x2`], not [`jumper_header`](Board::jumper_header). Those exist
//! for every board, including Ice boards without a header description.
//!
//! ## Why a bare number is an error
//!
//! `23` could be an MCU GPIO or a ROM socket pin. Driving the wrong pin can't
//! be undone, so a bare number is [`PinError::BareNumber`].
//!
//! ## Why address lines are errors
//!
//! Only image select pins and X pins are header pins here. Accepting `a17`
//! would invite `a11` or `d3`, which aren't header pins. `a<N>` still fails
//! with its own error, [`PinError::AddressPad`].
//!
//! ROM socket pins don't have a syntax.

use core::fmt;

use crate::hw::Board;

/// The board pin arrays' value for a missing pin.
const NO_PIN: u8 = 255;

/// The most image select pins a board can have.
///
/// An assertion in onerom-metadata checks it equals the firmware's
/// `MAX_IMG_SEL_PINS`.
pub const MAX_SELECT_PADS: u8 = 7;

/// A header pin with an MCU GPIO of its own.
///
/// Address lines are left out. See the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pad {
    /// An image select pin, indexed from 0 for `sel_a`.
    Select(u8),
    /// The X1 pin.
    X1,
    /// The X2 pin.
    X2,
}

impl Pad {
    /// The MCU GPIO wired to this pin on `board`, `None` where the board
    /// doesn't have the pin.
    pub fn gpio_on(&self, board: &Board) -> Option<u8> {
        let gpio = match self {
            Pad::Select(index) => board.sel_pins().get(*index as usize).copied()?,
            Pad::X1 => board.pin_x1(),
            Pad::X2 => board.pin_x2(),
        };
        (gpio != NO_PIN).then_some(gpio)
    }

    /// Whether `gpio` is wired to this pin on `board`.
    ///
    /// An X pin can be wired to two GPIOs.
    pub fn has_gpio_on(&self, board: &Board, gpio: u8) -> bool {
        if gpio == NO_PIN {
            return false;
        }
        let x_pin = match self {
            Pad::Select(_) => return self.gpio_on(board) == Some(gpio),
            Pad::X1 => 1,
            Pad::X2 => 2,
        };
        self.gpio_on(board) == Some(gpio) || board.gpios_for_x_pin(x_pin).contains(&gpio)
    }

    /// Each pin `board` has. Image select pins come first in order, then X1
    /// and X2.
    pub fn all_on(board: &Board) -> impl Iterator<Item = Pad> + '_ {
        (0..board.sel_pins().len() as u8)
            .map(Pad::Select)
            .chain([Pad::X1, Pad::X2])
            .filter(|pad| pad.gpio_on(board).is_some())
    }

    /// The pin's silkscreen label, for example `SEL_A`.
    ///
    /// [`Display`](fmt::Display) writes the form a user types, for example
    /// `sel_a`.
    pub fn silkscreen(self) -> Silkscreen {
        Silkscreen(self)
    }
}

impl fmt::Display for Pad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pad::Select(index) => write!(f, "sel_{}", select_letter(*index, b'a')),
            Pad::X1 => f.write_str("x1"),
            Pad::X2 => f.write_str("x2"),
        }
    }
}

/// A pin's silkscreen label, from [`Pad::silkscreen`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Silkscreen(Pad);

impl fmt::Display for Silkscreen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Pad::Select(index) => write!(f, "SEL_{}", select_letter(index, b'A')),
            Pad::X1 => f.write_str("X1"),
            Pad::X2 => f.write_str("X2"),
        }
    }
}

/// The letter of image select pin `index`, counting from `a`.
fn select_letter(index: u8, a: u8) -> char {
    match a.checked_add(index) {
        Some(letter) if index < 26 => letter as char,
        _ => '?',
    }
}

/// A pin as written, before a board is known.
///
/// [`Pin::resolve`] returns a [`ResolvedPin`] with its GPIO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pin {
    /// An MCU GPIO, written `gpioN`.
    ///
    /// The number isn't checked against the device's GPIO count.
    Gpio(u8),

    /// A header pin.
    Pad(Pad),
}

impl Pin {
    /// Resolve this pin to its MCU GPIO.
    ///
    /// A header pin requires `board`. A `gpioN` pin doesn't.
    pub fn resolve(&self, board: Option<&Board>) -> Result<ResolvedPin, ResolveError> {
        let gpio = match self {
            Pin::Gpio(gpio) => *gpio,
            Pin::Pad(pad) => {
                let Some(board) = board else {
                    return Err(ResolveError::NoBoard { pad: *pad });
                };
                pad.gpio_on(board).ok_or(ResolveError::NoSuchPad {
                    pad: *pad,
                    board: *board,
                })?
            }
        };

        Ok(ResolvedPin { pin: *self, gpio })
    }

    /// The header pin this pin identifies on `board`.
    ///
    /// A `gpioN` pin identifies the header pin wired to that GPIO. `None` where
    /// the board doesn't have the header pin or the GPIO isn't wired to one.
    pub fn pad_on(&self, board: &Board) -> Option<Pad> {
        match self {
            Pin::Pad(pad) => pad.gpio_on(board).map(|_| *pad),
            Pin::Gpio(gpio) => Pad::all_on(board).find(|pad| pad.has_gpio_on(board, *gpio)),
        }
    }
}

impl fmt::Display for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pin::Gpio(gpio) => write!(f, "gpio{gpio}"),
            Pin::Pad(pad) => write!(f, "{pad}"),
        }
    }
}

impl serde::Serialize for Pin {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Pin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PinVisitor;

        impl serde::de::Visitor<'_> for PinVisitor {
            type Value = Pin;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a header pin, for example 'sel_a' or 'x1'")
            }

            fn visit_str<E>(self, v: &str) -> Result<Pin, E>
            where
                E: serde::de::Error,
            {
                parse_pin(v).map_err(|e| E::custom(format_args!("'{}': {e}", v.trim())))
            }
        }

        deserializer.deserialize_str(PinVisitor)
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Pin {
    fn schema_name() -> alloc::borrow::Cow<'static, str> {
        "Pin".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A header pin ('sel_<letter>', 'x1' or 'x2') or an MCU GPIO ('gpio<N>'). Case is ignored. 'sel-a' and 'sela' are also accepted.",
            "type": "string",
            "pattern": r"^\s*([Gg][Pp][Ii][Oo][0-9]+|[Ss][Ee][Ll][_-]?[A-Ga-g]|[Xx][12])\s*$"
        })
    }
}

/// A [`Pin`] resolved against a board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedPin {
    pin: Pin,
    gpio: u8,
}

impl ResolvedPin {
    /// The MCU GPIO this pin identifies.
    pub fn gpio(&self) -> u8 {
        self.gpio
    }

    /// The pin as the user typed it.
    pub fn pin(&self) -> Pin {
        self.pin
    }
}

impl fmt::Display for ResolvedPin {
    /// `gpio9` for a GPIO, `sel_a (gpio9)` for a header pin.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.pin {
            Pin::Gpio(gpio) => write!(f, "gpio{gpio}"),
            Pin::Pad(pad) => write!(f, "{pad} (gpio{})", self.gpio),
        }
    }
}

/// The reason a pin didn't parse.
///
/// The text isn't stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PinError {
    /// The text is empty.
    Empty,
    /// `gpio` not followed by a number, for example `gpio1a`.
    MissingGpioNumber,
    /// A GPIO number too large for a `u8`.
    GpioOutOfRange,
    /// A bare number.
    BareNumber,
    /// An address line, for example `a17`.
    AddressPad,
    /// A dedicated MCU pin that isn't a GPIO, for example `run`.
    NotAGpio,
    /// Anything else.
    Unrecognised,
}

impl fmt::Display for PinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PinError::Empty => "no pin given",
            PinError::MissingGpioNumber => {
                "'gpio' must be followed by a GPIO number, for example 'gpio23'"
            }
            PinError::GpioOutOfRange => "GPIO numbers are 0 to 255",
            PinError::BareNumber => {
                "a bare number is ambiguous. Write an MCU GPIO as 'gpio<N>'"
            }
            PinError::AddressPad => "an address line is not a header pin",
            PinError::NotAGpio => "a dedicated MCU pin, not a GPIO",
            PinError::Unrecognised => {
                "not a pin. Write a header pin ('sel_<letter>', 'x1' or 'x2') or an MCU GPIO ('gpio<N>')"
            }
        })
    }
}

/// The reason a [`Pin`] didn't resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolveError {
    /// A header pin, and the board isn't known.
    NoBoard {
        /// The header pin.
        pad: Pad,
    },
    /// The board doesn't have the header pin.
    NoSuchPad {
        /// The header pin.
        pad: Pad,
        /// The board.
        board: Board,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::NoBoard { pad } => {
                write!(f, "'{pad}' is a header pin. Its GPIO depends on the board")
            }
            ResolveError::NoSuchPad { pad, board } => {
                write!(f, "board {} has no '{pad}' pin", board.name())
            }
        }
    }
}

/// Parse a pin.
///
/// Accepts `gpioN`, `sel_<letter>`, `x1` and `x2`, ignoring case and
/// surrounding whitespace. An image select pin may also be written `sel-a` or
/// `sela`.
///
/// Whether the board has the pin is checked by [`Pin::resolve`].
pub fn parse_pin(spec: &str) -> Result<Pin, PinError> {
    let name = spec.trim();

    if name.is_empty() {
        return Err(PinError::Empty);
    }

    if let Some(digits) = strip_prefix_ignore_case(name, "gpio") {
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(PinError::MissingGpioNumber);
        }
        return digits
            .parse::<u8>()
            .map(Pin::Gpio)
            .map_err(|_| PinError::GpioOutOfRange);
    }

    if let Some(pad) = parse_pad(name) {
        return Ok(Pin::Pad(pad));
    }

    if name.bytes().all(|b| b.is_ascii_digit()) {
        return Err(PinError::BareNumber);
    }

    if is_address_pad_name(name) {
        return Err(PinError::AddressPad);
    }

    if ["run", "bootsel", "swclk", "swdio"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
    {
        return Err(PinError::NotAGpio);
    }

    Err(PinError::Unrecognised)
}

fn strip_prefix_ignore_case<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let head = name.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &name[prefix.len()..])
}

/// Bare `a` isn't accepted. It is too easily confused with an address line.
fn parse_select_pad(name: &str) -> Option<Pad> {
    let rest = strip_prefix_ignore_case(name, "sel")?;
    let letter = rest
        .strip_prefix('_')
        .or_else(|| rest.strip_prefix('-'))
        .unwrap_or(rest);
    let [letter] = letter.as_bytes() else {
        return None;
    };
    let index = letter.to_ascii_lowercase().checked_sub(b'a')?;
    (index < MAX_SELECT_PADS).then_some(Pad::Select(index))
}

fn parse_pad(name: &str) -> Option<Pad> {
    if name.eq_ignore_ascii_case("x1") {
        Some(Pad::X1)
    } else if name.eq_ignore_ascii_case("x2") {
        Some(Pad::X2)
    } else {
        parse_select_pad(name)
    }
}

fn is_address_pad_name(name: &str) -> bool {
    strip_prefix_ignore_case(name, "a")
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Reserved header pins, as stored in the metadata header.
///
/// [`Display`](fmt::Display) lists them by silkscreen label, for example
/// `SEL_C, X1`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReservedPads {
    select: u8,
    x: u8,
}

impl ReservedPads {
    /// Nothing reserved.
    pub const fn new() -> Self {
        Self { select: 0, x: 0 }
    }

    /// From the metadata header's `reserved_sel_pins` and `reserved_x_pins`.
    pub const fn from_bits(select: u8, x: u8) -> Self {
        Self { select, x }
    }

    /// The metadata header's `reserved_sel_pins`.
    pub const fn select_bits(&self) -> u8 {
        self.select
    }

    /// The metadata header's `reserved_x_pins`.
    pub const fn x_bits(&self) -> u8 {
        self.x
    }

    pub const fn is_empty(&self) -> bool {
        self.select == 0 && self.x == 0
    }

    /// Reserve `pad`. Returns false for an image select pin past
    /// [`MAX_SELECT_PADS`].
    pub fn insert(&mut self, pad: Pad) -> bool {
        match pad {
            Pad::Select(index) if index < MAX_SELECT_PADS => self.select |= 1 << index,
            Pad::Select(_) => return false,
            Pad::X1 => self.x |= 1,
            Pad::X2 => self.x |= 2,
        }
        true
    }

    pub const fn contains(&self, pad: Pad) -> bool {
        match pad {
            Pad::Select(index) => index < MAX_SELECT_PADS && self.select & (1 << index) != 0,
            Pad::X1 => self.x & 1 != 0,
            Pad::X2 => self.x & 2 != 0,
        }
    }

    /// The reserved pins. Image select pins come first in order, then X1 and
    /// X2.
    pub fn pads(&self) -> impl Iterator<Item = Pad> + '_ {
        (0..MAX_SELECT_PADS)
            .map(Pad::Select)
            .chain([Pad::X1, Pad::X2])
            .filter(|pad| self.contains(*pad))
    }

    /// The image select pins read by the firmware on `board`, lowest bit
    /// first.
    ///
    /// The image select bits are assigned to the unreserved pins in order, so
    /// reserving SEL_C makes SEL_D bit 2.
    pub fn select_pads_read<'a>(&'a self, board: &'a Board) -> impl Iterator<Item = Pad> + 'a {
        (0..board.sel_pins().len() as u8)
            .map(Pad::Select)
            .filter(|pad| !self.contains(*pad))
    }
}

impl fmt::Display for ReservedPads {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, pad) in self.pads().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}", pad.silkscreen())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::string::ToString;
    use alloc::vec::Vec;

    use super::*;

    fn board(name: &str) -> Board {
        Board::try_from_str(name).unwrap_or_else(|| panic!("{name} is a known board"))
    }

    fn resolved(spec: &str, board: &Board) -> u8 {
        parse_pin(spec)
            .expect("parses")
            .resolve(Some(board))
            .expect("resolves")
            .gpio()
    }

    #[test]
    fn gpio_names_parse() {
        assert_eq!(parse_pin("gpio0"), Ok(Pin::Gpio(0)));
        assert_eq!(parse_pin("gpio23"), Ok(Pin::Gpio(23)));
        assert_eq!(parse_pin("gpio047"), Ok(Pin::Gpio(47)));
        assert_eq!(parse_pin("gpio255"), Ok(Pin::Gpio(255)));
        assert_eq!(parse_pin("GPIO23"), Ok(Pin::Gpio(23)));
        assert_eq!(parse_pin("  Gpio23  "), Ok(Pin::Gpio(23)));
    }

    #[test]
    fn pad_names_parse_in_every_spelling() {
        for spec in ["sel_a", "sel-a", "sela", "SEL_A", "Sel-A", "  sela  "] {
            assert_eq!(parse_pin(spec), Ok(Pin::Pad(Pad::Select(0))), "{spec}");
        }
        assert_eq!(parse_pin("sel_e"), Ok(Pin::Pad(Pad::Select(4))));
        assert_eq!(parse_pin("x1"), Ok(Pin::Pad(Pad::X1)));
        assert_eq!(parse_pin("X2"), Ok(Pin::Pad(Pad::X2)));
    }

    #[test]
    fn select_pads_stop_at_the_firmware_limit() {
        assert_eq!(parse_pin("sel_f"), Ok(Pin::Pad(Pad::Select(5))));
        assert_eq!(parse_pin("SEL-G"), Ok(Pin::Pad(Pad::Select(6))));
        assert_eq!(parse_pin("sel_h"), Err(PinError::Unrecognised));
        assert_eq!(MAX_SELECT_PADS, 7);
    }

    #[test]
    fn each_mistake_has_its_own_error() {
        for (spec, error) in [
            ("", PinError::Empty),
            ("   ", PinError::Empty),
            ("gpio", PinError::MissingGpioNumber),
            ("gpiox", PinError::MissingGpioNumber),
            ("gpio1a", PinError::MissingGpioNumber),
            ("gpio-1", PinError::MissingGpioNumber),
            ("gpio 1", PinError::MissingGpioNumber),
            ("gpio256", PinError::GpioOutOfRange),
            ("23", PinError::BareNumber),
            ("a0", PinError::AddressPad),
            ("A17", PinError::AddressPad),
            ("run", PinError::NotAGpio),
            ("BootSel", PinError::NotAGpio),
            ("swdio", PinError::NotAGpio),
            ("a", PinError::Unrecognised),
            ("banana", PinError::Unrecognised),
            ("sel_", PinError::Unrecognised),
            ("d3", PinError::Unrecognised),
        ] {
            assert_eq!(parse_pin(spec), Err(error), "{spec:?}");
        }
    }

    #[test]
    fn a_pin_displays_as_it_is_typed() {
        assert_eq!(Pin::Gpio(23).to_string(), "gpio23");
        assert_eq!(Pin::Pad(Pad::Select(0)).to_string(), "sel_a");
        assert_eq!(Pin::Pad(Pad::Select(6)).to_string(), "sel_g");
        assert_eq!(Pin::Pad(Pad::X1).to_string(), "x1");
        assert_eq!(Pad::X2.to_string(), "x2");
    }

    #[test]
    fn a_pad_has_a_silkscreen_name() {
        assert_eq!(Pad::Select(2).silkscreen().to_string(), "SEL_C");
        assert_eq!(Pad::X1.silkscreen().to_string(), "X1");
        assert_eq!(Pad::X2.silkscreen().to_string(), "X2");
    }

    #[test]
    fn every_pad_of_a_board_resolves() {
        let b = board("fire-24-f");
        assert_eq!(resolved("sel_a", &b), 26);
        assert_eq!(resolved("sel_d", &b), 24);
        assert_eq!(resolved("x1", &b), 9);
        assert_eq!(resolved("x2", &b), 8);
        let pads: Vec<Pad> = Pad::all_on(&b).collect();
        assert_eq!(
            pads,
            [
                Pad::Select(0),
                Pad::Select(1),
                Pad::Select(2),
                Pad::Select(3),
                Pad::X1,
                Pad::X2
            ]
        );
    }

    #[test]
    fn a_pad_the_board_lacks_does_not_resolve() {
        let b = board("fire-32-a");
        let x1 = parse_pin("x1").unwrap();
        assert_eq!(
            x1.resolve(Some(&b)),
            Err(ResolveError::NoSuchPad {
                pad: Pad::X1,
                board: b
            })
        );
        assert_eq!(
            parse_pin("sel_c")
                .unwrap()
                .resolve(Some(&board("fire-28-a"))),
            Err(ResolveError::NoSuchPad {
                pad: Pad::Select(2),
                board: board("fire-28-a")
            })
        );
        assert_eq!(
            x1.resolve(None),
            Err(ResolveError::NoBoard { pad: Pad::X1 })
        );
        // A GPIO resolves without a board.
        let gpio = parse_pin("gpio23").unwrap().resolve(None).unwrap();
        assert_eq!((gpio.gpio(), gpio.pin()), (23, Pin::Gpio(23)));
    }

    #[test]
    fn a_resolved_pad_shows_both_names() {
        let b = board("fire-24-f");
        let pin = parse_pin("x1").unwrap().resolve(Some(&b)).unwrap();
        assert_eq!(pin.to_string(), "x1 (gpio9)");
        assert_eq!(pin.pin(), Pin::Pad(Pad::X1));
    }

    #[test]
    fn a_gpio_identifies_the_pad_behind_it() {
        let b = board("fire-24-f");
        assert_eq!(Pin::Gpio(25).pad_on(&b), Some(Pad::Select(2)));
        assert_eq!(Pin::Gpio(9).pad_on(&b), Some(Pad::X1));
        assert_eq!(Pin::Gpio(23).pad_on(&b), None);
        assert_eq!(Pin::Pad(Pad::X2).pad_on(&b), Some(Pad::X2));
        assert_eq!(Pin::Pad(Pad::Select(4)).pad_on(&b), None);
        assert_eq!(Pin::Pad(Pad::X1).pad_on(&board("fire-40-a")), None);
    }

    #[test]
    fn reserved_pads_hold_the_metadata_bits() {
        let mut reserved = ReservedPads::new();
        assert!(reserved.is_empty());
        assert!(reserved.insert(Pad::Select(2)));
        assert!(reserved.insert(Pad::X2));
        assert!(!reserved.insert(Pad::Select(7)));
        assert_eq!((reserved.select_bits(), reserved.x_bits()), (0b100, 0b10));
        assert_eq!(reserved, ReservedPads::from_bits(0b100, 0b10));
        assert!(reserved.contains(Pad::Select(2)));
        assert!(!reserved.contains(Pad::X1));
        assert_eq!(reserved.to_string(), "SEL_C, X2");
        assert_eq!(ReservedPads::new().to_string(), "");
    }

    #[test]
    fn the_remaining_select_pads_take_the_bits_in_order() {
        let b = board("fire-24-f");
        let mut reserved = ReservedPads::new();
        reserved.insert(Pad::Select(2));
        let read: Vec<Pad> = reserved.select_pads_read(&b).collect();
        assert_eq!(read, [Pad::Select(0), Pad::Select(1), Pad::Select(3)]);
    }

    #[test]
    fn a_pin_round_trips_through_serde() {
        let pins: Vec<Pin> = serde_json::from_str(r#"["SEL-C", " x1 ", "GPIO25"]"#).unwrap();
        assert_eq!(
            pins,
            [Pin::Pad(Pad::Select(2)), Pin::Pad(Pad::X1), Pin::Gpio(25)]
        );
        assert_eq!(
            serde_json::to_string(&pins).unwrap(),
            r#"["sel_c","x1","gpio25"]"#
        );
        let error = serde_json::from_str::<Pin>(r#""a17""#).unwrap_err();
        assert!(error.to_string().contains("'a17'"), "{error}");
    }

    #[test]
    fn parsing_does_not_allocate() {
        let b = board("fire-24-f");
        let before = counting::allocations();
        for spec in [
            "sel_c", "SEL-G", "x1", "gpio25", "", "gpio", "gpio999", "23", "a17", "run", "banana",
        ] {
            match parse_pin(spec) {
                Ok(pin) => {
                    let _ = pin.resolve(Some(&b));
                    let _ = pin.pad_on(&b);
                }
                Err(error) => {
                    let mut buf = counting::FixedBuf::new();
                    fmt::write(&mut buf, format_args!("{error}")).unwrap();
                }
            }
        }
        let mut reserved = ReservedPads::new();
        reserved.insert(Pad::Select(2));
        let mut buf = counting::FixedBuf::new();
        fmt::write(&mut buf, format_args!("{reserved}")).unwrap();
        assert_eq!(counting::allocations(), before);

        // The count does move when something allocates.
        let _ = core::hint::black_box(Vec::<u8>::with_capacity(8));
        assert!(counting::allocations() > before);
    }

    /// A global allocator counting allocations made on the current thread, so
    /// tests running alongside don't disturb the count.
    mod counting {
        extern crate std;

        use core::fmt;
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        std::thread_local! {
            static COUNT: Cell<usize> = const { Cell::new(0) };
        }

        struct Counting;

        unsafe impl GlobalAlloc for Counting {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                let _ = COUNT.try_with(|count| count.set(count.get() + 1));
                unsafe { System.alloc(layout) }
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                unsafe { System.dealloc(ptr, layout) }
            }
        }

        #[global_allocator]
        static ALLOCATOR: Counting = Counting;

        /// Allocations made on this thread so far.
        pub fn allocations() -> usize {
            COUNT.with(Cell::get)
        }

        /// A `fmt::Write` into a fixed array.
        pub struct FixedBuf {
            buf: [u8; 256],
            len: usize,
        }

        impl FixedBuf {
            pub fn new() -> Self {
                Self {
                    buf: [0; 256],
                    len: 0,
                }
            }
        }

        impl fmt::Write for FixedBuf {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                let end = self.len + s.len();
                self.buf
                    .get_mut(self.len..end)
                    .ok_or(fmt::Error)?
                    .copy_from_slice(s.as_bytes());
                self.len = end;
                Ok(())
            }
        }
    }
}
