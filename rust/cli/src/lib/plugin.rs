// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Plugin specification parsing, manifest structs, and ROM configuration
//! JSON generation.
//!
//! # Plugin slot positions
//!
//! Plugin slot positions within the firmware image are fixed:
//!   - System plugin: always chip_set 0
//!   - User plugin:   always chip_set 1
//!   - ROM images:    chip_set 2 onwards
//!
//! A user plugin requires a system plugin to be present. At most one of each
//! type is supported per firmware image.
//!
//! # Processing pipeline
//!
//! Plugin specs are processed in two phases:
//!
//! ## Phase 1 - Parse ([`parse_plugins`])
//!
//! Converts raw `--plugin` strings into [`PluginSpec`] values. This phase
//! validates syntax and performs best-effort semantic checks for specs where
//! the plugin type is already known (i.e. `system/usb` or `user/foo` forms).
//! It cannot fully validate specs where the type is not yet known (bare name
//! or file= forms), as the type must be resolved from the manifest or binary
//! header first.
//!
//! ## Phase 2 - Resolution (caller responsibility)
//!
//! For [`PluginSpec::Named`] specs, the caller fetches the plugin manifest to
//! resolve the binary URL and confirm the type. For [`PluginSpec::File`]
//! specs, the caller fetches the binary and parses its header to determine the
//! type.
//!
//! Once all types are known, the caller must call
//! [`validate_resolved_plugin_types`] to perform the full semantic check
//! (no duplicates, user requires system).

use onerom_config::chip::ChipType as OraChipType;
use onerom_config::fw::FirmwareVersion;
use onerom_gen::{ChipConfig, ChipSetConfig, ChipSetType, SizeHandling};
use serde::Deserialize;

use crate::Error;

/// Base URL for plugin manifests on the images server.
const PLUGIN_SITE_BASE: &str = "https://images.onerom.org/plugins";

/// Maximum plugin binary size in bytes (64KB).
///
/// Plugins occupy exactly one 64KB slot in the firmware image. Binaries
/// smaller than this are padded; binaries larger are rejected.
const PLUGIN_MAX_SIZE: usize = 64 * 1024;

// Canonical plugin type string for system plugins, as used in JSON configs.
const PLUGIN_TYPE_SYSTEM: &str = "system_plugin";

// Canonical plugin type string for user plugins, as used in JSON configs.
const PLUGIN_TYPE_USER: &str = "user_plugin";

// ============================================================
// PluginVersion
// ============================================================

/// A plugin version with four components as defined in the plugin binary header.
///
/// Manifest version strings use three components (e.g. `0.1.0`); `build`
/// defaults to 0 when deserialising from such strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub build: u16,
}

impl PluginVersion {
    pub fn new(major: u16, minor: u16, patch: u16, build: u16) -> Self {
        Self {
            major,
            minor,
            patch,
            build,
        }
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        match parts.as_slice() {
            [major, minor, patch] => Some(Self {
                major: major.parse().ok()?,
                minor: minor.parse().ok()?,
                patch: patch.parse().ok()?,
                build: 0,
            }),
            [major, minor, patch, build] => Some(Self {
                major: major.parse().ok()?,
                minor: minor.parse().ok()?,
                patch: patch.parse().ok()?,
                build: build.parse().ok()?,
            }),
            _ => None,
        }
    }
}

impl std::fmt::Display for PluginVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.build == 0 {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        } else {
            write!(
                f,
                "{}.{}.{}.{}",
                self.major, self.minor, self.patch, self.build
            )
        }
    }
}

impl serde::Serialize for PluginVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for PluginVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        PluginVersion::try_from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid plugin version '{s}'")))
    }
}

// ============================================================
// PluginType
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginType {
    System,
    User,
}

impl std::fmt::Display for PluginType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.short())
    }
}

impl PluginType {
    pub fn try_from_str(s: &str) -> Option<Self> {
        match s {
            "system" | "system_plugin" => Some(PluginType::System),
            "user" | "user_plugin" => Some(PluginType::User),
            _ => None,
        }
    }

