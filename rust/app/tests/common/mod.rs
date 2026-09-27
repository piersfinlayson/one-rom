// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Fetchers shared by the integration tests.

use std::collections::HashMap;
use std::sync::Mutex;

use onerom_app::LocalFetch;

/// The mock's transport error. It holds a plain message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockErr(pub String);

impl std::fmt::Display for MockErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A `LocalFetch` backed by a fixed map from URLs to bytes.
///
/// A missing URL yields a [`MockErr`] to model a transport failure. Every
/// requested URL is recorded so tests can assert which fetches happened (for
/// example that `plugins.json` is fetched only when a bare name needs it).
pub struct MockFetch {
    responses: HashMap<String, Vec<u8>>,
    requested: Mutex<Vec<String>>,
}

impl MockFetch {
    pub fn new() -> Self {
        Self {
            responses: HashMap::new(),
            requested: Mutex::new(Vec::new()),
        }
    }

    pub fn with(mut self, url: &str, bytes: Vec<u8>) -> Self {
        self.responses.insert(url.to_string(), bytes);
        self
    }

    pub fn requested(&self) -> Vec<String> {
        self.requested.lock().unwrap().clone()
    }

    pub fn was_requested(&self, url: &str) -> bool {
        self.requested().iter().any(|u| u == url)
    }
}

impl LocalFetch for MockFetch {
    type Error = MockErr;

    async fn fetch(&self, source: &str) -> Result<Vec<u8>, Self::Error> {
        self.requested.lock().unwrap().push(source.to_string());
        self.responses
            .get(source)
            .cloned()
            .ok_or_else(|| MockErr(format!("no mock response for {source}")))
    }
}

/// A real HTTP-backed fetcher. Only the live canaries use it.
pub struct HttpFetch;

impl LocalFetch for HttpFetch {
    type Error = String;

    async fn fetch(&self, source: &str) -> Result<Vec<u8>, Self::Error> {
        // ureq is blocking so run it on a worker thread rather than the async
        // runtime.
        let url = source.to_string();
        tokio::task::spawn_blocking(move || {
            let mut resp = ureq::get(&url).call().map_err(|e| e.to_string())?;
            let bytes = resp.body_mut().read_to_vec().map_err(|e| e.to_string())?;
            Ok::<Vec<u8>, String>(bytes)
        })
        .await
        .map_err(|e| e.to_string())?
    }
}
