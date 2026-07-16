// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Plugin discovery, resolution, verification, and configuration.
//!
//! # Plugin model
//!
//! A One ROM plugin masquerades as a ROM but is an ARM code blob that runs from
//! the RP2350's flash. Two kinds exist:
//!
//! - a **system** plugin (for example the runtime USB stack), which always
//!   occupies chip-set slot 0, and
//! - a **user** plugin, which always occupies chip-set slot 1.
//!
//! At most one of each kind may be present in a firmware image, and a user
//! plugin requires a system plugin. ROM images occupy chip-set slots from 2
//! onwards (or from 0 when no plugins are present).
//!
//! # Types
//!
//! Plugins are published on the images server through two manifest files:
//!
//! - a top-level catalogue (`plugins.json`) listing every plugin's name and
//!   type, and
//! - a per-plugin release history (`releases.json`) listing every released
//!   version with its compatibility window and binary digest.
//!
//! The JSON of those files is deserialised into the private wire structs
//! [`PluginsManifestWire`] and [`PluginReleasesManifestWire`], then converted
//! into the public domain types [`Plugin`] and [`Release`]. The domain types
//! store a plugin's identity (name, type) exactly once: a [`Plugin`] owns its
//! identity and its [`Release`]s, and a [`Release`] carries only per-release
//! data - it does not restate the name or type, which it inherits from its
//! parent [`Plugin`].
//!
//! # Fetching
//!
//! This crate performs no I/O. The asynchronous entry points (in the logic
//! portion of this module) obtain manifest and binary bytes through the
//! host-supplied [`PluginFetch`] trait, keeping all transport - `reqwest`,
//! browser `fetch`, SWD, and so on - out of the crate.

use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use onerom_config::chip::ChipType as OraChipType;
use onerom_config::fw::FirmwareVersion;
use onerom_gen::{ChipConfig, ChipSetConfig, ChipSetType, SizeHandling};
use serde::{Deserialize, Serialize};

use crate::error::{Error, PluginError};

/// Base URL for plugin manifests and binaries on the images server.
///
/// All plugin resources live beneath this path: the top-level catalogue at
/// `{BASE}/plugins.json`, a plugin's releases at
/// `{BASE}/{type}/{name}/releases.json`, and a release's binary at
/// `{BASE}/{type}/{name}/{version_path}/{filename}`.
const PLUGIN_SITE_BASE: &str = "https://images.onerom.org/plugins";

/// Canonical plugin-type string for system plugins, as used in JSON configs.
const PLUGIN_TYPE_SYSTEM: &str = "system_plugin";

/// Canonical plugin-type string for user plugins, as used in JSON configs.
const PLUGIN_TYPE_USER: &str = "user_plugin";

/// Recognised `key=value` options in a named plugin specification.
const PLUGIN_SPEC_KEYS: &[&str] = &["version"];

// ============================================================
// PluginVersion
// ============================================================

/// A plugin version with four components, as recorded in a plugin binary
/// header.
///
/// Manifest version strings use three components (for example `0.1.0`); when a
/// three-component string is parsed the `build` component defaults to 0. The
/// [`Display`](fmt::Display) implementation omits a zero `build` so it
/// round-trips such strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PluginVersion {
    /// Major version component.
    pub major: u16,
    /// Minor version component.
    pub minor: u16,
    /// Patch version component.
    pub patch: u16,
    /// Build version component; 0 when absent from a three-component string.
    pub build: u16,
}

impl PluginVersion {
    /// Construct a version from its four components.
    pub fn new(major: u16, minor: u16, patch: u16, build: u16) -> Self {
        Self {
            major,
            minor,
            patch,
            build,
        }
    }

    /// Parse a version from a dotted string of three or four components.
    ///
    /// Returns `None` if the string does not have three or four dot-separated
    /// numeric components. A three-component string yields a `build` of 0.
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

impl fmt::Display for PluginVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.build == 0 {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        } else {
            write!(f, "{}.{}.{}.{}", self.major, self.minor, self.patch, self.build)
        }
    }
}

impl<'de> Deserialize<'de> for PluginVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        PluginVersion::try_from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(alloc::format!("invalid plugin version '{s}'")))
    }
}

// ============================================================
// PluginType
// ============================================================

/// The kind of a plugin: system or user.
///
/// The kind determines both the chip-set slot the plugin occupies (system in
/// slot 0, user in slot 1) and the directory segment used to locate it on the
/// images server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PluginType {
    /// A system plugin (for example the runtime USB stack). Occupies slot 0.
    System,
    /// A user plugin. Occupies slot 1 and requires a system plugin.
    User,
}

impl PluginType {
    /// Parse a plugin type from either its short form (`system`, `user`) or its
    /// canonical config form (`system_plugin`, `user_plugin`).
    pub fn try_from_str(s: &str) -> Option<Self> {
        match s {
            "system" | "system_plugin" => Some(PluginType::System),
            "user" | "user_plugin" => Some(PluginType::User),
            _ => None,
        }
    }

    /// The canonical type string used in One ROM JSON configurations.
    pub fn canonical(self) -> &'static str {
        match self {
            PluginType::System => PLUGIN_TYPE_SYSTEM,
            PluginType::User => PLUGIN_TYPE_USER,
        }
    }

    /// The short type string used in URLs and human-readable output.
    pub fn short(self) -> &'static str {
        match self {
            PluginType::System => "system",
            PluginType::User => "user",
        }
    }

    /// The fixed chip-set slot index this plugin type occupies.
    pub fn slot_index(self) -> usize {
        match self {
            PluginType::System => 0,
            PluginType::User => 1,
        }
    }

    /// The plugin type occupying chip-set slot `slot_index`, or `None` if that
    /// index is not a plugin slot.
    ///
    /// The inverse of [`slot_index`](Self::slot_index): slot 0 is the system
    /// plugin, slot 1 the user plugin. ROM slots (2 onwards, or 0 when no
    /// plugins are present) are not plugin slots and return `None`.
    pub fn from_slot_index(slot_index: usize) -> Option<Self> {
        match slot_index {
            0 => Some(PluginType::System),
            1 => Some(PluginType::User),
            _ => None,
        }
    }
}

impl fmt::Display for PluginType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.short())
    }
}

impl Serialize for PluginType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.canonical())
    }
}

impl<'de> Deserialize<'de> for PluginType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        PluginType::try_from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(alloc::format!("unrecognised plugin type '{s}'")))
    }
}

// ============================================================
// PluginSpec
// ============================================================

/// A parsed plugin specification, as produced from a `--plugin`-style input.
///
/// A specification names a plugin to include, but does not by itself resolve to
/// a concrete binary: for a [`PluginSpec::Named`] the release is chosen from the
/// manifest during resolution, and for a [`PluginSpec::File`] the type and
/// version are read from the binary header. Accepted textual forms:
///
/// ```text
/// usb                              name, latest compatible version
/// system/usb                       name with explicit type
/// usb,version=0.1.0                name, pinned version
/// system/usb,version=0.1.0         name with explicit type, pinned version
/// file=path/to/plugin.bin          local file, type from binary header
/// file=https://example.com/p.bin   remote file, type from binary header
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSpec {
    /// Look the plugin up by name (and optional type) in the manifest.
    ///
    /// `plugin_type` is `None` when the caller did not state the type; it is
    /// then resolved from the top-level manifest.
    Named {
        /// Plugin name (for example `usb`).
        name: String,
        /// Plugin type, if stated by the caller.
        plugin_type: Option<PluginType>,
        /// Pinned version, if requested. `None` selects the newest compatible
        /// release.
        version: Option<PluginVersion>,
    },
    /// Use a local or remote binary directly; its type is read from the header.
    File {
        /// Path or URL to the plugin binary.
        path: String,
    },
}

