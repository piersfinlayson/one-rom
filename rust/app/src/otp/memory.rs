// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! An in-memory OTP for tests. It follows the RP2350 bootrom's rules.

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use core::iter;
use core::ops::Range;

use onerom_metadata::OTP_PAGE_ROWS;
use onerom_metadata::otp::pico_otp::ecc_encode;

use super::{OtpAccess, OtpError, ROW_BITS, lock1_row, majority};

/// Rows in OTP.
const ROWS: u16 = 4096;

/// Pages in OTP.
const PAGES: usize = (ROWS / OTP_PAGE_ROWS) as usize;

/// Row of page 0's LOCK0 word. Each page has two lock words and they fill
/// pages 62 and 63.
const LOCK_WORDS_ROW: u16 = 0xf80;

/// Bits 23:22. The bootrom sets both where it inverted a row.
const INVERTED: u32 = 0xc0_0000;

/// The bits holding the value and its six parity bits.
const CODEWORD_BITS: u32 = 0x3f_ffff;

/// LOCK_BL's position in each copy of a LOCK1 word.
const LOCK_BL_SHIFT: u32 = 4;

/// LOCK_BL's width.
const LOCK_BL_BITS: u32 = 0b11;

/// The LOCK_BL value making a page read-only. A lock from this value up
/// refuses writes.
const READ_ONLY: u8 = 1;

/// The first LOCK_BL value refusing reads as well as writes. The datasheet
/// says 2 behaves as 3.
const INACCESSIBLE: u8 = 2;

/// An in-memory RP2350 OTP for tests. It follows the bootrom's rules.
///
/// It holds 4096 raw 24-bit rows. Each starts at 0.
///
/// - A raw write ignores bits 31–24. It's refused where it would clear a set
///   bit.
/// - An ECC write stores the value's encoding. Where that would clear a set bit
///   it stores the encoding inverted. It's refused where both would.
/// - An ECC read inverts a row with bits 23:22 both set. Then it returns:
///   - an exact codeword's value
///   - the corrected value of a codeword with a single wrong bit
///   - bits 15:0 of a row with more damage
/// - [`reset`](Self::reset) loads the page locks. A page locked read-only
///   refuses writes. A page locked inaccessible refuses reads too.
/// - A lock word is always readable. Its own page's lock refuses writes to it.
///
/// [`interrupt`](Self::interrupt) and [`corrupt`](Self::corrupt) make a chosen
/// write go wrong. Writes are counted from 1. Refused writes count too.
#[derive(Debug, Clone)]
pub struct MemoryOtp {
    rows: Vec<u32>,
    /// Each page's LOCK_BL value from the last reset.
    locks: [u8; PAGES],
    /// The number of writes made so far.
    writes: usize,
    interruption: Option<(usize, Interruption)>,
    corruption: Option<(usize, u32)>,
}

/// Whether an interrupted write lands before it fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interruption {
    /// The write lands and then fails as though its reply was lost.
    Landed,
    /// The write fails without reaching OTP.
    NotLanded,
}

impl MemoryOtp {
    /// A blank OTP with every page unlocked.
    pub fn new() -> Self {
        Self {
            rows: vec![0; usize::from(ROWS)],
            locks: [0; PAGES],
            writes: 0,
            interruption: None,
            corruption: None,
        }
    }

    /// The raw rows from row 0.
    pub fn rows(&self) -> &[u32] {
        &self.rows
    }

    /// Sets `row` to bits 23–0 of `value`. The bootrom's rules don't apply. The
    /// page locks change only at [`reset`](Self::reset).
    ///
    /// # Panics
    ///
    /// Where `row` is past the end of OTP.
    pub fn set_raw(&mut self, row: u16, value: u32) {
        self.rows[usize::from(row)] = value & ROW_BITS;
    }

    /// Loads each page's lock as a chip reset does. The lock is each LOCK_BL
    /// bit's majority across the three copies in the page's LOCK1 word. Page
    /// n's LOCK1 word is row `0xf81 + 2n`.
    pub fn reset(&mut self) {
        for (page, lock) in (0..).zip(self.locks.iter_mut()) {
            let word = self.rows[usize::from(lock1_row(page))];
            let copies = majority([word, word >> 8, word >> 16]);
            *lock = ((copies >> LOCK_BL_SHIFT) & LOCK_BL_BITS) as u8;
        }
    }

    /// The number of writes made so far. Refused writes count too.
    pub fn write_count(&self) -> usize {
        self.writes
    }

    /// Makes write `n` fail with [`OtpError::Transport`]. `interruption` says
    /// whether it lands first. This replaces an earlier interruption.
    pub fn interrupt(&mut self, n: usize, interruption: Interruption) {
        self.interruption = Some((n, interruption));
    }

    /// Makes write `n` flip the bits of `mask` in the row it stores. This
    /// replaces an earlier corruption.
    pub fn corrupt(&mut self, n: usize, mask: u32) {
        self.corruption = Some((n, mask));
    }

