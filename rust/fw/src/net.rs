// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use base64::Engine;
use base64::engine::general_purpose::STANDARD as base64_engine;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board as HwBoard;
use onerom_config::mcu::Variant as McuVariant;
use onerom_gen::{Error as GenError, INLINE_BASE64, file_for_display};

use crate::Error;

pub const FIRMWARE_SITE_BASE: &str = "images.onerom.org";
pub const FIRMWARE_RELEASE_MANIFEST: &str = "releases.json";

/// Retrieves a license from a URL
pub fn fetch_license(url: &str) -> Result<String, Error> {
    debug!("Fetching license from {}", url);
    let response = reqwest::blocking::get(url).map_err(|e| Error::network(url.to_string(), e))?;

    if !response.status().is_success() {
        return Err(Error::Http {
            url: url.to_string(),
            status: response.status().as_u16(),
        });
    }

    let body = response
        .text()
        .map_err(|e| Error::network(url.to_string(), e))?;
    Ok(body)
}

/// Retrieves a license from a URL
pub async fn fetch_license_async(url: &str) -> Result<String, Error> {
    debug!("Fetching license from {}", url);
    let response = reqwest::get(url)
        .await
        .map_err(|e| Error::network(url.to_string(), e))?;
    if !response.status().is_success() {
        return Err(Error::Http {
            url: url.to_string(),
            status: response.status().as_u16(),
        });
    }

    let body = response
        .text()
        .await
        .map_err(|e| Error::network(url.to_string(), e))?;
    Ok(body)
}

/// Retrieves a ROM file from a URL, extracting it from a zip file if needed
/// Function will skip using the filename if `file` data if provided (used for caching zip files)
/// If cache_return is true, the function will return the full file data as well as any extracted data
///
/// Returns:
/// - `Ok(Vec<u8>, Vec<u8>)` - The extracted file data and full file if cache_return
pub fn fetch_rom_file(
    file_to_retrieve: &str,
    file: &[u8],
    extract: Option<String>,
    cache_return: bool,
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let bytes = if file.is_empty() {
        // Get the file itself
        debug!(
            "Fetching ROM file from {}",
            file_for_display(file_to_retrieve)
        );

        if file_to_retrieve.starts_with(INLINE_BASE64) {
            // Base64 encoded file
            let b64_data = file_to_retrieve.trim_start_matches(INLINE_BASE64);
            let data = base64_engine
                .decode(b64_data)
                .map_err(|_| Error::parse(GenError::Base64))?;
            bytes::Bytes::from(data)
        } else if file_to_retrieve.starts_with("base16:") || file_to_retrieve.starts_with("hex:") {
            let hex_data = if file_to_retrieve.starts_with("base16:") {
                file_to_retrieve.trim_start_matches("base16:")
            } else {
                file_to_retrieve.trim_start_matches("hex:")
            };
            let hex_data = &hex_data
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>();
            let data = hex::decode(hex_data).map_err(|_| Error::parse(GenError::Base16))?;
            bytes::Bytes::from(data)
        } else if file_to_retrieve.starts_with("http://")
            || file_to_retrieve.starts_with("https://")
        {
            let response = reqwest::blocking::get(file_to_retrieve)
                .map_err(|e| Error::network(file_to_retrieve.to_string(), e))?;
            if !response.status().is_success() {
                return Err(Error::Http {
                    url: file_to_retrieve.to_string(),
                    status: response.status().as_u16(),
                });
            }
            response
                .bytes()
                .map_err(|e| Error::network(file_to_retrieve.to_string(), e))?
        } else {
            // Local file
            let data = std::fs::read(file_to_retrieve)
                .map_err(|e| Error::read(file_to_retrieve.to_string(), e))?;
            bytes::Bytes::from(data)
        }
    } else {
        debug!(
            "Using cached ROM file for {}",
            file_for_display(file_to_retrieve)
        );
        bytes::Bytes::from(file.to_vec())
    };

    // Now extract if needed
    let file = if let Some(extract) = extract {
        extract_file(&bytes, &extract)?
    } else {
        bytes.to_vec()
    };

    let cache = if cache_return {
        bytes.to_vec()
    } else {
        Vec::new()
    };

    Ok((file, cache))
}

