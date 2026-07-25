// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Intel HEX decoding for supplied ROM images.
//!
//! A ROM image supplied to the generator may be a raw binary or an Intel HEX
//! file (selected via [`FileFormat`](crate::FileFormat) on the chip config).
//! Intel HEX is an ASCII, record-oriented format that carries a load address
//! per record, so it describes *where* each chunk of data lands rather than a
//! flat blob.  [`decode_ihex`] turns such a file into the contiguous binary
//! image the rest of the generator expects.
//!
//! ## Supported record types
//!
//! | Type   | Name                          | Handling                          |
//! |--------|-------------------------------|-----------------------------------|
//! | `0x00` | Data                          | Placed at its (extended) address. |
//! | `0x01` | End Of File                   | Ends decoding.                    |
//! | `0x02` | Extended Segment Address      | Sets base to `value << 4`.        |
//! | `0x04` | Extended Linear Address       | Sets base to `value << 16`.       |
//! | `0x03` | Start Segment Address (CS:IP) | Parsed and validated, then        |
//! | `0x05` | Start Linear Address (EIP)    | ignored (execution entry point).  |
//!
//! Every record's checksum, framing and length are validated regardless of
//! type; any other record type is an error.
//!
//! ## Address handling
//!
//! Each data record's absolute address is `extended_base + record_offset`.
//! The caller-supplied `load_address` is subtracted from it to give the ROM
//! offset (so a ROM assembled at, say, `0xE000` uses `load_address = 0xE000`
//! to land at offset 0).  A record addressing a byte below `load_address` is
//! an error.  The returned image is sized to its own extent (highest ROM
//! offset + 1); gaps within it are filled with [`IHEX_BLANK_BYTE`].
//! Reconciling that image against the target chip size is the caller's job,
//! via the usual [`SizeHandling`](crate::SizeHandling).
//!
//! ## Deliberate deviations and policy choices
//!
//! - **Type `0x02` offset wraparound is not emulated.**  The strict 8086
//!   behaviour wraps a record's 16-bit offset within a 64 KB segment; this
//!   decoder treats an Extended Segment Address as a plain `(value << 4) +
//!   offset` linear addition.  Real ROM images never rely on segment wrap, and
//!   emulating it faithfully would be a footgun.
//! - **A terminating End Of File record is required.**  Its absence usually
//!   means a truncated or corrupt file, so it is reported rather than
//!   tolerated.
//! - **Bytes after the End Of File record are ignored.**
//! - **Overlapping data records are an error** — two records writing the same
//!   ROM offset almost always indicates a malformed or misunderstood file.

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use crate::MAX_IMAGE_SIZE;

/// Fill byte for any address an Intel HEX image leaves unwritten.
///
/// Distinct from [`PAD_BLANK_BYTE`](crate::PAD_BLANK_BYTE) (`0xAA`), which
/// pads raw binary images out to the chip size.  An unprogrammed ROM cell
/// reads as `0xFF`, so that is what unwritten addresses in an Intel HEX image
/// become — both gaps within the image and, when the user opts into
/// [`SizeHandling::Pad`](crate::SizeHandling::Pad), the padding out to the
/// chip size.
pub const IHEX_BLANK_BYTE: u8 = 0xFF;

