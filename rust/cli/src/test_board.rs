// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Fixtures for tests:
//! - a board
//! - a signing key
//! - a signer table
//!
//! The board is onerom-app's in-memory OTP.

use std::path::PathBuf;

use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::{Signer as _, SigningKey};
use onerom_app::{BoardSize, LocalFetch, MemoryOtp, Request, RequestDate, SignerTable, prepare};
use onerom_config::hw::Board;
use onerom_metadata::otp::pico_otp::ecc_encode;
use serde_json::json;

use crate::args::hardware::HardwareCommissionArgs;

/// CHIPID from rows 0x000–0x003 of an RP2350 A4.
pub const CHIP_ID: [u16; 4] = [0x5b6b, 0x2f65, 0x9c23, 0xde3f];

/// The name of the signer [`key`] belongs to.
pub const SIGNER_NAME: &str = "test signer";

/// The date the tests commission on.
pub const DATE: &str = "20260101";

/// A board with only CHIPID written.
pub fn blank_board() -> MemoryOtp {
    let mut otp = MemoryOtp::new();
    for (row, value) in (0..).zip(CHIP_ID) {
        otp.set_raw(row, ecc_encode(value));
    }
    otp
}

/// The key the tests sign with.
pub fn key() -> SigningKey {
    SigningKey::from_bytes(&[1; 32])
}

/// A board commissioned as `board` and `size` by piers.rocks on [`DATE`].
/// [`key`] signs it as signer 1.
pub async fn commissioned_board(board: &str, size: BoardSize) -> MemoryOtp {
    let mut otp = blank_board();
    let request = Request {
        board: Board::try_from_str(board).unwrap(),
        size,
        manufacturer: "piers.rocks".to_string(),
        date: RequestDate::Given(DATE.to_string()),
        signer: 1,
        force: false,
    };
    let prepared = prepare(&mut otp, &request).await.unwrap();
    let signature = key().sign(prepared.message()).to_bytes();
    let plan = prepared.plan(&signature).unwrap();
    plan.execute(&mut otp, |_| {}).await.unwrap();
    otp
}

/// [`key`] in a PKCS#8 PEM file.
pub fn key_file() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("key.pem");
    std::fs::write(&path, key().to_pkcs8_pem(LineEnding::LF).unwrap()).unwrap();
    (dir, path)
}

/// A signer table holding [`key`] as signer 1. `retired` is its `retired`
/// field.
pub fn table(retired: Option<serde_json::Value>) -> SignerTable {
    let key = key();
    let proof = key.sign(&[b"onerom-signer-v1".as_slice(), SIGNER_NAME.as_bytes()].concat());
    let mut signer = json!({
        "id": 1,
        "name": SIGNER_NAME,
        "public_key": hex::encode(key.verifying_key().to_bytes()),
        "proof": hex::encode(proof.to_bytes()),
    });
    if let Some(retired) = retired {
        signer["retired"] = retired;
    }
    let table = json!({ "version": 1, "signers": [signer] });
    SignerTable::parse(table.to_string().as_bytes()).unwrap()
}

/// A request to commission `board` as `size` by piers.rocks on [`DATE`]. It
/// signs with the key in the file `key`.
pub fn args(board: &str, size: BoardSize, key: PathBuf) -> HardwareCommissionArgs {
    HardwareCommissionArgs {
        board: Board::try_from_str(board).unwrap(),
        size,
        manufacturer: "piers.rocks".to_string(),
        signer: None,
        pin: None,
        key: Some(key),
        date: Some(DATE.to_string()),
        force: false,
    }
}

/// Files a test fetches by URL.
pub struct Files(pub Vec<(String, Vec<u8>)>);

impl LocalFetch for Files {
    type Error = String;

    async fn fetch(&self, source: &str) -> Result<Vec<u8>, Self::Error> {
        self.0
            .iter()
            .find(|(url, _)| url == source)
            .map(|(_, file)| file.clone())
            .ok_or_else(|| format!("{source} isn't there"))
    }
}
