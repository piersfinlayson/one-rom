// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - Intel HEX output
//!
//! Produces standard Intel HEX with 16-byte data records (`:10...`).
//! Extended linear address records (type 04) are emitted whenever the upper
//! 16 bits of the byte address change, including before the first record.
//! An EOF record (`:00000001FF`) closes the output.
//!
//! For a 512 KB ROM starting at address 0 this produces 8 extended address
//! records (one per 64 KB segment) and 32 768 data records.
//!
//! Record format reference:
//! ```text
//! :LLAAAATTDD...DDCC
//!   LL   byte count
//!   AAAA address (lower 16 bits of byte address)
//!   TT   record type (00 = data, 01 = EOF, 04 = extended linear address)
//!   DD   data bytes
//!   CC   checksum: two's complement of (LL + AH + AL + TT + sum(DD))
//! ```
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
// Data record:            ":10AAAA00" + 32 hex data + "CC\r\n" = 45 bytes
// Extended linear addr:   ":02000004HHHH" + "CC\r\n"           = 17 bytes
// EOF:                    ":00000001FF\r\n"                      = 13 bytes
// ---------------------------------------------------------------------------
const DATA_RECORD_BUF: usize = 45;
const EXTENDED_ADDR_BUF: usize = 17;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Dump `count` bytes of ROM starting at byte address `start` as Intel HEX,
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

    let end = start + count;
    let mut addr = start;
    let mut current_upper: Option<u16> = None;

    while addr < end {
        let chunk_len = (end - addr).min(16);
        let mut chunk = [0u8; 16];

        // Emit an extended linear address record whenever the upper 16 bits
        // of the byte address change (including before the very first record).
        let upper = (addr >> 16) as u16;
        if current_upper != Some(upper) {
            send_extended_linear_addr(upper)?;
            current_upper = Some(upper);
        }

        // Assert control lines, read one record's worth of bytes, deassert.
        reader.begin_read(8);
        for i in 0..chunk_len {
            chunk[i] = reader.read_byte_at(addr + i, 8);
        }
        reader.end_read();

        send_data_record(addr as u32, &chunk[..chunk_len])?;

        addr += chunk_len;

        // Yield to the executor so the USB task can send buffered data.
        Timer::after_millis(1).await;
    }

    send_eof_record()
}

// ---------------------------------------------------------------------------
// Record emitters
// ---------------------------------------------------------------------------

/// Emit a type-00 (data) record.
///
/// `addr` is the full 32-bit byte address; only the lower 16 bits are
/// encoded in the record itself.  The caller is responsible for emitting
/// extended linear address records for the upper bits.
fn send_data_record(addr: u32, data: &[u8]) -> Result<(), Error> {
    debug_assert!(!data.is_empty() && data.len() <= 16);

    let mut buf = [0u8; DATA_RECORD_BUF];
    let mut pos = 0usize;
    let byte_count = data.len() as u8;

    buf[pos] = b':';
    pos += 1;
    super::write_hex8(&mut buf, &mut pos, byte_count);
    super::write_hex16(&mut buf, &mut pos, addr as u16); // lower 16 bits
    super::write_hex8(&mut buf, &mut pos, 0x00); // record type: data

    // Checksum accumulator: byte_count + addr_high + addr_low + record_type.
    // Record type 0x00 contributes 0, so it is omitted from the sum.
    let mut csum = byte_count
        .wrapping_add((addr >> 8) as u8)
        .wrapping_add(addr as u8);

    for &b in data {
        super::write_hex8(&mut buf, &mut pos, b);
        csum = csum.wrapping_add(b);
    }

    super::write_hex8(&mut buf, &mut pos, 0u8.wrapping_sub(csum)); // two's complement
    buf[pos] = b'\r';
    pos += 1;
    buf[pos] = b'\n';
    pos += 1;

    let s = core::str::from_utf8(&buf[..pos]).map_err(|_| Error::Buffer)?;
    crate::cli::send(s)
}

/// Emit a type-04 (extended linear address) record.
///
/// `upper` is the upper 16 bits of the 32-bit byte address.  Subsequent
/// data records use `(upper << 16) | record_address` as their full address.
fn send_extended_linear_addr(upper: u16) -> Result<(), Error> {
    let mut buf = [0u8; EXTENDED_ADDR_BUF];
    let mut pos = 0usize;

    buf[pos] = b':';
    pos += 1;
    super::write_hex8(&mut buf, &mut pos, 0x02); // byte count: always 2
    super::write_hex16(&mut buf, &mut pos, 0x0000); // address field: always 0
    super::write_hex8(&mut buf, &mut pos, 0x04); // record type: extended linear address
    super::write_hex16(&mut buf, &mut pos, upper);

    // Checksum: 0x02 + 0x00 + 0x00 + 0x04 + upper_high + upper_low.
    let csum = 0x02u8
        .wrapping_add(0x04)
        .wrapping_add((upper >> 8) as u8)
        .wrapping_add(upper as u8);
    super::write_hex8(&mut buf, &mut pos, 0u8.wrapping_sub(csum));
    buf[pos] = b'\r';
    pos += 1;
    buf[pos] = b'\n';
    pos += 1;

    let s = core::str::from_utf8(&buf[..pos]).map_err(|_| Error::Buffer)?;
    crate::cli::send(s)
}

/// Emit the type-01 (EOF) record.  Always `:00000001FF`.
fn send_eof_record() -> Result<(), Error> {
    crate::cli::send(":00000001FF\r\n")
}