/// An Intel HEX load address: the absolute address that maps to byte 0 of the
/// decoded ROM image.
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

            fn visit_i64<E>(self, v: i64) -> Result<LoadAddress, E>
            where
                E: serde::de::Error,
            {
                usize::try_from(v)
                    .map(LoadAddress)
                    .map_err(|_| E::custom("load address must be non-negative and in range"))
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
            "description": "Intel HEX load address: a non-negative integer, or a string in decimal or 0x-/$-prefixed hexadecimal.",
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
    fn new(input: &str) -> Self {
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

/// Error returned when decoding an Intel HEX image.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IhexError {
    /// A record did not begin with the `:` start-of-record marker.
    MissingColon { line: usize },
    /// A record contained a non-hexadecimal digit or an odd number of digits.
    BadHex { line: usize },
    /// A record was shorter than the five-byte minimum, or its byte count did
    /// not match the data present.
    BadLength { line: usize },
    /// A record's checksum did not match.
    BadChecksum {
        line: usize,
        expected: u8,
        actual: u8,
    },
    /// A record used a type this decoder does not support.
    UnsupportedRecordType { line: usize, record_type: u8 },
    /// A data record addressed a byte below the configured load address.
    AddressBelowLoad {
        line: usize,
        address: usize,
        load_address: usize,
    },
    /// Two data records wrote to the same ROM offset.
    OverlappingData { offset: usize },
    /// The image extends beyond the maximum supported image size.
    ImageTooLarge { size: usize, max: usize },
    /// The file did not contain a terminating end-of-file (`:00000001FF`)
    /// record.
    MissingEof,
    /// The file contained no data records.
    NoData,
}

impl core::fmt::Display for IhexError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            IhexError::MissingColon { line } => {
                write!(f, "line {line}: record does not start with ':'")
            }
            IhexError::BadHex { line } => {
                write!(f, "line {line}: invalid or odd-length hexadecimal")
            }
            IhexError::BadLength { line } => {
                write!(f, "line {line}: record length or byte count is invalid")
            }
            IhexError::BadChecksum {
                line,
                expected,
                actual,
            } => write!(
                f,
                "line {line}: bad checksum, expected {expected:#04x} but found {actual:#04x}"
            ),
            IhexError::UnsupportedRecordType { line, record_type } => {
                write!(f, "line {line}: unsupported record type {record_type:#04x}")
            }
            IhexError::AddressBelowLoad {
                line,
                address,
                load_address,
            } => write!(
                f,
                "line {line}: address {address:#x} is below the load address {load_address:#x}"
            ),
            IhexError::OverlappingData { offset } => {
                write!(f, "overlapping data records write to offset {offset:#x}")
            }
            IhexError::ImageTooLarge { size, max } => write!(
                f,
                "the Intel HEX image extends to {size} bytes, beyond the {max}-byte maximum"
            ),
            IhexError::MissingEof => {
                write!(f, "missing end-of-file record (':00000001FF')")
            }
            IhexError::NoData => write!(f, "the Intel HEX file contained no data records"),
        }
    }
}

/// A single decoded Intel HEX record.
struct Record {
    address: u16,
    record_type: u8,
    data: Vec<u8>,
}

/// Decodes an Intel HEX image into a contiguous binary image.
///
/// The returned image is sized to the Intel HEX image's own extent (its
/// highest written ROM offset + 1); any gaps within that extent are filled
/// with [`IHEX_BLANK_BYTE`].  `load_address` is subtracted from every record's
/// absolute address to yield its ROM offset; a record addressing a byte below
/// `load_address` is an error.  Reconciling the returned image against the
/// target chip size (padding/truncating) is the caller's responsibility — this
/// returns exactly what the Intel HEX file describes.
///
/// See the [module documentation](self) for the supported record types and the
/// validation and policy rules.
pub fn decode_ihex(input: &[u8], load_address: usize) -> Result<Vec<u8>, IhexError> {
    let mut image: Vec<u8> = Vec::new();
    // Parallel "was this offset written?" map, for overlap detection.
    let mut written: Vec<bool> = Vec::new();
    // Base address contributed by the most recent type 0x02/0x04 record.
    let mut extended_base: usize = 0;
    let mut seen_eof = false;
    let mut any_data = false;

    for (idx, raw_line) in input.split(|&b| b == b'\n').enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim_ascii();
        if line.is_empty() {
            continue;
        }
        // Ignore anything after the end-of-file record.
        if seen_eof {
            break;
        }

        let record = parse_record(line, line_no)?;
        match record.record_type {
            // Data
            0x00 => {
                if !record.data.is_empty() {
                    any_data = true;
                }
                for (i, &byte) in record.data.iter().enumerate() {
                    let address = extended_base
                        .checked_add(record.address as usize)
                        .and_then(|a| a.checked_add(i))
                        .ok_or(IhexError::ImageTooLarge {
                            size: usize::MAX,
                            max: MAX_IMAGE_SIZE,
                        })?;
                    if address < load_address {
                        return Err(IhexError::AddressBelowLoad {
                            line: line_no,
                            address,
                            load_address,
                        });
                    }
                    let offset = address - load_address;
                    if offset >= MAX_IMAGE_SIZE {
                        return Err(IhexError::ImageTooLarge {
                            size: offset + 1,
                            max: MAX_IMAGE_SIZE,
                        });
                    }
                    if offset >= image.len() {
                        image.resize(offset + 1, IHEX_BLANK_BYTE);
                        written.resize(offset + 1, false);
                    }
                    if written[offset] {
                        return Err(IhexError::OverlappingData { offset });
                    }
                    written[offset] = true;
                    image[offset] = byte;
                }
            }
            // End Of File
            0x01 => seen_eof = true,
            // Extended Segment Address: base = value << 4
            0x02 => {
                if record.data.len() != 2 {
                    return Err(IhexError::BadLength { line: line_no });
                }
                let segment = ((record.data[0] as usize) << 8) | (record.data[1] as usize);
                extended_base = segment << 4;
            }
            // Extended Linear Address: base = value << 16
            0x04 => {
                if record.data.len() != 2 {
                    return Err(IhexError::BadLength { line: line_no });
                }
                let upper = ((record.data[0] as usize) << 8) | (record.data[1] as usize);
                extended_base = upper << 16;
            }
            // Start Segment / Start Linear Address: an execution entry point,
            // validated above but carrying no ROM data.
            0x03 | 0x05 => {}
            other => {
                return Err(IhexError::UnsupportedRecordType {
                    line: line_no,
                    record_type: other,
                });
            }
        }
    }

    if !seen_eof {
        return Err(IhexError::MissingEof);
    }
    if !any_data {
        return Err(IhexError::NoData);
    }

    Ok(image)
}