/// Parse a single plugin specification string.
///
/// Validates syntax only; performs no network or filesystem access. Returns
/// [`PluginError::SpecSyntax`] with a description of the problem on malformed
/// input.
fn parse_plugin(s: &str) -> Result<PluginSpec, PluginError> {
    // `file=` form: the remainder is the path or URL verbatim.
    if let Some(path) = s.strip_prefix("file=") {
        if path.is_empty() {
            return Err(PluginError::SpecSyntax(alloc::format!(
                "file path must not be empty in '{s}'"
            )));
        }
        return Ok(PluginSpec::File {
            path: path.to_string(),
        });
    }

    // Named form: split the name part from any trailing `key=value` options.
    let mut parts = s.splitn(2, ',');
    let name_part = parts.next().unwrap_or("");
    let kv_part = parts.next();

    // Parse `key=value` options. Only `version` is currently recognised.
    let mut version = None;
    if let Some(kv) = kv_part {
        let mut seen = BTreeSet::new();
        for kv in kv.split(',') {
            let (key, value) = kv.split_once('=').ok_or_else(|| {
                PluginError::SpecSyntax(alloc::format!(
                    "option '{kv}' is missing a value (expected '{kv}=<value>') in '{s}'"
                ))
            })?;
            if !seen.insert(key) {
                return Err(PluginError::SpecSyntax(alloc::format!(
                    "duplicate option '{key}' in '{s}'"
                )));
            }
            match key {
                "version" => {
                    version = Some(PluginVersion::try_from_str(value).ok_or_else(|| {
                        PluginError::SpecSyntax(alloc::format!(
                            "invalid version '{value}' in '{s}'"
                        ))
                    })?);
                }
                other => {
                    let supported = PLUGIN_SPEC_KEYS.join(", ");
                    return Err(PluginError::SpecSyntax(alloc::format!(
                        "unrecognised option '{other}' in '{s}' (supported: {supported})"
                    )));
                }
            }
        }
    }

    // Parse an optional `type/` prefix on the name part.
    let (plugin_type, name) = if let Some((type_str, name_str)) = name_part.split_once('/') {
        let pt = PluginType::try_from_str(type_str).ok_or_else(|| {
            PluginError::SpecSyntax(alloc::format!(
                "invalid type '{type_str}' (expected 'system' or 'user') in '{s}'"
            ))
        })?;
        if name_str.is_empty() {
            return Err(PluginError::SpecSyntax(alloc::format!(
                "plugin name must not be empty in '{s}'"
            )));
        }
        (Some(pt), name_str.to_string())
    } else {
        if name_part.is_empty() {
            return Err(PluginError::SpecSyntax(alloc::format!(
                "plugin name must not be empty in '{s}'"
            )));
        }
        (None, name_part.to_string())
    };

    Ok(PluginSpec::Named {
        name,
        plugin_type,
        version,
    })
}

/// Parse a slice of plugin specification strings.
///
/// Validates the syntax of each string and, where every specification's type is
/// already known (all are `system/...` or `user/...` typed named specs),
/// performs the duplicate/ordering checks eagerly. Specifications whose type is
/// not yet known (bare names and `file=` forms) are checked later, after
/// resolution, via [`validate_resolved_plugin_types`](super::validate_resolved_plugin_types).
///
/// Returns the first syntax or semantic error encountered.
pub fn parse_plugins(specs: &[String]) -> Result<Vec<PluginSpec>, PluginError> {
    let parsed: Vec<PluginSpec> = specs
        .iter()
        .map(|s| parse_plugin(s))
        .collect::<Result<_, _>>()?;

    // Eager duplicate/ordering validation is only possible when every spec's
    // type is already known. Bare names and `file=` forms defer this.
    let any_unknown = parsed.iter().any(|s| {
        matches!(
            s,
            PluginSpec::Named {
                plugin_type: None,
                ..
            } | PluginSpec::File { .. }
        )
    });

    if !any_unknown {
        let types: Vec<PluginType> = parsed
            .iter()
            .filter_map(|s| match s {
                PluginSpec::Named {
                    plugin_type: Some(t),
                    ..
                } => Some(*t),
                _ => None,
            })
            .collect();
        validate_plugin_type_set(&types)?;
    }

    Ok(parsed)
}

/// Validate a set of known plugin types: at most one system, at most one user,
/// and a user requires a system.
///
/// Shared by the eager check in [`parse_plugins`] and the post-resolution check
/// in [`validate_resolved_plugin_types`](super::validate_resolved_plugin_types).
pub(crate) fn validate_plugin_type_set(types: &[PluginType]) -> Result<(), PluginError> {
    let system = types.iter().filter(|&&t| t == PluginType::System).count();
    let user = types.iter().filter(|&&t| t == PluginType::User).count();

    if system > 1 {
        return Err(PluginError::DuplicatePlugin(PluginType::System));
    }
    if user > 1 {
        return Err(PluginError::DuplicatePlugin(PluginType::User));
    }
    if user > 0 && system == 0 {
        return Err(PluginError::UserPluginWithoutSystem);
    }
    Ok(())
}

// ============================================================
// Manifest wire structs (private)
// ============================================================

/// Wire form of the top-level plugins catalogue (`plugins.json`).
///
/// Deserialised, then converted into a [`Catalogue`] of [`Plugin`]s. Not part
/// of the public API - callers work with [`Catalogue`]/[`Plugin`].
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PluginsManifestWire {
    /// Manifest schema version.
    #[allow(dead_code)]
    pub version: u32,
    /// Catalogue entries.
    pub plugins: Vec<PluginEntryWire>,
}

/// Wire form of a single catalogue entry within [`PluginsManifestWire`].
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PluginEntryWire {
    /// Plugin slug (directory name in the images repo).
    pub name: String,
    /// Plugin type.
    #[serde(rename = "type")]
    pub plugin_type: PluginType,
    /// Relative path to the plugin directory (for example `system/usb`).
    ///
    /// Retained for completeness; binary and release URLs are derived from the
    /// type and name rather than from this field.
    #[allow(dead_code)]
    pub path: String,
}

/// Wire form of a per-plugin release history (`releases.json`).
///
/// Deserialised, then its [`releases`](Self::releases) are converted into
/// [`Release`]s and attached to the owning [`Plugin`]. The `display_name` and
/// `description` are lifted onto the [`Plugin`] during conversion.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PluginReleasesManifestWire {
    /// Manifest schema version.
    #[allow(dead_code)]
    pub version: u32,
    /// Human-readable plugin name.
    pub display_name: String,
    /// Short description of the plugin.
    pub description: String,
    /// The releases, newest first.
    pub releases: Vec<ReleaseWire>,
}

/// Wire form of a single release within [`PluginReleasesManifestWire`].
///
/// Carries the plugin name and type redundantly (the server makes each release
/// self-describing); the conversion to [`Release`] drops that restated identity,
/// which lives on the parent [`Plugin`].
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ReleaseWire {
    /// Release version.
    pub version: PluginVersion,
    /// Relative path to the version directory (for example `v0.1.0`).
    pub path: String,
    /// Binary filename within the version directory.
    pub filename: String,
    /// SHA-256 hex digest of the binary.
    pub sha256: String,
    /// Plugin API version this release targets.
    pub api_version: u32,
    /// Minimum One ROM firmware version required to run this release.
    #[serde(deserialize_with = "deserialize_fw_version")]
    pub min_fw_version: FirmwareVersion,
    /// First firmware version this release is known to be incompatible with, if
    /// any. The compatibility window is half-open:
    /// `[min_fw_version, incompatible_from)`.
    #[serde(default, deserialize_with = "deserialize_opt_fw_version")]
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

// ============================================================
// Domain types: Release, Plugin, Catalogue
// ============================================================

/// A single released version of a plugin.
///
/// A `Release` carries only per-release data. It does not restate the plugin's
/// name or type: those live on the owning [`Plugin`], and anything needing them
/// (such as building a binary URL) is done through the `Plugin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Release {
    /// Release version.
    pub version: PluginVersion,
    /// Relative path to the version directory (for example `v0.1.0`).
    pub path: String,
    /// Binary filename within the version directory.
    pub filename: String,
    /// SHA-256 hex digest of the binary.
    pub sha256: String,
    /// Plugin API version this release targets.
    pub api_version: u32,
    /// Minimum One ROM firmware version required to run this release.
    pub min_fw_version: FirmwareVersion,
    /// First firmware version this release is known to be incompatible with, if
    /// any. The compatibility window is half-open:
    /// `[min_fw_version, incompatible_from)`; `None` means no known upper bound.
    pub incompatible_from: Option<FirmwareVersion>,
}

impl Release {
    /// Build a [`Release`] from its wire form, dropping the restated identity.
    fn from_wire(w: ReleaseWire) -> Self {
        Self {
            version: w.version,
            path: w.path,
            filename: w.filename,
            sha256: w.sha256,
            api_version: w.api_version,
            min_fw_version: w.min_fw_version,
            incompatible_from: w.incompatible_from,
        }
    }

    /// Whether this release is compatible with the given firmware version.
    ///
    /// The compatibility window is half-open: `[min_fw_version, incompatible_from)`.
    /// This is the per-release predicate behind [`compatible_releases`] and
    /// [`newest_compatible`], exposed for callers that need to test a single
    /// release (for example, to flag incompatible ones in a listing).
    pub fn is_compatible(&self, fw: &FirmwareVersion) -> bool {
        release_incompat(self, fw).is_none()
    }
}

/// A plugin: its identity, human-readable metadata, and (once loaded) its
/// releases.
///
/// A `Plugin` is produced in two phases. [`Catalogue::fetch`](super::Catalogue)
/// yields plugins with identity only and an empty [`releases`](Self::releases);
/// [`Catalogue::load_all_releases`](super::Catalogue) then populates the
/// releases (and `display_name`/`description`) for every plugin. An empty
/// `releases` therefore means "not yet loaded", not "no releases exist".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plugin {
    /// Plugin slug (for example `usb`).
    pub name: String,
    /// Plugin type.
    pub plugin_type: PluginType,
    /// Human-readable name. `None` until releases are loaded.
    pub display_name: Option<String>,
    /// Short description. `None` until releases are loaded.
    pub description: Option<String>,
    /// The plugin's releases, newest first. Empty until loaded.
    pub releases: Vec<Release>,
}

