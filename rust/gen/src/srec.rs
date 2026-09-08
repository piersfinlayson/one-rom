// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Motorola S-record decoding and encoding for supplied ROM images.
//!
//! A ROM image supplied to the generator may be a raw binary, an Intel HEX file
//! or a Motorola S-record file (selected via [`FileFormat`](crate::FileFormat)
//! on the chip config).  Like Intel HEX, S-record is an ASCII, record-oriented
//! format carrying a load address per record; [`decode_srec`] turns such a file
//! into the contiguous binary image the rest of the generator expects.
//!
//! The image assembly itself — extent sizing, gap filling, overlap detection
//! and the load-address mapping — is shared with [`ihex`](crate::ihex) and
//! lives in [`hexfile`](crate::hexfile).  Two things about the framing differ
//! from Intel HEX and are easy to get wrong by analogy:
//!
//! - **The address field is variable width** — two, three or four bytes,
//!   selected by the record type, rather than a fixed two bytes plus separate
//!   extended-base records.
//! - **The checksum is a ones' complement**, not a two's complement.  It is
//!   `!(count + address bytes + data bytes)`, taken over the low byte of that
//!   sum.
//!
//! ## Record types
//!
//! A record is `S<type><count><address><data><checksum>`, where `count` is the
//! number of bytes that follow it: the address bytes, the data bytes and the
//! checksum byte.
//!
//! | Type | Address | Handling                                                   |
//! |------|---------|------------------------------------------------------------|
//! | `S0` | 2 bytes | Header.  Parsed and checksum-validated, then ignored.       |
//! | `S1` | 2 bytes | Data.  Placed at its address.                               |
//! | `S2` | 3 bytes | Data.                                                       |
//! | `S3` | 4 bytes | Data.                                                       |
//! | `S4` | —       | Reserved.  An error.                                        |
//! | `S5` | 2 bytes | Count of data records; the count is the address field.      |
//! | `S6` | 3 bytes | As `S5`, with a wider count.                                |
//! | `S7` | 4 bytes | Termination for `S3`.  Ends decoding.                       |
//! | `S8` | 3 bytes | Termination for `S2`.                                       |
//! | `S9` | 2 bytes | Termination for `S1`.                                       |
//!
//! ## Address handling
//!
//! A data record's address is absolute — there is no extended-base mechanism to
//! track.  The caller-supplied `load_address` is subtracted from it to give the
//! ROM offset (so a ROM assembled at, say, `0xE000` uses
//! `load_address = 0xE000` to land at offset 0).  A record addressing a byte
//! below `load_address` is an error.  The returned image is sized to its own
//! extent (highest ROM offset + 1); gaps within it are filled with
//! [`UNWRITTEN_BYTE`](crate::UNWRITTEN_BYTE).  Reconciling that image against
//! the target chip size is the caller's job, via the usual
//! [`SizeHandling`](crate::SizeHandling).
//!
//! ## Deliberate deviations and policy choices
//!
//! These mirror [`ihex`](crate::ihex)'s where the formats correspond:
//!
//! - **A termination record (`S7`/`S8`/`S9`) is optional.**  The standard asks
//!   for one, but a tool writing a pure data image often has no execution
//!   start address to put in it and leaves it out - `srec_cat` does so unless
//!   given `-execution_start_address`.  Where a file has none, the `S5`/`S6`
//!   count record is what is left to catch a truncated transfer, and the same
//!   tools do emit one.
//! - **Bytes after the termination record are ignored.**
//! - **Overlapping data records are an error.**
//! - **`S5`/`S6` record counts are validated** against the number of data
//!   records actually seen, and a mismatch is an error — that is exactly the
//!   truncation these records exist to catch.  The check is made at the end of
//!   the file rather than where the record appears, so a count record placed
//!   before the data (rather than in its conventional position after it) is
//!   still handled.  Where a file carries more than one, the first is the one
//!   checked.
//! - **The termination record's width need not match the data records'** — the
//!   standard pairs `S9` with `S1`, `S8` with `S2` and `S7` with `S3`, but real
//!   tools are inconsistent about it and nothing depends on the pairing here.
//! - **`S1`, `S2` and `S3` data records may be mixed within one file.**  Every
//!   address is absolute, so the width a record uses to spell it does not
//!   change where its bytes land.
//! - **The `S0` header is discarded.**  It carries a free-text module name,
//!   with nothing that maps onto a ROM image.
//! - **A data record carrying no data bytes is accepted and ignored.**
//! - The leading `S` may be upper or lower case, as may the hex digits.

