// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The host-supplied transport behind every asynchronous entry point.

use alloc::vec::Vec;

/// Host-supplied transport for everything the crate fetches.
///
/// `onerom-app` doesn't perform I/O of its own. It delegates every network or
/// filesystem access to an implementation of this trait. The single method
/// must return the bytes at `source` or the host's own error. `source` is a URL
/// or path the crate produced.
///
/// # `Send` variants
///
/// [`trait_variant`] generates two forms of this trait:
///
/// - The base trait [`LocalFetch`] suits single-threaded executors such as the
///   browser (WASM) and Embassy (embedded). Its `fetch` future need not be
///   `Send`.
/// - `Fetch` suits multi-threaded executors such as the CLI's Tokio runtime.
///   Its `fetch` future is `Send`.
///
/// A type that implements `Fetch` also implements `LocalFetch`.
#[trait_variant::make(Fetch: Send)]
pub trait LocalFetch {
    /// The host's transport error type.
    type Error;

    /// Fetch the bytes at `source` (a URL or path).
    async fn fetch(&self, source: &str) -> Result<Vec<u8>, Self::Error>;
}
