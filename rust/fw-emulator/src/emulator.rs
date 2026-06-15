// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Safe Rust wrapper around the C firmware emulation layer.
//!
//! # Lifecycle
//!
//! ```text
//!   Emulator::set_logging(enabled)  — optional, before boot, or after()
//!   Emulator::set_sel_image(n)
//!   Emulator::boot()                — calls firmware_main(), populates global state
//!        │
//!        ▼
//!   emu.limp_mode()                 — available immediately after boot
//!   emu.pios_enabled()
//!        │
//!   emu.setup_epio(word_size)       — creates epio_t, wires up SRAM + DMA chain
//!        │
//!        ▼
//!   emu.step_cycles(n)
//!   emu.drive_gpios(gpios, level)
//!   emu.read_pin_states()
//! ```
//!
//! # Thread safety
//!
//! `firmware_main` writes global C state.  Run tests with
//! `RUST_TEST_THREADS=1` (or `-- --test-threads=1`) to avoid races.

use crate::ffi;

/// A handle to a running One ROM firmware emulator instance.
pub struct Emulator {
    /// Non-null after [`Self::setup_epio`] has been called.
    epio: *mut ffi::epio_t,
}

impl Emulator {
    /// Initialise the firmware by calling `firmware_main`.
    ///
    /// Firmware global state (limp mode flag, PIO enable state, etc.) is
    /// valid immediately after this returns.  Call [`Self::setup_epio`]
    /// before using any cycle-stepping or GPIO methods.
    pub fn boot() -> Self {
        // SAFETY: no preconditions; firmware_main initialises global state
        // and returns (stubs prevent it from spinning or touching hardware).
        unsafe { ffi::firmware_main() };
        Self {
            epio: core::ptr::null_mut(),
        }
    }

    /// Enable or disable logging from the firmware (goes to stdout if
    /// enabled).
    pub fn set_logging(enabled: bool) {
        unsafe { ffi::ffi_set_logging(enabled as u8) };
    }

    /// Create and configure the emulated PIO handle.
    ///
    /// `word_size` is passed to `ffi_epio_setup_dma_chain`.
    ///
    /// # Panics
    ///
    /// Panics if called twice, or if `epio_from_apio` returns null.
    ///
    pub fn setup_epio(&mut self, word_size: u8) {
        assert!(self.epio.is_null(), "setup_epio called twice");

        // SAFETY: firmware_main has populated global state that epio_from_apio reads.
        let epio = unsafe { ffi::epio_from_apio() };
        assert!(!epio.is_null(), "epio_from_apio returned null");

        // SAFETY: epio is non-null and freshly allocated.
        unsafe { ffi::ffi_epio_setup_sram(epio) };
        unsafe { ffi::ffi_epio_setup_dma_chain(epio, word_size) };

        self.epio = epio;
    }

    // ── Firmware state queries (valid after boot()) ──────────────────────────

    /// Returns `true` if the firmware is operating in limp mode.
    pub fn limp_mode(&self) -> bool {
        unsafe { ffi::ffi_limp_mode() as i32 != 0 }
    }

    /// Returns `true` if the PIO state machines are enabled.
    pub fn pios_enabled(&self) -> bool {
        unsafe { ffi::ffi_pios_enabled() as i32 != 0 }
    }

    // ── ROM image selection ──────────────────────────────────────────────────

    /// Tell the stub which ROM image to present.
    pub fn set_sel_image(image: u8) {
        unsafe { ffi::stub_set_sel_image(image as _) };
    }

    // ── GPIO / cycle operations (require setup_epio()) ───────────────────────

    /// Drive external GPIO states into the emulator.
    ///
    /// `gpios` is a bitmask of pins to affect; `level` is the level for each.
    pub fn drive_gpios(&self, gpios: u64, level: u64) {
        unsafe { ffi::epio_drive_gpios_ext(self.epio_or_panic(), gpios, level) };
    }

    /// Read the current emulated GPIO pin states.
    pub fn read_pin_states(&self) -> u64 {
        unsafe { ffi::epio_read_pin_states(self.epio_or_panic()) }
    }

    /// Advance the emulation by `cycles` clock cycles.
    pub fn step_cycles(&self, cycles: u32) {
        unsafe { ffi::epio_step_cycles(self.epio_or_panic(), cycles) };
    }

    pub fn read_driven_pins(&self) -> u64 {
        unsafe { ffi::epio_read_driven_pins(self.epio_or_panic()) }
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn epio_or_panic(&self) -> *mut ffi::epio_t {
        assert!(
            !self.epio.is_null(),
            "call setup_epio() before using GPIO/cycle methods"
        );
        self.epio
    }
}

impl Drop for Emulator {
    fn drop(&mut self) {
        if !self.epio.is_null() {
            // SAFETY: epio was allocated by epio_from_apio and has not been freed.
            unsafe { ffi::epio_free(self.epio) };
            self.epio = core::ptr::null_mut();
        }
    }
}

// SAFETY: epio_t is heap-allocated C state with no thread-local components.
// We take responsibility for correct single-threaded usage in tests.
unsafe impl Send for Emulator {}