use alloc::string::String;
use alloc::vec::Vec;

use crate::hexfile::{ImageAccumulator, PlaceError, decode_hex_pairs, push_hex8};

/// Error returned when decoding a Motorola S-record image.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SrecError {
    /// A record did not begin with the `S` start-of-record marker.
    MissingS { line: usize },
    /// A record contained a non-hexadecimal digit or an odd number of digits.
    BadHex { line: usize },
    /// A record was too short for its type, or its byte count did not match the
    /// data present.
    BadLength { line: usize },
    /// A record's checksum did not match.
    BadChecksum {
        line: usize,
        expected: u8,
        actual: u8,
    },
    /// A record used a type this decoder does not support (`S4`, or a
    /// non-digit type character).
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
    /// An `S5`/`S6` record's count did not match the number of data records in
    /// the file.
    RecordCountMismatch { declared: usize, actual: usize },
    /// The file contained no data records.
    NoData,
}

impl core::fmt::Display for SrecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SrecError::MissingS { line } => {
                write!(f, "line {line}: record does not start with 'S'")
            }
            SrecError::BadHex { line } => {
                write!(f, "line {line}: invalid or odd-length hexadecimal")
            }
            SrecError::BadLength { line } => {
                write!(f, "line {line}: record length or byte count is invalid")
            }
            SrecError::BadChecksum {
                line,
                expected,
                actual,
            } => write!(
                f,
                "line {line}: bad checksum, expected {expected:#04x} but found {actual:#04x}"
            ),
            SrecError::UnsupportedRecordType { line, record_type } => {
                write!(f, "line {line}: unsupported record type 'S{record_type}'")
            }
            SrecError::AddressBelowLoad {
                line,
                address,
                load_address,
            } => write!(
                f,
                "line {line}: address {address:#x} is below the load address {load_address:#x}"
            ),
            SrecError::OverlappingData { offset } => {
                write!(f, "overlapping data records write to offset {offset:#x}")
            }
            SrecError::ImageTooLarge { size, max } => write!(
                f,
                "the S-record image extends to {size} bytes, beyond the {max}-byte maximum"
            ),
            SrecError::RecordCountMismatch { declared, actual } => write!(
                f,
                "the record count declares {declared} data records but the file contains {actual}"
            ),
            SrecError::NoData => write!(f, "the S-record file contained no data records"),
        }
    }
}

impl SrecError {
    /// Attaches the offending line number to a [`PlaceError`] from the shared
    /// image accumulator.  `OverlappingData` and `ImageTooLarge` are reported
    /// against the image rather than a line, so they drop it.
    fn from_place(source: PlaceError, line: usize) -> Self {
        match source {
            PlaceError::AddressBelowLoad {
                address,
                load_address,
            } => SrecError::AddressBelowLoad {
                line,
                address,
                load_address,
            },
            PlaceError::Overlap { offset } => SrecError::OverlappingData { offset },
            PlaceError::TooLarge { size, max } => SrecError::ImageTooLarge { size, max },
        }
    }
}

/// A single decoded S-record.
struct Record {
    record_type: u8,
    address: usize,
    data: Vec<u8>,
}

/// The width in bytes of the address field for a record type, or `None` for a
/// type this decoder does not support (`S4`).
fn address_width(record_type: u8) -> Option<usize> {
    match record_type {
        0 | 1 | 5 | 9 => Some(2),
        2 | 6 | 8 => Some(3),
        3 | 7 => Some(4),
        _ => None,
    }
}

