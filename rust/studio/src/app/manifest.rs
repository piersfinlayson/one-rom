// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Application manifest
//!
//! Contains runtime metadata about the application, including where to find
//! release and ROM config manifests, links, etc.

use arc_swap::ArcSwap;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use onerom_config::fw::FirmwareVersion;
use onerom_gen::MAX_SUPPORTED_FIRMWARE_VERSION_V2 as MAX_SUPPORTED_FIRMWARE_VERSION;
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use crate::AppMessage;
use crate::AppVersion;
use crate::studio::MAX_BUILD_FW_VERSIONS;

// Global application manifest state to save threading throughout the
// application.
static MANIFEST: LazyLock<ArcSwap<ManifestState>> =
    LazyLock::new(|| ArcSwap::from_pointee(ManifestState::default()));

/// Get read access to the manifest
pub fn manifest_read() -> Arc<ManifestState> {
    MANIFEST.load_full()
}

/// Update the manifest from network/cache/defaults.  Called once from app
/// initialization and subsequently on a timer.
///
/// `flag` indicates whether this update is being run at startup - this
/// function returns it on ManifestUpdated.
pub async fn update_manifest(flag: bool) -> AppMessage {
    let mut state = ManifestState::default();
    state.try_update().await;
    MANIFEST.store(Arc::new(state));
    AppMessage::ManifestUpdated(flag)
}

const MANIFEST_URL: &str = "https://images.onerom.org/studio.json";

/// Status of the manifest data
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum ManifestStatus {
    #[default]
    Uninitialized,
    Network,
    FileCache,
    Default,
}

/// Used by the application to reference specific links in the manifest
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManifestState {
    status: ManifestStatus,
    current: Manifest,
}

impl ManifestState {
    pub async fn try_update(&mut self) {
        if let Some(manifest) = Self::fetch_from_url().await {
            debug!(
                "Application manifest v{} r{} retrieved from network",
                self.current.version, self.current.revision,
            );
            self.current = manifest;
            self.status = ManifestStatus::Network;
            return;
        }

        if let Some(manifest) = Self::load_from_cache() {
            debug!(
                "Network info - application manifest v{} r{} loaded from cache",
                self.current.version, self.current.revision,
            );
            self.current = manifest;
            self.status = ManifestStatus::FileCache;
            return;
        }

        info!(
            "Using default application manifest v{} r{}",
            self.current.version, self.current.revision,
        );

        self.status = ManifestStatus::Default;
    }

    pub fn initialized(&self) -> bool {
        self.status != ManifestStatus::Uninitialized
    }

    async fn fetch_from_url() -> Option<Manifest> {
        // Retrieve and parse the manifest from the network
        let response = reqwest::get(MANIFEST_URL)
            .await
            .inspect_err(|e| warn!("Failed to fetch application manifest from network: {e}"))
            .ok()?;
        let manifest: Manifest = response
            .json()
            .await
            .inspect_err(|e| warn!("Failed to parse application manifest from network: {e}"))
            .ok()?;

        // Save to cache on success
        Self::save_to_cache(&manifest);

        Some(manifest)
    }

