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
    Catalogue, Error, LocalPluginFetch, PluginError, PluginNote, PluginType, PluginVersion,
    ResolvedSource, check_config_plugins, parse_plugins, resolve_plugins,
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

// ------------------------------------------------------------
// check_config_plugins: plugins named by a config
// ------------------------------------------------------------

/// A `releases.json` body for a single release carrying an upper bound as well
/// as a minimum - the window that exists only in the manifest, and the whole
/// reason [`check_config_plugins`] exists.
fn releases_json_bounded(
    version: &str,
    min_fw: &str,
    incompatible_from: &str,
    sha: &str,
) -> Vec<u8> {
    format!(
        r#"{{
            "version": 1,
            "display_name": "One ROM USB",
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
                    "min_fw_version": "{min_fw}",
                    "incompatible_from": "{incompatible_from}"
                }}
            ]
        }}"#
    )
    .into_bytes()
}

/// A `releases.json` body mirroring the shape of the real `usb` manifest:
/// a current release, newest first, and an older one withdrawn from the
/// firmware the current one requires.
fn releases_json_usb_pair(current_sha: &str, withdrawn_sha: &str) -> Vec<u8> {
    format!(
        r#"{{
            "version": 1,
            "display_name": "One ROM USB",
            "description": "A test plugin",
            "latest": "0.2.1",
            "releases": [
                {{
                    "version": "0.2.1",
                    "path": "v0.2.1",
                    "filename": "plugin.bin",
                    "sha256": "{current_sha}",
                    "api_version": 1,
                    "plugin_type": "system_plugin",
                    "min_fw_version": "0.7.0"
                }},
                {{
                    "version": "0.1.2",
                    "path": "v0.1.2",
                    "filename": "plugin.bin",
                    "sha256": "{withdrawn_sha}",
                    "api_version": 1,
                    "plugin_type": "system_plugin",
                    "min_fw_version": "0.6.9",
                    "incompatible_from": "0.7.0"
                }}
            ]
        }}"#
    )
    .into_bytes()
}

/// The URL a system `usb` plugin binary of the given version lives at.
fn usb_binary_url(version: &str) -> String {
    format!("{BASE}/system/usb/v{version}/plugin.bin")
}

/// A minimal config naming a single system plugin loaded from `source`.
///
/// A system plugin must occupy the first chip set, and one ROM follows it so
/// the config is one the builder accepts.
fn plugin_config_json(source: &str) -> String {
    format!(
        r#"{{
            "version": 1,
            "name": "plugin test",
            "description": "A config naming a plugin",
            "chip_sets": [
                {{
                    "type": "single",
                    "chips": [
                        {{ "file": "{source}", "type": "system_plugin", "size_handling": "pad" }}
                    ]
                }},
                {{
                    "type": "single",
                    "chips": [
                        {{ "file": "/tmp/rom.bin", "type": "2364", "cs1": "active_low" }}
                    ]
                }}
            ]
        }}"#
    )
}

/// Build a loaded [`Builder`] for a config naming one system plugin at
/// `source`, with `plugin` as that plugin's image.
fn loaded_builder(source: &str, plugin: &[u8]) -> onerom_gen::Builder {
    let mut builder = onerom_gen::Builder::from_json(
        fw("0.7.0"),
        onerom_config::mcu::Family::Rp2350,
        &plugin_config_json(source),
    )
    .expect("config should build");

    // Load every file the config names: the plugin image, and a blank ROM.
    for spec in builder.file_specs() {
        let data = if spec.source == source {
            plugin.to_vec()
        } else {
            vec![0xFFu8; 8 * 1024]
        };
        builder
            .add_file(onerom_gen::FileData::new(spec.id, data))
            .expect("file should load");
    }

    builder
}

