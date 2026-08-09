// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Machinery shared by the record-oriented ROM image formats.
//!
//! Intel HEX ([`ihex`](crate::ihex)) and Motorola S-record
//! ([`srec`](crate::srec)) differ in their framing — record types, address
//! widths and checksum polarity — but describe an image the same way: a series
//! of records, each carrying an absolute load address and a run of bytes.  What
//! that means for the decoder is identical in both cases, and lives here:
//!
//! - `ImageAccumulator` (crate-private), which places a record's bytes at their
//!   ROM offset, growing the image as it goes, filling anything no record wrote
//!   with [`UNWRITTEN_BYTE`], and rejecting overlaps and oversized images.
//! - [`LoadAddress`], the absolute address that maps to byte 0 of the decoded
//!   image, which both formats interpret identically.
//! - The ASCII hex helpers both decoders and encoders use.
//!
//! The format-specific modules keep their own error types, so the accumulator
//! reports failures as a neutral `PlaceError` that each maps into its own error
//! with the offending line number attached.

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use crate::MAX_IMAGE_SIZE;

/// Fill byte for any address a record-oriented image leaves unwritten.
///
/// Distinct from [`PAD_BLANK_BYTE`](crate::PAD_BLANK_BYTE) (`0xAA`), which pads
/// raw binary images out to the chip size.  An unprogrammed ROM cell reads as
/// `0xFF`, so that is what unwritten addresses become — both gaps within the
/// image and, when the user opts into [`SizeHandling::Pad`](crate::SizeHandling::Pad),
/// the padding out to the chip size.
pub const UNWRITTEN_BYTE: u8 = 0xFF;

/// Former name of [`UNWRITTEN_BYTE`], from when Intel HEX was the only
/// record-oriented format supported.
#[deprecated(
    since = "0.8.0",
    note = "renamed to `UNWRITTEN_BYTE`: the fill byte is shared by every record-oriented format, not just Intel HEX"
)]
pub const IHEX_BLANK_BYTE: u8 = UNWRITTEN_BYTE;

/// The absolute address that maps to byte 0 of a decoded ROM image.
///
/// Applies to every record-oriented format: a ROM assembled at, say, `0xE000`
/// uses `load_address = 0xE000` so its first byte lands at ROM offset 0.
///
/// Deserialises from either a JSON number or a string.  String forms accept a
/// plain decimal value, or hexadecimal prefixed with `0x` or `$`
/// (e.g. `"0xE000"`, `"$E000"`).  Serialises back to a `0x`-prefixed
/// hexadecimal string.  Defaults to 0.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LoadAddress(pub usize);

impl LoadAddress {
    /// Returns true if this is the default (zero) load address.
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    /// Parses a load address from a string: a plain decimal value, or
    /// hexadecimal prefixed with `0x`/`0X` or `$`.  Reused by the CLI `--slot`
    /// parser so config and command line accept identical spellings.
    pub fn parse_str(s: &str) -> Result<Self, AddressParseError> {
        let trimmed = s.trim();
        let value = if let Some(hex) = trimmed.strip_prefix('$') {
            usize::from_str_radix(hex, 16)
        } else if let Some(hex) = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
        {
            usize::from_str_radix(hex, 16)
        } else {
            trimmed.parse::<usize>()
        };
        value
            .map(LoadAddress)
            .map_err(|_| AddressParseError::new(trimmed))
    }
}

impl serde::Serialize for LoadAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Emit as a `0x`-prefixed hex string; round-trips through parse_str and
        // reads naturally for an address.
        serializer.serialize_str(&alloc::format!("{:#x}", self.0))
    }
}

impl<'de> serde::Deserialize<'de> for LoadAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct LoadAddressVisitor;

        impl serde::de::Visitor<'_> for LoadAddressVisitor {
            type Value = LoadAddress;

            fn expecting(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                f.write_str(
                    "a load address as a non-negative number, or a decimal / 0x- / $-prefixed hex string",
                )
            }

            fn visit_u64<E>(self, v: u64) -> Result<LoadAddress, E>
            where
                E: serde::de::Error,
            {
                usize::try_from(v)
                    .map(LoadAddress)
                    .map_err(|_| E::custom("load address out of range"))
            }

            fn visit_str<E>(self, v: &str) -> Result<LoadAddress, E>
            where
                E: serde::de::Error,
            {
                LoadAddress::parse_str(v).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(LoadAddressVisitor)
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for LoadAddress {
    fn schema_name() -> alloc::borrow::Cow<'static, str> {
        "LoadAddress".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "Load address for a record-oriented image (Intel HEX or S-record): a non-negative integer, or a string in decimal or 0x-/$-prefixed hexadecimal.",
            "oneOf": [
                { "type": "integer", "minimum": 0 },
                { "type": "string", "pattern": r"^(0[xX]|\$)?[0-9a-fA-F]+$" }
            ]
        })
    }
}

