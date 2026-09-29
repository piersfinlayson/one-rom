// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Fixtures for tests:
//! - a board
//! - a signing key
//! - a signer table
//! - a commissioning plan
//!
//! The board is onerom-app's in-memory OTP.
//!
//! [`holds`] and the checks beside it find the values in a command's output.

use std::path::PathBuf;

use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::{Signer as _, SigningKey};
use onerom_app::{
    BoardSize, LocalFetch, MemoryOtp, Plan, Request, RequestDate, SignerTable, prepare,
};
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

/// The plan to commission `otp` as `board` and `size` by piers.rocks on
/// [`DATE`]. [`key`] signs it as signer 1.
pub async fn plan(otp: &mut MemoryOtp, board: &str, size: BoardSize) -> Plan {
    let request = Request {
        board: Board::try_from_str(board).unwrap(),
        size,
        manufacturer: "piers.rocks".to_string(),
        date: RequestDate::Given(DATE.to_string()),
        signer: 1,
        force: false,
    };
    let prepared = prepare(otp, &request).await.unwrap();
    let signature = key().sign(prepared.message()).to_bytes();
    prepared.plan(&signature).unwrap()
}

/// A board commissioned as `board` and `size` by piers.rocks on [`DATE`].
/// [`key`] signs it as signer 1.
pub async fn commissioned_board(board: &str, size: BoardSize) -> MemoryOtp {
    let mut otp = blank_board();
    let plan = plan(&mut otp, board, size).await;
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

/// A signer table holding [`key`] as signer 1, which may sign any
/// manufacturer. `retired` is its `retired` field.
pub fn table(retired: Option<serde_json::Value>) -> SignerTable {
    signer_table(retired, &["*"])
}

/// A signer table holding [`key`] as current signer 1, which may sign only
/// `manufacturers`.
pub fn table_allowing(manufacturers: &[&str]) -> SignerTable {
    signer_table(None, manufacturers)
}

/// A signer table holding [`key`] as signer 1. `retired` is its `retired`
/// field and `manufacturers` its `manufacturers` field.
fn signer_table(retired: Option<serde_json::Value>, manufacturers: &[&str]) -> SignerTable {
    let key = key();
    let proof = key.sign(&[b"onerom-signer-v1".as_slice(), SIGNER_NAME.as_bytes()].concat());
    let mut signer = json!({
        "id": 1,
        "name": SIGNER_NAME,
        "public_key": hex::encode(key.verifying_key().to_bytes()),
        "proof": hex::encode(proof.to_bytes()),
        "manufacturers": manufacturers,
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
        size: Some(size),
        manufacturer: "piers.rocks".to_string(),
        signer: None,
        key_id: None,
        pin: None,
        key: Some(key),
        signature: None,
        date: Some(DATE.to_string()),
        force: false,
        dry_run: false,
    }
}

/// Files a test fetches by URL. A URL without a file fails as the CLI's fetch
/// does when the server answers HTTP 404.
pub struct Files(pub Vec<(String, Vec<u8>)>);

impl LocalFetch for Files {
    type Error = onerom_fw::Error;

    async fn fetch(&self, source: &str) -> Result<Vec<u8>, Self::Error> {
        self.0
            .iter()
            .find(|(url, _)| url == source)
            .map(|(_, file)| file.clone())
            .ok_or_else(|| onerom_fw::Error::Http {
                url: source.to_string(),
                status: 404,
            })
    }
}

/// Whether `line` holds each of `values`. A value counts only where neither
/// side of it is a letter, a digit or an underscore, so `1` isn't found in
/// `2026-01-01`. Tests look for the values output carries this way rather
/// than for its wording.
pub fn holds(line: &str, values: &[&str]) -> bool {
    values
        .iter()
        .all(|value| value_end(line, value, 0).is_some())
}

/// Whether `line` holds `values` in order, as [`holds`] finds them.
pub fn holds_in_order(line: &str, values: &[&str]) -> bool {
    let mut from = 0;
    values
        .iter()
        .all(|value| match value_end(line, value, from) {
            Some(end) => {
                from = end;
                true
            }
            None => false,
        })
}

/// Whether one of `lines` holds each of `values`, as [`holds`] finds them.
pub fn shows<S: AsRef<str>>(lines: impl IntoIterator<Item = S>, values: &[&str]) -> bool {
    lines.into_iter().any(|line| holds(line.as_ref(), values))
}

/// Whether consecutive lines of `lines` hold `values` in turn, as [`holds`]
/// finds them.
pub fn in_turn<S: AsRef<str>>(lines: &[S], values: &[&[&str]]) -> bool {
    lines.windows(values.len()).any(|run| {
        run.iter()
            .zip(values)
            .all(|(line, values)| holds(line.as_ref(), values))
    })
}

/// The byte past the first `value` in `line` at or after byte `from`, as
/// [`holds`] finds values.
fn value_end(line: &str, value: &str, from: usize) -> Option<usize> {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    line[from..]
        .match_indices(value)
        .map(|(i, _)| from + i)
        .find(|&i| {
            !word(line[..i].chars().next_back()) && !word(line[i + value.len()..].chars().next())
        })
        .map(|i| i + value.len())
}
