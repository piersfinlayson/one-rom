// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM Lab's releases.
//!
//! Lab's manifest is `lab/releases.json` on images.onerom.org, written by
//! `rust/lab/scripts/release.py`.  The images are files of the `lab-vX.Y.Z`
//! GitHub release, so a release's `path` is usually a full URL.  A `path` that
//! isn't a full URL is under `lab/` on images.onerom.org.
//!
//! A release has one flat image per Fire board, and one without a board built
//! in under the `no-board` key.  Each image carries its SHA-256, which is
//! checked on download.
//!
//! Unknown fields are ignored, so a field added to the manifest doesn't stop an
//! older host reading it.  The top-level `version` is a data marker and nothing
//! reads it.

use std::collections::BTreeMap;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use onerom_config::hw::Board;

use crate::Error;
use crate::net::{FIRMWARE_SITE_BASE, fetch_rom_file_async};

/// Lab's directory on images.onerom.org.
const LAB_DIR: &str = "lab";

/// The manifest's file name in `LAB_DIR`.
const MANIFEST: &str = "releases.json";

/// The manifest's key for the image without a board built in.
const NO_BOARD: &str = "no-board";

/// Lab's release manifest.
#[derive(Debug, Clone)]
pub struct LabReleases {
    /// The latest version for each board, and for `NO_BOARD`.
    latest: BTreeMap<String, String>,

    /// Every release, newest first.
    releases: Vec<LabRelease>,
}

impl LabReleases {
    /// Fetches lab/releases.json from images.onerom.org.
    pub async fn from_network_async() -> Result<Self, Error> {
        let url = format!("https://{FIRMWARE_SITE_BASE}/{LAB_DIR}/{MANIFEST}");
        debug!("Fetching Lab releases manifest from {url}");
        let (data, _) = fetch_rom_file_async(&url, &[], None, false).await?;
        Self::from_json(&data)
    }

    /// The latest release for `board`, or for the image without a board for `None`.
    pub fn latest(&self, board: Option<Board>) -> Option<&LabRelease> {
        let version = self.latest.get(image_name(board))?;
        self.release(version)
    }

    /// The release with this version, with or without a leading `v`.
    pub fn release(&self, version: &str) -> Option<&LabRelease> {
        let version = version.strip_prefix('v').unwrap_or(version);
        self.releases
            .iter()
            .find(|release| release.version == version)
    }

    fn from_json(data: &[u8]) -> Result<Self, Error> {
        let manifest: ManifestJson = serde_json::from_slice(data).map_err(Error::json)?;
        Ok(Self {
            latest: manifest.latest,
            releases: manifest.releases.into_iter().map(LabRelease::new).collect(),
        })
    }
}

/// One Lab release.
#[derive(Debug, Clone)]
pub struct LabRelease {
    version: String,
    images: Vec<LabImage>,
}

impl LabRelease {
    /// Its version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The image with `board` built in, or the image without a board for `None`.
    pub fn image(&self, board: Option<Board>) -> Option<&LabImage> {
        let name = image_name(board);
        self.images.iter().find(|image| image.name == name)
    }

    /// Resolves each image's URL from the release's `path`.
    fn new(release: ReleaseJson) -> Self {
        let base = release_url(&release.path);
        let images = release
            .boards
            .into_iter()
            .map(|image| LabImage {
                url: format!("{base}/{}", image.filename),
                name: image.name,
                sha256: image.sha256,
            })
            .collect();
        Self {
            version: release.version,
            images,
        }
    }
}

/// A flat image for the start of flash.
#[derive(Debug, Clone)]
pub struct LabImage {
    /// The board built in, or `NO_BOARD`.
    name: String,

    /// The file's SHA-256, in hex.
    sha256: String,

    /// Where the image downloads from.
    url: String,
}

impl LabImage {
    /// Downloads the image and checks its SHA-256.
    pub async fn download_async(&self) -> Result<Vec<u8>, Error> {
        debug!("Downloading Lab image from {}", self.url);
        let (data, _) = fetch_rom_file_async(&self.url, &[], None, false).await?;
        self.verify(&data)?;
        Ok(data)
    }