    fn save_to_cache(manifest: &Manifest) {
        trace!("Caching application manifest to file");

        let json = match serde_json::to_string_pretty(manifest) {
            Ok(json) => json,
            Err(e) => {
                warn!("Failed to serialize application manifest for caching: {e}");
                return;
            }
        };

        let file_path = Self::cache_file_path();

        // Create parent directory if it doesn't exist
        #[allow(clippy::collapsible_if)]
        if let Some(parent) = file_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                warn!("Failed to create cache directory {parent:?}: {e}");
                return;
            }
        }

        let _ = fs::write(&file_path, json).inspect_err(|e| {
            warn!("Failed to write application manifest to cache file {file_path:?}: {e}")
        });
    }

    fn load_from_cache() -> Option<Manifest> {
        let file_path = Self::cache_file_path();
        let contents = fs::read_to_string(&file_path)
            .inspect_err(|e| {
                warn!("Failed to read application manifest from cache file {file_path:?}: {e}")
            })
            .ok()?;
        serde_json::from_str(&contents).ok()
    }

    fn cache_file_path() -> PathBuf {
        directories::ProjectDirs::from("org", "onerom", "studio")
            .unwrap()
            .cache_dir()
            .join("manifest.json")
    }

    pub fn link_url(&self, link: Link) -> &str {
        match link {
            Link::OneRom => &self.current.link_urls.one_rom,
            Link::PiersRocks => &self.current.link_urls.piers_rocks,
            Link::Zadig => &self.current.link_urls.zadig,
            Link::WinUsb => &self.current.link_urls.win_usb,
            Link::GitHubIssue => &self.current.link_urls.github_issue,
            Link::LinuxUdev => &self.current.link_urls.linux_udev,
            Link::RomConfigs => &self.current.link_urls.rom_configs,
            Link::AppUpdate => &self.current.link_urls.app_update,
        }
    }

    pub fn manifest_url(&self, manifest: ManifestType) -> &str {
        match manifest {
            ManifestType::FirmwareRelease => &self.current.manifest_urls.fw_release,
            ManifestType::RomConfig => &self.current.manifest_urls.rom_config,
            ManifestType::AppRelease => &self.current.manifest_urls.app_release,
            ManifestType::Plugin => &self.current.manifest_urls.plugin,
        }
    }

    pub fn schema_url(&self, schema: Schema) -> &str {
        match schema {
            Schema::AppManifest => &self.current.schema_urls.app_manifest,
            Schema::RomConfig => &self.current.schema_urls.rom_config,
            Schema::FirmwareRelease => &self.current.schema_urls.fw_release,
            Schema::AppRelease => &self.current.schema_urls.app_release,
        }
    }

    fn path_url(&self, path: PathType) -> &str {
        match path {
            PathType::FirmwareRelease => &self.current.paths.firmware_releases,
            PathType::RomConfig => &self.current.paths.rom_configs,
        }
    }

    pub fn url_from_path(&self, path: PathType, partial: &str) -> String {
        let path = self.path_url(path);
        if path.ends_with("/") {
            format!("{}{}", path, partial)
        } else {
            format!("{}/{}", path, partial)
        }
    }

    pub fn update_available(&self) -> Option<String> {
        let current = Version::parse(env!("CARGO_PKG_VERSION"))
            .inspect_err(|e| warn!("Failed to parse current app version: {e}"))
            .ok()?;

        let latest = &self.current.latest_app_version;
        let latest = Version::parse(latest)
            .inspect_err(|e| warn!("Failed to parse latest app version {latest:?}: {e}"))
            .ok()?;

        if latest > current {
            Some(latest.to_string())
        } else {
            None
        }
    }

    /// Check if this is a development version - i.e. newer than the latest
    pub fn dev_version(&self) -> bool {
        let current = match Version::parse(env!("CARGO_PKG_VERSION")) {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to parse current app version {e}");
                return false;
            }
        };

        let latest = &self.current.latest_app_version;
        match Version::parse(latest) {
            Ok(v) => current > v,
            Err(e) => {
                warn!("Failed to parse latest app version {latest:?}: {e}");
                false
            }
        }
    }

    /// Get the maximum firmware version this app can build
    /// Returns None if no compatibility info exists for current app version
    pub fn max_buildable_firmware(&self) -> Option<&FirmwareVersion> {
        let current = AppVersion::default();
        self.current
            .firmware_build_compatibility
            .iter()
            .find(|c| c.app_version == current)
            .map(|c| &c.max_buildable_firmware_version)
    }
}

/// One ROM Studio application manifest
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Manifest {
    /// Manifest version - should be incremented on any breaking changes
    pub version: u32,
    /// Manifest revision - should be incremented whenever ANY content changes
    pub revision: u32,
    /// Latest application version available
    pub latest_app_version: String,
    /// Links used by the application within the UI
    pub link_urls: LinkUrls,
    /// Manifests used by the application
    pub manifest_urls: ManifestUrls,
    /// Schemas used by the application
    pub schema_urls: Schemas,
    /// URL paths
    pub paths: PathUrls,
    /// Firmware compatibility information.  Represents maximum firmware
    /// version that this application version can build.
    ///
    /// Introduced in v0.1.12.
    pub firmware_build_compatibility: Vec<FirmwareBuildCompatibility>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 1, // Update every time content changes, even if not a breaking change
            latest_app_version: env!("CARGO_PKG_VERSION").to_string(),
            link_urls: LinkUrls::default(),
            manifest_urls: ManifestUrls::default(),
            schema_urls: Schemas::default(),
            paths: PathUrls::default(),
            firmware_build_compatibility: MAX_BUILD_FW_VERSIONS.to_vec(),
        }
    }
}

/// Firmware build compatibility information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FirmwareBuildCompatibility {
    /// App version (semver)
    pub app_version: AppVersion,

    /// Maximum firmware version this app can build (not flash - any version can be flashed)
    pub max_buildable_firmware_version: FirmwareVersion,
}

impl Default for FirmwareBuildCompatibility {
    fn default() -> Self {
        Self {
            app_version: AppVersion::default(),
            max_buildable_firmware_version: MAX_SUPPORTED_FIRMWARE_VERSION,
        }
    }
}

/// Manifests used by the application
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ManifestUrls {
    /// One ROM firmware releases manifest
    pub fw_release: String,

    /// ROM configurations manifest
    pub rom_config: String,

    /// One ROM Studio app releases manifest
    pub app_release: String,

    /// Plugin manifest
    pub plugin: String,
}

