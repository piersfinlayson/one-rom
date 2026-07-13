// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Host transport for `onerom-app`'s plugin fetching.
//!
//! `onerom-app` performs no I/O of its own; it delegates all network and
//! filesystem access to a [`LocalPluginFetch`] implementation. [`CliFetch`] is
//! that implementation for native hosts (the CLI and Studio), backed by
//! `onerom-fw`'s file/URL retrieval - the same path used to fetch ROM images,
//! so plugin manifests and binaries are retrieved exactly as any other
//! artifact.

use onerom_app::LocalPluginFetch;

/// A [`LocalPluginFetch`] backed by `onerom-fw`'s async file/URL retrieval.
///
/// The unit struct carries no state; each `fetch` is an independent retrieval.
/// The associated error is `onerom-fw`'s own [`Error`](onerom_fw::Error), which
/// the CLI's error mapper carries through unchanged (so a network failure and
/// an HTTP status remain distinguishable at the call site).
pub struct CliFetch;

impl LocalPluginFetch for CliFetch {
    type Error = onerom_fw::Error;

    async fn fetch(&self, source: &str) -> Result<Vec<u8>, Self::Error> {
        // `onerom-fw` returns (data, cache); the plugin path does not reuse the
        // cache, so it is discarded. The trailing `false` matches the plugin
        // retrieval behaviour used before this logic moved into `onerom-app`.
        let (data, _cache) = onerom_fw::net::fetch_rom_file_async(source, &[], None, false).await?;
        Ok(data)
    }
}
