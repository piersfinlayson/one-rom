// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - continuous fast read
//!
//! Reads the chip in the socket pass after pass and compares every pass with
//! the first.  Each read sets every address pin with one GPIO write, asserts
//! the strobe line, samples every data pin with one GPIO read and deasserts the
//! strobe line, so the data lines are driven and released on every read.
//!
//! The comparison is of raw GPIO bits.  A byte is decoded to D0-D7 only to
//! report it.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::iter::once;
use embassy_rp::pac;
use sha1::{Digest, Sha1};

use crate::error::Error;
use crate::rom::FastReadPins;

/// The largest chip a fast read supports.  Every 24 and 28 pin chip fits.
pub const MAX_BYTES: usize = 64 * 1024;

/// Mismatches kept to list address by address.
pub const MAX_LISTED: usize = 8;

/// The default cycles at 150 MHz between setting an address and sampling the
/// data.
///
/// Every address pin and the strobe change at once, so this is longer than
/// the delay a normal read uses.  A Fire 28 C read early at 8 cycles and
/// cleanly at 12.
pub const DEFAULT_DELAY_CYCLES: u32 = 16;

/// The longest delay accepted.  A pass at this delay takes about half a
/// second, so a key still stops a fast read promptly.
pub const MAX_DELAY_CYCLES: u32 = 1000;

/// The first pass, as raw data bits.  Static rather than on the 16KB heap.
struct Reference(UnsafeCell<[u8; MAX_BYTES]>);

// Only the CLI task runs a fast read, one at a time.
unsafe impl Sync for Reference {}

static REFERENCE: Reference = Reference(UnsafeCell::new([0; MAX_BYTES]));

fn reference() -> &'static mut [u8; MAX_BYTES] {
    unsafe { &mut *REFERENCE.0.get() }
}

/// A mismatch against the first pass.
pub struct Mismatch {
    pub pass: u64,
    pub addr: usize,
    pub expected: u8,
    pub read: u8,
}

struct Tables {
    /// GPIO output bits for address bits 0-7
    addr_lo: [u32; 256],
    /// GPIO output bits for address bits 8-15
    addr_hi: [u32; 256],
    /// Raw data bits to the byte on D0-D7
    decode: [u8; 256],
}

pub struct FastReader {
    tables: Box<Tables>,
    strobe: u32,
    data_shift: u32,
    /// The data line each raw data bit is wired to
    raw_to_line: [u8; 8],
    size: usize,
    delay_cycles: u32,
    /// Address output bits currently driven
    addr_out: u32,
    pub passes: u64,
    pub mismatches: u64,
    /// Mismatches on each data line, D0 first
    pub line_mismatches: [u64; 8],
    pub listed: Vec<Mismatch>,
}

impl FastReader {
    /// `pins` must come from [`crate::rom::RomReader::begin_fast_read`] with
    /// every address pin driven low.
    pub fn new(pins: &FastReadPins, size: usize, delay_cycles: u32) -> Result<Self, Error> {
        if size > MAX_BYTES {
            return Err(Error::FastReadChipSize);
        }
        let strobe_gpio = pins.strobe_gpio.ok_or(Error::FastReadPins)?;
        let mut all = pins
            .addr_gpios
            .iter()
            .chain(&pins.data_gpios)
            .chain(once(&strobe_gpio));
        if pins.addr_gpios.len() > 16 || pins.data_gpios.len() != 8 || all.any(|&gpio| gpio >= 32) {
            return Err(Error::FastReadPins);
        }

        // The data GPIOs must span 8 or fewer bits to be read with a shift.
        let data_shift = *pins.data_gpios.iter().min().ok_or(Error::FastReadPins)?;
        let data_top = *pins.data_gpios.iter().max().ok_or(Error::FastReadPins)?;
        if data_top - data_shift > 7 {
            return Err(Error::FastReadPins);
        }
        let mut raw_to_line = [0u8; 8];
        for (line, &gpio) in pins.data_gpios.iter().enumerate() {
            raw_to_line[(gpio - data_shift) as usize] = line as u8;
        }

        let mut tables = Box::new(Tables {
            addr_lo: [0; 256],
            addr_hi: [0; 256],
            decode: [0; 256],
        });
        for value in 0..256usize {
            for (bit, &gpio) in pins.addr_gpios.iter().enumerate() {
                if bit < 8 && value & (1 << bit) != 0 {
                    tables.addr_lo[value] |= 1 << gpio;
                }
                if bit >= 8 && value & (1 << (bit - 8)) != 0 {
                    tables.addr_hi[value] |= 1 << gpio;
                }
            }
            for (raw_bit, &line) in raw_to_line.iter().enumerate() {
                if value & (1 << raw_bit) != 0 {
                    tables.decode[value] |= 1 << line;
                }
            }
        }

        Ok(Self {
            tables,
            strobe: 1 << strobe_gpio,
            data_shift: data_shift as u32,
            raw_to_line,
            size,
            delay_cycles,
            addr_out: 0,
            passes: 0,
            mismatches: 0,
            line_mismatches: [0; 8],
            listed: Vec::new(),
        })
    }

    /// Read one address and return its raw data bits.
    ///
    /// XOR writes change only the address and strobe bits, so the strobe
    /// works whatever its polarity, provided it starts deasserted.
    #[inline(always)]
    fn read(&mut self, addr: usize) -> u8 {
        let out = pac::SIO.gpio_out(0);
        let target = self.tables.addr_lo[addr & 0xFF] | self.tables.addr_hi[addr >> 8];
        out.value_xor()
            .write_value((self.addr_out ^ target) | self.strobe);
        self.addr_out = target;
        cortex_m::asm::delay(self.delay_cycles);
        let raw = (pac::SIO.gpio_in(0).read() >> self.data_shift) as u8;
        out.value_xor().write_value(self.strobe);
        raw
    }

    /// Read the first pass and return the SHA-1 of the bytes it read.
    pub fn read_reference(&mut self) -> [u8; 20] {
        let reference = &mut reference()[..self.size];
        for (addr, byte) in reference.iter_mut().enumerate() {
            *byte = self.read(addr);
        }
        let mut sha = Sha1::new();
        for &raw in reference.iter() {
            sha.update([self.tables.decode[raw as usize]]);
        }
        let mut sha1 = [0u8; 20];
        sha1.copy_from_slice(&sha.finalize());
        sha1
    }

    /// Read a pass and compare it with the first.
    #[inline(never)]
    pub fn compare_pass(&mut self) {
        self.passes += 1;
        let reference = &reference()[..self.size];
        for (addr, &expected) in reference.iter().enumerate() {
            let raw = self.read(addr);
            if raw != expected {
                self.record(addr, expected, raw);
            }
        }
    }

    #[cold]
    #[inline(never)]
    fn record(&mut self, addr: usize, expected: u8, raw: u8) {
        self.mismatches += 1;
        let diff = expected ^ raw;
        for (raw_bit, &line) in self.raw_to_line.iter().enumerate() {
            if diff & (1 << raw_bit) != 0 {
                self.line_mismatches[line as usize] += 1;
            }
        }
        if self.listed.len() < MAX_LISTED {
            self.listed.push(Mismatch {
                pass: self.passes,
                addr,
                expected: self.tables.decode[expected as usize],
                read: self.tables.decode[raw as usize],
            });
        }
    }

    /// Reads compared with the first pass.
    pub fn reads(&self) -> u64 {
        self.passes * self.size as u64
    }
}
