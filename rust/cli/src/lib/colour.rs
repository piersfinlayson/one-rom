// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The RGB LED's colour, and the `--colour` values a user can give for it.
//!
//! This lives in the library rather than the binary because a colour is
//! device-facing - these three bytes go on the wire as `SET_LED`'s colour - and
//! Studio is being moved onto the CLI library.

/// A colour for the RGB LED, as the device takes it.
///
/// Three bytes, one per channel, which is what a WS2812 is given and what
/// `SET_LED` carries. Not a general colour: there is no alpha and no colour
/// space, and brightness is a separate field on the wire rather than part of
/// this, so scaling a colour down is not the same as dimming the LED.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColour {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl RgbColour {
    /// The three components, in the order the wire wants them.
    pub fn rgb(&self) -> (u8, u8, u8) {
        (self.red, self.green, self.blue)
    }

    /// The word for this colour, where it has one.
    ///
    /// The reverse of what [`parse_colour`] reads, so a colour a user could
    /// have asked for by name is shown back to them by that name. Exact
    /// matches only - a colour a shade off one of these has no name, and
    /// saying it did would name a colour the LED is not showing.
    pub fn name(&self) -> Option<&'static str> {
        COLOURS_BY_NAME
            .iter()
            .find(|(_, rgb)| *rgb == self.rgb())
            .map(|(name, _)| *name)
    }

    /// Every colour that can be given by word, in the order they are offered.
    ///
    /// What a caller needs to say which words are accepted - the parse error
    /// here, and a chooser in a tool with somewhere to show them.
    pub fn names() -> impl Iterator<Item = &'static str> {
        COLOURS_BY_NAME.iter().map(|(name, _)| *name)
    }
}

impl std::fmt::Display for RgbColour {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
    }
}

/// The colours a word stands for.
///
/// A short list on purpose: it covers what someone marking one One ROM apart
/// from another reaches for, and anything else is expressible as hex.
const COLOURS_BY_NAME: &[(&str, (u8, u8, u8))] = &[
    ("red", (0xFF, 0x00, 0x00)),
    ("green", (0x00, 0xFF, 0x00)),
    ("blue", (0x00, 0x00, 0xFF)),
    ("white", (0xFF, 0xFF, 0xFF)),
    ("yellow", (0xFF, 0xFF, 0x00)),
    ("cyan", (0x00, 0xFF, 0xFF)),
    ("magenta", (0xFF, 0x00, 0xFF)),
    ("orange", (0xFF, 0x60, 0x00)),
    ("purple", (0x80, 0x00, 0xFF)),
    ("pink", (0xFF, 0x40, 0x60)),
];

/// Parse a colour named by word, or written as `#RRGGBB` or `0xRRGGBB`.
///
/// A free function rather than a method because this is what clap's
/// `value_parser` takes, the same shape as `pin::parse_pin`.
pub fn parse_colour(s: &str) -> Result<RgbColour, String> {
    let lower = s.to_ascii_lowercase();

    if let Some((_, (red, green, blue))) = COLOURS_BY_NAME.iter().find(|(name, _)| *name == lower) {
        return Ok(RgbColour {
            red: *red,
            green: *green,
            blue: *blue,
        });
    }

    let hex = lower
        .strip_prefix('#')
        .or_else(|| lower.strip_prefix("0x"))
        .unwrap_or(&lower);

    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let value = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
        return Ok(RgbColour {
            red: ((value >> 16) & 0xFF) as u8,
            green: ((value >> 8) & 0xFF) as u8,
            blue: (value & 0xFF) as u8,
        });
    }

    let names = RgbColour::names().collect::<Vec<_>>().join(", ");
    Err(format!(
        "'{s}' is not a colour. Give one of {names}, or a hex colour such as #FF8000."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_survives_the_round_trip() {
        // Each name parses to a colour, and that colour names itself again.
        // Written over the table rather than a hand-copied list, so a colour
        // added there is covered without touching this.
        for (name, _) in COLOURS_BY_NAME {
            let parsed = parse_colour(name).expect("a listed name parses");
            assert_eq!(
                parsed.name(),
                Some(*name),
                "{name} did not name itself again"
            );
        }
    }

    #[test]
    fn the_names_are_distinct_colours() {
        // The reverse lookup answers with the first match, so two names on one
        // colour would make it silently pick one.  They must not collide.
        for (i, (name, rgb)) in COLOURS_BY_NAME.iter().enumerate() {
            for (other, other_rgb) in COLOURS_BY_NAME.iter().skip(i + 1) {
                assert_ne!(rgb, other_rgb, "{name} and {other} are the same colour");
            }
        }
    }

    #[test]
    fn a_colour_off_by_one_has_no_name() {
        // Exact matches only.  #FE0000 is not red, and calling it red would
        // name a colour the LED is not showing.
        let cases = [
            ((0xFF, 0x00, 0x00), Some("red")),
            ((0xFE, 0x00, 0x00), None),
            ((0x00, 0x00, 0x00), None),
            ((0x12, 0x34, 0x56), None),
        ];

        for ((red, green, blue), want) in cases {
            let colour = RgbColour { red, green, blue };
            assert_eq!(colour.name(), want, "{colour} named itself wrongly");
        }
    }
}
