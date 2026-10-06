// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The signer table's file format and the pointer to its current address.
//!
//! `build.rs` also includes this file to build `signing-keys.json` into the
//! crate. So it uses only the crates the build script has and leaves the
//! checks that need a signature library or onerom-metadata to the parent
//! module.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::RangeInclusive;

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// The table format version this crate reads.
const TABLE_VERSION: u32 = 1;

/// The pointer format version this crate reads.
const POINTER_VERSION: u32 = 1;

/// The IDs reserved for piers.rocks's keys.
pub(crate) const PIERS_ROCKS_IDS: RangeInclusive<u16> = 1..=255;

/// The `manufacturers` entry that allows any manufacturer. Only a key in
/// [`PIERS_ROCKS_IDS`] can have it, and only as the list's one entry.
const ANY_MANUFACTURER: &str = "*";

/// Why one of these was refused:
/// - a signer table
/// - its pointer
/// - a retired signer's record file
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignerError {
    /// The file isn't JSON of the expected shape. Carries the JSON parser's
    /// message.
    #[error("the signing keys JSON is malformed: {0}")]
    Json(String),

    /// The file's `version` isn't one this crate reads.
    #[error("the signing keys file has unknown version {0}")]
    UnknownVersion(u32),

    /// A URL that must use https doesn't. Carries the URL.
    #[error("'{0}' isn't an https URL")]
    NotHttps(String),

    /// A signer's field doesn't hold a valid value.
    #[error("signer {id} has an invalid {field} field")]
    BadField {
        /// The signer's ID.
        id: u16,
        /// The field's name in the file.
        field: &'static str,
    },

    /// A signer has ID 0, which a signing key can't have.
    #[error("a signer has ID 0, which is invalid")]
    ZeroId,

    /// More than one signer has this ID.
    #[error("more than one signer has ID {0}")]
    DuplicateId(u16),

    /// The signer with this ID has an earlier signer's public key.
    #[error("signer {0} has an earlier signer's public key")]
    DuplicateKey(u16),

    /// The public key of the signer with this ID is one of Ed25519's
    /// small-order points.
    #[error("signer {0}'s public key is weak")]
    WeakKey(u16),

    /// The proof of the signer with this ID doesn't verify with its public
    /// key.
    #[error("signer {0}'s proof doesn't verify")]
    BadProof(u16),

    /// A line of a retired signer's record file isn't a record line.
    #[error("line {line} of signer {id}'s record file is malformed")]
    BadRecord {
        /// The signer's ID.
        id: u16,
        /// The line's number counted from 1.
        line: usize,
    },
}

/// A signer's standing in the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Status {
    /// The key signs new commissioning instances.
    Current,
    /// The key is retired. Its signatures are accepted only where its record
    /// file lists them.
    Retired {
        /// The key's record file. `None` where the key doesn't have one.
        record: Option<Record>,
    },
}

/// A retired key's record file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    /// The file's https URL.
    pub(crate) url: Cow<'static, str>,
    /// The file's SHA-256 hash when the key was retired.
    pub(crate) sha256: [u8; 32],
}

/// The manufacturers a key may sign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Manufacturers {
    /// Any manufacturer.
    Any,
    /// Only these names, each matched byte for byte.
    Listed(Cow<'static, [Cow<'static, str>]>),
}

/// A decoded signer entry. Everything but its public key, its proof and its
/// listed manufacturers' names has been checked.
pub(crate) struct Entry {
    pub(crate) id: u16,
    pub(crate) name: String,
    pub(crate) public_key: [u8; 32],
    pub(crate) proof: [u8; 64],
    pub(crate) status: Status,
    pub(crate) manufacturers: Manufacturers,
}

/// A file's `version`. It's read before the rest so a newer file's shape
/// doesn't matter.
#[derive(Deserialize)]
struct Versioned {
    version: u32,
}

#[derive(Deserialize)]
struct TableFile {
    signers: Vec<SignerFile>,
}

#[derive(Deserialize)]
struct SignerFile {
    id: u16,
    name: String,
    public_key: String,
    proof: String,
    #[serde(default)]
    retired: Option<RetiredFile>,
    manufacturers: Vec<String>,
}

#[derive(Deserialize)]
struct RetiredFile {
    #[serde(default)]
    record: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
}

#[derive(Deserialize)]
struct PointerFile {
    signing_keys_url: String,
}