/// Decodes a Motorola S-record image into a contiguous binary image.
///
/// The returned image is sized to the S-record image's own extent (its highest
/// written ROM offset + 1); any gaps within that extent are filled with
/// [`UNWRITTEN_BYTE`](crate::UNWRITTEN_BYTE).  `load_address` is subtracted
/// from every record's absolute address to yield its ROM offset; a record
/// addressing a byte below `load_address` is an error.  Reconciling the
/// returned image against the target chip size (padding/truncating) is the
/// caller's responsibility — this returns exactly what the S-record file
/// describes.
///
/// See the [module documentation](self) for the record types and the validation
/// and policy rules.
pub fn decode_srec(input: &[u8], load_address: usize) -> Result<Vec<u8>, SrecError> {
    let mut acc = ImageAccumulator::new();
    let mut seen_terminator = false;
    let mut data_records: usize = 0;
    // The first S5/S6 count seen, validated against `data_records` at the end
    // so its position in the file does not matter.
    let mut declared_count: Option<usize> = None;

    for (idx, raw_line) in input.split(|&b| b == b'\n').enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim_ascii();
        if line.is_empty() {
            continue;
        }
        // Ignore anything after the termination record.
        if seen_terminator {
            break;
        }

        let record = parse_record(line, line_no)?;
        match record.record_type {
            // Header: a free-text module name, validated above but carrying no
            // ROM data.
            0 => {}
            // Data, at 16-, 24- and 32-bit addresses respectively.
            1..=3 => {
                if !record.data.is_empty() {
                    data_records += 1;
                }
                acc.place(record.address, load_address, &record.data)
                    .map_err(|e| SrecError::from_place(e, line_no))?;
            }
            // Record count: the count is carried in the address field.
            5 | 6 => {
                declared_count.get_or_insert(record.address);
            }
            // Termination, carrying an execution start address.
            7..=9 => seen_terminator = true,
            other => {
                return Err(SrecError::UnsupportedRecordType {
                    line: line_no,
                    record_type: other,
                });
            }
        }
    }

    if let Some(declared) = declared_count
        && declared != data_records
    {
        return Err(SrecError::RecordCountMismatch {
            declared,
            actual: data_records,
        });
    }
    if acc.is_empty() {
        return Err(SrecError::NoData);
    }

    Ok(acc.into_image())
}

/// Parses one already-trimmed, non-empty S-record line.
fn parse_record(line: &[u8], line_no: usize) -> Result<Record, SrecError> {
    if !matches!(line.first(), Some(b'S') | Some(b's')) {
        return Err(SrecError::MissingS { line: line_no });
    }
    // The type is a single digit; everything after it is hex pairs.
    let record_type = match line.get(1) {
        Some(&c @ b'0'..=b'9') => c - b'0',
        Some(_) => {
            return Err(SrecError::BadHex { line: line_no });
        }
        None => return Err(SrecError::BadLength { line: line_no }),
    };
    let Some(width) = address_width(record_type) else {
        return Err(SrecError::UnsupportedRecordType {
            line: line_no,
            record_type,
        });
    };

    let bytes = decode_hex_pairs(&line[2..]).ok_or(SrecError::BadHex { line: line_no })?;

    // Minimum record is count(1) + address(width) + checksum(1).
    if bytes.len() < width + 2 {
        return Err(SrecError::BadLength { line: line_no });
    }
    // The count covers everything after itself: address, data and checksum.
    let count = bytes[0] as usize;
    if bytes.len() != count + 1 {
        return Err(SrecError::BadLength { line: line_no });
    }

    // Ones' complement checksum over the count, address and data bytes.
    // (Intel HEX uses a two's complement instead — see `ihex`.)
    let sum = bytes[..bytes.len() - 1]
        .iter()
        .fold(0u8, |acc, &b| acc.wrapping_add(b));
    let expected = !sum;
    let actual = bytes[bytes.len() - 1];
    if expected != actual {
        return Err(SrecError::BadChecksum {
            line: line_no,
            expected,
            actual,
        });
    }

    let address = bytes[1..1 + width]
        .iter()
        .fold(0usize, |acc, &b| (acc << 8) | b as usize);
    let data = bytes[1 + width..bytes.len() - 1].to_vec();
    Ok(Record {
        record_type,
        address,
        data,
    })
}