/// Error returned when a [`LoadAddress`] string cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AddressParseError {
    input: String,
}

impl AddressParseError {
    pub(crate) fn new(input: &str) -> Self {
        Self {
            input: input.to_owned(),
        }
    }
}

impl core::fmt::Display for AddressParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "invalid load address '{}': expected a decimal value or hexadecimal prefixed with 0x or $",
            self.input
        )
    }
}

/// Why [`ImageAccumulator::place`] rejected a record's bytes.
///
/// Deliberately format-neutral: each format's decoder maps these into its own
/// error type, attaching the line number it knows and the accumulator does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaceError {
    /// The record addressed a byte below the configured load address.
    AddressBelowLoad { address: usize, load_address: usize },
    /// Two records wrote to the same ROM offset.
    Overlap { offset: usize },
    /// The image would extend beyond [`MAX_IMAGE_SIZE`].
    TooLarge { size: usize, max: usize },
}

/// Assembles a contiguous ROM image from records placed at absolute addresses.
///
/// The image grows to fit whatever is placed into it, so it ends up sized to
/// the file's own extent — the highest written ROM offset plus one.  Offsets no
/// record wrote keep [`UNWRITTEN_BYTE`], whether they are gaps between records
/// or were never reached at all.  Reconciling that extent against the target
/// chip size is the caller's job, via the usual
/// [`SizeHandling`](crate::SizeHandling).
pub(crate) struct ImageAccumulator {
    image: Vec<u8>,
    /// Parallel "was this offset written?" map, for overlap detection.
    written: Vec<bool>,
    any_data: bool,
}

impl ImageAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            image: Vec::new(),
            written: Vec::new(),
            any_data: false,
        }
    }

    /// Returns true if no record has placed any bytes.
    pub(crate) fn is_empty(&self) -> bool {
        !self.any_data
    }

    /// Places `data` at absolute `address`, mapped to ROM offset
    /// `address - load_address`.
    ///
    /// An empty `data` is accepted and does nothing: an address-only data
    /// record carries no image content, so it neither counts as data nor
    /// extends the image.
    pub(crate) fn place(
        &mut self,
        address: usize,
        load_address: usize,
        data: &[u8],
    ) -> Result<(), PlaceError> {
        if data.is_empty() {
            return Ok(());
        }
        self.any_data = true;

        for (i, &byte) in data.iter().enumerate() {
            let address = address.checked_add(i).ok_or(PlaceError::TooLarge {
                size: usize::MAX,
                max: MAX_IMAGE_SIZE,
            })?;
            if address < load_address {
                return Err(PlaceError::AddressBelowLoad {
                    address,
                    load_address,
                });
            }
            let offset = address - load_address;
            if offset >= MAX_IMAGE_SIZE {
                return Err(PlaceError::TooLarge {
                    size: offset + 1,
                    max: MAX_IMAGE_SIZE,
                });
            }
            if offset >= self.image.len() {
                self.image.resize(offset + 1, UNWRITTEN_BYTE);
                self.written.resize(offset + 1, false);
            }
            if self.written[offset] {
                return Err(PlaceError::Overlap { offset });
            }
            self.written[offset] = true;
            self.image[offset] = byte;
        }
        Ok(())
    }

    /// Consumes the accumulator, returning the assembled image.
    pub(crate) fn into_image(self) -> Vec<u8> {
        self.image
    }
}

/// Converts a single ASCII hex digit to its value.
pub(crate) fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Decodes a run of ASCII hex digit pairs into bytes.
///
/// Returns `None` if the run has an odd length or contains a non-hex digit;
/// the caller turns that into its own format's error.
pub(crate) fn decode_hex_pairs(hex: &[u8]) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut i = 0;
    while i < hex.len() {
        let hi = hex_val(hex[i])?;
        let lo = hex_val(hex[i + 1])?;
        bytes.push((hi << 4) | lo);
        i += 2;
    }
    Some(bytes)
}

