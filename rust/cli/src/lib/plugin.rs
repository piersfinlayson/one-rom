// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Plugin logic, provided by `onerom-app`.
//!
//! All plugin parsing, manifest handling, compatibility selection, verification
//! and configuration generation now lives in the transport-free `onerom-app`
//! crate, shared by every One ROM application. This module re-exports it under
//! the stable `onerom_cli::plugin` path so the CLI, Studio and other native
//! hosts that depend on `onerom-cli` have a single place to reach it. Nothing
//! is implemented here.
//!
//! Fetching is host-specific and is provided by [`CliFetch`](crate::CliFetch),
//! which implements `onerom-app`'s [`LocalPluginFetch`] over `onerom-fw`.

pub use onerom_app::{
    // Catalogue and core types.
    Catalogue,
    // Fetch abstraction (implemented by `CliFetch`).
    LocalPluginFetch,
    Plugin,
    // Display of a device's plugin slot, resolved from its recorded image
    // source (manifest-backed or local).
    PluginDisplay,
    PluginError,
    PluginFetch,
    PluginOrigin,
    PluginSpec,
    PluginType,
    // Verification.
    PluginVerification,
    PluginVersion,
    Release,
    ResolvedPlugin,
    ResolvedSource,
    VerifyTarget,
    // Pure decision logic.
    compatible_releases,
    // Async fetch/resolution.
    fetch_releases,
    newest_compatible,
    parse_plugins,
    plugin_to_chip_set_config,
    // Resolve a device plugin slot to a PluginDisplay (delegates fetching).
    resolve_plugin_display,
    resolve_plugins,
    validate_resolved_plugin_types,
    verify_binary,
};
