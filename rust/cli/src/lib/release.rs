// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The CLI's own release channel - which versions of this tool are published,
//! and for which platforms.
//!
//! This is distinct from the firmware releases `onerom-fw` reads. The manifest
//! lives at `images.onerom.org/cli/releases.json` and is written by
//! `rust/cli/scripts/release.py` when a binary release is made. Each release
//! carries one artifact per platform - a `.deb` on Linux, a zip on Windows and
//! macOS - along with the SHA-256 of the file as published, which is verified
//! on download.
//!
//! The manifest is deliberately parsed leniently: an unknown field added
//! server-side must not stop an older CLI reading it, exactly as for the
//! firmware manifest.

use std::collections::BTreeMap;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::Error;

/// Base URL under which the manifest and every artifact are published.
const SITE_BASE: &str = "https://images.onerom.org/cli";

/// Manifest naming every published CLI release.
const MANIFEST: &str = "releases.json";

/// Where a user downloads the CLI by hand.
pub const DOWNLOAD_PAGE: &str = "https://onerom.org/cli";

/// The `--target` value asking for every platform's artifact at once.
pub const TARGET_ALL: &str = "all";

/// The published CLI releases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Releases {
    /// Manifest data marker.  Not a schema version to branch on - see the
    /// module docs.
    #[allow(dead_code)]
    version: usize,

    /// Newest published version, per platform.  Kept per-platform because a
    /// release need not ship every platform's artifact.
    latest: BTreeMap<String, String>,

    /// Every published release, newest first.
    releases: Vec<Release>,
}

/// One published CLI version, and the artifacts built for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    /// Release version, e.g. `0.3.0`.
    pub version: String,

    /// Path under the site base holding this release's artifacts, e.g. `v0.3.0`.
    pub path: String,

    /// One artifact per platform.
    pub platforms: Vec<Artifact>,
}

/// One platform's downloadable artifact within a release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Rust target triple naming the platform, e.g. `x86_64-unknown-linux-gnu`.
    ///
    /// `universal-apple-darwin` is not a real triple: the macOS artifact is a
    /// universal binary serving both architectures, so it gets a key of its
    /// own.  See [`current_target`].
    pub target: String,

    /// Published file name, which already carries the version and architecture.
    pub filename: String,

    /// SHA-256 of the file as published, lower-case hex.
    pub sha256: String,
}

/// How the running CLI compares with the newest published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Running the newest published release.
    UpToDate,

    /// A newer release than the running one is published.
    Available(Version),

    /// The running build is newer than anything published - a local build.
    Ahead(Version),
}

/// The manifest target key for the platform this binary was built for.
///
/// The macOS artifact is a universal binary, so both Apple architectures map to
/// the single `universal-apple-darwin` key; Windows and Linux keys carry the
/// architecture.  `None` on a platform with no published build: the CLI builds
/// from source anywhere, but only these five platforms are released.
pub fn current_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", _) => Some("universal-apple-darwin"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Some("aarch64-pc-windows-msvc"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

/// The platform this binary runs on, or an error naming what is published.
///
/// Called wherever the user did not name a `--target` of their own.
pub fn require_current_target(releases: &Releases) -> Result<&'static str, Error> {
    current_target().ok_or_else(|| {
        Error::CliPlatformUnsupported(
            format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
            releases.targets_str(),
        )
    })
}

fn parse_version(version: &str) -> Result<Version, Error> {
    Version::parse(version).map_err(|e| Error::CliVersionParse(version.to_string(), e.to_string()))
}

impl Releases {
    /// URL of the release manifest.
    pub fn manifest_url() -> String {
        format!("{SITE_BASE}/{MANIFEST}")
    }

    /// Fetch and parse the release manifest.
    pub async fn from_network() -> Result<Self, Error> {
        let url = Self::manifest_url();
        debug!("Fetching CLI releases manifest from {url}");

        let response = reqwest::get(&url)
            .await
            .map_err(|e| Error::Network(url.clone(), e.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Http(url.clone(), response.status().as_u16()));
        }
        let body = response
            .text()
            .await
            .map_err(|e| Error::Network(url.clone(), e.to_string()))?;

        Self::from_json(&url, &body)
    }

    /// Parse a manifest retrieved from `source`, which names it in any error.
    pub fn from_json(source: &str, data: &str) -> Result<Self, Error> {
        serde_json::from_str(data).map_err(|e| Error::Json(source.to_string(), e.to_string()))
    }

    /// Newest published version for `target`, if that platform has one.
    pub fn latest(&self, target: &str) -> Option<&str> {
        self.latest.get(target).map(String::as_str)
    }