impl Plugin {
    /// Build a `Plugin` (identity only, no releases) from a catalogue entry.
    fn from_entry(e: PluginEntryWire) -> Self {
        Self {
            name: e.name,
            plugin_type: e.plugin_type,
            display_name: None,
            description: None,
            releases: Vec::new(),
        }
    }

    /// The URL of this plugin's release-history manifest (`releases.json`).
    pub fn releases_manifest_url(&self) -> String {
        alloc::format!(
            "{PLUGIN_SITE_BASE}/{}/{}/releases.json",
            self.plugin_type.short(),
            self.name
        )
    }

    /// The URL of the binary for one of this plugin's releases.
    ///
    /// The `release` should be one of this plugin's own [`releases`](Self::releases);
    /// the URL is `{BASE}/{type}/{name}/{release.path}/{release.filename}`.
    pub fn binary_url(&self, release: &Release) -> String {
        binary_url(self.plugin_type, &self.name, release)
    }
}

/// Build a plugin binary URL from a type, name, and release.
///
/// Shared by [`Plugin::binary_url`] and [`ResolvedPlugin`], which both need to
/// locate a binary but hold identity in different forms.
pub(crate) fn binary_url(plugin_type: PluginType, name: &str, release: &Release) -> String {
    alloc::format!(
        "{PLUGIN_SITE_BASE}/{}/{}/{}/{}",
        plugin_type.short(),
        name,
        release.path,
        release.filename
    )
}

/// Recover the `(type, name, version)` encoded in an official plugin image
/// source, or `None` if the source is not an official plugin URL.
///
/// This is the inverse of [`binary_url`], which builds
/// `{PLUGIN_SITE_BASE}/{type}/{name}/{version_path}/{filename}`. A source is
/// official only if it begins with [`PLUGIN_SITE_BASE`] and has exactly those
/// four segments, the type is recognised, and the version path parses. Anything
/// else - a local build path, a non-official URL - yields `None` and is treated
/// as a user/sideloaded plugin by the caller.
fn official_plugin_from_source(source: &str) -> Option<(PluginType, String, PluginVersion)> {
    let rest = source.strip_prefix(PLUGIN_SITE_BASE)?.strip_prefix('/')?;

    // Exactly {type}/{name}/{version_path}/{filename}.
    let mut segs = rest.split('/');
    let type_str = segs.next()?;
    let name = segs.next()?;
    let version_path = segs.next()?;
    let _filename = segs.next()?;
    if segs.next().is_some() {
        return None; // more than four segments: not our URL shape
    }

    if name.is_empty() {
        return None;
    }

    let plugin_type = PluginType::try_from_str(type_str)?;
    // The version directory is `v0.1.2`; drop the leading `v` before parsing.
    let version =
        PluginVersion::try_from_str(version_path.strip_prefix('v').unwrap_or(version_path))?;

    Some((plugin_type, name.to_string(), version))
}

/// The URL of the top-level plugins catalogue (`plugins.json`).
pub(crate) fn plugins_manifest_url() -> String {
    alloc::format!("{PLUGIN_SITE_BASE}/plugins.json")
}

/// The catalogue of available plugins.
///
/// Fetched once via [`Catalogue::fetch`](super::Catalogue) (identities only),
/// then optionally populated with every plugin's releases via
/// [`Catalogue::load_all_releases`](super::Catalogue). Once loaded, interactive
/// callers (Studio, the web tool) can answer selection and compatibility
/// queries entirely in memory, with no further fetching.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    pub(crate) plugins: Vec<Plugin>,
}

impl Catalogue {
    /// Construct a catalogue from the deserialised top-level manifest.
    pub(crate) fn from_wire(w: PluginsManifestWire) -> Self {
        Self {
            plugins: w.plugins.into_iter().map(Plugin::from_entry).collect(),
        }
    }

    /// The plugins in this catalogue.
    pub fn plugins(&self) -> &[Plugin] {
        &self.plugins
    }

    /// Look up a plugin by name.
    pub fn plugin_by_name(&self, name: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.name == name)
    }
}

// ============================================================
// ResolvedPlugin
// ============================================================

/// A plugin that has been resolved to a concrete, verified binary.
///
/// Produced by [`resolve_plugins`](super::resolve_plugins) once a plugin's
/// binary has been located, fetched, and verified. The common metadata
/// (`plugin_type`, `name`, `version`, `size`) is known for every plugin
/// regardless of how it was resolved; the [`source`](Self::source) enum carries
/// the parts that differ between a manifest-backed plugin and a sideloaded one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPlugin {
    /// Plugin type (system or user).
    pub plugin_type: PluginType,
    /// Plugin name.
    pub name: String,
    /// The resolved version (from the release for named plugins, or from the
    /// binary header for sideloaded ones).
    pub version: PluginVersion,
    /// The verified binary size, in bytes.
    pub size: usize,
    /// Source-specific data.
    pub source: ResolvedSource,
}

/// Where a [`ResolvedPlugin`] was resolved from.
///
/// A named plugin carries the manifest [`Release`] it resolved to; a sideloaded
/// plugin carries only the path or URL it was loaded from, since it has no
/// manifest entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedSource {
    /// Resolved from the manifest by name; carries the selected release.
    Named {
        /// The release selected during resolution.
        release: Release,
    },
    /// Sideloaded from a local or remote binary; carries its path or URL.
    File {
        /// The path or URL the binary was loaded from.
        path: String,
    },
}

impl ResolvedPlugin {
    /// The binary URL (for a named plugin) or path (for a sideloaded one).
    pub fn file(&self) -> String {
        match &self.source {
            ResolvedSource::Named { release } => {
                binary_url(self.plugin_type, &self.name, release)
            }
            ResolvedSource::File { path } => path.clone(),
        }
    }

    /// Build the chip-set configuration that places this plugin into a firmware
    /// image.
    pub fn to_chip_set_config(&self) -> Result<ChipSetConfig, PluginError> {
        plugin_to_chip_set_config(&self.file(), self.plugin_type, self.size)
    }
}

// ============================================================
// PluginDisplay
// ============================================================

/// A plugin identified from a device's recorded image source, for display.
///
/// Produced by [`resolve_plugin_display`] from the source string a device
/// records for a plugin slot, together with the slot index. It is built without
/// the plugin binary, so unlike [`ResolvedPlugin`] it carries no size, and a
/// local plugin carries no version - only what its origin actually provides.
///
/// The plugin type is derived from the slot (system in slot 0, user in slot 1);
/// the origin-specific data - full manifest identity for an official plugin, or
/// just the source string for a local one - lives in [`origin`](Self::origin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginDisplay {
    /// The plugin type, derived from the slot the plugin occupies.
    pub plugin_type: PluginType,
    /// Origin-specific data: manifest identity, or a local source.
    pub origin: PluginOrigin,
}

/// Where a [`PluginDisplay`] came from, with the data specific to each origin.
///
/// The two origins carry genuinely different data: an official plugin has a
/// full catalogue identity (slug, display name, releases) plus the version its
/// URL names; a local plugin has only the source string it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PluginOrigin {
    /// An official plugin from the images server manifest.
    ///
    /// `plugin` carries the catalogue identity; its `display_name`/`description`
    /// and `releases` are populated when the manifest was successfully fetched,
    /// and left empty otherwise (so [`PluginDisplay::display_label`] falls back
    /// to the slug). `version` is the version named by the source URL.
    Manifest {
        /// Catalogue identity, enriched with manifest data where available.
        plugin: Plugin,
        /// The version named by the plugin's image-source URL.
        version: PluginVersion,
    },
    /// A user-built or sideloaded plugin: only the source string is known.
    Local {
        /// The image source the device recorded (a path or URL).
        source: String,
    },
}

impl PluginDisplay {
    /// A human-readable label, always displayable.
    ///
    /// For an official plugin this is the manifest display name when it was
    /// loaded, otherwise the slug. For a local plugin it is the file stem of
    /// the source. Never empty, never an error - a failed manifest fetch simply
    /// degrades the official label from display name to slug.
    pub fn display_label(&self) -> &str {
        match &self.origin {
            PluginOrigin::Manifest { plugin, .. } => {
                plugin.display_name.as_deref().unwrap_or(&plugin.name)
            }
            PluginOrigin::Local { source } => file_stem_str(source),
        }
    }
}

