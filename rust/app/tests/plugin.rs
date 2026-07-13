// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Integration tests for `onerom-app`'s asynchronous entry points.
//!
//! These exercise the public API through a mock [`PluginFetch`] that serves
//! manifest JSON and plugin binaries from an in-memory map, so the tests are
//! deterministic and offline. One `#[ignore]`d canary at the end fetches the
//! live manifest to confirm the real schema still deserialises; run it with
//! `cargo test -- --ignored`.

use std::collections::HashMap;
use std::sync::Mutex;

use onerom_app::{
    Catalogue, Error, LocalPluginFetch, PluginError, PluginType, PluginVersion,
    ResolvedSource, parse_plugins, resolve_plugins,
};

const BASE: &str = "https://images.onerom.org/plugins";

// ------------------------------------------------------------
// Mock fetcher
// ------------------------------------------------------------

/// Transport error type for the mock: a plain message.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MockErr(String);

impl std::fmt::Display for MockErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A `PluginFetch` backed by a fixed URL -> bytes map.
///
/// A missing URL yields a [`MockErr`], modelling a transport failure. Every
/// requested URL is recorded so tests can assert which fetches happened (for
/// example, that `plugins.json` is fetched only when a bare name needs it).
struct MockFetch {
    responses: HashMap<String, Vec<u8>>,
    requested: Mutex<Vec<String>>,
}

impl MockFetch {
    fn new() -> Self {
        Self {
            responses: HashMap::new(),
            requested: Mutex::new(Vec::new()),
        }
    }

    fn with(mut self, url: &str, bytes: Vec<u8>) -> Self {
        self.responses.insert(url.to_string(), bytes);
        self
    }

    fn requested(&self) -> Vec<String> {
        self.requested.lock().unwrap().clone()
    }

    fn was_requested(&self, url: &str) -> bool {
        self.requested().iter().any(|u| u == url)
    }
}

impl LocalPluginFetch for MockFetch {
    type Error = MockErr;

    async fn fetch(&self, source: &str) -> Result<Vec<u8>, Self::Error> {
        self.requested.lock().unwrap().push(source.to_string());
        self.responses
            .get(source)
            .cloned()
            .ok_or_else(|| MockErr(format!("no mock response for {source}")))
    }
}

// ------------------------------------------------------------
// Fixtures
// ------------------------------------------------------------

/// Build a valid 256-byte plugin header binary (type at offset 20, version at
/// offsets 8..16).
fn header(type_byte: u8, ver: (u16, u16, u16, u16)) -> Vec<u8> {
    let mut buf = vec![0u8; 256];
    buf[0..4].copy_from_slice(&0x2041_524Fu32.to_le_bytes()); // "ORA "
    buf[8..10].copy_from_slice(&ver.0.to_le_bytes());
    buf[10..12].copy_from_slice(&ver.1.to_le_bytes());
    buf[12..14].copy_from_slice(&ver.2.to_le_bytes());
    buf[14..16].copy_from_slice(&ver.3.to_le_bytes());
    buf[20] = type_byte;
    buf
}

fn sha_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data))
}

fn plugins_json() -> Vec<u8> {
    r#"{
        "version": 1,
        "plugins": [
            { "name": "usb", "type": "system_plugin", "path": "system/usb" },
            { "name": "rgb", "type": "user_plugin",   "path": "user/rgb" }
        ]
    }"#
    .as_bytes()
    .to_vec()
}

/// A `releases.json` body with a single release whose digest matches `sha`.
fn releases_json(display: &str, version: &str, min_fw: &str, sha: &str) -> Vec<u8> {
    format!(
        r#"{{
            "version": 1,
            "display_name": "{display}",
            "description": "A test plugin",
            "latest": "{version}",
            "releases": [
                {{
                    "version": "{version}",
                    "path": "v{version}",
                    "filename": "plugin.bin",
                    "sha256": "{sha}",
                    "api_version": 1,
                    "plugin_type": "system_plugin",
                    "min_fw_version": "{min_fw}"
                }}
            ]
        }}"#
    )
    .into_bytes()
}

// ------------------------------------------------------------
// resolve_plugins: file= path
// ------------------------------------------------------------

#[tokio::test]
async fn resolve_file_reads_header_and_skips_manifest() {
    // A user plugin needs a system plugin, so pair them: system via file=,
    // user via file=. Both headers are read; no manifest is fetched.
    let sys = header(0, (0, 1, 0, 0)); // system
    let usr = header(1, (0, 2, 0, 0)); // user

    let fetch = MockFetch::new()
        .with("/tmp/sys.bin", sys)
        .with("/tmp/usr.bin", usr);

    let specs = parse_plugins(&[
        "file=/tmp/sys.bin".to_string(),
        "file=/tmp/usr.bin".to_string(),
    ])
    .unwrap();

    let resolved = resolve_plugins(&specs, &fw("0.7.0"), &fetch).await.unwrap();

    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved[0].plugin_type, PluginType::System);
    assert_eq!(resolved[1].plugin_type, PluginType::User);
    assert_eq!(resolved[1].version, PluginVersion::new(0, 2, 0, 0));
    assert!(matches!(resolved[0].source, ResolvedSource::File { .. }));

    // No plugins.json fetch for file= specs.
    assert!(!fetch.was_requested(&format!("{BASE}/plugins.json")));
}