/// Retrieves a ROM file from a URL, or locally, extracting it from a zip file if needed
/// Function will skip using the filename if `file` data if provided (used for caching zip files)
/// If cache_return is true, the function will return the full file data as well as any extracted data
///
/// Returns:
/// - `Ok(Vec<u8>, Vec<u8>)` - The extracted file data and full file data if cache_return
pub async fn fetch_rom_file_async(
    file_to_retrieve: &str,
    file: &[u8],
    extract: Option<String>,
    cache_return: bool,
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let bytes = if file.is_empty() {
        // Get the file itself
        debug!(
            "Fetching ROM file from {}",
            file_for_display(file_to_retrieve)
        );

        if file_to_retrieve.starts_with(INLINE_BASE64) {
            // Base64 encoded file
            let b64_data = file_to_retrieve.trim_start_matches(INLINE_BASE64);
            let data = base64_engine
                .decode(b64_data)
                .map_err(|_| Error::parse(GenError::Base64))?;
            bytes::Bytes::from(data)
        } else if file_to_retrieve.starts_with("base16:") || file_to_retrieve.starts_with("hex:") {
            let hex_data = if file_to_retrieve.starts_with("base16:") {
                file_to_retrieve.trim_start_matches("base16:")
            } else {
                file_to_retrieve.trim_start_matches("hex:")
            };
            // Strip whitespace from hex data
            let hex_data = &hex_data
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>();
            let data = hex::decode(hex_data).map_err(|_| Error::parse(GenError::Base16))?;
            bytes::Bytes::from(data)
        } else if file_to_retrieve.starts_with("http://")
            || file_to_retrieve.starts_with("https://")
        {
            let response = reqwest::get(file_to_retrieve)
                .await
                .map_err(|e| Error::network(file_to_retrieve.to_string(), e))?;
            if !response.status().is_success() {
                return Err(Error::Http {
                    url: file_to_retrieve.to_string(),
                    status: response.status().as_u16(),
                });
            }
            response
                .bytes()
                .await
                .map_err(|e| Error::network(file_to_retrieve.to_string(), e))?
        } else {
            // Local file
            let data = std::fs::read(file_to_retrieve)
                .map_err(|e| Error::read(file_to_retrieve.to_string(), e))?;
            bytes::Bytes::from(data)
        }
    } else {
        debug!(
            "Using cached ROM file for {}",
            file_for_display(file_to_retrieve)
        );
        bytes::Bytes::from(file.to_vec())
    };

    // Now extract if needed
    let file = if let Some(extract) = extract {
        extract_file(&bytes, &extract)?
    } else {
        bytes.to_vec()
    };

    let cache = if cache_return {
        bytes.to_vec()
    } else {
        Vec::new()
    };

    Ok((file, cache))
}

fn extract_file(data: &[u8], extract: &str) -> Result<Vec<u8>, Error> {
    debug!("Extracting file `{}` from zip", extract);
    let reader = std::io::Cursor::new(data);
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| Error::zip(extract.to_string(), e))?;
    let mut file = zip
        .by_name(extract)
        .map_err(|e| Error::zip(extract.to_string(), e))?;
    let mut data = Vec::new();
    std::io::copy(&mut file, &mut data).map_err(|e| Error::read(extract.to_string(), e))?;
    Ok(data)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Releases {
    version: usize,
    pub latest: String,
    releases: Vec<Release>,
}

impl Releases {
    pub fn manifest_url() -> String {
        format!(
            "https://{}/{}",
            FIRMWARE_SITE_BASE, FIRMWARE_RELEASE_MANIFEST
        )
    }

    pub fn from_network() -> Result<Self, Error> {
        let url = Self::manifest_url();
        debug!("Fetching releases manifest from {}", url);
        let response =
            reqwest::blocking::get(&url).map_err(|e| Error::network(url.to_string(), e))?;
        if !response.status().is_success() {
            return Err(Error::Http {
                url: url.to_string(),
                status: response.status().as_u16(),
            });
        }

        let body = response
            .text()
            .map_err(|e| Error::network(url.to_string(), e))?;
        Self::from_json(&body)
    }

    pub async fn from_network_async() -> Result<Self, Error> {
        let url = Self::manifest_url();
        debug!("Fetching releases manifest from {}", url);
        Self::from_network_async_url(&url).await
    }

    pub async fn from_network_async_url(url: &str) -> Result<Self, Error> {
        debug!("Fetching releases manifest from {}", url);
        let response = reqwest::get(url)
            .await
            .map_err(|e| Error::network(url.to_string(), e))?;
        if !response.status().is_success() {
            return Err(Error::Http {
                url: url.to_string(),
                status: response.status().as_u16(),
            });
        }

        let body = response
            .text()
            .await
            .map_err(|e| Error::network(url.to_string(), e))?;
        Self::from_json(&body)
    }

    pub fn from_json(data: &str) -> Result<Releases, Error> {
        serde_json::from_str(data).map_err(Error::json)
    }

    pub fn version_str(version: &FirmwareVersion) -> String {
        format!(
            "{}.{}.{}",
            version.major(),
            version.minor(),
            version.patch()
        )
    }

    pub fn release(&self, version: &FirmwareVersion) -> Option<&Release> {
        let version = Self::version_str(version);
        self.releases.iter().find(|r| r.version == version)
    }

    pub fn releases(&self) -> &Vec<Release> {
        &self.releases
    }

    pub fn hw_releases(&self, board: &HwBoard, mcu: &McuVariant) -> Vec<Release> {
        self.releases
            .iter()
            .filter(|r| {
                r.board_str(&board.name().to_ascii_lowercase())
                    .and_then(|b| b.mcu(&mcu.to_string().to_ascii_lowercase()))
                    .is_some()
            })
            .cloned()
            .collect()
    }

