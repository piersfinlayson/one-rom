// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// `driver` and `pin_cache` derive everything from the board and chip
// definitions, so they live outside this crate, in `onerom-fw-geometry` — a
// crate with no build script, which One ROM Lens's build script can depend on
// without dragging in a second, host-side firmware build.  (`driver` is
// `onerom-fw-driver`, which geometry re-exports in turn.)  Re-exported here so
// existing `crate::driver` / `onerom_fw_tester::pin_cache` paths keep working.
pub use onerom_fw_geometry::{driver, pin_cache};
pub mod cs_timing;
pub mod geometry;
pub mod oracle;
pub mod runner;
pub mod timing;