    /// Checks `data` against the image's SHA-256.  Hex of either case matches.
    fn verify(&self, data: &[u8]) -> Result<(), Error> {
        let got = hex::encode(Sha256::digest(data));
        if got.eq_ignore_ascii_case(&self.sha256) {
            Ok(())
        } else {
            Err(Error::Sha256Mismatch {
                url: self.url.clone(),
                expected: self.sha256.clone(),
                got,
            })
        }
    }
}

/// The manifest's name for the image with `board` built in, or for the image
/// without a board.
fn image_name(board: Option<Board>) -> &'static str {
    board.map_or(NO_BOARD, |board| board.name())
}

/// Where a release's images are.  A full URL is used as given, and any other
/// `path` is under `LAB_DIR` on images.onerom.org.
fn release_url(path: &str) -> String {
    if path.starts_with("https://") || path.starts_with("http://") {
        path.to_string()
    } else {
        format!("https://{FIRMWARE_SITE_BASE}/{LAB_DIR}/{path}")
    }
}

/// The manifest as published.
#[derive(Deserialize)]
struct ManifestJson {
    latest: BTreeMap<String, String>,
    releases: Vec<ReleaseJson>,
}

/// A release as published.
#[derive(Deserialize)]
struct ReleaseJson {
    version: String,
    path: String,
    boards: Vec<ImageJson>,
}

/// An image as published.
#[derive(Deserialize)]
struct ImageJson {
    name: String,
    filename: String,
    sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of "abc", a test vector from FIPS 180-2.
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// SHA-256 of no data.
    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    /// Where the example's images are.
    const GITHUB: &str = "https://github.com/piersfinlayson/one-rom/releases/download/lab-v0.4.0";

