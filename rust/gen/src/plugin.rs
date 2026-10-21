// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Official plugin URLs, plugin names and plugin versions.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use onerom_config::chip::ChipType;
use serde::{Deserialize, Serialize};

/// Base URL for plugin manifests and binaries on the images server.
///
/// All plugin resources live beneath this path: the top-level catalogue at
/// `{BASE}/plugins.json`, a plugin's releases at
/// `{BASE}/{type}/{name}/releases.json`, and a release's binary at
/// `{BASE}/{type}/{name}/{version_path}/{filename}`.
pub const PLUGIN_SITE_BASE: &str = "https://images.onerom.org/plugins";

/// The type, name and version in an official plugin's URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct OfficialPlugin<'a> {
    /// The plugin's type.
    pub plugin_type: &'a str,
    /// The plugin's name.
    pub name: &'a str,
    /// The version without its leading `v`.
    pub version: &'a str,
}

/// The type, name and version in `source`, or `None` if `source` isn't an
/// official plugin's URL.
///
/// An official URL is `{PLUGIN_SITE_BASE}/{type}/{name}/v{version}/{filename}`
/// with a known type and a version that parses.
pub fn official_plugin(source: &str) -> Option<OfficialPlugin<'_>> {
    let rest = source.strip_prefix(PLUGIN_SITE_BASE)?.strip_prefix('/')?;
    let segments: Vec<&str> = rest.split('/').collect();
    let [plugin_type, name, version, _filename] = segments[..] else {
        return None;
    };
    let version = version.strip_prefix('v').unwrap_or(version);
    let official = plugin_chip_type(plugin_type).is_some()
        && PluginVersion::try_from_str(version).is_some()
        && !name.is_empty();
    official.then_some(OfficialPlugin {
        plugin_type,
        name,
        version,
    })
}

/// The chip type of a plugin type, in either its short form used in URLs or
/// its config form.
pub fn plugin_chip_type(plugin_type: &str) -> Option<ChipType> {
    match plugin_type {
        "system" | "system_plugin" => Some(ChipType::SystemPlugin),
        "user" | "user_plugin" => Some(ChipType::UserPlugin),
        _ => None,
    }
}

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
            write!(
                f,
                "{}.{}.{}.{}",
                self.major, self.minor, self.patch, self.build
            )
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

/// The final segment of `path` without its extension.
///
/// A name starting with a dot, for example `.bin`, is returned whole.
pub fn file_stem(path: &str) -> &str {
    let last = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match last.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => last,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn an_official_url_has_a_type_name_and_version() {
        let source = "https://images.onerom.org/plugins/system/usb/v0.1.2/plugin.bin";
        assert_eq!(
            official_plugin(source),
            Some(OfficialPlugin {
                plugin_type: "system",
                name: "usb",
                version: "0.1.2",
            })
        );
    }

    #[test]
    fn other_sources_are_not_official() {
        for source in [
            "../../plugins/system/usb/build/usb_system_plugin.bin",
            "https://example.com/plugins/system/usb/v0.1.2/plugin.bin",
            // No filename.
            "https://images.onerom.org/plugins/system/usb/v0.1.2",
            "https://images.onerom.org/plugins/system/usb/v0.1.2/sub/plugin.bin",
            "https://images.onerom.org/plugins/pio/usb/v0.1.2/plugin.bin",
            "https://images.onerom.org/plugins/system/usb/vX/plugin.bin",
            "https://images.onerom.org/plugins/system//v0.1.2/plugin.bin",
        ] {
            assert_eq!(official_plugin(source), None, "{source}");
        }
    }

    #[test]
    fn each_plugin_type_has_two_forms() {
        for (plugin_type, chip_type) in [
            ("system", Some(ChipType::SystemPlugin)),
            ("system_plugin", Some(ChipType::SystemPlugin)),
            ("user", Some(ChipType::UserPlugin)),
            ("user_plugin", Some(ChipType::UserPlugin)),
            ("pio", None),
            ("pio_plugin", None),
        ] {
            assert_eq!(plugin_chip_type(plugin_type), chip_type, "{plugin_type}");
        }
    }

    #[test]
    fn a_plugin_version_has_three_or_four_numbers() {
        for (version, parsed) in [
            ("0.1.2", Some(PluginVersion::new(0, 1, 2, 0))),
            ("1.2.3.4", Some(PluginVersion::new(1, 2, 3, 4))),
            ("1.2", None),
            ("1.2.3.4.5", None),
            ("1.2.x", None),
            ("1.2.70000", None),
        ] {
            assert_eq!(PluginVersion::try_from_str(version), parsed, "{version}");
        }
    }

    #[test]
    fn plugin_version_display_omits_zero_build() {
        assert_eq!(PluginVersion::new(0, 1, 2, 0).to_string(), "0.1.2");
        assert_eq!(PluginVersion::new(0, 1, 2, 5).to_string(), "0.1.2.5");
    }

    #[test]
    fn plugin_version_orders_correctly() {
        assert!(PluginVersion::new(0, 1, 0, 0) < PluginVersion::new(0, 2, 0, 0));
        assert!(PluginVersion::new(1, 0, 0, 0) > PluginVersion::new(0, 9, 9, 0));
    }

    #[test]
    fn file_stem_strips_directory_and_extension() {
        assert_eq!(file_stem("/tmp/foo.bin"), "foo");
        assert_eq!(file_stem("https://x/y/rgb.bin"), "rgb");
        assert_eq!(file_stem("plain"), "plain");
        assert_eq!(file_stem("a\\b\\c.rom"), "c");
        assert_eq!(file_stem(".bin"), ".bin");
    }
}