#[tokio::test]
async fn config_plugin_withdrawn_for_this_firmware_is_rejected() {
    // The real case from issue #288: One ROM USB v0.1.2 declares min_fw 0.6.9
    // in its binary header - which firmware 0.7.0 satisfies - but the manifest
    // withdraws it from 0.7.0, where it hard faults. Only the manifest knows.
    let bin = header(0, (0, 1, 2, 0));
    let current = header(0, (0, 2, 1, 0));
    let url = usb_binary_url("0.1.2");
    let fetch = MockFetch::new().with(
        &format!("{BASE}/system/usb/releases.json"),
        releases_json_usb_pair(&sha_hex(&current), &sha_hex(&bin)),
    );

    let builder = loaded_builder(&url, &bin);

    let err = check_config_plugins(&builder, &fw("0.7.0"), &fetch)
        .await
        .expect_err("a withdrawn plugin must be refused");

    let PluginError::IncompatibleNewer {
        ref name,
        version,
        from,
        fw: got,
        ref newest_compatible,
    } = err
    else {
        panic!("expected IncompatibleNewer, got {err:?}");
    };

    assert_eq!(name, "usb");
    assert_eq!(version, PluginVersion::new(0, 1, 2, 0));
    assert_eq!(from, fw("0.7.0"));
    assert_eq!(got, fw("0.7.0"));

    // The way out: the newest release that does support this firmware, and the
    // URL a config has to be pointed at to use it.
    let suggested = newest_compatible
        .as_ref()
        .expect("0.2.1 supports 0.7.0, so it should be suggested");
    assert_eq!(suggested.version, PluginVersion::new(0, 2, 1, 0));
    assert_eq!(suggested.binary_url, usb_binary_url("0.2.1"));

    // And it reaches the rendered message, which is what a user actually sees.
    let rendered = err.to_string();
    assert!(
        rendered.contains("plugin version 0.2.1 supports it"),
        "message should name the way out: {rendered}"
    );
    assert!(
        rendered.contains(&usb_binary_url("0.2.1")),
        "message should carry the URL: {rendered}"
    );
}

#[tokio::test]
async fn config_plugin_with_no_way_out_says_so() {
    // Every release of this plugin is withdrawn from 0.7.0, so there is no
    // version to suggest. Discriminates a real suggestion from one that just
    // echoes the newest release whether or not it helps.
    let bin = header(0, (0, 1, 2, 0));
    let url = usb_binary_url("0.1.2");
    let fetch = MockFetch::new().with(
        &format!("{BASE}/system/usb/releases.json"),
        releases_json_bounded("0.1.2", "0.6.9", "0.7.0", &sha_hex(&bin)),
    );

    let builder = loaded_builder(&url, &bin);

    let err = check_config_plugins(&builder, &fw("0.7.0"), &fetch)
        .await
        .expect_err("a withdrawn plugin must be refused");

    let PluginError::IncompatibleNewer {
        ref newest_compatible,
        ..
    } = err
    else {
        panic!("expected IncompatibleNewer, got {err:?}");
    };

    assert!(newest_compatible.is_none());
    assert!(
        err.to_string().contains("no version of this plugin"),
        "message should say there is no way out: {err}"
    );
}

#[tokio::test]
async fn config_plugin_inside_its_window_is_accepted() {
    // The same plugin, the same manifest, on the last firmware it supports.
    // Discriminates the check above from one that rejects any bounded release.
    let bin = header(0, (0, 1, 2, 0));
    let url = usb_binary_url("0.1.2");
    let fetch = MockFetch::new().with(
        &format!("{BASE}/system/usb/releases.json"),
        releases_json_bounded("0.1.2", "0.6.9", "0.7.0", &sha_hex(&bin)),
    );

    let builder = loaded_builder(&url, &bin);

    let notes = check_config_plugins(&builder, &fw("0.6.9"), &fetch)
        .await
        .expect("a plugin inside its window is fine");

    assert!(matches!(
        notes.as_slice(),
        [PluginNote::Checked { name, version }]
            if name == "usb" && *version == PluginVersion::new(0, 1, 2, 0)
    ));
}