    /// One release, with SHA-256 test vectors for its digests.
    const EXAMPLE: &str = r#"{
      "version": 1,
      "latest": { "no-board": "0.4.0", "fire-24-a": "0.4.0" },
      "releases": [
        {
          "version": "0.4.0",
          "path": "https://github.com/piersfinlayson/one-rom/releases/download/lab-v0.4.0",
          "boards": [
            { "name": "no-board", "filename": "onerom-lab-no-board-0.4.0.bin", "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" },
            { "name": "fire-24-a", "filename": "onerom-lab-fire-24-a-0.4.0.bin", "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" }
          ]
        }
      ]
    }"#;

    /// The example with an older release, whose `path` is relative.  0.3.0 is
    /// the latest for fire-28-a, which 0.4.0 doesn't have.
    const TWO_RELEASES: &str = r#"{
      "version": 1,
      "latest": { "no-board": "0.4.0", "fire-24-a": "0.4.0", "fire-28-a": "0.3.0" },
      "releases": [
        {
          "version": "0.4.0",
          "path": "https://github.com/piersfinlayson/one-rom/releases/download/lab-v0.4.0",
          "boards": [
            { "name": "no-board", "filename": "onerom-lab-no-board-0.4.0.bin", "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" },
            { "name": "fire-24-a", "filename": "onerom-lab-fire-24-a-0.4.0.bin", "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" }
          ]
        },
        {
          "version": "0.3.0",
          "path": "v0.3.0",
          "boards": [
            { "name": "no-board", "filename": "onerom-lab-no-board-0.3.0.bin", "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" },
            { "name": "fire-28-a", "filename": "onerom-lab-fire-28-a-0.3.0.bin", "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" }
          ]
        }
      ]
    }"#;

    fn parse(json: &str) -> LabReleases {
        LabReleases::from_json(json.as_bytes()).expect("manifest parses")
    }

    #[test]
    fn example_parses() {
        let releases = parse(EXAMPLE);
        assert_eq!(releases.releases.len(), 1);

        let release = releases.release("0.4.0").expect("0.4.0 is listed");
        assert_eq!(release.version(), "0.4.0");
        assert_eq!(release.images.len(), 2);

        let image = release
            .image(Some(Board::Fire24A))
            .expect("fire-24-a is listed");
        assert_eq!(image.sha256, ABC_SHA256);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = EXAMPLE
            .replace(r#""version": 1,"#, r#""version": 1, "channel": "stable","#)
            .replace(
                r#""version": "0.4.0","#,
                r#""version": "0.4.0", "notes": {"a": 1},"#,
            )
            .replace(
                r#""name": "fire-24-a","#,
                r#""name": "fire-24-a", "size": 174688,"#,
            );
        assert_eq!(json.matches(r#""channel""#).count(), 1);
        assert_eq!(json.matches(r#""notes""#).count(), 1);
        assert_eq!(json.matches(r#""size""#).count(), 1);

        let releases = parse(&json);
        let release = releases
            .latest(Some(Board::Fire24A))
            .expect("latest parses");
        assert!(release.image(Some(Board::Fire24A)).is_some());
    }

    #[test]
    fn latest_is_per_board_and_for_no_board() {
        let releases = parse(TWO_RELEASES);

        let latest = |board| releases.latest(board).map(LabRelease::version);
        assert_eq!(latest(Some(Board::Fire24A)), Some("0.4.0"));
        assert_eq!(latest(Some(Board::Fire28A)), Some("0.3.0"));
        assert_eq!(latest(None), Some("0.4.0"));

        // A board without an image in any release has no latest.
        assert_eq!(latest(Some(Board::Fire40C)), None);
    }

    #[test]
    fn release_takes_a_version_with_or_without_v() {
        let releases = parse(TWO_RELEASES);

        let release = |version| releases.release(version).map(LabRelease::version);
        assert_eq!(release("0.3.0"), Some("0.3.0"));
        assert_eq!(release("v0.3.0"), Some("0.3.0"));
        assert_eq!(release("9.9.9"), None);

        // One `v` is stripped, not every one.
        assert_eq!(release("vv0.3.0"), None);
    }

    #[test]
    fn image_is_per_board_and_for_no_board() {
        let releases = parse(EXAMPLE);
        let release = releases.release("0.4.0").unwrap();

        let url = |board| release.image(board).map(|image| image.url.as_str());
        assert_eq!(
            url(Some(Board::Fire24A)),
            Some(format!("{GITHUB}/onerom-lab-fire-24-a-0.4.0.bin").as_str())
        );
        assert_eq!(
            url(None),
            Some(format!("{GITHUB}/onerom-lab-no-board-0.4.0.bin").as_str())
        );

        // 0.4.0 in the example has no fire-28-a image.
        assert_eq!(url(Some(Board::Fire28A)), None);
    }

    #[test]
    fn full_url_path_is_used_as_given() {
        let releases = parse(TWO_RELEASES);
        let image = releases
            .release("0.4.0")
            .and_then(|release| release.image(None))
            .unwrap();
        assert_eq!(image.url, format!("{GITHUB}/onerom-lab-no-board-0.4.0.bin"));

        // release.py writes the base URL it downloaded from, which is plain
        // HTTP when testing against a local server.
        assert_eq!(
            release_url("http://localhost:8000/lab-v0.4.0"),
            "http://localhost:8000/lab-v0.4.0"
        );
    }

    #[test]
    fn relative_path_is_under_lab_on_the_site() {
        let releases = parse(TWO_RELEASES);
        let image = releases
            .release("0.3.0")
            .and_then(|release| release.image(Some(Board::Fire28A)))
            .unwrap();
        assert_eq!(
            image.url,
            "https://images.onerom.org/lab/v0.3.0/onerom-lab-fire-28-a-0.3.0.bin"
        );
    }

    #[test]
    fn sha256_mismatch_is_refused() {
        let releases = parse(EXAMPLE);
        let image = releases
            .latest(Some(Board::Fire24A))
            .and_then(|release| release.image(Some(Board::Fire24A)))
            .unwrap();

        // fire-24-a's digest is the SHA-256 of "abc".
        assert!(image.verify(b"abc").is_ok());

        let Err(Error::Sha256Mismatch { url, expected, got }) = image.verify(b"") else {
            panic!("data with a different SHA-256 was accepted");
        };
        assert_eq!(url, format!("{GITHUB}/onerom-lab-fire-24-a-0.4.0.bin"));
        assert_eq!(expected, ABC_SHA256);
        assert_eq!(got, EMPTY_SHA256);

        // Truncated data is refused too.
        assert!(image.verify(b"ab").is_err());

        // An upper-case digest matches the same data and refuses other data.
        let upper = LabImage {
            sha256: ABC_SHA256.to_uppercase(),
            ..image.clone()
        };
        assert!(upper.verify(b"abc").is_ok());
        assert!(upper.verify(b"abd").is_err());
    }
}
