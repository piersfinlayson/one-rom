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

pub mod ffi;
mod emulator;

pub use emulator::Emulator;