    pub fn canonical(self) -> &'static str {
        match self {
            PluginType::System => PLUGIN_TYPE_SYSTEM,
            PluginType::User => PLUGIN_TYPE_USER,
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            PluginType::System => "system",
            PluginType::User => "user",
        }
    }

    pub fn slot_index(self) -> usize {
        match self {
            PluginType::System => 0,
            PluginType::User => 1,
        }
    }
}

impl serde::Serialize for PluginType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.canonical())
    }
}

impl<'de> serde::Deserialize<'de> for PluginType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        PluginType::try_from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unrecognised plugin type '{s}'")))
    }
}

/// Recognised keys in a named `--plugin` spec.
const PLUGIN_SPEC_KEYS: &[&str] = &["version"];

// ============================================================
// Manifest structs
// ============================================================

/// Top-level plugin manifest (`plugins.json`).
///
/// Lists all available first-party plugins. Fetched from
/// `https://images.onerom.org/plugins/plugins.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginsManifest {
    /// Manifest schema version.
    pub version: u32,
    /// List of available plugins.
    pub plugins: Vec<PluginEntry>,
}

/// A single plugin entry in the top-level manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginEntry {
    /// Plugin slug, matches the directory name in the images repo.
    pub name: String,
    /// Plugin type.
    #[serde(rename = "type")]
    pub plugin_type: PluginType,
    /// Relative path to the plugin directory (e.g. `system/usb`).
    pub path: String,
}

/// Per-plugin release manifest (`releases.json`).
///
/// Contains release history for a single plugin. Fetched from
/// `https://images.onerom.org/plugins/{type}/{name}/releases.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginReleasesManifest {
    /// Manifest schema version.
    pub version: u32,
    /// Human-readable plugin name.
    pub display_name: String,
    /// Short description of the plugin.
    pub description: String,
    /// Latest released version, or None if no releases.
    pub latest: Option<PluginVersion>,
    /// All releases, newest first.
    pub releases: Vec<PluginRelease>,
}

/// A single plugin release entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginRelease {
    /// Plugin version.
    pub version: PluginVersion,
    /// Relative path to the version directory (e.g. `v0.1.0`).
    pub path: String,
    /// Binary filename within the version directory.
    pub filename: String,
    /// SHA256 hex digest of the binary.
    pub sha256: String,
    /// Plugin API version this release targets.
    pub api_version: u32,
    /// Plugin type.
    pub plugin_type: PluginType,
    /// Minimum One ROM firmware version required to run this plugin.
    #[serde(deserialize_with = "deserialize_fw_version")]
    pub min_fw_version: FirmwareVersion,
    /// First One ROM firmware version this release is known to be incompatible
    /// with, recorded after release once a breaking firmware change is found.
    ///
    /// The compatibility window is half-open: `[min_fw_version, incompatible_from)`.
    /// `None` means there is no known upper bound.
    ///
    /// This is advisory only. The bound is discovered after the plugin binary
    /// is built, so it cannot live in the binary header and the firmware does
    /// not enforce it; it is honoured by manifest-aware tooling (this CLI)
    /// alone. Tools that bypass the manifest (sideloaded `file=` binaries, the
    /// web tool, third-party tooling) do not see it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_fw_version"
    )]
    pub incompatible_from: Option<FirmwareVersion>,
}

fn deserialize_fw_version<'de, D>(d: D) -> Result<FirmwareVersion, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    FirmwareVersion::try_from_str(&s).map_err(serde::de::Error::custom)
}

fn deserialize_opt_fw_version<'de, D>(d: D) -> Result<Option<FirmwareVersion>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(d)?;
    opt.map(|s| FirmwareVersion::try_from_str(&s))
        .transpose()
        .map_err(serde::de::Error::custom)
}

/// Why a release is incompatible with a given firmware version.
#[derive(Debug, Clone, Copy)]
enum FwIncompat {
    /// Firmware predates the release's minimum supported firmware version.
    TooOld,
    /// Firmware is at or beyond the version from which this release is known to
    /// be incompatible. Carries that boundary version.
    TooNew(FirmwareVersion),
}

