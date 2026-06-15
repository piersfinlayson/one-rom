// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! GPIO bitmask builders and data-byte extractor.
//!
//! All functions produce or consume raw `u64` bitmasks as consumed by
//! `Emulator::drive_gpios(mask, levels)`:
//!
//! - bit N set in `mask`   → GPIO N is actively driven
//! - bit N set in `levels` → GPIO N is driven HIGH (LOW if clear in levels)
//! - bit N clear in `mask` → GPIO N is unaffected

use crate::pin_cache::ControlLine;

/// Build a `(mask, levels)` pair to drive address GPIOs for logical address
/// `addr`.
///
/// Bit i of `addr` is placed on every GPIO in `addr_gpios[i]`.  Where a
/// socket pin is wired to multiple GPIOs (fly-lead boards), all are driven
/// to the same level.
pub fn addr_mask(addr: usize, addr_gpios: &[Vec<u8>]) -> (u64, u64) {
    let mut mask = 0u64;
    let mut levels = 0u64;
    for (bit, gpios) in addr_gpios.iter().enumerate() {
        let high = (addr >> bit) & 1 == 1;
        for &gpio in gpios {
            mask |= 1u64 << gpio;
            if high {
                levels |= 1u64 << gpio;
            }
        }
    }
    (mask, levels)
}

/// Build a `(mask, levels)` pair to drive all control lines asserted or
/// deasserted.
///
/// Assertion logic:
/// - `asserted == true,  assert_high == true`  → drive HIGH  (active-high assert)
/// - `asserted == true,  assert_high == false` → drive LOW   (active-low assert)
/// - `asserted == false, assert_high == true`  → drive LOW   (active-high deassert)
/// - `asserted == false, assert_high == false` → drive HIGH  (active-low deassert)
pub fn ctrl_mask(control_lines: &[ControlLine], asserted: bool) -> (u64, u64) {
    let mut mask = 0u64;
    let mut levels = 0u64;
    for ctrl in control_lines {
        // XNOR: drive high when (asserted ↔ assert_high)
        let drive_high = asserted == ctrl.assert_high;
        for &gpio in &ctrl.gpios {
            mask |= 1u64 << gpio;
            if drive_high {
                levels |= 1u64 << gpio;
            }
        }
    }
    (mask, levels)
}

/// Build a `(mask, levels)` pair for the BYTE# pin.
///
/// BYTE# is active-low: low = 8-bit mode (asserted), high = 16-bit mode
/// (deasserted / default).
pub fn byte_n_mask(gpio: u8, mode: u8) -> (u64, u64) {
    let mask = 1u64 << gpio;
    let levels = if mode == 16 { mask } else { 0 };
    (mask, levels)
}

/// Merge two `(mask, levels)` pairs by OR-ing both components.
///
/// The caller must ensure the two masks do not overlap; this is not checked
/// at runtime.
#[inline]
pub fn merge(a: (u64, u64), b: (u64, u64)) -> (u64, u64) {
    (a.0 | b.0, a.1 | b.1)
}

/// Extract an 8-bit value from raw GPIO pin states.
///
/// `data_gpios[i]` is the GPIO carrying data bit D_i.  Because the SRAM
/// image is pre-mangled at build time, the GPIO for physical data pin D_i
/// carries the original (unmangled) logical bit i.  Reading each GPIO at
/// position i and placing it at bit i therefore reconstructs the raw ROM byte
/// with no further transformation.
pub fn extract_byte(pin_states: u64, data_gpios: &[u8]) -> u8 {
    let mut byte = 0u8;
    for (bit, &gpio) in data_gpios.iter().enumerate() {
        if (pin_states >> gpio) & 1 == 1 {
            byte |= 1u8 << bit;
        }
    }
    byte
}

/// Returns `true` if all GPIOs in `data_gpios` are low (undriven / tristated).
// Not yet called; retained for future pre-CS and post-read tristate checking.
#[allow(dead_code)]
pub fn data_pins_low(pin_states: u64, data_gpios: &[u8]) -> bool {
    data_gpios.iter().all(|&gpio| (pin_states >> gpio) & 1 == 0)
}
