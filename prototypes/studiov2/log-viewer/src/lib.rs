//! Log and console panes on iced 0.14, over a whole-session log.
//!
//! [`screen::Screen`] is the whole thing. `src/main.rs` runs it on its own and
//! the shell at `../../shell` runs it beside the builder, and neither knows
//! anything the other does not.
//!
//! Everything below `screen` is a library rather than a window, so the tests
//! in `tests/` can drive the log pane through a real, headless iced
//! `UserInterface` — which is the only way to test mouse selection and
//! `Cmd+C` on a machine that will not grant synthetic input events.

pub mod device;
pub mod error;
pub mod logpane;
pub mod logsrc;
pub mod logview;
pub mod metrics;
pub mod options;
pub mod screen;

/// The log store, which lives in `studiov2-shared` because the log is shared
/// with every other screen.  Re-exported so this crate's own modules and
/// tests reach it where they always did.
pub use studiov2_shared::store;