/// The data record type — 1, 2 or 3 — narrow enough to address `top_address`.
fn data_record_type(top_address: usize) -> u8 {
    if top_address <= 0xFFFF {
        1
    } else if top_address <= 0xFF_FFFF {
        2
    } else {
        3
    }
}

/// Encodes a binary image as Motorola S-record text.
///
/// Produces an empty `S0` header, 16-byte data records, an `S5`/`S6` record
/// count and a matching termination record, in uppercase hex with CRLF line
/// endings.  Records are addressed starting at `load_address`.
///
/// One data record type is used throughout the file — the narrowest that can
/// address its highest byte, so an image that fits below 64 KB is emitted as
/// `S1` records, which is what the older tooling that only speaks `S1`
/// expects.  The termination record matches: `S9` for `S1`, `S8` for `S2`,
/// `S7` for `S3`.  Its start address is zero, the convention for an image with
/// no entry point, which is what a ROM image is.
///
/// This is deliberately byte-for-byte the same wire format as One ROM Lab's
/// ROM-dump emitter (`rust/lab/src/output/srec.rs`): lab keeps its own
/// no-alloc, streaming implementation for the embedded firmware, and the two
/// must not drift.  `decode_srec(encode_srec(data, la), la)` round-trips to
/// `data`.
pub fn encode_srec(data: &[u8], load_address: usize) -> String {
    let mut out = String::new();

    // Empty header: count 0x03 (two address bytes plus the checksum), address
    // zero, no data.
    push_record(&mut out, 0, 2, 0, &[]);

    let top_address = load_address + data.len().saturating_sub(1);
    let record_type = data_record_type(top_address);
    let width = address_width(record_type).expect("data record types have a width");

    let mut records: usize = 0;
    let mut offset = 0;
    while offset < data.len() {
        let chunk_len = (data.len() - offset).min(16);
        push_record(
            &mut out,
            record_type,
            width,
            load_address + offset,
            &data[offset..offset + chunk_len],
        );
        records += 1;
        offset += chunk_len;
    }

    // Record count: S5 while it fits a 16-bit field, S6 beyond that.
    if records <= 0xFFFF {
        push_record(&mut out, 5, 2, records, &[]);
    } else {
        push_record(&mut out, 6, 3, records, &[]);
    }

    // Termination, paired with the data record type, with no start address.
    let terminator = match record_type {
        1 => 9,
        2 => 8,
        _ => 7,
    };
    push_record(&mut out, terminator, width, 0, &[]);

    out
}

