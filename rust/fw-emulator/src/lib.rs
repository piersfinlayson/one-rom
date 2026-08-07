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

mod emulator;
pub mod ffi;

// The bitmask builders that feed `Emulator::drive_gpios` need no emulator to
// compute, so they live in `onerom-fw-driver` — a crate with no build script
// and no dependencies of its own, so this re-export costs One ROM Lens's wasm
// build nothing.  Re-exported so existing `onerom_fw_emulator::driver` paths
// keep working.
pub use onerom_fw_driver as driver;

pub use emulator::{
    Emulator, FlashSlotInfo, GpioInfo, ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS,
    ORA_FLASH_SLOT_FLAG_EXCLUDE_PLUGINS, OraResult, RamSlotInfo, ServingAlg,
};
