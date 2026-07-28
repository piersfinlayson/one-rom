// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `--pin` decoding.
//!
//! A pin selector names one MCU GPIO. Today the only accepted spelling is
//! `gpioN`, but `--pin` is the seam through which the header pad names
//! (`sel_a`, `x1`, …) will later resolve, so this module lives in the library
//! rather than the binary: more than one command takes a `--pin`, and Studio is
//! being moved onto the CLI library.
//!
//! ## Why a bare number is rejected
//!
//! `--pin 23` cannot be resolved without guessing which namespace the user
//! meant. `23` is a plausible MCU GPIO, a plausible ROM socket leg, and - on a
//! board whose pads are silkscreened by role rather than by GPIO - a plausible
//! reading of neither. Guessing one and driving a pin is not a recoverable
//! mistake, so a bare number is an error whose message names the namespaces
//! instead.
//!
//! Names that will later resolve to a GPIO are recognised and rejected with a
//! message that says so, rather than falling through to "unrecognised": a user
//! who types `--pin x1` has the right idea and needs pointing at `onerom
//! inspect header`, not telling their input is meaningless.
//!
//! No syntax is reserved for the ROM socket legs. That namespace has not been
//! designed and this module must not pre-empt it.

use crate::Error;

/// Where to send a user who needs to know which GPIO is behind a header pad.
const HEADER_HINT: &str =
    "Run 'onerom inspect header' to see which GPIO is behind each header pad.";

/// A pin named on the command line.
///
/// Non-exhaustive because the header pad names (§6.2 of the GPIO control
/// design) will add variants that resolve through a `&Board`; `Gpio` needs no
/// board and so is all there is today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pin {
    /// An MCU GPIO, written `gpioN`.
    ///
    /// The number is not range-checked here - how many GPIOs a device has is
    /// read from it (`num_gpios` in its capabilities), never assumed.
    Gpio(u8),
}

impl Pin {
    /// The MCU GPIO this pin resolves to.
    pub fn gpio(&self) -> u8 {
        match self {
            Pin::Gpio(gpio) => *gpio,
        }
    }
}

impl std::fmt::Display for Pin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Pin::Gpio(gpio) => write!(f, "gpio{gpio}"),
        }
    }
}

/// Names of pads that will later resolve to a GPIO, but do not yet.
fn is_deferred_pad_name(name: &str) -> bool {
    // sel_a..sel_e - the image-select pads.
    if let Some(letter) = name.strip_prefix("sel_")
        && letter.len() == 1
        && matches!(letter.as_bytes()[0], b'a'..=b'e')
    {
        return true;
    }

    // The two X pads.
    if name == "x1" || name == "x2" {
        return true;
    }

    // a<N> - a broken-out address pad.
    if let Some(digits) = name.strip_prefix('a')
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
    {
        return true;
    }

    false
}

/// Names of pins that are not GPIOs at all and never will be.
fn is_not_a_gpio_name(name: &str) -> bool {
    matches!(name, "run" | "bootsel" | "swclk" | "swdio")
}

/// Decode a `--pin` value.
///
/// Accepts `gpioN` (case-insensitively), for example `gpio23` or `GPIO0`.
/// Everything else is an error whose message teaches the namespace rather than
/// guessing at what was meant.
pub fn parse_pin(spec: &str) -> Result<Pin, Error> {
    let trimmed = spec.trim();
    let name = trimmed.to_ascii_lowercase();

    let invalid = |detail: String| Err(Error::InvalidPin(trimmed.to_string(), detail));

    if name.is_empty() {
        return invalid(format!(
            "No pin given.\n  --pin takes an MCU GPIO, written 'gpio<N>' - for example 'gpio23'.\n  {HEADER_HINT}"
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

    if name.bytes().all(|b| b.is_ascii_digit()) {
        return invalid(format!(
            "A bare number is ambiguous: it could be an MCU GPIO, an image-select pad,\n  an X pad or a ROM socket pin.\n  Write an MCU GPIO as 'gpio{name}'.\n  {HEADER_HINT}"
        ));
    }

    if is_deferred_pad_name(&name) {
        return invalid(format!(
            "Header pad names are not yet supported - use the MCU GPIO behind the pad,\n  written 'gpio<N>'.\n  {HEADER_HINT}"
        ));
    }

    if is_not_a_gpio_name(&name) {
        return invalid(format!(
            "'{name}' is not a GPIO - it is a dedicated MCU pin and cannot be driven.\n  --pin takes an MCU GPIO, written 'gpio<N>' - for example 'gpio23'."
        ));
    }

    invalid(format!(
        "Unrecognised pin.\n  --pin takes an MCU GPIO, written 'gpio<N>' - for example 'gpio23'.\n  {HEADER_HINT}"
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
    fn a_pin_displays_as_it_is_written() {
        assert_eq!(Pin::Gpio(23).to_string(), "gpio23");
        assert_eq!(Pin::Gpio(23).gpio(), 23);
    }

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
    fn header_pad_names_say_they_are_not_yet_supported() {
        for spec in [
            "sel_a", "sel_b", "sel_c", "sel_d", "sel_e", "SEL_A", "x1", "x2", "X2", "a0", "a13",
        ] {
            let msg = rejection(spec);
            assert!(msg.contains("not yet supported"), "{spec}: {msg}");
            assert!(msg.contains("gpio<N>"), "{spec}: {msg}");
            assert!(msg.contains("onerom inspect header"), "{spec}: {msg}");
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
        for spec in ["banana", "pin23", "sel_f", "gpio-1", "gpio 23", "d3", "cs1"] {
            let msg = rejection(spec);
            assert!(
                msg.contains("gpio<N>") || msg.contains("gpio23"),
                "{spec}: {msg}"
            );
        }
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
        for spec in ["23", "x1", "run", "banana", "gpio", "GPIO256"] {
            let msg = rejection(spec);
            assert!(msg.contains(spec), "{spec}: {msg}");
        }
    }
}