#[tokio::test]
async fn resolve_file_user_without_system_is_rejected() {
    // A lone user plugin (via file=, so type is only known after the header is
    // read) must be rejected by the post-resolution type validation.
    let usr = header(1, (0, 2, 0, 0));
    let fetch = MockFetch::new().with("/tmp/usr.bin", usr);

    let specs = parse_plugins(&["file=/tmp/usr.bin".to_string()]).unwrap();
    let err = resolve_plugins(&specs, &fw("0.7.0"), &fetch).await;

    assert!(matches!(
        err,
        Err(Error::Plugin(PluginError::UserPluginWithoutSystem))
    ));
}

// ------------------------------------------------------------
// resolve_plugins: named path
// ------------------------------------------------------------

#[tokio::test]
async fn resolve_typed_named_skips_plugins_manifest() {
    // system/usb: type is stated, so plugins.json must NOT be fetched.
    let bin = header(0, (0, 1, 0, 0));
    let sha = sha_hex(&bin);

    let fetch = MockFetch::new()
        .with(
            &format!("{BASE}/system/usb/releases.json"),
            releases_json("USB", "0.1.0", "0.7.0", &sha),
        )
        .with(&format!("{BASE}/system/usb/v0.1.0/plugin.bin"), bin);

    let specs = parse_plugins(&["system/usb".to_string()]).unwrap();
    let resolved = resolve_plugins(&specs, &fw("0.7.0"), &fetch).await.unwrap();

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, "usb");
    assert_eq!(resolved[0].version, PluginVersion::new(0, 1, 0, 0));
    assert!(matches!(resolved[0].source, ResolvedSource::Named { .. }));

    assert!(!fetch.was_requested(&format!("{BASE}/plugins.json")));
    assert!(fetch.was_requested(&format!("{BASE}/system/usb/releases.json")));
}

#[tokio::test]
async fn resolve_bare_name_fetches_plugins_manifest() {
    // Bare "usb": type unknown, so plugins.json IS fetched to resolve it.
    let bin = header(0, (0, 1, 0, 0));
    let sha = sha_hex(&bin);

    let fetch = MockFetch::new()
        .with(&format!("{BASE}/plugins.json"), plugins_json())
        .with(
            &format!("{BASE}/system/usb/releases.json"),
            releases_json("USB", "0.1.0", "0.7.0", &sha),
        )
        .with(&format!("{BASE}/system/usb/v0.1.0/plugin.bin"), bin);

    let specs = parse_plugins(&["usb".to_string()]).unwrap();
    let resolved = resolve_plugins(&specs, &fw("0.7.0"), &fetch).await.unwrap();

    assert_eq!(resolved[0].plugin_type, PluginType::System);
    assert!(fetch.was_requested(&format!("{BASE}/plugins.json")));
}

#[tokio::test]
async fn resolve_pinned_incompatible_is_hard_error() {
    // Pin 0.1.0 which needs fw 0.8.0, but build for 0.7.0 -> incompatible.
    let bin = header(0, (0, 1, 0, 0));
    let sha = sha_hex(&bin);

    let fetch = MockFetch::new().with(
        &format!("{BASE}/system/usb/releases.json"),
        releases_json("USB", "0.1.0", "0.8.0", &sha),
    );

    let specs = parse_plugins(&["system/usb,version=0.1.0".to_string()]).unwrap();
    let err = resolve_plugins(&specs, &fw("0.7.0"), &fetch).await;

    assert!(matches!(
        err,
        Err(Error::Plugin(PluginError::Incompatible { .. }))
    ));
}

#[tokio::test]
async fn resolve_sha_mismatch_is_rejected() {
    // releases.json advertises a digest that does not match the binary.
    let bin = header(0, (0, 1, 0, 0));

    let fetch = MockFetch::new()
        .with(
            &format!("{BASE}/system/usb/releases.json"),
            releases_json("USB", "0.1.0", "0.7.0", "deadbeef"),
        )
        .with(&format!("{BASE}/system/usb/v0.1.0/plugin.bin"), bin);

    let specs = parse_plugins(&["system/usb".to_string()]).unwrap();
    let err = resolve_plugins(&specs, &fw("0.7.0"), &fetch).await;

    assert!(matches!(
        err,
        Err(Error::Plugin(PluginError::Sha256Mismatch { .. }))
    ));
}

// ------------------------------------------------------------
// Error propagation
// ------------------------------------------------------------

