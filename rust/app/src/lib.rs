// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Shared, transport-free logic for One ROM applications.
//!
//! `onerom-app` holds the logic that every One ROM *application* needs but that
//! is not specific to any one of them: the CLI, Studio, the web tool (via
//! WASM), and embedded programmers such as Airfrog. It is deliberately
//! `no_std` (with `alloc`) and performs **no I/O of its own** - all network and
//! filesystem access is delegated to the host through the [`PluginFetch`]
//! trait.
//!
//! # Design
//!
//! The crate separates two concerns that different hosts combine differently:
//!
//! - **Decisions** are pure and synchronous: parsing manifests, selecting a
//!   compatible release, verifying a binary, generating a chip-set config.
//!   These never touch the network and return [`PluginError`] directly.
//! - **Fetching** is asynchronous and host-specific. The crate never fetches;
//!   it asks the host to, via [`PluginFetch`], and threads the host's own error
//!   type back out through [`Error`].
//!
//! This split is what lets the same logic serve a `reqwest`-based CLI, a
//! JS-`fetch`-based WASM build, and an SWD-based embedded programmer without
//! any of them sharing a transport.
//!
//! # Membership
//!
//! Code belongs in `onerom-app` only if it is **shared across One ROM
//! applications** *and* buildable under `no_std`. Logic specific to a single
//! application (for example the CLI's `--slot` string grammar, or its
//! interactive confirmation prompting) stays in that application. The `no_std`
//! constraint is enforced by a CI job that builds the crate for a bare-metal
//! target with `--no-default-features`.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

mod error;
mod plugin;

pub use error::{Error, PluginError};
pub use plugin::{
    // Catalogue and core types.
    Catalogue,
    // Fetch abstraction (host-implemented). `trait_variant` generates the
    // `Send` variant `PluginFetch` from the base `LocalPluginFetch`.
    LocalPluginFetch,
    Plugin,
    // Display of a device's plugin slot, resolved from its recorded image
    // source (manifest-backed or local).
    PluginDisplay,
    PluginFetch,
    PluginOrigin,
    // Plugin specifications (parsed `--plugin` style inputs).
    PluginSpec,
    PluginType,
    PluginVerification,
    PluginVersion,
    Release,
    ResolvedPlugin,
    ResolvedSource,
    // Verification target selector and result.
    VerifyTarget,
    compatible_releases,
    // Async resolution (delegate fetching to `PluginFetch`).
    fetch_releases,
    newest_compatible,
    // Pure decision logic.
    parse_plugins,
    plugin_to_chip_set_config,
    // Resolve a device plugin slot to a PluginDisplay (delegates fetching).
    resolve_plugin_display,
    resolve_plugins,
    validate_resolved_plugin_types,
    verify_binary,
};

/// Returns the version of this crate, as set in `Cargo.toml`.
pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}