/// Resolve a device's plugin slot to a [`PluginDisplay`] for presentation.
///
/// `slot_index` identifies the slot (0 = system, 1 = user); `source` is the
/// image source the device recorded for it. Returns `None` only if `slot_index`
/// is not a plugin slot.
///
/// For an official source, this fetches the plugin's release manifest to
/// populate its display name and releases. That fetch is best-effort: on any
/// failure the plugin is still returned as [`PluginOrigin::Manifest`] with the
/// manifest data unset, so the caller always gets a usable result and
/// [`PluginDisplay::display_label`] falls back to the slug. A local source
/// performs no I/O.
pub async fn resolve_plugin_display<F: LocalPluginFetch>(
    slot_index: usize,
    source: &str,
    fetch: &F,
) -> Option<PluginDisplay> {
    let plugin_type = PluginType::from_slot_index(slot_index)?;

    let origin = match official_plugin_from_source(source) {
        Some((_url_type, name, version)) => {
            // Build the catalogue identity from the URL, then enrich it from the
            // manifest. The type comes from the slot (authoritative), not the
            // URL; the two agree for well-formed firmware.
            let mut plugin = Plugin {
                name,
                plugin_type,
                display_name: None,
                description: None,
                releases: Vec::new(),
            };
            // Best-effort: ignore fetch/parse failure and keep the slug.
            let _ = fetch_releases(&mut plugin, fetch).await;
            PluginOrigin::Manifest { plugin, version }
        }
        None => PluginOrigin::Local {
            source: source.to_string(),
        },
    };

    Some(PluginDisplay {
        plugin_type,
        origin,
    })
}


// ============================================================
// PluginFetch trait
// ============================================================

/// Host-supplied transport for fetching plugin manifests and binaries.
///
/// `onerom-app` performs no I/O of its own: every network or filesystem access
/// is delegated to an implementation of this trait. The single method is given
/// a source - a URL or path produced by the crate - and must return the bytes,
/// or the host's own error.
///
/// # `Send` variants
///
/// This trait is declared through [`trait_variant`], which generates two forms:
///
/// - [`LocalPluginFetch`] - the base trait, whose `fetch` future need not be
///   `Send`. Suitable for single-threaded executors such as the browser
///   (WASM) and Embassy (embedded).
/// - `PluginFetch` - a variant whose `fetch` future is `Send`, suitable for
///   multi-threaded executors such as the CLI's Tokio runtime.
///
/// A type that implements `LocalPluginFetch` and whose future is `Send`
/// automatically satisfies `PluginFetch`.
#[trait_variant::make(PluginFetch: Send)]
pub trait LocalPluginFetch {
    /// The host's transport error type.
    type Error;

    /// Fetch the bytes at `source` (a URL or path).
    async fn fetch(&self, source: &str) -> Result<Vec<u8>, Self::Error>;
}

// ============================================================
// VerifyTarget
// ============================================================

/// What a plugin binary is verified against.
///
/// Manifest-backed plugins are checked against their [`Release`] (SHA-256,
/// version, and type all cross-checked). Sideloaded `file=` binaries have no
/// manifest entry, so only their header can be checked.
pub enum VerifyTarget<'a> {
    /// A manifest-backed release: the SHA-256 digest is checked against the
    /// release, and the header type is cross-checked against `expected_type`.
    Release {
        /// The release whose digest the binary must match.
        release: &'a Release,
        /// The plugin type the manifest says this plugin is.
        expected_type: PluginType,
    },
    /// A sideloaded binary: only the header is read; there is no manifest digest
    /// to compare against and no manifest type to cross-check.
    Header,
}

// ============================================================
// Catalogue construction accessors already defined above.
// The remainder of this module is the logic layer: header parsing and
// verification, compatibility filtering, config generation, the async fetch
// helpers, and the resolution entry points.
// ============================================================

// ------------------------------------------------------------
// Plugin binary header parsing and verification
// ------------------------------------------------------------

/// Magic value at the start of every One ROM plugin binary (`"ORA "`, little
/// endian).
const ORA_PLUGIN_MAGIC: u32 = 0x2041_524F;

/// Size of the plugin header, in bytes. The full header must be present for the
/// binary to be a valid plugin.
const ORA_PLUGIN_HEADER_SIZE: usize = 256;

/// Maximum plugin binary size, in bytes.
///
/// A plugin occupies exactly one 64 KB slot in the firmware image. Smaller
/// binaries are padded during the build; larger binaries cannot fit and are
/// rejected.
const PLUGIN_MAX_SIZE: usize = 64 * 1024;

/// The parts of a plugin binary header that `onerom-app` reads.
///
/// Only the fields needed for verification are parsed here: the type (offset
/// 20) and version (offsets 8-14). The magic, API version, and minimum firmware
/// checks are performed by `onerom-gen` at build time, so they are deliberately
/// not duplicated in this crate. The magic is checked here purely as a guard,
/// so the type byte is only trusted when the binary really is a plugin.
struct PluginHeader {
    plugin_type: PluginType,
    version: PluginVersion,
}

/// Parse the fields of a plugin header that this crate needs.
///
/// Validates the header is present and the magic matches (a guard for the type
/// read), then extracts the type and version. Does not check API version or
/// minimum firmware - those belong to `onerom-gen`'s build-time validation.
fn parse_plugin_header(data: &[u8], source: &str) -> Result<PluginHeader, PluginError> {
    if data.len() < ORA_PLUGIN_HEADER_SIZE {
        return Err(PluginError::BinaryTooSmall(
            source.into(),
            data.len(),
            ORA_PLUGIN_HEADER_SIZE,
        ));
    }

    // Magic guard: only trust the type byte if this is really a plugin binary.
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != ORA_PLUGIN_MAGIC {
        return Err(PluginError::InvalidMagic(source.into(), magic, ORA_PLUGIN_MAGIC));
    }

    // Type at offset 20 (`ora_plugin_type_t`: 0 = system, 1 = user, 2 = PIO).
    let plugin_type = match data[20] {
        0 => PluginType::System,
        1 => PluginType::User,
        2 => return Err(PluginError::PioNotSupported(source.into())),
        other => return Err(PluginError::UnknownBinaryType(source.into(), other)),
    };

    // Version at offsets 8-14 (major, minor, patch, build; each little-endian u16).
    let version = PluginVersion::new(
        u16::from_le_bytes([data[8], data[9]]),
        u16::from_le_bytes([data[10], data[11]]),
        u16::from_le_bytes([data[12], data[13]]),
        u16::from_le_bytes([data[14], data[15]]),
    );

    Ok(PluginHeader {
        plugin_type,
        version,
    })
}

/// Compute the lower-case hex SHA-256 digest of `data`.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data))
}

/// Verify a fetched plugin binary and return its type and version.
///
/// This performs only the checks that `onerom-gen` cannot: the SHA-256 digest
/// against the manifest (for named plugins), and the manifest-versus-header
/// type cross-check. The header magic, API version, and minimum-firmware checks
/// are `onerom-gen`'s responsibility and run at build time; they are not
/// repeated here (the magic is read only as an internal guard for the type
/// byte).
///
/// - For [`VerifyTarget::Release`], the digest is compared against
///   [`Release::sha256`], and the header type is cross-checked against the
///   manifest type. The size is bounded to a single plugin slot.
/// - For [`VerifyTarget::Header`] (a sideloaded binary), there is no manifest
///   digest to compare against, so only the header is parsed and the size
///   bounded.
///
/// `source` identifies the binary (a URL or path) for error messages.
pub fn verify_binary(
    data: &[u8],
    target: VerifyTarget<'_>,
    source: &str,
) -> Result<PluginVerification, PluginError> {
    if data.len() > PLUGIN_MAX_SIZE {
        return Err(PluginError::TooLarge(data.len(), PLUGIN_MAX_SIZE));
    }

    let header = parse_plugin_header(data, source)?;

    if let VerifyTarget::Release { release, expected_type } = target {
        // SHA-256 against the manifest digest - the check onerom-gen cannot do.
        let actual = sha256_hex(data);
        if actual != release.sha256.to_lowercase() {
            return Err(PluginError::Sha256Mismatch {
                binary: source.into(),
                expected: release.sha256.clone(),
                got: actual,
            });
        }

        // Manifest-versus-header type cross-check.
        if header.plugin_type != expected_type {
            return Err(PluginError::TypeMismatch(
                source.into(),
                expected_type,
                header.plugin_type,
            ));
        }
    }

    Ok(PluginVerification {
        plugin_type: header.plugin_type,
        version: header.version,
        size: data.len(),
    })
}

/// The result of verifying a plugin binary: its type, version, and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginVerification {
    /// Plugin type read from the binary header.
    pub plugin_type: PluginType,
    /// Version read from the binary header.
    pub version: PluginVersion,
    /// The binary size, in bytes.
    pub size: usize,
}

// ------------------------------------------------------------
// Compatibility filtering (pure)
// ------------------------------------------------------------

/// Why a release is incompatible with a given firmware version.
enum FwIncompat {
    /// Firmware predates the release's minimum supported firmware.
    TooOld,
    /// Firmware is at or beyond the release's upper bound; carries that bound.
    TooNew(FirmwareVersion),
}

/// Return why `release` is incompatible with `fw`, or `None` if compatible.
///
/// The compatibility window is half-open: `[min_fw_version, incompatible_from)`.
fn release_incompat(release: &Release, fw: &FirmwareVersion) -> Option<FwIncompat> {
    if fw < &release.min_fw_version {
        Some(FwIncompat::TooOld)
    } else if let Some(from) = release.incompatible_from
        && fw >= &from
    {
        Some(FwIncompat::TooNew(from))
    } else {
        None
    }
}