/// Decodes a signer table. Checks everything except each signer's public key,
/// proof and listed manufacturers' names.
pub(crate) fn decode_table(json: &[u8]) -> Result<Vec<Entry>, SignerError> {
    check_version(json, TABLE_VERSION)?;
    let file: TableFile = from_json(json)?;
    let mut entries: Vec<Entry> = Vec::with_capacity(file.signers.len());
    for signer in file.signers {
        let entry = decode_signer(signer)?;
        if entries.iter().any(|earlier| earlier.id == entry.id) {
            return Err(SignerError::DuplicateId(entry.id));
        }
        if entries
            .iter()
            .any(|earlier| earlier.public_key == entry.public_key)
        {
            return Err(SignerError::DuplicateKey(entry.id));
        }
        entries.push(entry);
    }
    Ok(entries)
}

/// Decodes the pointer and returns the table's URL.
pub(crate) fn decode_pointer(json: &[u8]) -> Result<String, SignerError> {
    check_version(json, POINTER_VERSION)?;
    let file: PointerFile = from_json(json)?;
    https(file.signing_keys_url)
}

fn check_version(json: &[u8], known: u32) -> Result<(), SignerError> {
    let Versioned { version } = from_json(json)?;
    if version == known {
        Ok(())
    } else {
        Err(SignerError::UnknownVersion(version))
    }
}

fn from_json<T: DeserializeOwned>(json: &[u8]) -> Result<T, SignerError> {
    serde_json::from_slice(json).map_err(|e| SignerError::Json(alloc::format!("{e}")))
}

fn decode_signer(signer: SignerFile) -> Result<Entry, SignerError> {
    let id = signer.id;
    if id == 0 {
        return Err(SignerError::ZeroId);
    }
    if signer.name.is_empty() {
        return Err(SignerError::BadField { id, field: "name" });
    }
    let public_key = hex_bytes(&signer.public_key, id, "public_key")?;
    let proof = hex_bytes(&signer.proof, id, "proof")?;
    let status = match signer.retired {
        None => Status::Current,
        Some(RetiredFile {
            record: None,
            sha256: None,
        }) => Status::Retired { record: None },
        Some(RetiredFile {
            record: Some(url),
            sha256: Some(sha256),
        }) => Status::Retired {
            record: Some(Record {
                url: Cow::Owned(https(url)?),
                sha256: hex_bytes(&sha256, id, "sha256")?,
            }),
        },
        Some(
            RetiredFile {
                record: Some(_),
                sha256: None,
            }
            | RetiredFile {
                record: None,
                sha256: Some(_),
            },
        ) => {
            return Err(SignerError::BadField {
                id,
                field: "retired",
            });
        }
    };
    let manufacturers = decode_manufacturers(signer.manufacturers, id)?;
    Ok(Entry {
        id,
        name: signer.name,
        public_key,
        proof,
        status,
        manufacturers,
    })
}

/// Decodes signer `id`'s `manufacturers`. It refuses an empty list, and
/// [`ANY_MANUFACTURER`] beside other entries or for a key outside
/// [`PIERS_ROCKS_IDS`].
fn decode_manufacturers(names: Vec<String>, id: u16) -> Result<Manufacturers, SignerError> {
    let refused = SignerError::BadField {
        id,
        field: "manufacturers",
    };
    match names.as_slice() {
        [] => Err(refused),
        [only] if only == ANY_MANUFACTURER && PIERS_ROCKS_IDS.contains(&id) => {
            Ok(Manufacturers::Any)
        }
        names if names.iter().any(|name| name == ANY_MANUFACTURER) => Err(refused),
        _ => Ok(Manufacturers::Listed(Cow::Owned(
            names.into_iter().map(Cow::Owned).collect(),
        ))),
    }
}

/// The `N` bytes `text` holds in hex of either case.
fn hex_bytes<const N: usize>(
    text: &str,
    id: u16,
    field: &'static str,
) -> Result<[u8; N], SignerError> {
    let mut bytes = [0; N];
    hex::decode_to_slice(text, &mut bytes).map_err(|_| SignerError::BadField { id, field })?;
    Ok(bytes)
}

fn https(url: String) -> Result<String, SignerError> {
    if url.starts_with("https://") {
        Ok(url)
    } else {
        Err(SignerError::NotHttps(url))
    }
}