/// Build the appropriate error for a release that is incompatible with `fw`.
fn fw_incompat_error(
    name: &str,
    release: &PluginRelease,
    reason: FwIncompat,
    fw: &FirmwareVersion,
) -> Error {
    match reason {
        FwIncompat::TooOld => Error::PluginIncompatible(
            name.to_string(),
            release.version,
            release.min_fw_version,
            *fw,
        ),
        FwIncompat::TooNew(from) => {
            Error::PluginIncompatibleNewer(name.to_string(), release.version, from, *fw)
        }
    }
}

impl PluginRelease {
    /// Full URL to the plugin binary for this release.
    pub fn binary_url(&self, plugin_type: PluginType, plugin_name: &str) -> String {
        format!(
            "{}/{}/{}/{}/{}",
            PLUGIN_SITE_BASE,
            plugin_type.short(),
            plugin_name,
            self.path,
            self.filename
        )
    }

    /// Returns why this release is incompatible with `fw`, or `None` if it is
    /// compatible.
    ///
    /// The compatibility window is half-open: `[min_fw_version, incompatible_from)`.
    /// A release with no `incompatible_from` has no upper bound.
    fn fw_incompat(&self, fw: &FirmwareVersion) -> Option<FwIncompat> {
        if fw < &self.min_fw_version {
            Some(FwIncompat::TooOld)
        } else if let Some(from) = self.incompatible_from
            && fw >= &from
        {
            Some(FwIncompat::TooNew(from))
        } else {
            None
        }
    }

    /// Returns true if this release is compatible with the given firmware version.
    pub fn compatible_with_firmware(&self, fw: &FirmwareVersion) -> bool {
        self.fw_incompat(fw).is_none()
    }
}

// ============================================================
// Manifest fetch functions
// ============================================================

/// Fetch the top-level plugins manifest from the images server.
pub async fn fetch_plugins_manifest() -> Result<PluginsManifest, Error> {
    let url = format!("{}/plugins.json", PLUGIN_SITE_BASE);
    fetch_json(&url).await
}

/// Fetch the per-plugin releases manifest from the images server.
pub async fn fetch_plugin_releases(
    plugin_type: PluginType,
    plugin_name: &str,
) -> Result<PluginReleasesManifest, Error> {
    let url = format!(
        "{}/{}/{}/releases.json",
        PLUGIN_SITE_BASE,
        plugin_type.short(),
        plugin_name
    );
    fetch_json(&url).await
}

/// Fetch and deserialise JSON from a URL.
async fn fetch_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, Error> {
    log::debug!("Fetching {url}");
    let response = reqwest::get(url)
        .await
        .map_err(|e| Error::Network(url.to_string(), e.to_string()))?;

    if !response.status().is_success() {
        return Err(Error::Http(url.to_string(), response.status().as_u16()));
    }

    response
        .json::<T>()
        .await
        .map_err(|e| Error::Json(url.to_string(), e.to_string()))
}

// ============================================================
// PluginSpec parsing
// ============================================================

/// A parsed plugin specification from a `--plugin` argument.
///
/// Produced by [`parse_plugins`] (phase 1). The plugin type and resolved
/// binary path are not available until phase 2 resolution is complete.
///
/// Accepted argument forms:
///
/// ```text
/// usb                            name lookup, latest compatible version
/// system/usb                     name lookup with explicit type
/// usb,version=0.1.0              name lookup, pinned version
/// system/usb,version=0.1.0       name lookup with explicit type, pinned version
/// file=path/to/plugin.bin        local file, type from binary header
/// file=https://example.com/p.bin remote file, type from binary header
/// ```
#[derive(Debug, Clone)]
pub enum PluginSpec {
    /// Look up plugin by name (and optional type) from the release manifest.
    ///
    /// `plugin_type` is `None` when the type was not specified by the user;
    /// the manifest lookup in phase 2 will confirm it.
    Named {
        name: String,
        plugin_type: Option<PluginType>,
        version: Option<PluginVersion>,
    },
    /// Use a local or remote file directly.
    ///
    /// The plugin type is unknown until phase 2, when the binary header is
    /// parsed.
    File { path: String },
}