    pub fn releases_str(&self) -> String {
        self.releases
            .iter()
            .map(|r| r.version.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn vec_str(&self) -> Vec<&str> {
        self.releases.iter().map(|r| r.version.as_str()).collect()
    }

    pub fn latest(&self) -> &str {
        &self.latest
    }

    fn download_firmware_prep(
        &self,
        version: &FirmwareVersion,
        board: &HwBoard,
        mcu: &McuVariant,
    ) -> Result<String, Error> {
        let board = board.name();
        let mcu = mcu.to_string();

        // Get the release
        let release = self.release(version).ok_or_else(|| {
            debug!("Failed to find release for {version:?}");
            Error::release_not_found()
        })?;

        // Get the firmware path
        let path = release.path(board, &mcu)?;
        let url = format!("https://{}/{}/firmware.bin", FIRMWARE_SITE_BASE, path);
        Ok(url)
    }

    pub fn download_firmware(
        &self,
        version: &FirmwareVersion,
        board: &HwBoard,
        mcu: &McuVariant,
    ) -> Result<Vec<u8>, Error> {
        let url = self.download_firmware_prep(version, board, mcu)?;

        // Download the firmware
        debug!("Downloading firmware from {}", url);
        let response =
            reqwest::blocking::get(&url).map_err(|e| Error::network(url.to_string(), e))?;
        if !response.status().is_success() {
            return Err(Error::Http {
                url: url.to_string(),
                status: response.status().as_u16(),
            });
        }
        let bytes = response
            .bytes()
            .map_err(|e| Error::network(url.to_string(), e))?;
        Ok(bytes.to_vec())
    }

    pub async fn download_firmware_async(
        &self,
        version: &FirmwareVersion,
        board: &HwBoard,
        mcu: &McuVariant,
    ) -> Result<Vec<u8>, Error> {
        let url = self.download_firmware_prep(version, board, mcu)?;

        // Download the firmware
        debug!("Downloading firmware from {}", url);
        let response = reqwest::get(&url)
            .await
            .map_err(|e| Error::network(url.to_string(), e))?;
        if !response.status().is_success() {
            return Err(Error::Http {
                url: url.to_string(),
                status: response.status().as_u16(),
            });
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| Error::network(url.to_string(), e))?;
        Ok(bytes.to_vec())
    }

    pub fn release_from_string(&self, release: &str) -> Option<&Release> {
        let release = if let Some(release) = release.strip_prefix('v') {
            release
        } else {
            release
        };
        self.releases.iter().find(|r| r.version == release)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Release {
    pub version: String,
    pub path: Option<String>,
    pub notes: Option<String>,
    pub boards: Vec<Board>,
}

impl core::fmt::Display for Release {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "v{}", self.version)
    }
}

impl Release {
    pub fn url(&self, board: &HwBoard, mcu: &McuVariant) -> Result<String, Error> {
        let path = self.path(board.name(), &mcu.to_string())?;
        Ok(format!(
            "https://{}/{}/firmware.bin",
            FIRMWARE_SITE_BASE, path
        ))
    }

    pub fn supports_hw(&self, board: &HwBoard, mcu: &McuVariant) -> bool {
        self.board_str(&board.name().to_ascii_lowercase())
            .and_then(|b| b.mcu(&mcu.to_string().to_ascii_lowercase()))
            .is_some()
    }

    fn path(&self, board: &str, mcu: &str) -> Result<String, Error> {
        let board = self.board_str(&board.to_ascii_lowercase()).ok_or_else(|| {
            debug!("Failed to find board for {board:?}");
            Error::release_not_found()
        })?;
        let path = self.path.clone().unwrap_or_else(|| self.version.clone());

        Ok(format!("{path}/{}", board.path(mcu)?))
    }

    fn board_str(&self, board: &str) -> Option<&Board> {
        self.boards.iter().find(|b| b.name == board)
    }

    pub fn firmware_version(&self) -> Result<FirmwareVersion, Error> {
        FirmwareVersion::try_from_str(&self.version).map_err(|_| {
            debug!("Failed to parse firmware version from {:?}", self.version);
            Error::release_not_found()
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Board {
    pub name: String,
    pub path: Option<String>,
    pub mcus: Vec<Mcu>,
}

impl Board {
    fn path(&self, mcu: &str) -> Result<String, Error> {
        let mcu = self.mcu(&mcu.to_ascii_lowercase()).ok_or_else(|| {
            debug!("Failed to find MCU for {mcu:?}");
            Error::release_not_found()
        })?;
        let path = self.path.clone().unwrap_or_else(|| self.name.clone());

        Ok(format!("{path}/{}", mcu.path()))
    }

    fn mcu(&self, mcu: &str) -> Option<&Mcu> {
        self.mcus.iter().find(|m| m.name == mcu)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Mcu {
    name: String,
    path: Option<String>,
}

impl Mcu {
    fn path(&self) -> String {
        self.path.clone().unwrap_or_else(|| self.name.clone())
    }
}