/// Parses one already-trimmed, non-empty Intel HEX record line.
fn parse_record(line: &[u8], line_no: usize) -> Result<Record, IhexError> {
    if line.first() != Some(&b':') {
        return Err(IhexError::MissingColon { line: line_no });
    }
    let hex = &line[1..];
    if !hex.len().is_multiple_of(2) {
        return Err(IhexError::BadHex { line: line_no });
    }

    // Decode the hex pairs into raw bytes.
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut i = 0;
    while i < hex.len() {
        let hi = hex_val(hex[i]).ok_or(IhexError::BadHex { line: line_no })?;
        let lo = hex_val(hex[i + 1]).ok_or(IhexError::BadHex { line: line_no })?;
        bytes.push((hi << 4) | lo);
        i += 2;
    }

    // Minimum record is count(1) + address(2) + type(1) + checksum(1).
    if bytes.len() < 5 {
        return Err(IhexError::BadLength { line: line_no });
    }
    let count = bytes[0] as usize;
    if bytes.len() != count + 5 {
        return Err(IhexError::BadLength { line: line_no });
    }

    // The two's-complement checksum makes every byte (checksum included) sum to
    // zero modulo 256.
    let sum = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    if sum != 0 {
        let data_sum = bytes[..bytes.len() - 1]
            .iter()
            .fold(0u8, |acc, &b| acc.wrapping_add(b));
        return Err(IhexError::BadChecksum {
            line: line_no,
            expected: 0u8.wrapping_sub(data_sum),
            actual: bytes[bytes.len() - 1],
        });
    }

    let address = ((bytes[1] as u16) << 8) | (bytes[2] as u16);
    let record_type = bytes[3];
    let data = bytes[4..4 + count].to_vec();
    Ok(Record {
        address,
        record_type,
        data,
    })
}

/// Converts a single ASCII hex digit to its value.
fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Encodes a binary image as Intel HEX text.
///
/// Produces 16-byte data records, uppercase hex and CRLF line endings, a
/// type-04 extended-linear-address record whenever the upper 16 bits of the
/// byte address change (including before the first record), and a terminating
/// EOF record.  Records are addressed starting at `load_address`.
///
/// This is deliberately byte-for-byte the same wire format as One ROM Lab's
/// ROM-dump emitter (`rust/lab/src/output/ihex.rs`): lab keeps its own
/// no-alloc, streaming implementation for the embedded firmware, and the two
/// must not drift.  `decode_ihex(encode_ihex(data, la), la)` round-trips to
/// `data`.
pub fn encode_ihex(data: &[u8], load_address: usize) -> String {
    let mut out = String::new();
    if !data.is_empty() {
        let mut current_upper: Option<u16> = None;
        let mut offset = 0;
        while offset < data.len() {
            let address = load_address + offset;
            let chunk_len = (data.len() - offset).min(16);

            // Emit an extended-linear-address record whenever the upper 16 bits
            // change, including before the first data record.
            let upper = ((address >> 16) & 0xFFFF) as u16;
            if current_upper != Some(upper) {
                push_ela_record(&mut out, upper);
                current_upper = Some(upper);
            }

            push_data_record(&mut out, address as u16, &data[offset..offset + chunk_len]);
            offset += chunk_len;
        }
    }
    out.push_str(":00000001FF\r\n");
    out
}

/// Appends one byte as two uppercase hex characters.
fn push_hex8(out: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 0x0F) as usize] as char);
}