#[tokio::test]
async fn fetch_error_propagates_with_host_error_intact() {
    // No response registered for the releases URL -> MockErr surfaces as
    // Error::Fetch carrying the host error untouched.
    let fetch = MockFetch::new();
    let specs = parse_plugins(&["system/usb".to_string()]).unwrap();
    let err = resolve_plugins(&specs, &fw("0.7.0"), &fetch).await;

    match err {
        Err(Error::Fetch { source, error }) => {
            assert_eq!(source, format!("{BASE}/system/usb/releases.json"));
            assert!(error.0.contains("no mock response"));
        }
        other => panic!("expected Error::Fetch, got {other:?}"),
    }
}

#[tokio::test]
async fn empty_specs_resolve_to_empty() {
    let fetch = MockFetch::new();
    let resolved = resolve_plugins(&[], &fw("0.7.0"), &fetch).await.unwrap();
    assert!(resolved.is_empty());
    // Nothing should have been fetched.
    assert!(fetch.requested().is_empty());
}

// ------------------------------------------------------------
// Catalogue
// ------------------------------------------------------------

#[tokio::test]
async fn catalogue_fetch_then_load_releases() {
    let usb_bin = header(0, (0, 1, 0, 0));
    let rgb_bin = header(1, (0, 2, 0, 0));
    let usb_sha = sha_hex(&usb_bin);
    let rgb_sha = sha_hex(&rgb_bin);

    let fetch = MockFetch::new()
        .with(&format!("{BASE}/plugins.json"), plugins_json())
        .with(
            &format!("{BASE}/system/usb/releases.json"),
            releases_json("One ROM USB", "0.1.0", "0.7.0", &usb_sha),
        )
        .with(
            &format!("{BASE}/user/rgb/releases.json"),
            releases_json("One ROM RGB", "0.2.0", "0.7.0", &rgb_sha),
        );

    let mut cat = Catalogue::fetch(&fetch).await.unwrap();

    // Identities only after fetch.
    assert_eq!(cat.plugins().len(), 2);
    assert!(cat.plugin_by_name("usb").unwrap().releases.is_empty());
    assert!(cat.plugin_by_name("usb").unwrap().display_name.is_none());

    // Releases populated after load.
    cat.load_all_releases(&fetch).await.unwrap();
    let usb = cat.plugin_by_name("usb").unwrap();
    assert_eq!(usb.display_name.as_deref(), Some("One ROM USB"));
    assert_eq!(usb.releases.len(), 1);
    assert_eq!(usb.releases[0].version, PluginVersion::new(0, 1, 0, 0));
}

#[tokio::test]
async fn catalogue_resilient_load_tolerates_one_failure() {
    let usb_bin = header(0, (0, 1, 0, 0));
    let usb_sha = sha_hex(&usb_bin);

    // usb's releases are served; rgb's are NOT (no mock response) -> rgb fails.
    let fetch = MockFetch::new()
        .with(&format!("{BASE}/plugins.json"), plugins_json())
        .with(
            &format!("{BASE}/system/usb/releases.json"),
            releases_json("One ROM USB", "0.1.0", "0.7.0", &usb_sha),
        );

    let mut cat = Catalogue::fetch(&fetch).await.unwrap();
    let failures = cat.load_all_releases_resilient(&fetch).await;

    // rgb failed, usb succeeded: one failure, and it names rgb.
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, "rgb");

    // usb is fully loaded; rgb kept its empty releases rather than aborting all.
    assert_eq!(cat.plugin_by_name("usb").unwrap().releases.len(), 1);
    assert!(cat.plugin_by_name("rgb").unwrap().releases.is_empty());
}

/// Fetches the real plugins manifest and confirms it still deserialises into
/// the crate's types. Ignored by default (needs network and tracks a live
/// server); run with `cargo test -- --ignored`.
#[tokio::test]
#[ignore = "hits the live images server; run explicitly with --ignored"]
async fn live_manifest_still_parses() {
    /// A real HTTP-backed fetcher, used only by the canary.
    struct HttpFetch;
    impl LocalPluginFetch for HttpFetch {
        type Error = String;
        async fn fetch(&self, source: &str) -> Result<Vec<u8>, Self::Error> {
            // ureq is blocking, so run it on a worker thread rather than the
            // async runtime.
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

    let cat = Catalogue::fetch(&HttpFetch)
        .await
        .expect("live plugins.json should parse into Catalogue");
    assert!(
        !cat.plugins().is_empty(),
        "live catalogue should list at least one plugin"
    );

    // Load every plugin's releases, confirming releases.json also still parses.
    let mut cat = cat;
    cat.load_all_releases(&HttpFetch)
        .await
        .expect("live releases.json should parse for every plugin");
}

/// Parse a firmware version from a `major.minor.patch` string.
fn fw(s: &str) -> onerom_config::fw::FirmwareVersion {
    onerom_config::fw::FirmwareVersion::try_from_str(s).expect("valid fw version")
}