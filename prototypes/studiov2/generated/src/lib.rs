// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Studio v2 prototype: a pane for every CLI command, drawn from a
//! description of those commands and nothing else.
//!
//! The question this answers is whether a GUI can be generated rather than
//! written.  [`studiov2_commands::COMMANDS`] is plain data with no idea what a
//! widget is, and everything here reads it: the tabs, the pane, the widget for
//! each option, the command line and the result.  **There is no code anywhere
//! in this crate that names a command.**  A command added to the CLI reaches
//! the screen with nobody typing anything, or it does not reach it at all —
//! which is what makes the panes worth looking at.
//!
//! Four commands run for real, against the crates the CLI itself calls —
//! [`real`] is the registry and, more to the point, the shape that finds a
//! command an implementation **without naming one**.  Read its module docs
//! before anything else here: they also say which command that shape cannot
//! reach and why.
//!
//! Everything else is stubbed.  Two places hold something the description does
//! not carry, and both are marked PLACEHOLDER: [`stub::shape`] guesses what a
//! command prints, and [`stub::run`] makes up the content.  Nothing here
//! touches a device, the network or the USB bus.
//!
//! [`resolve`] is where a named value set becomes values, read from the crates
//! that own them.  The three sets it makes up instead are named in
//! [`resolve::FAKED`], which the page shows.
//!
//! [`screen::Screen`] is the whole thing.  `src/main.rs` runs it on its own,
//! and the shell hosts the same type beside the other screens.

pub mod dev;
pub mod form;
pub mod real;
pub mod resolve;
pub mod screen;
pub mod stub;
pub mod style;
pub mod tree;
pub mod ui;

pub use form::{Field, Form, Target, Where};
pub use screen::{Message, Screen};
pub use stub::{Body, Node, Output, Section, Shape};

/// The fonts the screen draws with.
///
/// A font is registered on the application, not on a widget, so a screen
/// cannot register its own — whoever calls `iced::application` has to.  The
/// list is here so the caller does not have to know the paths.
pub const FONTS: [&[u8]; 2] = [INTER, INTER_SEMIBOLD];

/// Inter, the body face.
const INTER: &[u8] = include_bytes!("../../../../docs/pdf/fonts/Inter-Regular.ttf");
/// Inter Semibold, for a heading.
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../../docs/pdf/fonts/Inter-SemiBold.ttf");
