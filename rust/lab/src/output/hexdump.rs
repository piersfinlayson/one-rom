// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - Hex dump output
//!
//! Produces output in the style of `hexdump -C`:
//! ```text
//! 00000000  00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  |................|
//! ```
//!
//! Lines are 16 bytes wide.  The final line may be shorter if `count` is not
//! a multiple of 16.
//!
//! Control lines are asserted and deasserted around each 16-byte read so
//! that the executor can be yielded between lines without holding a mutable
//! borrow across an `.await` point.

use embassy_time::Timer;

use crate::error::Error;
use crate::rom::RomReader;

// ---------------------------------------------------------------------------
// Line buffer sizing
//
// "AAAAAAAA  HH HH HH HH HH HH HH HH  HH HH HH HH HH HH HH HH  |................|\r\n"
//   8         + 2 + 23 + 2 + 23 + 2 + 18 + 2 = 80 bytes.
// Use 82 to be safe.
// ---------------------------------------------------------------------------
const LINE_BUF: usize = 82;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Dump `count` bytes of ROM starting at byte address `start` in hex dump
/// format, writing to the CDC interface.
///
/// Reads and formats one 16-byte line at a time, yielding to the executor
/// between lines so the USB task can drain the TX channel.
///
/// `start` and `count` must already be resolved against the chip's actual
/// address space (use `commands::resolve_range`).
pub async fn dump(reader: &mut RomReader, start: usize, count: usize) -> Result<(), Error> {
    if count == 0 {
        return Ok(());
    }

    let end = start + count;
    let mut addr = start;

    while addr < end {
        let chunk_len = (end - addr).min(16);
        let mut chunk = [0u8; 16];

        // Assert control lines, read exactly one line's worth of bytes, then
        // deassert.  This keeps the borrow of `reader` confined to a sync
        // scope so it does not cross the `.await` below.
        reader.begin_read(8);
        for i in 0..chunk_len {
            chunk[i] = reader.read_byte_at(addr + i, 8);
        }
        reader.end_read();

        // Format into a stack buffer — no heap allocation.
        let mut line = [0u8; LINE_BUF];
        let n = format_line(&mut line, addr, &chunk[..chunk_len]);

        let s = core::str::from_utf8(&line[..n]).map_err(|_| Error::Buffer)?;
        crate::cli::send(s)?;

        addr += chunk_len;

        // Yield to the executor so the USB task can send buffered data.
        Timer::after_millis(1).await;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Line formatter
// ---------------------------------------------------------------------------

/// Format one hex dump line into `buf`.  Returns the number of bytes written.
///
/// `data` may be shorter than 16 bytes (for the final line of a range that
/// is not a multiple of 16); missing positions are padded with spaces so
/// that the ASCII sidebar column stays aligned.
fn format_line(buf: &mut [u8; LINE_BUF], addr: usize, data: &[u8]) -> usize {
    let len = data.len(); // 1 ..= 16
    let mut pos = 0usize;

    // 8-digit address.
    super::write_hex32(buf, &mut pos, addr as u32);
    buf[pos] = b' ';
    pos += 1;
    buf[pos] = b' ';
    pos += 1;

    // 16 hex bytes in two groups of 8, separated by an extra space.
    for i in 0..16 {
        if i == 8 {
            buf[pos] = b' ';
            pos += 1; // gap between the two groups
        }
        if i < len {
            super::write_hex8(buf, &mut pos, data[i]);
        } else {
            buf[pos] = b' ';
            pos += 1; // pad missing byte: two spaces
            buf[pos] = b' ';
            pos += 1;
        }
        if i < 15 {
            buf[pos] = b' ';
            pos += 1; // space between bytes
        }
    }

    // ASCII sidebar.
    buf[pos] = b' ';
    pos += 1;
    buf[pos] = b' ';
    pos += 1;
    buf[pos] = b'|';
    pos += 1;
    for i in 0..16 {
        buf[pos] = if i < len {
            let c = data[i];
            if (0x20..0x7F).contains(&c) { c } else { b'.' }
        } else {
            b' '
        };
        pos += 1;
    }
    buf[pos] = b'|';
    pos += 1;

    buf[pos] = b'\r';
    pos += 1;
    buf[pos] = b'\n';
    pos += 1;

    pos
}
