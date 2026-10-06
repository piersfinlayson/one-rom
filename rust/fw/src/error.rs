// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use reqwest::Error as ReqwestError;
use serde_json::Error as SerdeJsonError;
use zip::result::ZipError;

use onerom_config::Error as ConfigError;
use onerom_gen::Error as GenError;

#[derive(Debug)]
pub enum Error {
    Config {
        details: String,
    },
    Read {
        file: String,
        error: std::io::Error,
    },
    Parse {
        error: GenError,
    },
    Build {
        error: GenError,
    },
    License {
        error: GenError,
    },
    FirmwareVersion {
        error: ConfigError,
    },
    Network {
        url: String,
        error: ReqwestError,
    },
    Http {
        url: String,
        status: u16,
    },
    Json {
        error: SerdeJsonError,
    },
    ReleaseNotFound,
    TooLarge {
        portion: String,
        size: usize,
        max: usize,
    },
    FileWrite {
        file: String,
        error: std::io::Error,
    },
    LicenseNotAccepted,
    Zip {
        file: String,
        error: ZipError,
    },
    Sha256Mismatch {
        url: String,
        expected: String,
        got: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Config { details } => write!(f, "Configuration error.\n  {details}"),
            Error::Read { file, error } => write!(f, "Error reading file {file}.\n  {error}"),
            Error::Parse { error } => {
                write!(f, "Hit error parsing supplied configuration.\n  {error}")
            }
            Error::Build { error } => write!(f, "Hit error building firmware.\n  {error}"),
            Error::License { error } => write!(f, "Hit error processing a license.\n  {error}"),
            Error::FirmwareVersion { error } => match error {
                ConfigError::InvalidFirmwareVersion => {
                    write!(f, "Invalid firmware version supplied")
                }
                ConfigError::InvalidMcuVariant { variant } => {
                    write!(f, "Invalid MCU variant: {variant}")
                }
            },
            Error::Network { url, error } => {
                write!(f, "Hit network error accessing URL {url}.\n  {error}")
            }
            Error::Http { url, status } => {
                write!(
                    f,
                    "Hit HTTP error accessing URL {url}.\n  Status code {status}"
                )
            }
            Error::Json { error } => {
                write!(f, "Hit an error parsing the supplied JSON file.\n  {error}")
            }
            Error::ReleaseNotFound => write!(f, "Requested firmware release not found."),
            Error::TooLarge { portion, size, max } => {
                write!(
                    f,
                    "Hit an error: {portion} size {size} exceeds maximum of {max}."
                )
            }
            Error::FileWrite { file, error } => {
                write!(f, "Hit an error writing file {file}.\n  {error}")
            }
            Error::LicenseNotAccepted => write!(f, "License not accepted by user"),
            Error::Zip { file, error } => {
                write!(f, "Hit an error extracting zip file {file}.\n  {error}")
            }
            Error::Sha256Mismatch { url, expected, got } => {
                write!(
                    f,
                    "Hit a SHA-256 mismatch downloading {url}.\n  Expected {expected}\n  Got      {got}"
                )
            }
        }
    }
}

impl Error {
    pub fn config(details: String) -> Self {
        Self::Config { details }
    }
    pub fn read(file: String, error: std::io::Error) -> Self {
        Self::Read { file, error }
    }
    pub fn parse(error: GenError) -> Self {
        Self::Parse { error }
    }
    pub fn build(error: GenError) -> Self {
        Self::Build { error }
    }
    pub fn license(error: GenError) -> Self {
        Self::License { error }
    }
    pub fn firmware_version(error: ConfigError) -> Self {
        Self::FirmwareVersion { error }
    }
    pub fn network(url: String, error: ReqwestError) -> Self {
        Self::Network { url, error }
    }
    pub fn json(error: SerdeJsonError) -> Self {
        Self::Json { error }
    }
    pub fn release_not_found() -> Self {
        Self::ReleaseNotFound
    }
    pub fn too_large(portion: String, size: usize, max: usize) -> Self {
        Self::TooLarge { portion, size, max }
    }
    pub fn write(file: String, error: std::io::Error) -> Self {
        Self::FileWrite { file, error }
    }
    pub fn license_not_accepted() -> Self {
        Self::LicenseNotAccepted
    }
    pub fn zip(file: String, error: ZipError) -> Self {
        Self::Zip { file, error }
    }
}