/// Return the releases of `plugin` that are compatible with `fw`, newest first.
///
/// The plugin's releases are assumed to be ordered newest-first (as published),
/// and the returned slice preserves that order, so the first element is the
/// newest compatible release. Returns an empty vector if none are compatible.
pub fn compatible_releases<'a>(plugin: &'a Plugin, fw: &FirmwareVersion) -> Vec<&'a Release> {
    plugin
        .releases
        .iter()
        .filter(|r| release_incompat(r, fw).is_none())
        .collect()
}

/// Return the newest release of `plugin` compatible with `fw`, or `None`.
///
/// Equivalent to the first element of [`compatible_releases`], but avoids
/// allocating the intermediate vector.
pub fn newest_compatible<'a>(plugin: &'a Plugin, fw: &FirmwareVersion) -> Option<&'a Release> {
    plugin
        .releases
        .iter()
        .find(|r| release_incompat(r, fw).is_none())
}

/// Build a [`PluginError`] describing why `release` is incompatible with `fw`.
fn incompat_error(name: &str, release: &Release, reason: FwIncompat, fw: &FirmwareVersion) -> PluginError {
    match reason {
        FwIncompat::TooOld => PluginError::Incompatible {
            name: name.into(),
            version: release.version,
            min_fw: release.min_fw_version,
            fw: *fw,
        },
        FwIncompat::TooNew(from) => PluginError::IncompatibleNewer {
            name: name.into(),
            version: release.version,
            from,
            fw: *fw,
        },
    }
}

// ------------------------------------------------------------
// Resolved-type validation (pure)
// ------------------------------------------------------------

/// Validate the full set of resolved plugin types.
///
/// Enforces the invariants that could not be checked earlier for bare-name or
/// `file=` specs: at most one system plugin, at most one user plugin, and a
/// user plugin requires a system plugin.
pub fn validate_resolved_plugin_types(types: &[PluginType]) -> Result<(), PluginError> {
    validate_plugin_type_set(types)
}

// ------------------------------------------------------------
// Config generation (pure)
// ------------------------------------------------------------

/// Determine the size handling for a plugin binary of the given size.
///
/// A binary of exactly one slot needs no handling; a smaller one is padded to
/// fill the slot. Larger binaries are rejected (they cannot fit a slot).
fn plugin_size_handling(size: usize) -> Result<SizeHandling, PluginError> {
    if size > PLUGIN_MAX_SIZE {
        Err(PluginError::TooLarge(size, PLUGIN_MAX_SIZE))
    } else if size == PLUGIN_MAX_SIZE {
        Ok(SizeHandling::None)
    } else {
        Ok(SizeHandling::Pad)
    }
}

/// Build the [`ChipSetConfig`] that places a plugin binary into a firmware
/// image.
///
/// The system plugin must be placed at chip-set index 0 and the user plugin at
/// index 1; ordering is the caller's responsibility. `file` is the binary URL
/// (named plugins) or path (sideloaded). `size` is the verified binary size.
pub fn plugin_to_chip_set_config(
    file: &str,
    plugin_type: PluginType,
    size: usize,
) -> Result<ChipSetConfig, PluginError> {
    let size_handling = plugin_size_handling(size)?;
    let chip_type = match plugin_type {
        PluginType::System => OraChipType::SystemPlugin,
        PluginType::User => OraChipType::UserPlugin,
    };

    Ok(ChipSetConfig {
        set_type: ChipSetType::Single,
        description: None,
        chips: alloc::vec![ChipConfig {
            file: file.into(),
            license: None,
            description: None,
            chip_type,
            cs1: None,
            cs2: None,
            cs3: None,
            cs4: None,
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

// ------------------------------------------------------------
// Async fetch helpers
// ------------------------------------------------------------

/// Fetch a source through the host and deserialise it as JSON.
///
/// Fetch failures are wrapped as [`Error::Fetch`], carrying the host's own
/// error untouched; JSON failures become [`PluginError::ManifestJson`].
async fn fetch_json<F, T>(url: &str, fetch: &F) -> Result<T, Error<F::Error>>
where
    F: LocalPluginFetch,
    T: serde::de::DeserializeOwned,
{
    let bytes = fetch
        .fetch(url)
        .await
        .map_err(|e| Error::fetch(url, e))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| PluginError::ManifestJson(url.into(), alloc::format!("{e}")).into())
}

/// Fetch the top-level plugins catalogue and return it as a [`Catalogue`].
///
/// The catalogue holds plugin identities only; call
/// [`Catalogue::load_all_releases`] to populate releases.
async fn fetch_catalogue<F: LocalPluginFetch>(fetch: &F) -> Result<Catalogue, Error<F::Error>> {
    let wire: PluginsManifestWire = fetch_json(&plugins_manifest_url(), fetch).await?;
    Ok(Catalogue::from_wire(wire))
}

/// Fetch a single plugin's release history into the given plugin.
///
/// Populates `plugin.releases` (newest first) and the `display_name` and
/// `description` from the release manifest. Existing releases are replaced.
pub async fn fetch_releases<F: LocalPluginFetch>(
    plugin: &mut Plugin,
    fetch: &F,
) -> Result<(), Error<F::Error>> {
    let url = plugin.releases_manifest_url();
    let wire: PluginReleasesManifestWire = fetch_json(&url, fetch).await?;

    plugin.display_name = Some(wire.display_name);
    plugin.description = Some(wire.description);
    plugin.releases = wire.releases.into_iter().map(Release::from_wire).collect();

    Ok(())
}

impl Catalogue {
    /// Fetch the top-level catalogue of available plugins (identities only).
    ///
    /// Performs a single fetch of the top-level manifest. Releases are not
    /// loaded; call [`Catalogue::load_all_releases`] to populate them.
    pub async fn fetch<F: LocalPluginFetch>(fetch: &F) -> Result<Self, Error<F::Error>> {
        fetch_catalogue(fetch).await
    }

    /// Load every plugin's release history into the catalogue.
    ///
    /// Fetches one release manifest per plugin and populates each plugin's
    /// releases and metadata. After this returns, interactive callers can
    /// answer selection and compatibility queries entirely in memory. This is
    /// one fetch per plugin; with the small number of plugins that exist, that
    /// is acceptable for a listing.
    ///
    /// This aborts on the first fetch failure. Callers that must tolerate an
    /// individual plugin being unreachable (for example, a listing that should
    /// still show the plugins that *are* available) should use
    /// [`load_all_releases_resilient`](Self::load_all_releases_resilient)
    /// instead.
    pub async fn load_all_releases<F: LocalPluginFetch>(
        &mut self,
        fetch: &F,
    ) -> Result<(), Error<F::Error>> {
        for plugin in &mut self.plugins {
            fetch_releases(plugin, fetch).await?;
        }
        Ok(())
    }

    /// Load every plugin's release history, tolerating per-plugin failures.
    ///
    /// Like [`load_all_releases`](Self::load_all_releases), but a failure to
    /// fetch or parse one plugin's releases does not abort the rest: that
    /// plugin keeps its empty [`releases`](Plugin::releases) and the failure is
    /// collected and returned. The returned vector is empty when every plugin
    /// loaded successfully.
    ///
    /// This lets a caller (a CLI listing, the web dropdown) show every plugin
    /// that *is* reachable while reporting - or ignoring - the ones that are
    /// not, rather than losing the whole list to a single unreachable manifest.
    pub async fn load_all_releases_resilient<F: LocalPluginFetch>(
        &mut self,
        fetch: &F,
    ) -> Vec<(String, Error<F::Error>)> {
        let mut failures = Vec::new();
        for plugin in &mut self.plugins {
            if let Err(e) = fetch_releases(plugin, fetch).await {
                failures.push((plugin.name.clone(), e));
            }
        }
        failures
    }
}

// ------------------------------------------------------------
// Resolution (async)
// ------------------------------------------------------------

/// Resolve a set of plugin specifications to concrete, verified binaries.
///
/// For each spec this selects the release (named) or reads the header
/// (`file=`), fetches and verifies the binary, and produces a
/// [`ResolvedPlugin`]. The full set is then checked with
/// [`validate_resolved_plugin_types`].
///
/// The top-level manifest is fetched only if at least one spec is a bare name
/// whose type must be looked up; specs that state their type (`system/...`,
/// `user/...`) and `file=` specs never trigger that fetch.
///
/// `fw` is the firmware version being built for, used to select the newest
/// compatible release for unpinned named specs and to reject incompatible ones.
pub async fn resolve_plugins<F: LocalPluginFetch>(
    specs: &[PluginSpec],
    fw: &FirmwareVersion,
    fetch: &F,
) -> Result<Vec<ResolvedPlugin>, Error<F::Error>> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }

    // Only fetch the top-level catalogue if a bare-name spec needs its type.
    let needs_catalogue = specs.iter().any(|s| {
        matches!(
            s,
            PluginSpec::Named {
                plugin_type: None,
                ..
            }
        )
    });
    let catalogue = if needs_catalogue {
        Some(fetch_catalogue(fetch).await?)
    } else {
        None
    };

    let mut resolved = Vec::with_capacity(specs.len());
    for spec in specs {
        resolved.push(resolve_one(spec, catalogue.as_ref(), fw, fetch).await?);
    }

    let types: Vec<PluginType> = resolved.iter().map(|r| r.plugin_type).collect();
    validate_resolved_plugin_types(&types)?;

    Ok(resolved)
}

/// Resolve a single specification.
async fn resolve_one<F: LocalPluginFetch>(
    spec: &PluginSpec,
    catalogue: Option<&Catalogue>,
    fw: &FirmwareVersion,
    fetch: &F,
) -> Result<ResolvedPlugin, Error<F::Error>> {
    match spec {
        PluginSpec::Named {
            name,
            plugin_type,
            version,
        } => resolve_named(name, *plugin_type, *version, catalogue, fw, fetch).await,
        PluginSpec::File { path } => resolve_file(path, fetch).await,
    }
}

/// Resolve a named specification: select the release, fetch and verify its
/// binary, and build the [`ResolvedPlugin`].
async fn resolve_named<F: LocalPluginFetch>(
    name: &str,
    known_type: Option<PluginType>,
    pinned: Option<PluginVersion>,
    catalogue: Option<&Catalogue>,
    fw: &FirmwareVersion,
    fetch: &F,
) -> Result<ResolvedPlugin, Error<F::Error>> {
    // Resolve the type: stated by the caller, or looked up in the catalogue.
    let plugin_type = if let Some(t) = known_type {
        t
    } else {
        let cat = catalogue.expect("catalogue must be fetched before resolving bare-name specs");
        cat.plugin_by_name(name)
            .ok_or_else(|| PluginError::NotFound(name.into()))?
            .plugin_type
    };

    // Fetch this plugin's releases.
    let mut plugin = Plugin {
        name: name.into(),
        plugin_type,
        display_name: None,
        description: None,
        releases: Vec::new(),
    };
    fetch_releases(&mut plugin, fetch).await?;

    // Select the release: an explicit pin is a hard requirement (an
    // incompatible pin is an error, never a silent substitution); an unpinned
    // spec takes the newest compatible release.
    let release = select_release(&plugin, name, pinned, fw)?;
    let url = plugin.binary_url(release);

    // Fetch and verify the binary.
    let data = fetch.fetch(&url).await.map_err(|e| Error::fetch(&url, e))?;
    let verification = verify_binary(
        &data,
        VerifyTarget::Release {
            release,
            expected_type: plugin_type,
        },
        &url,
    )?;

    // Cross-check the header version against the manifest.
    if verification.version != release.version {
        return Err(PluginError::VersionMismatch(
            name.into(),
            release.version,
            verification.version,
        )
        .into());
    }

    Ok(ResolvedPlugin {
        plugin_type,
        name: name.into(),
        version: release.version,
        size: verification.size,
        source: ResolvedSource::Named {
            release: release.clone(),
        },
    })
}

/// Select the release for a named spec.
///
/// A pinned version selects that exact release (a missing pin is an error, and
/// an incompatible pin is a hard error, never a substitution). An unpinned spec
/// selects the newest compatible release.
fn select_release<'a>(
    plugin: &'a Plugin,
    name: &str,
    pinned: Option<PluginVersion>,
    fw: &FirmwareVersion,
) -> Result<&'a Release, PluginError> {
    if let Some(v) = pinned {
        let release = plugin
            .releases
            .iter()
            .find(|r| r.version == v)
            .ok_or_else(|| PluginError::VersionNotFound(name.into(), v))?;
        // Enforce the pin's compatibility (too old or too new is a hard error).
        if let Some(reason) = release_incompat(release, fw) {
            return Err(incompat_error(name, release, reason, fw));
        }
        Ok(release)
    } else {
        // Unpinned: newest compatible. If none, report against the newest
        // release's incompatibility.
        newest_compatible(plugin, fw).ok_or_else(|| match plugin.releases.first() {
            Some(newest) => match release_incompat(newest, fw) {
                Some(reason) => incompat_error(name, newest, reason, fw),
                // Newest is compatible yet newest_compatible returned None: not
                // possible, but map to NotFound rather than panic.
                None => PluginError::NotFound(name.into()),
            },
            None => PluginError::NotFound(name.into()),
        })
    }
}

