// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Runs a One ROM plugin's own C source natively against the firmware
//! emulator.
//!
//! The plugin is compiled for the host (see the plugin's `host` Makefile
//! target) and linked against the firmware test library, so it runs its real
//! code against the real plugin API — not a reimplementation of either.
//!
//! See [`harness`] for how the plugin and the test driver share the emulator.

pub mod ffi;
pub mod harness;
