// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `onerom-fw-emulator` — safe Rust interface to the One ROM firmware
//! emulation layer.
//!
//! Builds `libonerom-test.a` via `build.rs` and provides:
//!
//! * [`ffi`] — raw, unsafe bindgen-generated bindings (escape hatch)
//! * [`Emulator`] — safe wrapper for test code
//! * [`driver`] — GPIO bitmask builders shared by the tester and One ROM Lens
//! * [`build_options`] — the logging the C under test was compiled with

mod emulator;
pub mod ffi;

// The bitmask builders that feed `Emulator::drive_gpios` need no emulator to
// compute, so they live in `onerom-fw-driver` — a crate with no build script
// and no dependencies of its own, so this re-export costs One ROM Lens's wasm
// build nothing.  Re-exported so existing `onerom_fw_emulator::driver` paths
// keep working.
pub use onerom_fw_driver as driver;

/// The logging options the firmware C in this build of the library was
/// compiled with, as `firmware/test.mk`'s `TEST_LOGGING` settled them.
///
/// A test states what the firmware must report from these rather than from the
/// firmware's own answer, which would agree with itself whatever it did.
/// `build.rs` records them as cfgs on this crate, and a cfg reaches only the
/// crate whose build script emitted it, so they are re-published here for the
/// testers.
pub mod build_options {
    /// Whether `DEBUG_LOGGING` was defined.  One ROM's own `DEBUG()` lines and
    /// a plugin's `ora_debug_log` are absent from the build without it.
    pub const DEBUG_LOGGING: bool = cfg!(fw_debug_logging);

    /// Whether `PLUGIN_LOGGING` was defined.  `ora_log` and `ora_debug_log`
    /// are compiled away without it - the `ora_log_write` channel family is
    /// not gated on it, and neither is `ora_err_log`.
    pub const PLUGIN_LOGGING: bool = cfg!(fw_plugin_logging);

    /// Whether `BOOT_LOGGING` was defined.  Always, on every build: include.h
    /// defines it unconditionally, and what a device varies is the runtime
    /// metadata flag it gates on.  Named here so a test asserting it reads as
    /// one of the three rather than as a bare 1.
    pub const BOOT_LOGGING: bool = true;
}

pub use emulator::{
    Emulator, FlashSlotInfo, GpioInfo, ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS,
    ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS, OraResult, RamSlotInfo, ServingAlg,
};
