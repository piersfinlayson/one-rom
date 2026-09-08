// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The metadata-schema constants an option's help and default quote.
//!
//! The CLI cannot state one of these numbers directly: clap reads a help line
//! as a literal, and a `pub const` is not one.  It writes each constant to a
//! file at build time and pastes the file in with `const_str!`, so what the
//! source holds at the point this reads it is a name.
//!
//! Reading `ALL_CONSTANTS` here is the same arrangement, one step earlier: the
//! CLI's `build.rs` reads the same list, so a period shown in a pane is the
//! period the firmware was built with rather than a number typed twice.

/// The value of a schema constant, by the name the CLI quotes it under.
///
/// A name with no constant fails the build rather than reaching a pane, the
/// same way the CLI's `include_str!` fails on a missing file.
pub fn value(name: &str) -> &'static str {
    onerom_metadata::ALL_CONSTANTS
        .iter()
        .find(|(constant, _)| *constant == name)
        .map(|(_, value)| *value)
        .unwrap_or_else(|| {
            panic!(
                "no constant {name} in the metadata schema \
                 (rust/metadata/metadata_schema.toml)"
            )
        })
}
