// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Studio v2 prototype: the ROM Slot Builder in Iced.
//!
//! A build of the site's multi-slot builder against the real crates, to judge
//! how close Iced gets to the shipped HTML and what it costs to write. There
//! is no device I/O here at all — no USB, no flashing — and the firmware
//! version list is the one thing not read from a source of truth.
//!
//! [`screen::Screen`] is the whole builder. `src/main.rs` runs it on its own
//! and the shell at `../../shell` runs it beside the log pane, and neither
//! knows anything the other does not.

pub mod build;
pub mod catalog;
pub mod dev;
pub mod error;
pub mod screen;
pub mod slot;
pub mod ui;

pub use screen::{Message, Screen};

/// The fonts the builder draws with.
///
/// A font is registered on the application, not on a widget, so a screen
/// cannot register its own — whoever calls `iced::application` has to.  The
/// list is here so the caller does not have to know the paths.
pub const FONTS: [&[u8]; 3] = [MICHROMA, INTER, INTER_SEMIBOLD];

/// Michroma, for the board brand mark.
const MICHROMA: &[u8] = include_bytes!("../../../../rust/studio/fonts/Michroma-Regular.ttf");
/// Inter, for everything else.
const INTER: &[u8] = include_bytes!("../../../../docs/pdf/fonts/Inter-Regular.ttf");
/// Inter Semibold, for a slot title.
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../../docs/pdf/fonts/Inter-SemiBold.ttf");