    /// Newest published version across every platform.
    ///
    /// Used only where no single platform is in view - `--target all` without a
    /// `--version`.  Taken as the highest version in the per-platform map
    /// rather than the manifest's ordering, which nothing guarantees.
    pub fn latest_any(&self) -> Option<&str> {
        self.latest
            .values()
            .filter_map(|v| Version::parse(v).ok().map(|parsed| (parsed, v.as_str())))
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(_, v)| v)
    }

    /// The release for an exact version string.
    pub fn release(&self, version: &str) -> Option<&Release> {
        self.releases.iter().find(|r| r.version == version)
    }

    /// Every published version, for error messages.
    pub fn versions_str(&self) -> String {
        self.releases
            .iter()
            .map(|r| r.version.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Every platform with a published build, for error messages.
    pub fn targets_str(&self) -> String {
        self.latest
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Resolve an explicit version, or the newest one, to a release.
    ///
    /// `target` selects whose "newest" is meant; `None` means no single
    /// platform is in view, and the newest across all of them is used.
    pub fn resolve(&self, version: Option<&str>, target: Option<&str>) -> Result<&Release, Error> {
        let version = match version {
            Some(version) => version.to_string(),
            None => {
                let latest = match target {
                    Some(target) => self.latest(target).ok_or_else(|| {
                        Error::CliTargetUnknown(target.to_string(), self.targets_str())
                    })?,
                    None => self.latest_any().ok_or(Error::NoLatestRelease)?,
                };
                latest.to_string()
            }
        };

        self.release(&version)
            .ok_or_else(|| Error::VersionNotFound(version, self.versions_str()))
    }

    /// Compare the running `current` version with the newest for `target`.
    pub fn status(&self, current: &str, target: &str) -> Result<Status, Error> {
        let latest = self
            .latest(target)
            .ok_or_else(|| Error::CliTargetUnknown(target.to_string(), self.targets_str()))?;

        let current = parse_version(current)?;
        let latest = parse_version(latest)?;

        Ok(match latest.cmp(&current) {
            std::cmp::Ordering::Greater => Status::Available(latest),
            std::cmp::Ordering::Less => Status::Ahead(latest),
            std::cmp::Ordering::Equal => Status::UpToDate,
        })
    }
}

impl Release {
    /// This release's artifact for `target`.
    pub fn artifact(&self, target: &str) -> Option<&Artifact> {
        self.platforms.iter().find(|p| p.target == target)
    }

    /// Every platform this release was built for, for error messages.
    pub fn targets_str(&self) -> String {
        let mut targets: Vec<&str> = self.platforms.iter().map(|p| p.target.as_str()).collect();
        targets.sort_unstable();
        targets.join(", ")
    }

    /// URL of one of this release's artifacts.
    pub fn url(&self, artifact: &Artifact) -> String {
        format!("{SITE_BASE}/{}/{}", self.path, artifact.filename)
    }

    /// Download an artifact, verifying it against the manifest's digest.
    ///
    /// The digest comes from the same host as the file, so it catches a
    /// truncated or corrupted download rather than a compromised server -
    /// TLS is what makes the source trustworthy.
    pub async fn download(&self, artifact: &Artifact) -> Result<Vec<u8>, Error> {
        let url = self.url(artifact);
        debug!("Downloading CLI artifact from {url}");

        let response = reqwest::get(&url)
            .await
            .map_err(|e| Error::Network(url.clone(), e.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Http(url.clone(), response.status().as_u16()));
        }
        let data = response
            .bytes()
            .await
            .map_err(|e| Error::Network(url.clone(), e.to_string()))?
            .to_vec();

        verify(artifact, &data)?;

        Ok(data)
    }
}

/// Check downloaded data against an artifact's published SHA-256.
pub fn verify(artifact: &Artifact, data: &[u8]) -> Result<(), Error> {
    use sha2::{Digest, Sha256};

    let actual = hex::encode(Sha256::digest(data));
    if actual != artifact.sha256.to_lowercase() {
        return Err(Error::DownloadSha256Mismatch {
            file: artifact.filename.clone(),
            expected: artifact.sha256.clone(),
            got: actual,
        });
    }

    trace!("Verified {} against published sha256", artifact.filename);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the live manifest: two releases, the older one missing a
    /// platform, so per-platform "latest" is not the same question as "newest
    /// release".
    const MANIFEST_JSON: &str = r#"{
      "version": 1,
      "latest": {
        "x86_64-pc-windows-msvc": "0.3.0",
        "x86_64-unknown-linux-gnu": "0.3.0",
        "aarch64-unknown-linux-gnu": "0.3.0",
        "universal-apple-darwin": "0.3.0",
        "aarch64-pc-windows-msvc": "0.2.0"
      },
      "releases": [
        {
          "version": "0.3.0",
          "path": "v0.3.0",
          "platforms": [
            {
              "target": "universal-apple-darwin",
              "filename": "onerom-cli-mac-0.3.0.zip",
              "sha256": "8e8f3c92bd272938716562cdbb0127c2576106f629d7675053983fdfb8af8efc"
            },
            {
              "target": "x86_64-unknown-linux-gnu",
              "filename": "onerom-cli_0.3.0-1_amd64.deb",
              "sha256": "d84fb3595514b90a3be7e7e7d9c4357423559c81a2db8e13bd6de5eccc66c318"
            },
            {
              "target": "aarch64-unknown-linux-gnu",
              "filename": "onerom-cli_0.3.0-1_arm64.deb",
              "sha256": "16a45d4fd927646affadbfaf300d28cdcb45cefc8a75ba87a849478c702d2fca"
            },
            {
              "target": "x86_64-pc-windows-msvc",
              "filename": "onerom-cli-win-0.3.0-x86_64.zip",
              "sha256": "b32b341f8421951952b03de090c65da3e33b74ec7b3c9bc5d6a47b0a022606e5"
            }
          ]
        },
        {
          "version": "0.2.0",
          "path": "v0.2.0",
          "platforms": [
            {
              "target": "aarch64-pc-windows-msvc",
              "filename": "onerom-cli-win-0.2.0-arm64.zip",
              "sha256": "4dee48610c94504799c41ed0f98ab1962e7b1318491e11a7527c87dcdb2201ca"
            }
          ]
        }
      ]
    }"#;

    fn releases() -> Releases {
        Releases::from_json("test", MANIFEST_JSON).expect("manifest parses")
    }

    #[test]
    fn manifest_parses_to_its_published_contents() {
        let releases = releases();

        assert_eq!(releases.latest("x86_64-unknown-linux-gnu"), Some("0.3.0"));
        assert_eq!(releases.latest("aarch64-pc-windows-msvc"), Some("0.2.0"));
        assert_eq!(releases.latest("sparc-unknown-none-elf"), None);

        let release = releases.release("0.3.0").expect("0.3.0 present");
        assert_eq!(release.path, "v0.3.0");
        assert_eq!(release.platforms.len(), 4);
        assert_eq!(releases.release("9.9.9").map(|r| &r.version), None);
    }

    /// An unknown field added server-side must not stop an older CLI reading
    /// the manifest - the same forward-compatibility the firmware manifest has.
    #[test]
    fn unknown_manifest_fields_are_ignored() {
        let json = MANIFEST_JSON.replace(
            r#""version": 1,"#,
            r#""version": 1, "channel": "stable", "notes": {"a": 1},"#,
        );
        let releases = Releases::from_json("test", &json).expect("manifest still parses");
        assert_eq!(releases.latest("universal-apple-darwin"), Some("0.3.0"));
    }

    #[test]
    fn artifact_lookup_and_url_address_the_published_file() {
        let releases = releases();
        let release = releases.release("0.3.0").unwrap();

        let artifact = release.artifact("x86_64-unknown-linux-gnu").unwrap();
        assert_eq!(artifact.filename, "onerom-cli_0.3.0-1_amd64.deb");
        assert_eq!(
            release.url(artifact),
            "https://images.onerom.org/cli/v0.3.0/onerom-cli_0.3.0-1_amd64.deb"
        );

        // 0.3.0 shipped no Windows ARM build, so that target is absent from
        // this release even though the manifest knows the target exists.
        assert!(release.artifact("aarch64-pc-windows-msvc").is_none());
        assert!(release.targets_str().contains("x86_64-pc-windows-msvc"));
    }

    #[test]
    fn status_distinguishes_older_matching_and_newer_builds() {
        let releases = releases();
        let target = "x86_64-unknown-linux-gnu";

        assert_eq!(
            releases.status("0.2.0", target).unwrap(),
            Status::Available(Version::parse("0.3.0").unwrap())
        );
        assert_eq!(releases.status("0.3.0", target).unwrap(), Status::UpToDate);
        assert_eq!(
            releases.status("0.4.0", target).unwrap(),
            Status::Ahead(Version::parse("0.3.0").unwrap())
        );
    }

    /// A pre-release build sorts below the release of the same number, so a
    /// developer running 0.4.0-rc1 is told 0.4.0 is available rather than that
    /// they are ahead of it.
    #[test]
    fn status_orders_pre_release_builds_below_their_release() {
        let json = MANIFEST_JSON.replace(
            r#""x86_64-unknown-linux-gnu": "0.3.0""#,
            r#""x86_64-unknown-linux-gnu": "0.4.0""#,
        );
        let releases = Releases::from_json("test", &json).unwrap();

        assert_eq!(
            releases
                .status("0.4.0-rc1", "x86_64-unknown-linux-gnu")
                .unwrap(),
            Status::Available(Version::parse("0.4.0").unwrap())
        );
    }

    #[test]
    fn status_names_the_available_targets_for_an_unknown_one() {
        let releases = releases();
        let err = releases
            .status("0.3.0", "mips-unknown-linux-gnu")
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("mips-unknown-linux-gnu"), "{msg}");
        assert!(msg.contains("universal-apple-darwin"), "{msg}");
    }

    #[test]
    fn resolve_takes_an_explicit_version_or_the_platform_latest() {
        let releases = releases();

        // Explicit version wins, even when it is not the latest.
        let release = releases.resolve(Some("0.2.0"), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(release.unwrap().version, "0.2.0");

        // No version: the latest for that platform, which differs per platform.
        let release = releases.resolve(None, Some("x86_64-unknown-linux-gnu"));
        assert_eq!(release.unwrap().version, "0.3.0");
        let release = releases.resolve(None, Some("aarch64-pc-windows-msvc"));
        assert_eq!(release.unwrap().version, "0.2.0");

        // No platform in view: the newest across all of them.
        assert_eq!(releases.resolve(None, None).unwrap().version, "0.3.0");
    }

    #[test]
    fn resolve_reports_an_unpublished_version_with_the_published_list() {
        let releases = releases();
        let err = releases
            .resolve(Some("0.9.9"), Some("x86_64-unknown-linux-gnu"))
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("0.9.9"), "{msg}");
        assert!(msg.contains("0.3.0, 0.2.0"), "{msg}");
    }

    /// `latest_any` reads the map, not the manifest's ordering, so a manifest
    /// listing its releases oldest-first still resolves the newest.
    #[test]
    fn latest_any_ignores_manifest_ordering() {
        let releases = releases();
        assert_eq!(releases.latest_any(), Some("0.3.0"));

        let reordered = MANIFEST_JSON.replace(
            r#""x86_64-pc-windows-msvc": "0.3.0""#,
            r#""x86_64-pc-windows-msvc": "0.10.0""#,
        );
        let releases = Releases::from_json("test", &reordered).unwrap();
        assert_eq!(releases.latest_any(), Some("0.10.0"));
    }

    #[test]
    fn verify_accepts_the_published_digest_and_rejects_altered_data() {
        // Compute the digest the way the manifest generator does, so the
        // fixture exercises the check rather than a hand-copied constant.
        use sha2::{Digest, Sha256};
        let data = b"one rom";
        let artifact = Artifact {
            target: "x86_64-unknown-linux-gnu".to_string(),
            filename: "onerom-cli_0.3.0-1_amd64.deb".to_string(),
            sha256: hex::encode(Sha256::digest(data)),
        };

        assert!(verify(&artifact, data).is_ok());
        // Upper-case digests in the manifest compare equal too.
        let upper = Artifact {
            sha256: artifact.sha256.to_uppercase(),
            ..artifact.clone()
        };
        assert!(verify(&upper, data).is_ok());

        // A single altered byte fails, naming both digests.
        let err = verify(&artifact, b"one rot").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(&artifact.sha256), "{msg}");
        assert!(msg.contains("onerom-cli_0.3.0-1_amd64.deb"), "{msg}");
        // Truncation fails too.
        assert!(verify(&artifact, b"one ro").is_err());
    }

    /// Every platform the CLI is released for maps to a manifest key, and the
    /// key it maps to is one the live manifest actually publishes.
    #[test]
    fn current_target_is_a_published_key_on_released_platforms() {
        let target = current_target();

        if cfg!(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "windows"
        )) {
            let target = target.expect("released platform has a manifest key");
            assert!(
                releases().latest(target).is_some(),
                "{target} is not published in the manifest"
            );
        }

        // macOS is the one platform whose key is not its target triple: both
        // architectures share the universal binary.
        if cfg!(target_os = "macos") {
            assert_eq!(target, Some("universal-apple-darwin"));
        }
    }
}