/// Appends one S-record — `S<type><count><address><data><checksum>` — plus
/// CRLF, with `width` bytes of address.
fn push_record(out: &mut String, record_type: u8, width: usize, address: usize, data: &[u8]) {
    // The count covers the address bytes, the data bytes and the checksum.
    let count = (width + data.len() + 1) as u8;

    out.push('S');
    out.push((b'0' + record_type) as char);
    push_hex8(out, count);

    let mut csum = count;
    for i in (0..width).rev() {
        let byte = (address >> (i * 8)) as u8;
        push_hex8(out, byte);
        csum = csum.wrapping_add(byte);
    }
    for &b in data {
        push_hex8(out, b);
        csum = csum.wrapping_add(b);
    }

    push_hex8(out, !csum);
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MAX_IMAGE_SIZE, UNWRITTEN_BYTE};

    /// Builds one S-record line of the given type and address width.
    fn record(record_type: u8, width: usize, address: usize, data: &[u8]) -> String {
        let mut line = String::new();
        push_record(&mut line, record_type, width, address, data);
        // Trim the CRLF: the tests join lines themselves.
        line.trim_end().into()
    }

    /// An S1 data record at a 16-bit address.
    fn s1(address: u16, data: &[u8]) -> String {
        record(1, 2, address as usize, data)
    }

    /// The S9 terminator paired with S1 records.
    const S9: &str = "S9030000FC";

    #[test]
    fn decodes_contiguous_image() {
        let src = alloc::format!("{}\n{}\n", s1(0, &[0xDE, 0xAD, 0xBE, 0xEF]), S9);
        let out = decode_srec(src.as_bytes(), 0).unwrap();
        assert_eq!(out, alloc::vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn terminator_constant_matches_the_encoder() {
        // Guards the hand-written S9 the tests above use against a change in
        // how `push_record` frames a terminator.
        assert_eq!(record(9, 2, 0, &[]), S9);
    }

    #[test]
    fn fills_internal_gaps_with_blank_byte() {
        let src = alloc::format!(
            "{}\n{}\n{}\n",
            s1(0, &[0x11, 0x22]),
            s1(4, &[0x33, 0x44]),
            S9
        );
        let out = decode_srec(src.as_bytes(), 0).unwrap();
        assert_eq!(
            out,
            alloc::vec![0x11, 0x22, UNWRITTEN_BYTE, UNWRITTEN_BYTE, 0x33, 0x44]
        );
    }

    #[test]
    fn tolerates_crlf_blank_lines_and_lower_case() {
        let src = alloc::format!("\r\n{}\r\n\r\n{}\r\n", s1(0, &[0xAB]).to_lowercase(), S9);
        let out = decode_srec(src.as_bytes(), 0).unwrap();
        assert_eq!(out, alloc::vec![0xAB]);
    }

    #[test]
    fn applies_load_address_offset() {
        let src = alloc::format!("{}\n{}\n", s1(0xE000, &[0x01, 0x02]), S9);
        let out = decode_srec(src.as_bytes(), 0xE000).unwrap();
        assert_eq!(out, alloc::vec![0x01, 0x02]);
    }

    #[test]
    fn address_below_load_is_an_error() {
        let src = alloc::format!("{}\n{}\n", s1(0x00, &[0x01]), S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0x10),
            Err(SrecError::AddressBelowLoad { .. })
        ));
    }

    #[test]
    fn header_record_is_ignored() {
        // A conventional "HDR" S0 header, not the empty one the encoder emits.
        let hdr = record(0, 2, 0, b"HDR");
        let src = alloc::format!("{}\n{}\n{}\n", hdr, s1(0, &[0x42]), S9);
        assert_eq!(decode_srec(src.as_bytes(), 0).unwrap(), alloc::vec![0x42]);
    }

    #[test]
    fn decodes_24_and_32_bit_data_records() {
        // S2 at a 24-bit address and S3 at a 32-bit one, both beyond 64 KB.
        let s2 = record(2, 3, 0x01_0000, &[0xAA]);
        let src = alloc::format!("{}\n{}\n", s2, S9);
        let out = decode_srec(src.as_bytes(), 0x01_0000).unwrap();
        assert_eq!(out, alloc::vec![0xAA]);

        let s3 = record(3, 4, 0x01_0000, &[0xBB]);
        let src = alloc::format!("{}\n{}\n", s3, S9);
        let out = decode_srec(src.as_bytes(), 0x01_0000).unwrap();
        assert_eq!(out, alloc::vec![0xBB]);
    }

    #[test]
    fn mixed_width_data_records_are_accepted() {
        // The same absolute addresses, spelled at three different widths.
        let src = alloc::format!(
            "{}\n{}\n{}\n{}\n",
            record(1, 2, 0, &[0x01]),
            record(2, 3, 1, &[0x02]),
            record(3, 4, 2, &[0x03]),
            S9
        );
        let out = decode_srec(src.as_bytes(), 0).unwrap();
        assert_eq!(out, alloc::vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn any_terminator_width_is_accepted() {
        // S1 data closed by S7 rather than the conventional S9.
        let src = alloc::format!("{}\n{}\n", s1(0, &[0x01]), record(7, 4, 0, &[]));
        assert_eq!(decode_srec(src.as_bytes(), 0).unwrap(), alloc::vec![0x01]);
    }

    #[test]
    fn a_missing_terminator_is_accepted() {
        // `srec_cat` writes no termination record unless given an execution
        // start address, so its default output ends at the count record.
        let src = alloc::format!("{}\n", s1(0, &[0x01]));
        assert_eq!(decode_srec(src.as_bytes(), 0).unwrap(), alloc::vec![0x01]);
    }

    #[test]
    fn bytes_after_terminator_are_ignored() {
        let src = alloc::format!(
            "{}\n{}\n{}\n",
            s1(0, &[0x01]),
            S9,
            "this is not a valid record"
        );
        assert_eq!(decode_srec(src.as_bytes(), 0).unwrap(), alloc::vec![0x01]);
    }

    #[test]
    fn overlapping_records_are_an_error() {
        let src = alloc::format!("{}\n{}\n{}\n", s1(0, &[0x01, 0x02]), s1(1, &[0x03]), S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0),
            Err(SrecError::OverlappingData { offset: 1 })
        ));
    }

    #[test]
    fn bad_checksum_is_an_error() {
        // A valid record with its last checksum digit corrupted.
        let mut line = s1(0, &[0x01, 0x02]);
        line.pop();
        line.push('0');
        let src = alloc::format!("{}\n{}\n", line, S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0),
            Err(SrecError::BadChecksum { .. })
        ));
    }

    #[test]
    fn checksum_is_ones_complement_not_twos() {
        // The two differ by one, so a record carrying the Intel HEX-style
        // two's complement of the same bytes must be rejected.  This is the
        // divergence most likely to be introduced by copying from `ihex`.
        let good = s1(0, &[0x01, 0x02]);
        let bytes = decode_hex_pairs(&good.as_bytes()[2..]).unwrap();
        let sum = bytes[..bytes.len() - 1]
            .iter()
            .fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(bytes[bytes.len() - 1], !sum);
        assert_eq!(
            bytes[bytes.len() - 1],
            0u8.wrapping_sub(sum).wrapping_sub(1)
        );

        let mut twos = String::from(&good[..good.len() - 2]);
        twos.push_str(&alloc::format!("{:02X}", 0u8.wrapping_sub(sum)));
        let src = alloc::format!("{}\n{}\n", twos, S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0),
            Err(SrecError::BadChecksum { .. })
        ));
    }

    #[test]
    fn record_count_is_validated() {
        // S5 declaring the right number of data records passes...
        let src = alloc::format!(
            "{}\n{}\n{}\n{}\n",
            s1(0, &[0x01]),
            s1(1, &[0x02]),
            record(5, 2, 2, &[]),
            S9
        );
        assert_eq!(
            decode_srec(src.as_bytes(), 0).unwrap(),
            alloc::vec![0x01, 0x02]
        );

        // ...and one declaring the wrong number is an error, which is how a
        // truncated file is caught.
        let src = alloc::format!("{}\n{}\n{}\n", s1(0, &[0x01]), record(5, 2, 2, &[]), S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0),
            Err(SrecError::RecordCountMismatch {
                declared: 2,
                actual: 1
            })
        ));
    }

    #[test]
    fn record_count_before_the_data_is_accepted() {
        // Validation happens at the end of the file, so a count record in an
        // unconventional position still works.
        let src = alloc::format!("{}\n{}\n{}\n", record(5, 2, 1, &[]), s1(0, &[0x01]), S9);
        assert_eq!(decode_srec(src.as_bytes(), 0).unwrap(), alloc::vec![0x01]);
    }

    #[test]
    fn reserved_s4_is_an_error() {
        let src = alloc::format!("S4030000FB\n{}\n", S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0),
            Err(SrecError::UnsupportedRecordType { record_type: 4, .. })
        ));
    }

    #[test]
    fn no_data_records_is_an_error() {
        assert!(matches!(
            decode_srec(S9.as_bytes(), 0),
            Err(SrecError::NoData)
        ));
    }

    #[test]
    fn empty_data_records_do_not_count_as_data() {
        // An address-only S1 carries no image content.
        let src = alloc::format!("{}\n{}\n", s1(0x100, &[]), S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0),
            Err(SrecError::NoData)
        ));
    }

    #[test]
    fn bad_hex_is_an_error() {
        let src = alloc::format!("S10500000ZZ\n{}\n", S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0),
            Err(SrecError::BadHex { .. })
        ));
    }

    #[test]
    fn missing_s_marker_is_an_error() {
        let src = alloc::format!(":0400000000010203F6\n{}\n", S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0),
            Err(SrecError::MissingS { .. })
        ));
    }

    #[test]
    fn bad_byte_count_is_an_error() {
        // A well-formed record whose count byte claims one byte too many.
        let good = s1(0, &[0x01, 0x02]);
        let mut bytes = decode_hex_pairs(&good.as_bytes()[2..]).unwrap();
        bytes[0] += 1;
        let sum = bytes[..bytes.len() - 1]
            .iter()
            .fold(0u8, |acc, &b| acc.wrapping_add(b));
        let last = bytes.len() - 1;
        bytes[last] = !sum;
        let mut line = String::from("S1");
        for b in bytes {
            line.push_str(&alloc::format!("{b:02X}"));
        }
        let src = alloc::format!("{}\n{}\n", line, S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0),
            Err(SrecError::BadLength { .. })
        ));
    }

    #[test]
    fn oversized_image_is_an_error() {
        let src = alloc::format!("{}\n{}\n", record(3, 4, MAX_IMAGE_SIZE, &[0x01]), S9);
        assert!(matches!(
            decode_srec(src.as_bytes(), 0),
            Err(SrecError::ImageTooLarge { .. })
        ));
    }

    #[test]
    fn encode_matches_expected_wire_format() {
        // Golden reference. This exact byte layout — an empty S0 header, one
        // data record type throughout, 16-byte data records, an S5 count, a
        // paired terminator with a zero start address, uppercase hex and CRLF
        // endings — is also what lab's `output/srec.rs` emits; the two must
        // stay identical.  A change here that is not mirrored in lab (or vice
        // versa) is a regression.
        let out = encode_srec(&[0x00, 0x01, 0x02, 0x03], 0);
        assert_eq!(
            out,
            concat!(
                "S0030000FC\r\n",         // empty header
                "S107000000010203F2\r\n", // 4 data bytes at 0x0000
                "S5030001FB\r\n",         // one data record
                "S9030000FC\r\n",         // terminator, no start address
            )
        );
    }

    #[test]
    fn encode_picks_the_narrowest_data_record_type() {
        // Top address inside 64 KB -> S1/S9.
        let out = encode_srec(&[0x00], 0xFFFF);
        assert!(out.contains("S104FFFF00FD\r\n"), "{out}");
        assert!(out.ends_with("S9030000FC\r\n"), "{out}");

        // Just beyond -> S2/S8.
        let out = encode_srec(&[0x00], 0x1_0000);
        assert!(out.contains("S20501000000F9\r\n"), "{out}");
        assert!(out.ends_with("S804000000FB\r\n"), "{out}");

        // Beyond 24 bits -> S3/S7.
        let out = encode_srec(&[0x00], 0x0100_0000);
        assert!(out.contains("S30601000000"), "{out}");
        assert!(out.ends_with("S70500000000FA\r\n"), "{out}");
    }

    #[test]
    fn encode_decode_round_trips() {
        // Contiguous images round-trip exactly (no internal gaps to fill).
        for len in [1usize, 15, 16, 17, 256, 8192] {
            let data: Vec<u8> = (0..len)
                .map(|i| (i.wrapping_mul(37) ^ 0x5A) as u8)
                .collect();
            for la in [0usize, 0x10, 0xE000, 0x1_0000] {
                let src = encode_srec(&data, la);
                let back = decode_srec(src.as_bytes(), la).unwrap();
                assert_eq!(back, data, "round-trip failed at len={len}, la={la:#x}");
            }
        }
    }

    #[test]
    fn encode_of_a_large_image_round_trips() {
        // Crosses 64 KB, so it is emitted as S2 records with an S8 terminator,
        // and carries enough records to exercise the S5 count.
        let data: Vec<u8> = (0..0x1_0001).map(|i| (i % 251) as u8).collect();
        let src = encode_srec(&data, 0);
        assert!(src.contains("\r\nS8"), "expected an S8 terminator");
        assert_eq!(decode_srec(src.as_bytes(), 0).unwrap(), data);
    }

    #[test]
    fn encode_empty_is_header_count_and_terminator() {
        assert_eq!(
            encode_srec(&[], 0),
            concat!("S0030000FC\r\n", "S5030000FC\r\n", "S9030000FC\r\n",)
        );
    }
}