    /// The `count` rows from `row`. Refuses a row that can't be read.
    fn readable(&self, row: u16, count: u16) -> Result<&[u32], OtpError> {
        let rows = range(row, count)?;
        // Lock words are always readable.
        let refused = rows
            .clone()
            .any(|row| row < LOCK_WORDS_ROW && self.locks[page(row)] >= INACCESSIBLE);
        if refused {
            return Err(OtpError::NotPermitted);
        }
        Ok(&self.rows[usize::from(rows.start)..usize::from(rows.end)])
    }

    /// Makes a write to `row`. `new_value` returns the row's new raw value from
    /// its current one.
    fn write(
        &mut self,
        row: u16,
        new_value: impl FnOnce(u32) -> Result<u32, OtpError>,
    ) -> Result<(), OtpError> {
        self.writes += 1;
        let n = self.writes;
        let interruption = self
            .interruption
            .filter(|&(at, _)| at == n)
            .map(|(_, interruption)| interruption);
        if interruption == Some(Interruption::NotLanded) {
            return Err(interrupted(n));
        }
        let stored = self.store(row, new_value);
        match interruption {
            Some(Interruption::Landed) => Err(interrupted(n)),
            Some(Interruption::NotLanded) | None => stored,
        }
    }

    /// Stores the value `new_value` returns for `row`. Refuses a locked row.
    fn store(
        &mut self,
        row: u16,
        new_value: impl FnOnce(u32) -> Result<u32, OtpError>,
    ) -> Result<(), OtpError> {
        range(row, 1)?;
        if self.locks[lock_page(row)] >= READ_ONLY {
            return Err(OtpError::NotPermitted);
        }
        let index = usize::from(row);
        let mut value = new_value(self.rows[index])?;
        if let Some((at, mask)) = self.corruption
            && at == self.writes
        {
            value ^= mask;
        }
        self.rows[index] = value & ROW_BITS;
        Ok(())
    }
}

impl Default for MemoryOtp {
    fn default() -> Self {
        Self::new()
    }
}

impl OtpAccess for MemoryOtp {
    async fn read_ecc(&mut self, row: u16, count: u16) -> Result<Vec<u16>, OtpError> {
        Ok(self
            .readable(row, count)?
            .iter()
            .map(|&raw| ecc_read(raw))
            .collect())
    }

    async fn read_raw(&mut self, row: u16, count: u16) -> Result<Vec<u32>, OtpError> {
        Ok(self.readable(row, count)?.to_vec())
    }

    async fn write_ecc(&mut self, row: u16, value: u16) -> Result<(), OtpError> {
        self.write(row, |stored| {
            let encoding = ecc_encode(value);
            [encoding, !encoding & ROW_BITS]
                .into_iter()
                .find(|&target| keeps(stored, target))
                .ok_or(OtpError::UnsupportedModification)
        })
    }

    async fn write_raw(&mut self, row: u16, value: u32) -> Result<(), OtpError> {
        self.write(row, |stored| {
            let target = value & ROW_BITS;
            if keeps(stored, target) {
                Ok(target)
            } else {
                Err(OtpError::UnsupportedModification)
            }
        })
    }
}

/// The `count` rows from `row`. Refuses rows past the end of OTP.
fn range(row: u16, count: u16) -> Result<Range<u16>, OtpError> {
    match row.checked_add(count) {
        Some(end) if end <= ROWS => Ok(row..end),
        Some(_) | None => Err(OtpError::Transport(format!(
            "{count} rows from row {row:#05x} run past the end of OTP"
        ))),
    }
}

/// The page holding `row`.
fn page(row: u16) -> usize {
    usize::from(row / OTP_PAGE_ROWS)
}

/// The page whose lock refuses writes to `row`. A lock word takes the lock of
/// the page it locks.
fn lock_page(row: u16) -> usize {
    if row >= LOCK_WORDS_ROW {
        usize::from((row - LOCK_WORDS_ROW) / 2)
    } else {
        page(row)
    }
}

/// Whether storing `target` over `stored` keeps every set bit.
fn keeps(stored: u32, target: u32) -> bool {
    stored & !target == 0
}

/// The value an ECC read returns for a row with raw value `raw`.
fn ecc_read(raw: u32) -> u16 {
    // Bit repair by polarity inverts a row with bits 23:22 both set.
    let row = (if raw & INVERTED == INVERTED {
        !raw
    } else {
        raw
    }) & CODEWORD_BITS;
    // An exact codeword and then a codeword with a single wrong bit.
    iter::once(0)
        .chain((0..22).map(|bit| 1_u32 << bit))
        .map(|flip| row ^ flip)
        .find(|&candidate| ecc_encode(candidate as u16) == candidate)
        .map_or(row as u16, |codeword| codeword as u16)
}

/// The error an interrupted write `n` returns.
fn interrupted(n: usize) -> OtpError {
    OtpError::Transport(format!("write {n} was interrupted"))
}
