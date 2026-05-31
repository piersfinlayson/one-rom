// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - Output formatters
//!
//! Each submodule exposes a single `dump` async function.  The caller
//! (commands.rs) resolves the read range and constructs the `RomReader`
//! before calling; the output module drives the ROM reads and CDC output.

pub mod hexdump;
pub mod ihex;

// ---------------------------------------------------------------------------
// Shared hex-formatting helpers
//
// All output is uppercase hex, matching common ROM tool conventions.
// Functions write directly into a caller-supplied stack buffer to avoid
// heap allocation in the tight per-line loop.
// ---------------------------------------------------------------------------

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Write one byte as two uppercase hex characters into `buf` at `*pos`,
/// then advance `*pos` by 2.
#[inline]
pub fn write_hex8(buf: &mut [u8], pos: &mut usize, val: u8) {
    buf[*pos] = HEX[(val >> 4) as usize];
    buf[*pos + 1] = HEX[(val & 0xF) as usize];
    *pos += 2;
}

/// Write a `u16` as four uppercase hex characters.
#[inline]
pub fn write_hex16(buf: &mut [u8], pos: &mut usize, val: u16) {
    write_hex8(buf, pos, (val >> 8) as u8);
    write_hex8(buf, pos, val as u8);
}

/// Write a `u32` as eight uppercase hex characters.
#[inline]
pub fn write_hex32(buf: &mut [u8], pos: &mut usize, val: u32) {
    write_hex16(buf, pos, (val >> 16) as u16);
    write_hex16(buf, pos, val as u16);
}