/// Parse a single raw `--plugin` string into a [`PluginSpec`].
///
/// Validates syntax only. Does not access the network or filesystem.
fn parse_plugin(s: &str) -> Result<PluginSpec, Error> {
    // file= form: remainder is the path or URL, no further parsing
    if let Some(path) = s.strip_prefix("file=") {
        if path.is_empty() {
            return Err(Error::InvalidArgument(
                "--plugin".to_string(),
                format!("file path must not be empty\n    --plugin '{s}'"),
            ));
        }
        return Ok(PluginSpec::File {
            path: path.to_string(),
        });
    }

    // Named form: split on first comma to separate the name part from
    // any key=value options that follow
    let mut parts = s.splitn(2, ',');
    let name_part = parts.next().unwrap();
    let kv_part = parts.next();

    // Parse key=value options, currently only version= is supported
    let mut version = None;
    if let Some(kv) = kv_part {
        let mut seen = std::collections::HashSet::new();
        for kv in kv.split(',') {
            let (key, value) = kv.split_once('=').ok_or_else(|| {
                Error::InvalidArgument(
                    "--plugin".to_string(),
                    format!(
                        "Plugin option '{kv}' is missing a value - expected '{kv}=<value>'\n    --plugin '{s}'"
                    ),
                )
            })?;
            if !seen.insert(key) {
                return Err(Error::InvalidArgument(
                    "--plugin".to_string(),
                    format!("Duplicate plugin option '{key}'\n    --plugin '{s}'"),
                ));
            }
            match key {
                "version" => {
                    version = Some(PluginVersion::try_from_str(value).ok_or_else(|| {
                        Error::InvalidArgument(
                            "--plugin".to_string(),
                            format!("Invalid plugin version format '{value}'\n    --plugin '{s}'"),
                        )
                    })?);
                }
                other => {
                    let supported = PLUGIN_SPEC_KEYS.join(", ");
                    return Err(Error::InvalidArgument(
                        "--plugin".to_string(),
                        format!(
                            "Unrecognised plugin option '{other}'\n    --plugin '{s}'\n  Supported options: {supported}"
                        ),
                    ));
                }
            }
        }
    }

    // Parse optional type/ prefix from the name part.
    // e.g. "system/usb" -> type=System, name=usb
    //      "usb"        -> type=None,   name=usb
    let (plugin_type, name) = if let Some((type_str, name_str)) = name_part.split_once('/') {
        let pt = PluginType::try_from_str(type_str).ok_or_else(|| {
            Error::InvalidArgument(
                "--plugin".to_string(),
                format!(
                    "Invalid plugin type '{type_str}': expected 'system' or 'user'\n    --plugin '{s}'"
                ),
            )
        })?;
        if name_str.is_empty() {
            return Err(Error::InvalidArgument(
                "--plugin".to_string(),
                format!("Plugin name must not be empty\n    --plugin '{s}'"),
            ));
        }
        (Some(pt), name_str.to_string())
    } else {
        if name_part.is_empty() {
            return Err(Error::InvalidArgument(
                "--plugin".to_string(),
                format!("Plugin name must not be empty\n    --plugin '{s}'"),
            ));
        }
        (None, name_part.to_string())
    };

    Ok(PluginSpec::Named {
        name,
        plugin_type,
        version,
    })
}

/// Parse all `--plugin` strings into a vec of [`PluginSpec`] values (phase 1).
///
/// Returns the first parse error encountered.
///
/// Performs best-effort semantic validation for specs where the type is
/// already known at this stage (e.g. `system/usb`). For specs where the type
/// is unknown (bare name or `file=` forms), full validation is deferred to
/// phase 2. After resolving all types, the caller must call
/// [`validate_resolved_plugin_types`].
pub fn parse_plugins(plugins: &[String]) -> Result<Vec<PluginSpec>, Error> {
    let specs: Vec<PluginSpec> = plugins
        .iter()
        .map(|s| parse_plugin(s))
        .collect::<Result<_, _>>()?;

    // We can't perform a check if we don't yet know the plugin type(s)
    let any_unknown = specs.iter().any(|s| {
        matches!(
            s,
            PluginSpec::Named {
                plugin_type: None,
                ..
            } | PluginSpec::File { .. }
        )
    });

    if !any_unknown {
        let mut seen_system = false;
        let mut seen_user = false;
        for spec in &specs {
            if let PluginSpec::Named {
                plugin_type: Some(t),
                ..
            } = spec
            {
                match t {
                    PluginType::System => {
                        if seen_system {
                            return Err(Error::DuplicatePlugin(PluginType::System));
                        }
                        seen_system = true;
                    }
                    PluginType::User => {
                        if seen_user {
                            return Err(Error::DuplicatePlugin(PluginType::User));
                        }
                        seen_user = true;
                    }
                }
            }
        }
        if seen_user && !seen_system {
            return Err(Error::UserPluginWithoutSystem);
        }
    }

    Ok(specs)
}

