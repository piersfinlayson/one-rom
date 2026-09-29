// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The table of signing keys and the check of a commissioning instance's
//! signature against it.
//!
//! `docs/OTP.md`'s "Manufacturer Signature" section specifies both. The
//! table in `signing-keys.json` is built into the crate. A host can download
//! the current one to use in its place.

mod wire;

use alloc::borrow::Cow;
use alloc::vec::Vec;

use ed25519_dalek::{Signature, VerifyingKey};
use onerom_metadata::otp::{CommissioningInstance, RecordLine, check_manufacturer, parse_record};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::Error;
use crate::fetch::LocalFetch;

pub use wire::SignerError;
use wire::{Entry, Manufacturers, Status};

/// The pointer holding the current signer table's address.
const POINTER_URL: &str = "https://images.onerom.org/signers.json";

/// The start of the message a signer's proof covers. The signer's name
/// follows it.
const PROOF_PREFIX: &[u8] = b"onerom-signer-v1";

/// The table `build.rs` builds from `signing-keys.json`.
const BUILT_IN: &[Signer] = include!(concat!(env!("OUT_DIR"), "/built_in_signers.rs"));

/// A table of signing keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerTable {
    signers: Vec<Signer>,
}

impl SignerTable {
    /// The table built into this crate from `signing-keys.json`.
    pub fn built_in() -> Self {
        Self {
            signers: BUILT_IN.to_vec(),
        }
    }

    /// Parses a table. It refuses the table unless its `version` is 1 and each
    /// signer has:
    ///
    /// - a unique ID other than 0
    /// - a name that isn't empty
    /// - a valid 32-byte public key that is unique and isn't weak
    /// - a 64-byte proof that verifies with the public key
    /// - for a retired key with a record file, the file's https URL and its
    ///   32-byte SHA-256 hash
    /// - a list of the manufacturers the key may sign, each a valid
    ///   `COMMISSIONING_MANUFACTURER` value. `["*"]` in place of a list allows
    ///   any manufacturer, and only a piers.rocks key with an ID from 1 to 255
    ///   can have it.
    ///
    /// Unknown fields are ignored.
    pub fn parse(json: &[u8]) -> Result<Self, SignerError> {
        let signers = wire::decode_table(json)?
            .into_iter()
            .map(|entry| {
                let id = entry.id;
                let key = VerifyingKey::from_bytes(&entry.public_key).map_err(|_| {
                    SignerError::BadField {
                        id,
                        field: "public_key",
                    }
                })?;
                if key.is_weak() {
                    return Err(SignerError::WeakKey(id));
                }
                let message = [PROOF_PREFIX, entry.name.as_bytes()].concat();
                key.verify_strict(&message, &Signature::from_bytes(&entry.proof))
                    .map_err(|_| SignerError::BadProof(id))?;
                if let Manufacturers::Listed(names) = &entry.manufacturers
                    && names.iter().any(|name| check_manufacturer(name).is_err())
                {
                    return Err(SignerError::BadField {
                        id,
                        field: "manufacturers",
                    });
                }
                Ok(Signer::from_entry(entry))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { signers })
    }

    /// Downloads the current table.
    ///
    /// It fetches the pointer at `https://images.onerom.org/signers.json` and
    /// then the table at the address the pointer holds. The table returned has
    /// passed [`parse`](Self::parse). A caller uses it in place of the
    /// built-in table and doesn't merge the two.
    pub async fn download<F: LocalFetch>(fetch: &F) -> Result<Self, Error<F::Error>> {
        let pointer = fetch
            .fetch(POINTER_URL)
            .await
            .map_err(|e| Error::fetch(POINTER_URL, e))?;
        let url = wire::decode_pointer(&pointer)?;
        let table = fetch.fetch(&url).await.map_err(|e| Error::fetch(&url, e))?;
        Ok(Self::parse(&table)?)
    }

    /// The signer whose ID is `id`.
    pub fn get(&self, id: u16) -> Option<&Signer> {
        self.signers.iter().find(|signer| signer.id == id)
    }

    /// The signer whose public key is `public_key`.
    pub fn find(&self, public_key: &[u8; 32]) -> Option<&Signer> {
        self.signers
            .iter()
            .find(|signer| &signer.public_key == public_key)
    }
}

/// A signing key in a [`SignerTable`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signer {
    id: u16,
    name: Cow<'static, str>,
    public_key: [u8; 32],
    status: Status,
    manufacturers: Manufacturers,
}

impl Signer {
    /// The key's ID. `COMMISSIONING_SIGNER` holds it.
    pub fn id(&self) -> u16 {
        self.id
    }

    /// The signer's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the key is retired. A retired key's signatures are accepted
    /// only where its record file lists them.
    pub fn is_retired(&self) -> bool {
        match self.status {
            Status::Current => false,
            Status::Retired { .. } => true,
        }
    }

    /// Whether the key may sign `manufacturer`. Unless the key allows any
    /// manufacturer, `manufacturer` must match a listed name byte for byte.
    pub fn allows(&self, manufacturer: &str) -> bool {
        match &self.manufacturers {
            Manufacturers::Any => true,
            Manufacturers::Listed(names) => names.iter().any(|name| name == manufacturer),
        }
    }

