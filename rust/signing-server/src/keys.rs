// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

//! The server's signing keys.
//!
//! Each key is a directory with the key's ID as its name. It has two files:
//! - `key.pem` is the private key as encrypted PKCS#8. The server decrypts it
//!   with each request's PIN. It doesn't store the PIN.
//! - `public.pem` is the public key. The server can't read it from `key.pem`
//!   without the PIN.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use pkcs8::der::pem::PemLabel;
use pkcs8::{DecodePrivateKey, DecodePublicKey, EncryptedPrivateKeyInfoRef, SecretDocument};

/// The private key's file in a key's directory.
const PRIVATE_KEY_FILE: &str = "key.pem";

/// The public key's file in a key's directory.
const PUBLIC_KEY_FILE: &str = "public.pem";

/// The server's keys, by ID.
pub struct Keys(BTreeMap<u16, Key>);

/// A signing key.
#[derive(Clone)]
pub struct Key {
    /// The private key as encrypted PKCS#8 PEM.
    private: String,
    public: VerifyingKey,
}

/// Why [`Keys::load`] refused the keys.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("can't read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("{path}'s name isn't a key ID from 1 to 65535")]
    BadId { path: PathBuf },
    #[error("{path} isn't an encrypted PKCS#8 private key")]
    NotEncrypted { path: PathBuf },
    #[error("{path} isn't an Ed25519 public key")]
    BadPublicKey { path: PathBuf },
    #[error("{path} is a weak Ed25519 key")]
    WeakKey { path: PathBuf },
    #[error("{path} doesn't contain any keys")]
    NoKeys { path: PathBuf },
}

/// Why [`Key::sign`] didn't sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignError {
    /// The PIN didn't decrypt the private key.
    WrongPin,
    /// The private key isn't the public key's.
    Mismatch,
    /// The signature didn't verify with the public key.
    BadSignature,
}

impl Keys {
    /// Loads a key from each directory in `dir`. Files in `dir` are ignored.
    pub fn load(dir: &Path) -> Result<Self, Error> {
        let read_dir_error = |source| Error::Read {
            path: dir.into(),
            source,
        };
        let mut keys = BTreeMap::new();
        for entry in fs::read_dir(dir).map_err(read_dir_error)? {
            let path = entry.map_err(read_dir_error)?.path();
            if !path.is_dir() {
                continue;
            }
            let id = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(parse_id)
                .ok_or_else(|| Error::BadId { path: path.clone() })?;
            keys.insert(id, Key::load(&path)?);
        }
        if keys.is_empty() {
            return Err(Error::NoKeys { path: dir.into() });
        }
        Ok(Self(keys))
    }

    /// The key whose ID is `id`.
    pub fn get(&self, id: u16) -> Option<&Key> {
        self.0.get(&id)
    }

    /// The keys' IDs, in order.
    pub fn ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.0.keys().copied()
    }
}

impl Key {
    /// Loads the key in `dir`.
    fn load(dir: &Path) -> Result<Self, Error> {
        let private_path = dir.join(PRIVATE_KEY_FILE);
        let private = read(&private_path)?;
        if !is_encrypted(&private) {
            return Err(Error::NotEncrypted { path: private_path });
        }
        let public_path = dir.join(PUBLIC_KEY_FILE);
        let public = VerifyingKey::from_public_key_pem(&read(&public_path)?).map_err(|_| {
            Error::BadPublicKey {
                path: public_path.clone(),
            }
        })?;
        if public.is_weak() {
            return Err(Error::WeakKey { path: public_path });
        }
        Ok(Self { private, public })
    }

    /// The public key.
    pub fn public(&self) -> &VerifyingKey {
        &self.public
    }

    /// Signs `message` with the private key after decrypting it with `pin`.
    /// The decrypted key is zeroized before this returns.
    pub fn sign(&self, pin: &str, message: &[u8]) -> Result<Signature, SignError> {
        let private = SigningKey::from_pkcs8_encrypted_pem(&self.private, pin)
            .map_err(|_| SignError::WrongPin)?;
        if private.verifying_key() != self.public {
            return Err(SignError::Mismatch);
        }
        let signature = private.sign(message);
        self.public
            .verify_strict(message, &signature)
            .map_err(|_| SignError::BadSignature)?;
        Ok(signature)
    }
}

/// A key ID in decimal, from 1 to 65535 without leading zeros.
pub fn parse_id(text: &str) -> Option<u16> {
    text.parse::<u16>()
        .ok()
        .filter(|id| *id != 0 && id.to_string() == text)
}

fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.into(),
        source,
    })
}

/// Whether `pem` is an encrypted PKCS#8 private key.
fn is_encrypted(pem: &str) -> bool {
    SecretDocument::from_pem(pem).is_ok_and(|(label, document)| {
        EncryptedPrivateKeyInfoRef::validate_pem_label(label).is_ok()
            && EncryptedPrivateKeyInfoRef::try_from(document.as_bytes()).is_ok()
    })
}