/// Validate the full set of resolved plugin types (phase 2).
///
/// Must be called after all plugin types have been resolved from the manifest
/// or binary headers.
///
/// Checks:
/// - At most one system plugin
/// - At most one user plugin
/// - A user plugin requires a system plugin
pub fn validate_resolved_plugin_types(plugin_types: &[PluginType]) -> Result<(), Error> {
    let system_count = plugin_types
        .iter()
        .filter(|&&t| t == PluginType::System)
        .count();
    let user_count = plugin_types
        .iter()
        .filter(|&&t| t == PluginType::User)
        .count();

    if system_count > 1 {
        return Err(Error::DuplicatePlugin(PluginType::System));
    }
    if user_count > 1 {
        return Err(Error::DuplicatePlugin(PluginType::User));
    }
    if user_count > 0 && system_count == 0 {
        return Err(Error::UserPluginWithoutSystem);
    }

    Ok(())
}

// ============================================================
// JSON generation
// ============================================================

pub fn plugin_size_handling(size: usize) -> Result<SizeHandling, Error> {
    if size > PLUGIN_MAX_SIZE {
        return Err(Error::PluginTooLarge(size, PLUGIN_MAX_SIZE));
    }
    if size == PLUGIN_MAX_SIZE {
        Ok(SizeHandling::None)
    } else {
        Ok(SizeHandling::Pad)
    }
}

/// Build a complete chip_set config for a plugin, ready to be inserted into
/// the chip_sets array. System plugins must be at index 0, user plugins at
/// index 1.
pub fn plugin_to_chip_set_config(
    file: &str,
    plugin_type: PluginType,
    size: usize,
) -> Result<ChipSetConfig, Error> {
    let size_handling = plugin_size_handling(size)?;
    let chip_type = match plugin_type {
        PluginType::System => OraChipType::SystemPlugin,
        PluginType::User => OraChipType::UserPlugin,
    };
    Ok(ChipSetConfig {
        set_type: ChipSetType::Single,
        description: None,
        chips: vec![ChipConfig {
            file: file.to_string(),
            license: None,
            description: None,
            chip_type,
            cs1: None,
            cs2: None,
            cs3: None,
            ce: None,
            oe: None,
            size_handling,
            extract: None,
            label: None,
            location: None,
            allow_cs_ignore: false,
        }],
        serve_alg: None,
        firmware_overrides: None,
    })
}

// ============================================================
// Plugin header parsing
// ============================================================

const ORA_PLUGIN_MAGIC: u32 = 0x2041524F;
const ORA_PLUGIN_HEADER_SIZE: usize = 256;

struct PluginHeader {
    version: PluginVersion,
    plugin_type: PluginType,
    min_fw: FirmwareVersion,
}

