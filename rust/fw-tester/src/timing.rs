// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! PIO emulator cycle counts for ROM read timing.
//!
//! These are deliberately tuned to be **as aggressive as possible** — just
//! enough for correct operation — so that any firmware change that slows
//! down byte serving will cause test failures.
//!
//! At 150 MHz, one cycle ≈ 6.67 ns.  ns figures in comments are approximate.
//!
//! The values are carried over from the old C tester (`test_main.c`) which
//! tuned them empirically.  Do not relax them without a documented reason.

/// Initial settle before the first read.
///
/// A deliberately non-round value so that timing edge cases surface clearly.
pub const CYCLES_BEFORE_START: u32 = 173;

/// Cycles between driving the address (CS inactive) and asserting CS.
pub const CYCLES_ADDR_BEFORE_CS: u32 = 6; // ~40 ns

/// Cycles between asserting CS and reading the data GPIOs (standard ROMs).
pub const CYCLES_CS_TO_DATA: u32 = 6; // ~40 ns

/// CS-to-data delay for multi-ROM sets.
///
/// The address is only sampled after CS goes active, requiring an extra
/// address→DMA→data write chain before output is valid.
pub const CYCLES_CS_TO_DATA_MULTI: u32 = 12; // ~80 ns

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
