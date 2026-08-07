// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `onerom-fw-geometry` — the pure, config-derived layer shared by the host
//! tester (`onerom-fw-tester`) and One ROM Lens's build script.
//!
//! * [`pin_cache`] — ROM-socket-pin → MCU-GPIO mappings for one chip on one board
//! * [`substitution`] — the pin-compatible chip a board serves in place of another
//!
//! [`driver`], the GPIO bitmask builders these mappings feed, is
//! `onerom-fw-driver` re-exported: it needs no config at all, and keeping it in
//! a dependency-free crate keeps `onerom-gen` and serde out of One ROM Lens's
//! wasm build (see that crate's docs).
//!
//! Nothing here touches the emulated firmware, the network or the filesystem:
//! every answer is derived from the `onerom-config` board/chip definitions and
//! from `onerom-gen`'s view of a chip's configuration.
//!
//! That is why this is a crate of its own rather than a module of
//! `onerom-fw-emulator` or `onerom-fw-tester`.  `onerom-fw-emulator`'s build
//! script compiles the whole firmware C tree by running `make` in the shared
//! working tree, so two concurrent instances of it race over
//! `firmware/generated/gen-config.c`, `firmware/apio` and `firmware/epio` and
//! fail unpredictably.  One ROM Lens hit exactly that: its build script needs
//! only [`pin_cache`] and [`substitution`], but reaching them through
//! `onerom-fw-tester` made the emulator a *host* build-dependency of a *wasm*
//! crate that already depends on it, and cargo duly built the firmware twice at
//! once.  Depending on this crate instead leaves Lens with a single, wasm-only
//! firmware build.
//!
//! Two rules keep that true, and both are load-bearing:
//!
//! - **No build script here**, ever, and no dependency that has one — above
//!   all never `onerom-fw-emulator`.  That is the whole point: Lens reaches
//!   this crate from a build script, so a firmware build behind it is a
//!   firmware build running concurrently with Lens's own.
//! - **No dependency that cannot build for `wasm32-unknown-emscripten`.**
//!   That rules out `onerom-fw`, whose `smol` async stack does not; the
//!   metadata-reading `geometry` module, which calls
//!   `onerom_fw::get_rom_files`, stays in `onerom-fw-tester` for that reason.

// Re-exported so `onerom_fw_geometry::driver` and `crate::driver` paths keep
// working now that the bitmask builders live in their own dependency-free
// crate.
pub use onerom_fw_driver as driver;

pub mod pin_cache;
pub mod substitution;
