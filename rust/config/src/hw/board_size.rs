// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Board sizes, which differ in the flash chips a board has.

use core::fmt;
use core::str::FromStr;

/// A board's size. `docs/OTP.md`'s "Board Sizes" section describes each.
///
/// It parses from its name in either case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BoardSize {
    /// 2MB of built-in flash without a chip on chip select 1.
    M,
    /// 2MB of built-in flash and 2MB of external flash on chip select 1.
    L,
}

impl BoardSize {
    /// Every size, smallest first.
    ///
    /// [`FromStr`] and [`BoardSizeError`]'s text are built from this list. The
    /// match makes a new size a compile error here until it's listed.
    pub fn supported_values() -> &'static [Self] {
        match Self::M {
            Self::M | Self::L => {}
        }
        &[Self::M, Self::L]
    }

    /// The size's name, such as `M`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::M => "M",
            Self::L => "L",
        }
    }

    /// A short description of the size's flash.
    pub fn description(&self) -> &'static str {
        match self {
            Self::M => "2MB of flash",
            Self::L => "2MB of flash and an additional 2MB flash chip",
        }
    }
}

impl FromStr for BoardSize {
    type Err = BoardSizeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::supported_values()
            .iter()
            .copied()
            .find(|size| size.name().eq_ignore_ascii_case(s))
            .ok_or(BoardSizeError::Unknown)
    }
}

impl fmt::Display for BoardSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a board size didn't parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardSizeError {
    /// Text that isn't a board size.
    Unknown,
}

impl fmt::Display for BoardSizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => {
                // Every size as a list, such as `M or L`.
                f.write_str("the size must be ")?;
                let sizes = BoardSize::supported_values();
                for (i, size) in sizes.iter().enumerate() {
                    if i > 0 {
                        f.write_str(if i + 1 == sizes.len() { " or " } else { ", " })?;
                    }
                    f.write_str(size.name())?;
                }
                Ok(())
            }
        }
    }
}

impl core::error::Error for BoardSizeError {}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::string::ToString;

    use super::*;

    #[test]
    fn a_board_size_parses_from_its_letter_in_either_case() {
        for (text, size) in [
            ("m", BoardSize::M),
            ("M", BoardSize::M),
            ("l", BoardSize::L),
            ("L", BoardSize::L),
        ] {
            assert_eq!(text.parse(), Ok(size), "{text}");
        }
    }

    #[test]
    fn other_text_is_refused() {
        for text in ["xl", "XL", "Q", "", "LL"] {
            assert_eq!(
                text.parse::<BoardSize>(),
                Err(BoardSizeError::Unknown),
                "{text}"
            );
        }
    }

    #[test]
    fn the_error_lists_every_size() {
        let text = BoardSizeError::Unknown.to_string();
        for size in BoardSize::supported_values() {
            assert!(text.contains(&size.to_string()), "{text}");
        }
    }

    #[test]
    fn each_size_has_its_own_description() {
        let sizes = BoardSize::supported_values();
        for (i, size) in sizes.iter().enumerate() {
            assert!(!size.description().is_empty(), "{size}");
            for other in &sizes[i + 1..] {
                assert_ne!(size.description(), other.description(), "{size} {other}");
            }
        }
    }

    #[test]
    fn a_board_size_round_trips_through_serde_as_its_letter() {
        for size in BoardSize::supported_values() {
            let json = serde_json::to_string(size).unwrap();
            assert_eq!(json, alloc::format!("\"{size}\""));
            assert_eq!(serde_json::from_str::<BoardSize>(&json).unwrap(), *size);
        }
    }
}