/// Resolve a `file=` specification: fetch the binary, read its header for type
/// and version, and build the [`ResolvedPlugin`]. There is no manifest, so the
/// SHA-256 check is skipped and verification is header-only.
async fn resolve_file<F: LocalPluginFetch>(
    path: &str,
    fetch: &F,
) -> Result<ResolvedPlugin, Error<F::Error>> {
    let data = fetch.fetch(path).await.map_err(|e| Error::fetch(path, e))?;

    // Header-only verification: no manifest, so no digest or type cross-check.
    let verification = verify_binary(&data, VerifyTarget::Header, path)?;

    Ok(ResolvedPlugin {
        plugin_type: verification.plugin_type,
        name: file_stem(path),
        version: verification.version,
        size: verification.size,
        source: ResolvedSource::File { path: path.into() },
    })
}

/// The final path segment of `path` with any extension removed, borrowed from
/// `path`.
///
/// A leading-dot name (for example `.bin`) is returned whole rather than
/// becoming empty.
fn file_stem_str(path: &str) -> &str {
    let last = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match last.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => last,
    }
}

/// Extract a plugin name from a file path or URL: the final path segment with
/// any extension removed.
fn file_stem(path: &str) -> String {
    file_stem_str(path).to_string()
}

// ============================================================
// Unit tests
// ============================================================
//
// These cover the pure, synchronous logic and the private helpers that the
// public API is built from. The asynchronous entry points (which require a
// `PluginFetch` mock) are exercised by the integration tests in `tests/`.

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    // --- Construction helpers ---------------------------------------------

    fn fw(major: u16, minor: u16, patch: u16) -> FirmwareVersion {
        FirmwareVersion::new(major, minor, patch, 0)
    }

    fn pv(major: u16, minor: u16, patch: u16) -> PluginVersion {
        PluginVersion::new(major, minor, patch, 0)
    }

    /// Build a release with a compatibility window of
    /// `[min_fw, incompatible_from)`.
    fn release(
        version: PluginVersion,
        min_fw: FirmwareVersion,
        incompatible_from: Option<FirmwareVersion>,
        sha256: &str,
    ) -> Release {
        Release {
            version,
            path: alloc::format!("v{version}"),
            filename: "plugin.bin".to_string(),
            sha256: sha256.to_string(),
            api_version: 1,
            min_fw_version: min_fw,
            incompatible_from,
        }
    }

    fn plugin(name: &str, plugin_type: PluginType, releases: Vec<Release>) -> Plugin {
        Plugin {
            name: name.to_string(),
            plugin_type,
            display_name: None,
            description: None,
            releases,
        }
    }

    /// Build a valid 256-byte plugin header binary.
    ///
    /// `type_byte` goes at offset 20; the version (major, minor, patch, build)
    /// at offsets 8..16. `magic` controls whether the ORA magic is written
    /// correctly, so tests can exercise the magic guard.
    fn header(magic_ok: bool, type_byte: u8, ver: (u16, u16, u16, u16)) -> Vec<u8> {
        let mut buf = vec![0u8; ORA_PLUGIN_HEADER_SIZE];
        let magic = if magic_ok { ORA_PLUGIN_MAGIC } else { 0xDEAD_BEEF };
        buf[0..4].copy_from_slice(&magic.to_le_bytes());
        buf[8..10].copy_from_slice(&ver.0.to_le_bytes());
        buf[10..12].copy_from_slice(&ver.1.to_le_bytes());
        buf[12..14].copy_from_slice(&ver.2.to_le_bytes());
        buf[14..16].copy_from_slice(&ver.3.to_le_bytes());
        buf[20] = type_byte;
        buf
    }

    fn sha_of(data: &[u8]) -> String {
        sha256_hex(data)
    }

    // --- PluginVersion ----------------------------------------------------

    #[test]
    fn plugin_version_parses_three_and_four_components() {
        assert_eq!(PluginVersion::try_from_str("0.1.2"), Some(pv(0, 1, 2)));
        assert_eq!(
            PluginVersion::try_from_str("1.2.3.4"),
            Some(PluginVersion::new(1, 2, 3, 4))
        );
        assert_eq!(PluginVersion::try_from_str("1.2"), None);
        assert_eq!(PluginVersion::try_from_str("1.2.x"), None);
    }

    #[test]
    fn plugin_version_display_omits_zero_build() {
        assert_eq!(pv(0, 1, 2).to_string(), "0.1.2");
        assert_eq!(PluginVersion::new(0, 1, 2, 5).to_string(), "0.1.2.5");
    }

    #[test]
    fn plugin_version_orders_correctly() {
        assert!(pv(0, 1, 0) < pv(0, 2, 0));
        assert!(pv(1, 0, 0) > pv(0, 9, 9));
    }

    // --- PluginType -------------------------------------------------------

    #[test]
    fn plugin_type_parses_short_and_canonical() {
        assert_eq!(PluginType::try_from_str("system"), Some(PluginType::System));
        assert_eq!(
            PluginType::try_from_str("system_plugin"),
            Some(PluginType::System)
        );
        assert_eq!(PluginType::try_from_str("user"), Some(PluginType::User));
        assert_eq!(
            PluginType::try_from_str("user_plugin"),
            Some(PluginType::User)
        );
        assert_eq!(PluginType::try_from_str("pio"), None);
    }

    #[test]
    fn plugin_type_slot_indices_are_fixed() {
        assert_eq!(PluginType::System.slot_index(), 0);
        assert_eq!(PluginType::User.slot_index(), 1);
    }

    // --- Spec parsing -----------------------------------------------------

    #[test]
    fn parse_bare_name() {
        let specs = parse_plugins(&["usb".to_string()]).unwrap();
        assert_eq!(
            specs[0],
            PluginSpec::Named {
                name: "usb".to_string(),
                plugin_type: None,
                version: None,
            }
        );
    }

    #[test]
    fn parse_typed_name() {
        let specs = parse_plugins(&["system/usb".to_string()]).unwrap();
        assert_eq!(
            specs[0],
            PluginSpec::Named {
                name: "usb".to_string(),
                plugin_type: Some(PluginType::System),
                version: None,
            }
        );
    }

    #[test]
    fn parse_pinned_version() {
        let specs = parse_plugins(&["usb,version=0.1.2".to_string()]).unwrap();
        assert_eq!(
            specs[0],
            PluginSpec::Named {
                name: "usb".to_string(),
                plugin_type: None,
                version: Some(pv(0, 1, 2)),
            }
        );
    }

    #[test]
    fn parse_file_spec() {
        let specs = parse_plugins(&["file=/tmp/p.bin".to_string()]).unwrap();
        assert_eq!(
            specs[0],
            PluginSpec::File {
                path: "/tmp/p.bin".to_string()
            }
        );
    }

    #[test]
    fn parse_rejects_malformed() {
        // Empty file path.
        assert!(matches!(
            parse_plugins(&["file=".to_string()]),
            Err(PluginError::SpecSyntax(_))
        ));
        // Option without value.
        assert!(matches!(
            parse_plugins(&["usb,version".to_string()]),
            Err(PluginError::SpecSyntax(_))
        ));
        // Unknown option.
        assert!(matches!(
            parse_plugins(&["usb,foo=bar".to_string()]),
            Err(PluginError::SpecSyntax(_))
        ));
        // Bad version.
        assert!(matches!(
            parse_plugins(&["usb,version=x".to_string()]),
            Err(PluginError::SpecSyntax(_))
        ));
        // Bad type prefix.
        assert!(matches!(
            parse_plugins(&["pio/usb".to_string()]),
            Err(PluginError::SpecSyntax(_))
        ));
        // Empty name.
        assert!(matches!(
            parse_plugins(&["system/".to_string()]),
            Err(PluginError::SpecSyntax(_))
        ));
        // Duplicate option.
        assert!(matches!(
            parse_plugins(&["usb,version=0.1.0,version=0.1.1".to_string()]),
            Err(PluginError::SpecSyntax(_))
        ));
    }

    #[test]
    fn parse_eager_validation_catches_duplicates_when_typed() {
        // Two system plugins, both typed - detectable at parse time.
        let err = parse_plugins(&["system/usb".to_string(), "system/foo".to_string()]);
        assert!(matches!(
            err,
            Err(PluginError::DuplicatePlugin(PluginType::System))
        ));
    }

    #[test]
    fn parse_eager_validation_catches_user_without_system_when_typed() {
        let err = parse_plugins(&["user/rgb".to_string()]);
        assert!(matches!(err, Err(PluginError::UserPluginWithoutSystem)));
    }

    #[test]
    fn parse_defers_validation_when_any_type_unknown() {
        // A bare name means the type set is not fully known, so the eager
        // duplicate/user checks must be deferred (no error here).
        let specs = parse_plugins(&["user/rgb".to_string(), "usb".to_string()]).unwrap();
        assert_eq!(specs.len(), 2);
    }

    // --- Compatibility window --------------------------------------------

    #[test]
    fn compatibility_window_is_half_open() {
        // Window [0.6.0, 0.8.0): 0.6.0 in, 0.8.0 out.
        let r = release(pv(1, 0, 0), fw(0, 6, 0), Some(fw(0, 8, 0)), "aa");

        // Too old.
        assert!(release_incompat(&r, &fw(0, 5, 9)).is_some());
        // Lower bound inclusive.
        assert!(release_incompat(&r, &fw(0, 6, 0)).is_none());
        // Inside.
        assert!(release_incompat(&r, &fw(0, 7, 0)).is_none());
        // Upper bound exclusive.
        assert!(release_incompat(&r, &fw(0, 8, 0)).is_some());
        // Beyond.
        assert!(release_incompat(&r, &fw(0, 9, 0)).is_some());
    }

    #[test]
    fn compatibility_no_upper_bound() {
        let r = release(pv(1, 0, 0), fw(0, 6, 0), None, "aa");
        assert!(release_incompat(&r, &fw(9, 9, 9)).is_none());
    }

    #[test]
    fn compatible_releases_preserves_newest_first_order() {
        // Newest first, as published.
        let p = plugin(
            "usb",
            PluginType::System,
            vec![
                release(pv(0, 3, 0), fw(0, 7, 0), None, "c3"), // too new for fw 0.6
                release(pv(0, 2, 0), fw(0, 6, 0), None, "c2"),
                release(pv(0, 1, 0), fw(0, 5, 0), Some(fw(0, 6, 0)), "c1"), // excluded at 0.6
            ],
        );
        let compat = compatible_releases(&p, &fw(0, 6, 0));
        // Only 0.2.0 is compatible with fw 0.6.0.
        assert_eq!(compat.len(), 1);
        assert_eq!(compat[0].version, pv(0, 2, 0));

        // newest_compatible agrees.
        assert_eq!(newest_compatible(&p, &fw(0, 6, 0)).unwrap().version, pv(0, 2, 0));
    }

    #[test]
    fn newest_compatible_none_when_all_incompatible() {
        let p = plugin(
            "usb",
            PluginType::System,
            vec![release(pv(0, 1, 0), fw(0, 9, 0), None, "c1")],
        );
        assert!(newest_compatible(&p, &fw(0, 6, 0)).is_none());
    }

    // --- Resolved type validation ----------------------------------------

    #[test]
    fn validate_types_enforces_invariants() {
        assert!(validate_resolved_plugin_types(&[PluginType::System]).is_ok());
        assert!(
            validate_resolved_plugin_types(&[PluginType::System, PluginType::User]).is_ok()
        );
        assert!(matches!(
            validate_resolved_plugin_types(&[PluginType::User]),
            Err(PluginError::UserPluginWithoutSystem)
        ));
        assert!(matches!(
            validate_resolved_plugin_types(&[PluginType::System, PluginType::System]),
            Err(PluginError::DuplicatePlugin(PluginType::System))
        ));
    }

    // --- Size handling ----------------------------------------------------

    #[test]
    fn size_handling_boundaries() {
        assert!(matches!(plugin_size_handling(1), Ok(SizeHandling::Pad)));
        assert!(matches!(
            plugin_size_handling(PLUGIN_MAX_SIZE),
            Ok(SizeHandling::None)
        ));
        assert!(matches!(
            plugin_size_handling(PLUGIN_MAX_SIZE + 1),
            Err(PluginError::TooLarge(_, _))
        ));
    }

    // --- Header parsing / verification -----------------------------------

    #[test]
    fn verify_header_only_reads_type_and_version() {
        let bin = header(true, 1, (0, 4, 2, 0));
        let v = verify_binary(&bin, VerifyTarget::Header, "p.bin").unwrap();
        assert_eq!(v.plugin_type, PluginType::User);
        assert_eq!(v.version, pv(0, 4, 2));
        assert_eq!(v.size, ORA_PLUGIN_HEADER_SIZE);
    }

    #[test]
    fn verify_rejects_bad_magic() {
        let bin = header(false, 0, (0, 1, 0, 0));
        assert!(matches!(
            verify_binary(&bin, VerifyTarget::Header, "p.bin"),
            Err(PluginError::InvalidMagic(_, _, _))
        ));
    }

    #[test]
    fn verify_rejects_pio_and_unknown_type() {
        let pio = header(true, 2, (0, 1, 0, 0));
        assert!(matches!(
            verify_binary(&pio, VerifyTarget::Header, "p.bin"),
            Err(PluginError::PioNotSupported(_))
        ));
        let unknown = header(true, 9, (0, 1, 0, 0));
        assert!(matches!(
            verify_binary(&unknown, VerifyTarget::Header, "p.bin"),
            Err(PluginError::UnknownBinaryType(_, 9))
        ));
    }

    #[test]
    fn verify_rejects_too_small() {
        let bin = vec![0u8; 10];
        assert!(matches!(
            verify_binary(&bin, VerifyTarget::Header, "p.bin"),
            Err(PluginError::BinaryTooSmall(_, 10, ORA_PLUGIN_HEADER_SIZE))
        ));
    }

    #[test]
    fn verify_rejects_oversize() {
        let bin = vec![0u8; PLUGIN_MAX_SIZE + 1];
        assert!(matches!(
            verify_binary(&bin, VerifyTarget::Header, "p.bin"),
            Err(PluginError::TooLarge(_, _))
        ));
    }

    #[test]
    fn verify_release_checks_sha_and_type() {
        let bin = header(true, 0, (0, 1, 0, 0)); // system type
        let digest = sha_of(&bin);
        let rel = release(pv(0, 1, 0), fw(0, 6, 0), None, &digest);

        // Correct digest and matching type -> ok.
        let v = verify_binary(
            &bin,
            VerifyTarget::Release {
                release: &rel,
                expected_type: PluginType::System,
            },
            "u.bin",
        )
        .unwrap();
        assert_eq!(v.plugin_type, PluginType::System);

        // Type mismatch (header says system, manifest says user).
        assert!(matches!(
            verify_binary(
                &bin,
                VerifyTarget::Release {
                    release: &rel,
                    expected_type: PluginType::User,
                },
                "u.bin",
            ),
            Err(PluginError::TypeMismatch(_, PluginType::User, PluginType::System))
        ));
    }

    #[test]
    fn verify_release_rejects_sha_mismatch() {
        let bin = header(true, 0, (0, 1, 0, 0));
        let rel = release(pv(0, 1, 0), fw(0, 6, 0), None, "deadbeef");
        assert!(matches!(
            verify_binary(
                &bin,
                VerifyTarget::Release {
                    release: &rel,
                    expected_type: PluginType::System,
                },
                "u.bin",
            ),
            Err(PluginError::Sha256Mismatch { .. })
        ));
    }

    // --- Config generation ------------------------------------------------

    #[test]
    fn plugin_config_maps_type_and_pads() {
        let cfg = plugin_to_chip_set_config("http://x/p.bin", PluginType::System, 1024).unwrap();
        assert_eq!(cfg.chips.len(), 1);
        let chip = &cfg.chips[0];
        assert_eq!(chip.chip_type, OraChipType::SystemPlugin);
        assert_eq!(chip.file, "http://x/p.bin");
        assert!(matches!(chip.size_handling, SizeHandling::Pad));
        assert!(chip.cs1.is_none() && chip.ce.is_none() && chip.oe.is_none());
        assert!(!chip.allow_cs_ignore);

        let user = plugin_to_chip_set_config("f", PluginType::User, PLUGIN_MAX_SIZE).unwrap();
        assert_eq!(user.chips[0].chip_type, OraChipType::UserPlugin);
        assert!(matches!(user.chips[0].size_handling, SizeHandling::None));
    }

    // --- URL building / file_stem ----------------------------------------

    #[test]
    fn urls_are_built_from_type_name_and_release() {
        let p = plugin(
            "usb",
            PluginType::System,
            vec![release(pv(0, 1, 2), fw(0, 6, 0), None, "aa")],
        );
        assert_eq!(
            p.releases_manifest_url(),
            "https://images.onerom.org/plugins/system/usb/releases.json"
        );
        assert_eq!(
            p.binary_url(&p.releases[0]),
            "https://images.onerom.org/plugins/system/usb/v0.1.2/plugin.bin"
        );
        assert_eq!(
            plugins_manifest_url(),
            "https://images.onerom.org/plugins/plugins.json"
        );
    }

    #[test]
    fn file_stem_strips_directory_and_extension() {
        assert_eq!(file_stem("/tmp/foo.bin"), "foo");
        assert_eq!(file_stem("https://x/y/rgb.bin"), "rgb");
        assert_eq!(file_stem("plain"), "plain");
        assert_eq!(file_stem("a\\b\\c.rom"), "c");
        // Leading-dot names keep their name rather than becoming empty.
        assert_eq!(file_stem(".bin"), ".bin");
    }

    // --- ResolvedPlugin ---------------------------------------------------

    #[test]
    fn resolved_named_derives_binary_url() {
        let rel = release(pv(0, 1, 2), fw(0, 6, 0), None, "aa");
        let rp = ResolvedPlugin {
            plugin_type: PluginType::System,
            name: "usb".to_string(),
            version: pv(0, 1, 2),
            size: 1024,
            source: ResolvedSource::Named { release: rel },
        };
        assert_eq!(
            rp.file(),
            "https://images.onerom.org/plugins/system/usb/v0.1.2/plugin.bin"
        );
    }

    #[test]
    fn resolved_file_uses_path() {
        let rp = ResolvedPlugin {
            plugin_type: PluginType::User,
            name: "custom".to_string(),
            version: pv(0, 1, 0),
            size: 512,
            source: ResolvedSource::File {
                path: "/tmp/custom.bin".to_string(),
            },
        };
        assert_eq!(rp.file(), "/tmp/custom.bin");
    }

    // --- from_slot_index --------------------------------------------------

    #[test]
    fn plugin_type_from_slot_index_maps_plugin_slots_only() {
        assert_eq!(PluginType::from_slot_index(0), Some(PluginType::System));
        assert_eq!(PluginType::from_slot_index(1), Some(PluginType::User));
        assert_eq!(PluginType::from_slot_index(2), None);
        // Round-trips with slot_index.
        assert_eq!(
            PluginType::from_slot_index(PluginType::System.slot_index()),
            Some(PluginType::System)
        );
        assert_eq!(
            PluginType::from_slot_index(PluginType::User.slot_index()),
            Some(PluginType::User)
        );
    }

    // --- official_plugin_from_source --------------------------------------

    #[test]
    fn official_source_parses_type_name_and_version() {
        let src = "https://images.onerom.org/plugins/system/usb/v0.1.2/plugin.bin";
        assert_eq!(
            official_plugin_from_source(src),
            Some((PluginType::System, "usb".to_string(), pv(0, 1, 2)))
        );
    }

    #[test]
    fn official_source_rejects_non_official_and_malformed() {
        // Local build path (the shape seen on test firmware).
        assert_eq!(
            official_plugin_from_source("../../plugins/system/usb/build/usb_system_plugin.bin"),
            None
        );
        // Wrong host.
        assert_eq!(
            official_plugin_from_source("https://example.com/plugins/system/usb/v0.1.2/plugin.bin"),
            None
        );
        // Too few segments (missing filename).
        assert_eq!(
            official_plugin_from_source("https://images.onerom.org/plugins/system/usb/v0.1.2"),
            None
        );
        // Too many segments.
        assert_eq!(
            official_plugin_from_source(
                "https://images.onerom.org/plugins/system/usb/v0.1.2/sub/plugin.bin"
            ),
            None
        );
        // Unrecognised type.
        assert_eq!(
            official_plugin_from_source(
                "https://images.onerom.org/plugins/pio/usb/v0.1.2/plugin.bin"
            ),
            None
        );
        // Unparseable version.
        assert_eq!(
            official_plugin_from_source(
                "https://images.onerom.org/plugins/system/usb/vX/plugin.bin"
            ),
            None
        );
        // Empty name.
        assert_eq!(
            official_plugin_from_source(
                "https://images.onerom.org/plugins/system//v0.1.2/plugin.bin"
            ),
            None
        );
    }

    // --- PluginDisplay::display_label -------------------------------------

    #[test]
    fn display_label_prefers_manifest_display_name() {
        let mut p = plugin("usb", PluginType::System, vec![]);
        p.display_name = Some("One ROM USB".to_string());
        let pd = PluginDisplay {
            plugin_type: PluginType::System,
            origin: PluginOrigin::Manifest {
                plugin: p,
                version: pv(0, 1, 2),
            },
        };
        assert_eq!(pd.display_label(), "One ROM USB");
    }

    #[test]
    fn display_label_falls_back_to_slug_when_manifest_unloaded() {
        // display_name None models a failed/absent manifest fetch.
        let pd = PluginDisplay {
            plugin_type: PluginType::System,
            origin: PluginOrigin::Manifest {
                plugin: plugin("usb", PluginType::System, vec![]),
                version: pv(0, 1, 2),
            },
        };
        assert_eq!(pd.display_label(), "usb");
    }

    #[test]
    fn display_label_uses_file_stem_for_local() {
        let pd = PluginDisplay {
            plugin_type: PluginType::User,
            origin: PluginOrigin::Local {
                source: "../../plugins/user/rgb/build/plugin_user.bin".to_string(),
            },
        };
        assert_eq!(pd.display_label(), "plugin_user");
    }
}