    /// Whether `signature` is this key's Ed25519 signature over `message`.
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
        // parse() refuses a key that isn't a curve point so this fails only
        // for a built-in key the crate's tests have already refused.
        VerifyingKey::from_bytes(&self.public_key).is_ok_and(|key| {
            key.verify_strict(message, &Signature::from_bytes(signature))
                .is_ok()
        })
    }

    fn from_entry(entry: Entry) -> Self {
        Self {
            id: entry.id,
            name: Cow::Owned(entry.name),
            public_key: entry.public_key,
            status: entry.status,
            manufacturers: entry.manufacturers,
        }
    }
}

/// [`verify_instance`]'s verdict on an instance's signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The signature verifies with a current key.
    Verified,
    /// The signature verifies with a retired key. The key's record file lists
    /// it.
    Recorded,
    /// The signature verifies with a retired key. The key's record file doesn't
    /// list it or the key doesn't have a record file.
    NotRecorded,
    /// The signature verifies with a retired key. The key's record file has
    /// changed since the key was retired so it can't be used.
    RecordChanged,
    /// The signature verifies with the signer's key. The key doesn't allow
    /// the instance's manufacturer.
    ManufacturerNotAllowed,
    /// The signature doesn't verify with the signer's key.
    BadSignature,
    /// The table doesn't have the instance's signer.
    UnknownSigner,
    /// `COMMISSIONING_SIG` doesn't end the instance.
    Incomplete,
    /// The instance doesn't have a value for each of the five commissioning
    /// keys.
    Invalid,
}

impl Verdict {
    /// Whether the signature is accepted.
    pub fn is_accepted(&self) -> bool {
        match self {
            Self::Verified | Self::Recorded => true,
            Self::NotRecorded
            | Self::RecordChanged
            | Self::ManufacturerNotAllowed
            | Self::BadSignature
            | Self::UnknownSigner
            | Self::Incomplete
            | Self::Invalid => false,
        }
    }
}

/// Checks `instance`'s signature for the chip whose CHIPID is `chip_id`.
/// `chip_id` holds rows `0x000`–`0x003` read with ECC. Row `0x000` comes
/// first.
///
/// The verdict is the first of these that applies:
///
/// - [`Verdict::Incomplete`]
/// - [`Verdict::Invalid`]
/// - [`Verdict::UnknownSigner`]
/// - [`Verdict::BadSignature`]
/// - [`Verdict::ManufacturerNotAllowed`]
/// - [`Verdict::Verified`] for a current key
///
/// For a retired key it fetches the key's record file and takes the verdict
/// from it. A retired key without a record file is [`Verdict::NotRecorded`]
/// without a fetch.
///
/// An error means the record file couldn't be fetched or has a line that isn't
/// a record line.
pub async fn verify_instance<F: LocalFetch>(
    instance: &CommissioningInstance,
    chip_id: [u16; 4],
    table: &SignerTable,
    fetch: &F,
) -> Result<Verdict, Error<F::Error>> {
    if !instance.is_complete() {
        return Ok(Verdict::Incomplete);
    }
    // A complete instance has a message. A valid one has a manufacturer, a
    // signer and a signature.
    let (true, Some(manufacturer), Some(id), Some(signature), Some(message)) = (
        instance.is_valid(),
        instance.manufacturer(),
        instance.signer(),
        instance.signature(),
        instance.message(chip_id),
    ) else {
        return Ok(Verdict::Invalid);
    };
    let Some(signer) = table.get(id) else {
        return Ok(Verdict::UnknownSigner);
    };
    if !signer.verify(&message, signature) {
        return Ok(Verdict::BadSignature);
    }
    if !signer.allows(manufacturer) {
        return Ok(Verdict::ManufacturerNotAllowed);
    }
    let record = match &signer.status {
        Status::Current => return Ok(Verdict::Verified),
        Status::Retired { record: None } => return Ok(Verdict::NotRecorded),
        Status::Retired {
            record: Some(record),
        } => record,
    };
    let file = fetch
        .fetch(&record.url)
        .await
        .map_err(|e| Error::fetch(record.url.as_ref(), e))?;
    let hash: [u8; 32] = Sha256::digest(&file).into();
    if hash != record.sha256 {
        return Ok(Verdict::RecordChanged);
    }
    let text = core::str::from_utf8(&file).map_err(|e| SignerError::BadRecord {
        id,
        line: line_of(&file, e.valid_up_to()),
    })?;
    let lines = parse_record(text).map_err(|e| SignerError::BadRecord { id, line: e.line })?;
    if lines.contains(&RecordLine::new(signature)) {
        Ok(Verdict::Recorded)
    } else {
        Ok(Verdict::NotRecorded)
    }
}

/// The number of the line holding byte `offset` of `file`. Lines are counted
/// from 1.
fn line_of(file: &[u8], offset: usize) -> usize {
    file[..offset].iter().filter(|&&byte| byte == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `tests/fixtures/signing-keys.json` as `build.rs` writes a table.
    const FIXTURE: &[Signer] = include!(concat!(env!("OUT_DIR"), "/fixture_signers.rs"));

    /// The fixture has a signer with each status, a key allowing any
    /// manufacturer, keys listing one and two manufacturers, and a name and a
    /// manufacturer needing escapes. So this covers every form `build.rs`
    /// writes.
    #[test]
    fn build_rs_writes_the_table_the_parser_reads() {
        let json = include_bytes!("../../tests/fixtures/signing-keys.json");
        let decoded: Vec<Signer> = wire::decode_table(json)
            .unwrap()
            .into_iter()
            .map(Signer::from_entry)
            .collect();
        assert_eq!(FIXTURE, decoded);
    }
}