impl Default for ManifestUrls {
    fn default() -> Self {
        Self {
            fw_release: "https://images.onerom.org/releases.json".to_string(),
            rom_config: "https://images.onerom.org/configs.json".to_string(),
            app_release: "https://images.onerom.org/studio/releases.json".to_string(),
            plugin: "https://images.onerom.org/plugins/plugins.json".to_string(),
        }
    }
}

impl ManifestUrls {
    /// Get the firmware release manifest URL
    pub fn fw_release_url(&self) -> &str {
        &self.fw_release
    }

    /// Get the ROM configuration manifest URL
    pub fn rom_config_url(&self) -> &str {
        &self.rom_config
    }

    /// Get the application release manifest URL
    pub fn app_release_url(&self) -> &str {
        &self.app_release
    }
}

/// Links used within the application
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LinkUrls {
    /// Main One ROM website
    pub one_rom: String,

    /// Main piers.rocks website
    pub piers_rocks: String,

    /// Main Zadig website
    pub zadig: String,

    /// Windows USB driver instructions
    pub win_usb: String,

    /// One ROM Studio GitHub issues page
    pub github_issue: String,

    /// Linux udev rules instructions
    pub linux_udev: String,

    /// ROM configuration information page
    pub rom_configs: String,

    /// Application update page
    pub app_update: String,
}

/// Links used within the application
impl Default for LinkUrls {
    fn default() -> Self {
        Self {
            one_rom: "https://onerom.org".to_string(),
            piers_rocks: "https://piers.rocks".to_string(),
            zadig: "https://zadig.akeo.ie/".to_string(),
            win_usb: "https://onerom.org/web/#windows".to_string(),
            github_issue: "https://github.com/piersfinlayson/one-rom/issues".to_string(),
            linux_udev: "https://onerom.org/web/#linux".to_string(),
            rom_configs: "https://images.onerom.org/#rom-configs".to_string(),
            app_update: "https://onerom.org/studio/#downloads".to_string(),
        }
    }
}

/// Schemas used within the application
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Schemas {
    /// JSON schema for the application manifest (this schema)
    pub app_manifest: String,

    /// JSON schema for ROM configuration files
    pub rom_config: String,

    /// JSON schema for firmware release manifest
    pub fw_release: String,

    /// JSON schema for application release manifest
    pub app_release: String,

    /// JSON schema for plugin manifest
    pub plugin: String,

    /// JSON schema for plugin release manifest
    pub plugin_release: String,
}

impl Default for Schemas {
    fn default() -> Self {
        Self {
            app_manifest: "https://images.onerom.org/studio/app-schema.json".to_string(),
            rom_config: "https://images.onerom.org/configs/schema.json".to_string(),
            fw_release: "".to_string(),
            app_release: "".to_string(),
            plugin: "https://images.onerom.org/plugins/plugin-schema.json".to_string(),
            plugin_release: "https://images.onerom.org/plugins/plugin-release-schema.json"
                .to_string(),
        }
    }
}

/// URL paths used within the application
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PathUrls {
    /// Path to firmware releases on the server
    pub firmware_releases: String,

    /// Path to rom configs on the server
    pub rom_configs: String,
}

impl Default for PathUrls {
    fn default() -> Self {
        Self {
            firmware_releases: "https://images.onerom.org/".to_string(), // Not in releases/, also currently unused!
            rom_configs: "https://images.onerom.org/".to_string(), // Manifest contains configs/
        }
    }
}

/// Supported links
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// https://onerom.org
    OneRom,
    /// https://piers.rocks
    PiersRocks,
    /// https://zadig.akeo.ie/
    Zadig,
    /// https://onerom.org/web/#windows
    WinUsb,
    /// https://github.com/piersfinlayson/one-rom/issues
    GitHubIssue,
    /// https://onerom.org/web/#linux
    LinuxUdev,
    /// https://images.onerom.org/#rom-configs
    RomConfigs,
    /// https://onerom.org/studio/
    AppUpdate,
}

/// Supported manifests
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestType {
    /// Firmware release manifest
    FirmwareRelease,
    /// ROM configuration manifest
    RomConfig,
    /// Application release manifest
    AppRelease,
    /// Plugin manifest
    Plugin,
}

/// Supported schemas
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schema {
    /// Application manifest schema
    AppManifest,
    /// ROM configuration schema
    RomConfig,
    /// Firmware release schema
    FirmwareRelease,
    /// Application release schema
    AppRelease,
}

/// Supported paths
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathType {
    /// Firmware releases path
    FirmwareRelease,
    /// ROM configuration path
    RomConfig,
}