fn parse_plugin_header(data: &[u8], source: &str) -> Result<PluginHeader, Error> {
    if data.len() < ORA_PLUGIN_HEADER_SIZE {
        return Err(Error::PluginBinaryTooSmall(
            source.to_string(),
            data.len(),
            ORA_PLUGIN_HEADER_SIZE,
        ));
    }

    let magic = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if magic != ORA_PLUGIN_MAGIC {
        return Err(Error::PluginInvalidMagic(
            source.to_string(),
            magic,
            ORA_PLUGIN_MAGIC,
        ));
    }

    let plugin_type = match data[20] {
        0 => PluginType::System,
        1 => PluginType::User,
        2 => return Err(Error::PluginPioNotSupported(source.to_string())),
        other => return Err(Error::PluginUnknownBinaryType(source.to_string(), other)),
    };

    Ok(PluginHeader {
        version: PluginVersion::new(
            u16::from_le_bytes(data[8..10].try_into().unwrap()),
            u16::from_le_bytes(data[10..12].try_into().unwrap()),
            u16::from_le_bytes(data[12..14].try_into().unwrap()),
            u16::from_le_bytes(data[14..16].try_into().unwrap()),
        ),
        plugin_type,
        min_fw: FirmwareVersion::new(
            u16::from_le_bytes(data[24..26].try_into().unwrap()),
            u16::from_le_bytes(data[26..28].try_into().unwrap()),
            u16::from_le_bytes(data[28..30].try_into().unwrap()),
            0,
        ),
    })
}

fn check_header_min_fw(
    header: &PluginHeader,
    plugin_version: PluginVersion,
    fw_version: &FirmwareVersion,
    source: &str,
) -> Result<(), Error> {
    if fw_version < &header.min_fw {
        return Err(Error::PluginIncompatible(
            source.to_string(),
            plugin_version,
            header.min_fw,
            *fw_version,
        ));
    }
    Ok(())
}

fn verify_sha256(data: &[u8], expected_hex: &str, source: &str) -> Result<(), Error> {
    use sha2::{Digest, Sha256};
    let actual = hex::encode(Sha256::digest(data));
    if actual != expected_hex.to_lowercase() {
        return Err(Error::PluginSha256Mismatch(
            source.to_string(),
            expected_hex.to_string(),
            actual,
        ));
    }
    Ok(())
}

// ============================================================
// Phase 2: plugin resolution
// ============================================================

pub struct ResolvedPlugin {
    pub plugin_type: PluginType,
    pub name: String,
    pub file: String,
    pub size: usize,
    pub version: PluginVersion,
}

pub async fn resolve_plugins(
    specs: &[PluginSpec],
    fw_version: Option<FirmwareVersion>,
) -> Result<Vec<ResolvedPlugin>, Error> {
    if specs.is_empty() {
        return Ok(vec![]);
    }

    let manifest = if specs.iter().any(|s| {
        matches!(
            s,
            PluginSpec::Named {
                plugin_type: None,
                ..
            }
        )
    }) {
        Some(fetch_plugins_manifest().await?)
    } else {
        None
    };

    let mut resolved = Vec::with_capacity(specs.len());
    for spec in specs {
        resolved.push(resolve_plugin(spec, manifest.as_ref(), fw_version.as_ref()).await?);
    }

    let types: Vec<PluginType> = resolved.iter().map(|r| r.plugin_type).collect();
    validate_resolved_plugin_types(&types)?;

    Ok(resolved)
}

async fn resolve_plugin(
    spec: &PluginSpec,
    manifest: Option<&PluginsManifest>,
    fw_version: Option<&FirmwareVersion>,
) -> Result<ResolvedPlugin, Error> {
    match spec {
        PluginSpec::Named {
            name,
            plugin_type,
            version,
        } => resolve_named_plugin(name, *plugin_type, *version, manifest, fw_version).await,
        PluginSpec::File { path } => resolve_file_plugin(path, fw_version).await,
    }
}

/// Select the release to use for an unpinned named spec.
///
/// Releases are ordered newest-first. With a known firmware version this walks
/// that list and returns the newest release whose compatibility window includes
/// `fw`; if none qualifies it returns an error describing why the newest release
/// is incompatible. With no firmware version it returns the newest release
/// without checking compatibility (it cannot be checked).
fn select_unpinned<'a>(
    releases: &'a PluginReleasesManifest,
    name: &str,
    fw_version: Option<&FirmwareVersion>,
) -> Result<&'a PluginRelease, Error> {
    let Some(newest) = releases.releases.first() else {
        return Err(Error::PluginNotFound(name.to_string()));
    };

    let Some(fw) = fw_version else {
        // Firmware version unknown: compatibility cannot be checked, so use the
        // newest release and let later header checks catch a hard mismatch.
        return Ok(newest);
    };

    // Newest-first: if the newest release fits this firmware it is the answer.
    // Otherwise walk the older releases for the newest one that does. Binding
    // `reason` from the newest's incompatibility means the no-compatible-release
    // error reports against the newest release without recomputing anything.
    match newest.fw_incompat(fw) {
        None => Ok(newest),
        Some(reason) => releases
            .releases
            .iter()
            .skip(1)
            .find(|r| r.compatible_with_firmware(fw))
            .ok_or_else(|| fw_incompat_error(name, newest, reason, fw)),
    }
}