/// Appends a type-00 data record (`:LLAAAA00DD..CC`) plus CRLF.
fn push_data_record(out: &mut String, address: u16, data: &[u8]) {
    let byte_count = data.len() as u8;
    out.push(':');
    push_hex8(out, byte_count);
    push_hex8(out, (address >> 8) as u8);
    push_hex8(out, address as u8);
    push_hex8(out, 0x00); // record type: data
    let mut csum = byte_count
        .wrapping_add((address >> 8) as u8)
        .wrapping_add(address as u8);
    for &b in data {
        push_hex8(out, b);
        csum = csum.wrapping_add(b);
    }
    push_hex8(out, 0u8.wrapping_sub(csum));
    out.push_str("\r\n");
}

/// Appends a type-04 extended-linear-address record plus CRLF.
fn push_ela_record(out: &mut String, upper: u16) {
    out.push_str(":02000004");
    push_hex8(out, (upper >> 8) as u8);
    push_hex8(out, upper as u8);
    let csum = 0x02u8
        .wrapping_add(0x04)
        .wrapping_add((upper >> 8) as u8)
        .wrapping_add(upper as u8);
    push_hex8(out, 0u8.wrapping_sub(csum));
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Builds a single data record line for the given 16-bit address.
    fn data_record(address: u16, data: &[u8]) -> String {
        let mut bytes = alloc::vec![data.len() as u8, (address >> 8) as u8, address as u8, 0x00];
        bytes.extend_from_slice(data);
        let data_sum = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        bytes.push(0u8.wrapping_sub(data_sum));
        let mut line = String::from(":");
        for b in bytes {
            line.push_str(&alloc::format!("{b:02X}"));
        }
        line
    }

    const EOF: &str = ":00000001FF";

    #[test]
    fn decodes_contiguous_image() {
        let hex = alloc::format!("{}\n{}\n", data_record(0, &[0xDE, 0xAD, 0xBE, 0xEF]), EOF);
        let out = decode_ihex(hex.as_bytes(), 0).unwrap();
        assert_eq!(out, alloc::vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn fills_internal_gaps_with_blank_byte() {
        // Data at offset 0 and offset 4, leaving a two-byte gap.
        let hex = alloc::format!(
            "{}\n{}\n{}\n",
            data_record(0, &[0x11, 0x22]),
            data_record(4, &[0x33, 0x44]),
            EOF
        );
        let out = decode_ihex(hex.as_bytes(), 0).unwrap();
        assert_eq!(
            out,
            alloc::vec![0x11, 0x22, IHEX_BLANK_BYTE, IHEX_BLANK_BYTE, 0x33, 0x44]
        );
    }

    #[test]
    fn tolerates_crlf_and_blank_lines() {
        let hex = alloc::format!("\r\n{}\r\n\r\n{}\r\n", data_record(0, &[0xAB]), EOF);
        let out = decode_ihex(hex.as_bytes(), 0).unwrap();
        assert_eq!(out, alloc::vec![0xAB]);
    }

    #[test]
    fn applies_load_address_offset() {
        // Data at absolute 0xE000 with load_address 0xE000 lands at offset 0.
        let hex = alloc::format!("{}\n{}\n", data_record(0xE000, &[0x01, 0x02]), EOF);
        let out = decode_ihex(hex.as_bytes(), 0xE000).unwrap();
        assert_eq!(out, alloc::vec![0x01, 0x02]);
    }

    #[test]
    fn address_below_load_is_an_error() {
        let hex = alloc::format!("{}\n{}\n", data_record(0x00, &[0x01]), EOF);
        assert!(matches!(
            decode_ihex(hex.as_bytes(), 0x10),
            Err(IhexError::AddressBelowLoad { .. })
        ));
    }

    #[test]
    fn extended_linear_address_reaches_beyond_64k() {
        // ELA of 0x0001 -> base 0x10000, plus offset 0 -> 64 KiB in.
        let ela = {
            let mut bytes = alloc::vec![0x02u8, 0x00, 0x00, 0x04, 0x00, 0x01];
            let sum = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
            bytes.push(0u8.wrapping_sub(sum));
            let mut line = String::from(":");
            for b in bytes {
                line.push_str(&alloc::format!("{b:02X}"));
            }
            line
        };
        let hex = alloc::format!("{}\n{}\n{}\n", ela, data_record(0x0000, &[0x99]), EOF);
        let out = decode_ihex(hex.as_bytes(), 0).unwrap();
        assert_eq!(out.len(), 0x10001);
        assert_eq!(out[0x10000], 0x99);
        assert_eq!(out[0], IHEX_BLANK_BYTE);
    }

    #[test]
    fn start_address_records_are_ignored() {
        // A type 0x05 start-linear-address record carries no ROM data.
        let sla = {
            let mut bytes = alloc::vec![0x04u8, 0x00, 0x00, 0x05, 0x00, 0x00, 0x80, 0x00];
            let sum = bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
            bytes.push(0u8.wrapping_sub(sum));
            let mut line = String::from(":");
            for b in bytes {
                line.push_str(&alloc::format!("{b:02X}"));
            }
            line
        };
        let hex = alloc::format!("{}\n{}\n{}\n", sla, data_record(0, &[0x42]), EOF);
        let out = decode_ihex(hex.as_bytes(), 0).unwrap();
        assert_eq!(out, alloc::vec![0x42]);
    }

    #[test]
    fn missing_eof_is_an_error() {
        let hex = alloc::format!("{}\n", data_record(0, &[0x01]));
        assert!(matches!(
            decode_ihex(hex.as_bytes(), 0),
            Err(IhexError::MissingEof)
        ));
    }

    #[test]
    fn bytes_after_eof_are_ignored() {
        let hex = alloc::format!(
            "{}\n{}\n{}\n",
            data_record(0, &[0x01]),
            EOF,
            "this is not a valid record"
        );
        let out = decode_ihex(hex.as_bytes(), 0).unwrap();
        assert_eq!(out, alloc::vec![0x01]);
    }

    #[test]
    fn overlapping_records_are_an_error() {
        let hex = alloc::format!(
            "{}\n{}\n{}\n",
            data_record(0, &[0x01, 0x02]),
            data_record(1, &[0x03]),
            EOF
        );
        assert!(matches!(
            decode_ihex(hex.as_bytes(), 0),
            Err(IhexError::OverlappingData { offset: 1 })
        ));
    }

    #[test]
    fn bad_checksum_is_an_error() {
        // A valid record with its last checksum digit corrupted.
        let mut line = data_record(0, &[0x01, 0x02]);
        line.pop();
        line.push('0');
        let hex = alloc::format!("{}\n{}\n", line, EOF);
        assert!(matches!(
            decode_ihex(hex.as_bytes(), 0),
            Err(IhexError::BadChecksum { .. })
        ));
    }

    #[test]
    fn no_data_records_is_an_error() {
        assert!(matches!(
            decode_ihex(EOF.as_bytes(), 0),
            Err(IhexError::NoData)
        ));
    }

    #[test]
    fn bad_hex_is_an_error() {
        let hex = alloc::format!(":10000000ZZ\n{}\n", EOF);
        assert!(matches!(
            decode_ihex(hex.as_bytes(), 0),
            Err(IhexError::BadHex { .. })
        ));
    }

    #[test]
    fn encode_matches_expected_wire_format() {
        // Golden reference. This exact byte layout — a leading type-04 record
        // even for the first 64 KB, 16-byte data records, uppercase hex and
        // CRLF endings — is also what lab's `output/ihex.rs` emits; the two
        // must stay identical.  A change here that is not mirrored in lab (or
        // vice versa) is a regression.
        let out = encode_ihex(&[0x00, 0x01, 0x02, 0x03], 0);
        assert_eq!(
            out,
            concat!(
                ":020000040000FA\r\n",     // type-04 ELA, upper = 0x0000
                ":0400000000010203F6\r\n", // 4 data bytes at 0x0000
                ":00000001FF\r\n",         // EOF
            )
        );
    }

    #[test]
    fn encode_decode_round_trips() {
        // Contiguous images round-trip exactly (no internal gaps to fill).
        for len in [1usize, 15, 16, 17, 256, 8192] {
            let data: Vec<u8> = (0..len)
                .map(|i| (i.wrapping_mul(37) ^ 0x5A) as u8)
                .collect();
            for la in [0usize, 0x10, 0xE000] {
                let hex = encode_ihex(&data, la);
                let back = decode_ihex(hex.as_bytes(), la).unwrap();
                assert_eq!(back, data, "round-trip failed at len={len}, la={la:#x}");
            }
        }
    }

    #[test]
    fn encode_emits_extended_linear_across_64k() {
        // A >64 KB image gets a fresh type-04 record for the second segment;
        // it still round-trips.
        let data = alloc::vec![0xABu8; 0x10001];
        let hex = encode_ihex(&data, 0);
        assert_eq!(hex.matches(":02000004").count(), 2);
        assert_eq!(decode_ihex(hex.as_bytes(), 0).unwrap(), data);
    }

    #[test]
    fn encode_empty_is_just_eof() {
        assert_eq!(encode_ihex(&[], 0), ":00000001FF\r\n");
    }
}
