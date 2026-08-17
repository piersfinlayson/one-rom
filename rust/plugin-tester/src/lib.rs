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
//! This crate is what a tester is built from, not a tester itself.  Each
//! plugin gets its own binary crate, because the shim standing in for the
//! plugin's device environment names that plugin's entry point, and because two
//! plugins cannot share a process — each defines its own `ora_plugin_header`
//! and its own file-scope state.  `onerom-rbcp-tester` is the worked example.
//!
//! What lives here is everything those testers share:
//!
//! - [`harness`], for how the plugin and the test driver share the emulator.
//! - [`ffi`], declaring the three entry points a tester's shim defines.  They
//!   are left unresolved here and supplied at the final link, which is why this
//!   crate can be shared by testers whose shims are different.
//! - [`run`], the command line and the reporting.

pub mod ffi;
pub mod harness;
pub mod run;