async fn resolve_named_plugin(
    name: &str,
    known_type: Option<PluginType>,
    version: Option<PluginVersion>,
    manifest: Option<&PluginsManifest>,
    fw_version: Option<&FirmwareVersion>,
) -> Result<ResolvedPlugin, Error> {
    let plugin_type: PluginType = if let Some(kt) = known_type {
        kt
    } else {
        let m = manifest.expect("manifest must be fetched before resolving bare-name specs");
        let entry = m
            .plugins
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| Error::PluginNotFound(name.to_string()))?;
        entry.plugin_type
    };

    let releases = fetch_plugin_releases(plugin_type, name).await?;

    // An explicit version pin selects that exact release; an incompatible pin
    // is a hard error (handled by the compatibility check below), never a
    // reason to silently substitute a different version. An unpinned spec walks
    // the releases for the newest compatible one.
    let release = if let Some(v) = version {
        releases
            .releases
            .iter()
            .find(|r| r.version == v)
            .ok_or_else(|| Error::PluginVersionNotFound(name.to_string(), v.to_string()))?
    } else {
        select_unpinned(&releases, name, fw_version)?
    };

    // Fail fast on compatibility before downloading. For an explicit version
    // pin this enforces the pin (too-old or too-new is a hard error). For an
    // unpinned spec with known firmware the release is already compatible, so
    // this is a no-op. With unknown firmware compatibility cannot be checked.
    if let Some(fw) = fw_version
        && let Some(reason) = release.fw_incompat(fw)
    {
        return Err(fw_incompat_error(name, release, reason, fw));
    }

    let url = release.binary_url(plugin_type, name);

    // Download to verify SHA256, parse header, and cross-check version and type.
    // Note: builder will download again via get_rom_files_async. Caching is a future optimisation.
    let (data, _) = onerom_fw::net::fetch_rom_file_async(&url, &[], None, false)
        .await
        .map_err(Error::from)?;

    verify_sha256(&data, &release.sha256, &url)?;

    let header = parse_plugin_header(&data, &url)?;

    if header.version != release.version {
        return Err(Error::PluginVersionMismatch(
            name.to_string(),
            release.version,
            header.version,
        ));
    }

    if header.plugin_type != plugin_type {
        return Err(Error::PluginTypeMismatch(
            name.to_string(),
            plugin_type.canonical().to_string(),
            header.plugin_type.canonical().to_string(),
        ));
    }

    // Cross-check header min_fw (manifest already checked above).
    if let Some(fw) = fw_version {
        check_header_min_fw(&header, header.version, fw, &url)?;
    }

    Ok(ResolvedPlugin {
        plugin_type,
        name: name.to_string(),
        file: url,
        size: data.len(),
        version: header.version,
    })
}

async fn resolve_file_plugin(
    path: &str,
    fw_version: Option<&FirmwareVersion>,
) -> Result<ResolvedPlugin, Error> {
    // Download/read binary to parse header. Note: builder will fetch again
    // via get_rom_files_async. Caching is a future optimisation.
    let (data, _) = onerom_fw::net::fetch_rom_file_async(path, &[], None, false)
        .await
        .map_err(Error::from)?;

    let header = parse_plugin_header(&data, path)?;

    if let Some(fw) = fw_version {
        check_header_min_fw(&header, header.version, fw, path)?;
    }

    let name = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string();

    Ok(ResolvedPlugin {
        plugin_type: header.plugin_type,
        name,
        file: path.to_string(),
        size: data.len(),
        version: header.version,
    })
}