#[tokio::test]
async fn config_plugin_below_its_minimum_is_rejected() {
    // The lower bound still applies here, so both ends of the window are
    // enforced from one place.
    let bin = header(0, (0, 1, 2, 0));
    let url = usb_binary_url("0.1.2");
    let fetch = MockFetch::new().with(
        &format!("{BASE}/system/usb/releases.json"),
        releases_json_bounded("0.1.2", "0.6.9", "0.7.0", &sha_hex(&bin)),
    );

    let builder = loaded_builder(&url, &bin);

    let err = check_config_plugins(&builder, &fw("0.6.8"), &fetch)
        .await
        .expect_err("firmware below the minimum must be refused");

    assert!(
        matches!(err, PluginError::Incompatible { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn config_plugin_binary_not_matching_the_manifest_is_rejected() {
    // The binary at the official URL is not the one the manifest published.
    // The digest is the only thing that catches this, and it is the check
    // `onerom-gen` cannot do.
    let published = header(0, (0, 1, 2, 0));
    let actual = header(0, (0, 1, 2, 1)); // different build: different digest
    let url = usb_binary_url("0.1.2");
    let fetch = MockFetch::new().with(
        &format!("{BASE}/system/usb/releases.json"),
        releases_json_bounded("0.1.2", "0.6.9", "0.8.0", &sha_hex(&published)),
    );

    let builder = loaded_builder(&url, &actual);

    let err = check_config_plugins(&builder, &fw("0.7.0"), &fetch)
        .await
        .expect_err("a binary that is not the published one must be refused");

    assert!(
        matches!(err, PluginError::Sha256Mismatch { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn config_plugin_from_a_local_path_is_not_checked() {
    // A local path names no release, so there is nothing to check it against.
    // It must not be rejected, and no manifest should be fetched for it.
    let bin = header(0, (0, 1, 2, 0));
    let fetch = MockFetch::new();

    let builder = loaded_builder("/tmp/my-plugin.bin", &bin);

    let notes = check_config_plugins(&builder, &fw("0.7.0"), &fetch)
        .await
        .expect("a local plugin is its author's business");

    assert!(matches!(
        notes.as_slice(),
        [PluginNote::Unofficial { source }] if source == "/tmp/my-plugin.bin"
    ));
    assert!(fetch.requested().is_empty());
}

#[tokio::test]
async fn config_plugin_survives_an_unreachable_manifest() {
    // Offline, or the images server down: the check cannot run, but an
    // otherwise valid build must not be blocked - it is reported instead.
    let bin = header(0, (0, 1, 2, 0));
    let url = usb_binary_url("0.1.2");
    let fetch = MockFetch::new(); // no releases.json: the fetch fails

    let builder = loaded_builder(&url, &bin);

    let notes = check_config_plugins(&builder, &fw("0.7.0"), &fetch)
        .await
        .expect("an unreachable manifest must not fail the build");

    assert!(matches!(
        notes.as_slice(),
        [PluginNote::Unchecked { source, error: Error::Fetch { .. } }] if *source == url
    ));
}

#[tokio::test]
async fn config_plugin_absent_from_the_manifest_is_not_checked() {
    // The binary exists on the server but the manifest no longer lists that
    // release. Nothing to check it against, and no grounds to refuse it.
    let bin = header(0, (0, 1, 1, 0));
    let url = usb_binary_url("0.1.1");
    let fetch = MockFetch::new().with(
        &format!("{BASE}/system/usb/releases.json"),
        releases_json_bounded("0.1.2", "0.6.9", "0.7.0", &sha_hex(&bin)),
    );

    let builder = loaded_builder(&url, &bin);

    let notes = check_config_plugins(&builder, &fw("0.7.0"), &fetch)
        .await
        .expect("an unlisted release must not fail the build");

    assert!(matches!(
        notes.as_slice(),
        [PluginNote::Unchecked {
            error: Error::Plugin(PluginError::VersionNotFound(..)),
            ..
        }]
    ));
}

#[tokio::test]
async fn config_without_plugins_checks_nothing() {
    // A ROM-only config must not fetch anything, so the overwhelmingly common
    // build gains no network round trip from this check.
    let builder = onerom_gen::Builder::from_json(
        fw("0.7.0"),
        onerom_config::mcu::Family::Rp2350,
        r#"{
            "version": 1,
            "description": "ROMs only",
            "chip_sets": [
                {
                    "type": "single",
                    "chips": [
                        { "file": "/tmp/rom.bin", "type": "2364", "cs1": "active_low" }
                    ]
                }
            ]
        }"#,
    )
    .expect("config should build");

    let fetch = MockFetch::new();
    let notes = check_config_plugins(&builder, &fw("0.7.0"), &fetch)
        .await
        .unwrap();

    assert!(notes.is_empty());
    assert!(fetch.requested().is_empty());
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