/// Appends one byte as two uppercase hex characters.
pub(crate) fn push_hex8(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 0x0F) as usize] as char);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_fills_gaps_with_unwritten_byte() {
        let mut acc = ImageAccumulator::new();
        acc.place(0, 0, &[0x01, 0x02]).unwrap();
        acc.place(5, 0, &[0x03]).unwrap();
        assert_eq!(
            acc.into_image(),
            [
                0x01,
                0x02,
                UNWRITTEN_BYTE,
                UNWRITTEN_BYTE,
                UNWRITTEN_BYTE,
                0x03
            ]
        );
    }

    #[test]
    fn place_subtracts_the_load_address() {
        let mut acc = ImageAccumulator::new();
        acc.place(0xE000, 0xE000, &[0xAA, 0xBB]).unwrap();
        assert_eq!(acc.into_image(), [0xAA, 0xBB]);
    }

    #[test]
    fn place_rejects_an_address_below_the_load_address() {
        let mut acc = ImageAccumulator::new();
        assert_eq!(
            acc.place(0x1000, 0x2000, &[0x00]),
            Err(PlaceError::AddressBelowLoad {
                address: 0x1000,
                load_address: 0x2000,
            })
        );
    }

    #[test]
    fn place_rejects_overlapping_records() {
        let mut acc = ImageAccumulator::new();
        acc.place(0, 0, &[0x01, 0x02, 0x03]).unwrap();
        assert_eq!(
            acc.place(2, 0, &[0xFF]),
            Err(PlaceError::Overlap { offset: 2 })
        );
    }

    #[test]
    fn place_rejects_an_oversized_image() {
        let mut acc = ImageAccumulator::new();
        assert_eq!(
            acc.place(MAX_IMAGE_SIZE, 0, &[0x00]),
            Err(PlaceError::TooLarge {
                size: MAX_IMAGE_SIZE + 1,
                max: MAX_IMAGE_SIZE,
            })
        );
    }

    #[test]
    fn place_of_no_bytes_is_not_data() {
        let mut acc = ImageAccumulator::new();
        acc.place(0x100, 0, &[]).unwrap();
        assert!(acc.is_empty());
        assert!(acc.into_image().is_empty());
    }

    #[test]
    fn decode_hex_pairs_rejects_odd_length_and_bad_digits() {
        assert_eq!(decode_hex_pairs(b"0aFF"), Some(alloc::vec![0x0A, 0xFF]));
        assert_eq!(decode_hex_pairs(b"0aF"), None);
        assert_eq!(decode_hex_pairs(b"0aFG"), None);
    }

    #[test]
    fn parse_str_accepts_decimal_and_hex_forms() {
        assert_eq!(LoadAddress::parse_str("0").unwrap(), LoadAddress(0));
        assert_eq!(
            LoadAddress::parse_str("57344").unwrap(),
            LoadAddress(0xE000)
        );
        assert_eq!(
            LoadAddress::parse_str("0xE000").unwrap(),
            LoadAddress(0xE000)
        );
        assert_eq!(
            LoadAddress::parse_str("0Xe000").unwrap(),
            LoadAddress(0xE000)
        );
        assert_eq!(
            LoadAddress::parse_str("$E000").unwrap(),
            LoadAddress(0xE000)
        );
        assert_eq!(
            LoadAddress::parse_str("  $E000 ").unwrap(),
            LoadAddress(0xE000)
        );
        assert!(LoadAddress::parse_str("").is_err());
        assert!(LoadAddress::parse_str("$").is_err());
        assert!(LoadAddress::parse_str("0xZZ").is_err());
        assert!(LoadAddress::parse_str("nope").is_err());
    }

    #[test]
    fn load_address_serde_round_trips() {
        // Number and string inputs both deserialise; output is a hex string.
        let from_num: LoadAddress = serde_json::from_str("57344").unwrap();
        assert_eq!(from_num, LoadAddress(0xE000));
        let from_hex: LoadAddress = serde_json::from_str("\"0xE000\"").unwrap();
        assert_eq!(from_hex, LoadAddress(0xE000));
        let from_dollar: LoadAddress = serde_json::from_str("\"$E000\"").unwrap();
        assert_eq!(from_dollar, LoadAddress(0xE000));
        assert_eq!(
            serde_json::to_string(&LoadAddress(0xE000)).unwrap(),
            "\"0xe000\""
        );
        // A negative number is rejected.
        assert!(serde_json::from_str::<LoadAddress>("-1").is_err());
    }
}
