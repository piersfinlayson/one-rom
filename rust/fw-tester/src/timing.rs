// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! PIO emulator cycle counts for ROM read timing.
//!
//! These are **correctness margins for the bulk read pass**: long enough that
//! the device really is serving this cycle's byte when the pass samples it.  A
//! value below the requirement makes the pass report failures that are the
//! tester's fault, so they are not somewhere to be aggressive.
//!
//! Detecting a firmware change that slows serving down is
//! [`crate::cs_timing`]'s job instead.  It measures the latency and asserts an
//! exact value, which is both more sensitive than a tight margin here and
//! immune to the two effects that made a tight margin look like it was working:
//!
//! * a single-chip image is replicated across the SRAM index bits the chip's
//!   address lines do not drive, so the stale pre-CS fetch returns the right
//!   byte anyway and almost any margin passes; and
//! * the address state machine free-runs, so the requirement depends on where
//!   CS assertion lands in its loop.  A fixed-cadence pass only ever visits
//!   some of those phases, and can sit below the real requirement for years
//!   without failing — `CYCLES_CS_TO_DATA_MULTI` was 12 against a measured 13,
//!   and inserting a single extra cycle before a multi-ROM pass turned it from
//!   clean to ~16k failures.
//!
//! At 150 MHz, one cycle ≈ 6.67 ns.  ns figures in comments are approximate.
//! Raise a value if a measurement says to; do not lower one to make a pass go
//! green.

/// Initial settle before the first read.
///
/// A deliberately non-round value so that timing edge cases surface clearly.
pub const CYCLES_BEFORE_START: u32 = 173;

/// Cycles between driving the address (CS inactive) and asserting CS.
pub const CYCLES_ADDR_BEFORE_CS: u32 = 6; // ~40 ns

/// Cycles between asserting CS and reading the data GPIOs (standard ROMs).
///
/// Covers the worst case, which is a chip whose selects the address state
/// machine samples: asserting CS changes the SRAM index, so the fetch already
/// in flight is discarded and the loop has to come round again.  Chips whose
/// selects sit outside that window need only 7, but which case applies depends
/// on the board's routing as much as the chip, so the pass uses the larger
/// figure throughout rather than deciding per chip.
pub const CYCLES_CS_TO_DATA: u32 = 13; // ~87 ns

/// CS-to-data delay for multi-ROM sets.
///
/// The same refetch cost as [`CYCLES_CS_TO_DATA`] — a multi set's chip selects
/// are necessarily inside the address window, since that is how the set's
/// images are indexed apart.  Unlike a single-chip set there is no replication
/// to fall back on: the entry the stale fetch reaches belongs to a *different
/// chip*, so a short margin here reads another ROM's data rather than the same
/// byte from another copy.
pub const CYCLES_CS_TO_DATA_MULTI: u32 = 13; // ~87 ns

/// Cycles with CS deasserted between consecutive reads.
pub const CYCLES_AFTER_READ: u32 = 6; // ~40 ns

// ── 27C400 / 27C200 ──────────────────────────────────────────────────────────
//
// BYTE# handling adds cycles to the PIO address-read loop, so this family
// needs longer settling times both before and after CS assertion.

/// Address-to-CS delay for 27C400/27C200.
///
/// The address-read loop is deliberately slowed to 7 cycles to give BYTE#
/// mode logic time to complete before the address is sampled.
pub const CYCLES_27C400_ADDR_BEFORE_CS: u32 = 13; // ~86.7 ns

/// CS-to-data delay for 27C400/27C200 in 8-bit (BYTE# asserted) mode.
///
/// 8-bit mode has a longer delay than 16-bit because of the BYTE# pin
/// handling path in the PIO program.
pub const CYCLES_27C400_CS_TO_DATA_BYTE: u32 = 9; // ~60 ns
