// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `onerom-fw-driver` — GPIO bitmask builders and data-byte extractor.
//!
//! All functions produce or consume raw `u64` bitmasks in the form
//! `onerom-fw-emulator`'s `Emulator::drive_gpios(mask, levels)` takes:
//!
//! - bit N set in `mask`   → GPIO N is actively driven
//! - bit N set in `levels` → GPIO N is driven HIGH (LOW if clear in levels)
//! - bit N clear in `mask` → GPIO N is unaffected
//!
//! Building a mask needs no emulator, so these helpers live outside it.  Both
//! `onerom-fw-emulator` and `onerom-fw-geometry` re-export this crate as
//! `driver`, so the host-side tester (`onerom-fw-tester`) and One ROM Lens
//! (`onerom-lens`) reach it by the `driver::` paths they always used.
//!
//! # Why this is a crate of its own
//!
//! **This crate has no dependencies, and that is its defining property.**  It
//! is deliberately one module's worth of code in a crate of its own so that
//! there is somewhere with nothing behind it to depend on.
//!
//! `onerom-fw-emulator` re-exports these builders, and One ROM Lens compiles
//! `onerom-fw-emulator` to `wasm32-unknown-emscripten`.  Whatever this crate
//! pulls in therefore lands in Lens's wasm binary and in Lens's build times.
//! These helpers previously sat in `onerom-fw-geometry` alongside `pin_cache`
//! and `substitution`, which need `onerom-config` and `onerom-gen`; the single
//! re-export dragged `onerom-gen`, serde, serde_json and hashbrown into Lens's
//! wasm graph for the sake of a handful of shifts and ORs.
//!
//! Two rules keep that from coming back, and both are load-bearing:
//!
//! - **No dependencies here**, ever — not even a small one, and not even one
//!   that builds for wasm.  An empty `[dependencies]` table is easy to check;
//!   "only cheap dependencies" is not.
//! - **No build script here**, ever.  A build script would make this crate a
//!   host build even where it is only wanted for wasm.
//!
//! Anything needing config, generated data or the emulated firmware belongs in
//! `onerom-fw-geometry` or `onerom-fw-tester`, not here.

/// A single decoded control line with its assertion polarity baked in.
pub struct ControlLine {
    /// Name for diagnostics ("ce", "oe", "cs1", "cs2", "cs3", "x_cs").
    // Not read in the hot path; retained for future diagnostic/tristate use.
    #[allow(dead_code)]
    pub name: &'static str,
    /// Every MCU GPIO driven by this physical pin.
    /// Usually one; some boards (e.g. Fire32B fly-leads) wire one socket pin
    /// to two GPIOs and both must be driven.
    pub gpios: Vec<u8>,
    /// `true` → assert by driving HIGH; `false` → assert by driving LOW.
    pub assert_high: bool,
    /// `true` for a *commoned* line on a Multi-set primary: a CS line asserted
    /// on every read that does not select a chip. Set by the runner after
    /// `build`; `false` everywhere else. The tristate combo sweep holds these
    /// deasserted and never enumerates them (an asserted commoned line fires
    /// the CS gate on its own).
    pub commoned: bool,
}

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
