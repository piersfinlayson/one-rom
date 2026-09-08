// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - Motorola S-record output
//!
//! This is a deliberate duplicate of `onerom_gen::encode_srec`: lab is
//! embedded (`no_std`, `thumbv8m`) and streams records over USB one at a time
//! from stack buffers, so it cannot use gen's whole-image, allocating encoder.
//! **Both must emit byte-for-byte identical output.** `onerom-gen`'s
//! `srec::tests::encode_matches_expected_wire_format` pins that exact wire
//! format (empty S0 header, one data record type throughout, 16-byte data
//! records, an S5 count, a paired terminator with a zero start address,
//! uppercase hex, CRLF); any change to the format here must be mirrored there,
//! and vice versa.  The same contract applies between `output/ihex.rs` and
//! gen's `encode_ihex`.
//!
//! One data record type is used for the whole dump — the narrowest that can
//! address its highest byte — so an image that fits below 64 KB is emitted as
//! `S1` records, which is what the older tooling that only speaks `S1`
//! expects.  Since `start` and `count` are both known before the first record
//! is emitted, that choice is made up front and the dump still streams.
//!
//! Record format reference:
//! ```text
//! S<T><LL><AA..><DD..><CC>
//!   T     record type (0 = header, 1/2/3 = data, 5 = count, 7/8/9 = end)
//!   LL    byte count: address bytes + data bytes + checksum byte
//!   AA..  address, 2 bytes (S1/S5/S9), 3 bytes (S2/S8) or 4 bytes (S3/S7)
//!   DD..  data bytes
//!   CC    checksum: ones' complement of (LL + sum(AA) + sum(DD))
//! ```
//!
//! Note the checksum differs from Intel HEX's, which is a *two's* complement.
//!
//! Control lines are asserted and deasserted around each 16-byte read so
//! that the executor can be yielded between records without holding a mutable
//! borrow across an `.await` point.

use embassy_time::Timer;

use crate::error::Error;
use crate::rom::RomReader;

// ---------------------------------------------------------------------------
// Buffer sizes (all stack-allocated)
//
// Worst case is an S3 data record:
//   "S3" + count(2) + address(8) + 32 hex data + checksum(2) + "\r\n" = 48
// Every other record this module emits is shorter.
// ---------------------------------------------------------------------------
const RECORD_BUF: usize = 48;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Dump `count` bytes of ROM starting at byte address `start` as S-records,
/// writing to the CDC interface.
///
/// Reads and emits one 16-byte data record at a time, yielding to the
/// executor between records so the USB task can drain the TX channel.
///
/// `start` and `count` must already be resolved against the chip's actual
/// address space (use `commands::resolve_range`).
pub async fn dump(reader: &mut RomReader, start: usize, count: usize) -> Result<(), Error> {
    if count == 0 {
        return Ok(());
    }

    // Empty header, as gen's encoder emits.
    send_record(0, 2, 0, &[])?;

    let end = start + count;
    // One record type for the whole dump, chosen from its highest address.
    let record_type = data_record_type(end - 1);
    let width = address_width(record_type);

    let mut addr = start;
    let mut records: usize = 0;

    while addr < end {
        let chunk_len = (end - addr).min(16);
        let mut chunk = [0u8; 16];

        // Assert control lines, read one record's worth of bytes, deassert.
        reader.begin_read(8);
        for (i, byte) in chunk.iter_mut().enumerate().take(chunk_len) {
            *byte = reader.read_byte_at(addr + i, 8);
        }
        reader.end_read();

        send_record(record_type, width, addr, &chunk[..chunk_len])?;
        records += 1;
        addr += chunk_len;

        // Yield to the executor so the USB task can send buffered data.
        Timer::after_millis(1).await;
    }

    // Record count: S5 while it fits a 16-bit field, S6 beyond that.
    if records <= 0xFFFF {
        send_record(5, 2, records, &[])?;
    } else {
        send_record(6, 3, records, &[])?;
    }

    // Termination, paired with the data record type, with no start address.
    let terminator = match record_type {
        1 => 9,
        2 => 8,
        _ => 7,
    };
    send_record(terminator, width, 0, &[])
}

// ---------------------------------------------------------------------------
// Record emitters
// ---------------------------------------------------------------------------

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

/// The width in bytes of the address field for a record type.
fn address_width(record_type: u8) -> usize {
    match record_type {
        2 | 6 | 8 => 3,
        3 | 7 => 4,
        _ => 2,
    }
}

/// Emit one S-record with `width` bytes of address.
fn send_record(record_type: u8, width: usize, address: usize, data: &[u8]) -> Result<(), Error> {
    debug_assert!(data.len() <= 16);
    debug_assert!((2..=4).contains(&width));

    let mut buf = [0u8; RECORD_BUF];
    let mut pos = 0usize;

    // The count covers the address bytes, the data bytes and the checksum.
    let count = (width + data.len() + 1) as u8;

    buf[pos] = b'S';
    pos += 1;
    buf[pos] = b'0' + record_type;
    pos += 1;
    super::write_hex8(&mut buf, &mut pos, count);

    let mut csum = count;
    for i in (0..width).rev() {
        let byte = (address >> (i * 8)) as u8;
        super::write_hex8(&mut buf, &mut pos, byte);
        csum = csum.wrapping_add(byte);
    }
    for &b in data {
        super::write_hex8(&mut buf, &mut pos, b);
        csum = csum.wrapping_add(b);
    }

    super::write_hex8(&mut buf, &mut pos, !csum); // ones' complement
    buf[pos] = b'\r';
    pos += 1;
    buf[pos] = b'\n';
    pos += 1;

    let s = core::str::from_utf8(&buf[..pos]).map_err(|_| Error::Buffer)?;
    crate::cli::send(s)
}
