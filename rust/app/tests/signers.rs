// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the signer table and the check of a commissioning instance's
//! signature.
//!
//! The keys are made here from fixed seeds. The fetches go to the mock in
//! `common`. One `#[ignore]`d canary at the end downloads the live table. Run
//! it with `cargo test -- --ignored`.

mod common;

use common::{HttpFetch, MockErr, MockFetch};
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use onerom_app::{Error, SignerError, SignerTable, Verdict, verify_instance};
use onerom_config::hw::Board;
use onerom_metadata::otp::{
    CommissioningArea, CommissioningValues, NewCommissioningInstance, RecordLine, RowWrite,
};
use onerom_metadata::{OTP_COMMISSIONING_AREA_FIRST_ROW, OTP_COMMISSIONING_AREA_LAST_ROW};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The pointer's address.
const POINTER_URL: &str = "https://images.onerom.org/signers.json";

/// The table's address in the pointers these tests serve.
const TABLE_URL: &str = "https://example.com/signing-keys.json";

/// Retired signer 2's record file.
const RECORD_URL: &str = "https://example.com/signatures/2.txt";

/// CHIPID from rows 0x000–0x003 of an RP2350 A4.
const CHIP_ID: [u16; 4] = [0x5b6b, 0x2f65, 0x9c23, 0xde3f];

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

/// The test key made from `seed`.
fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// A current signer's entry with its proof made by `key`. It allows only
/// piers.rocks.
fn signer(id: u16, name: &str, key: &SigningKey) -> Value {
    let proof = key.sign(&[b"onerom-signer-v1".as_slice(), name.as_bytes()].concat());
    json!({
        "id": id,
        "name": name,
        "public_key": hex::encode(key.verifying_key().as_bytes()),
        "proof": hex::encode(proof.to_bytes()),
        "manufacturers": ["piers.rocks"],
    })
}

/// `entry` with `retired` as its `retired` object.
fn retired(mut entry: Value, retired: Value) -> Value {
    entry["retired"] = retired;
    entry
}

/// `entry` with `manufacturers` as its `manufacturers` list.
fn allowing(mut entry: Value, manufacturers: &[&str]) -> Value {
    entry["manufacturers"] = json!(manufacturers);
    entry
}

/// A version 1 table holding `signers`.
fn table(signers: Vec<Value>) -> Vec<u8> {
    serde_json::to_vec(&json!({ "version": 1, "signers": signers })).unwrap()
}

/// A table with current signer 1 and retired signer 2. Signer 2's record file
/// is `record` at [`RECORD_URL`].
fn table_with_record(record: &[u8]) -> Vec<u8> {
    table(vec![
        signer(1, "piers.rocks", &key(1)),
        retired(
            signer(2, "piers.rocks", &key(2)),
            json!({ "record": RECORD_URL, "sha256": hex::encode(Sha256::digest(record)) }),
        ),
    ])
}

/// Why a table holding `signers` is refused.
fn refusal(signers: Vec<Value>) -> SignerError {
    SignerTable::parse(&table(signers)).unwrap_err()
}

/// A version 1 pointer to `url`.
fn pointer(url: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({ "version": 1, "signing_keys_url": url })).unwrap()
}

// ---------------------------------------------------------------------------
// The built-in table
// ---------------------------------------------------------------------------

#[test]
fn signing_keys_json_passes_every_check() {
    SignerTable::parse(include_bytes!("../signing-keys.json")).unwrap();
}

#[test]
fn the_built_in_table_is_signing_keys_json() {
    assert_eq!(
        SignerTable::built_in(),
        SignerTable::parse(include_bytes!("../signing-keys.json")).unwrap()
    );
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[test]
fn a_table_reads_back_as_written() {
    let table = SignerTable::parse(&table(vec![
        signer(1, "piers.rocks", &key(1)),
        retired(signer(2, "piers.rocks", &key(2)), json!({})),
        retired(
            signer(256, "another signer", &key(3)),
            json!({ "record": RECORD_URL, "sha256": "00".repeat(32) }),
        ),
    ]))
    .unwrap();

    let first = table.get(1).unwrap();
    assert_eq!(first.id(), 1);
    assert_eq!(first.name(), "piers.rocks");
    assert!(!first.is_retired());
    assert!(table.get(2).unwrap().is_retired());
    assert_eq!(table.get(256).unwrap().name(), "another signer");
    assert!(table.get(256).unwrap().is_retired());
    assert!(table.get(3).is_none());

    let found = table.find(key(2).verifying_key().as_bytes()).unwrap();
    assert_eq!(found.id(), 2);
    assert!(table.find(key(4).verifying_key().as_bytes()).is_none());
}

#[test]
fn unknown_fields_are_ignored() {
    let mut entry = signer(1, "piers.rocks", &key(1));
    entry["contact"] = json!("security@example.com");
    let json = json!({ "version": 1, "updated": "20260926", "signers": [entry] });
    let table = SignerTable::parse(&serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(table.get(1).is_some());
}

#[test]
fn a_signer_verifies_its_own_signatures_only() {
    let table = SignerTable::parse(&table(vec![signer(1, "piers.rocks", &key(1))])).unwrap();
    let signer = table.get(1).unwrap();
    let signature = key(1).sign(b"message").to_bytes();
    assert!(signer.verify(b"message", &signature));
    assert!(!signer.verify(b"another message", &signature));
    assert!(!signer.verify(b"message", &key(2).sign(b"message").to_bytes()));
}

// One test per rule a table must meet.

#[test]
fn an_unknown_table_version_is_refused() {
    let json = json!({ "version": 2, "keys": "in a shape this crate doesn't know" });
    assert_eq!(
        SignerTable::parse(&serde_json::to_vec(&json).unwrap()),
        Err(SignerError::UnknownVersion(2))
    );
}

#[test]
fn a_table_that_isnt_json_of_the_right_shape_is_refused() {
    for json in [
        &b"not JSON"[..],
        br#"{ "version": 1 }"#,
        br#"{ "version": 1, "signers": [{ "id": 1 }] }"#,
        br#"{ "version": 1, "signers": [{ "id": 65536, "name": "a", "public_key": "", "proof": "" }] }"#,
    ] {
        assert!(
            matches!(SignerTable::parse(json), Err(SignerError::Json(_))),
            "{}",
            String::from_utf8_lossy(json)
        );
    }
}

#[test]
fn id_0_is_refused() {
    assert_eq!(
        refusal(vec![signer(0, "piers.rocks", &key(1))]),
        SignerError::ZeroId
    );
}

#[test]
fn an_empty_name_is_refused() {
    assert_eq!(
        refusal(vec![signer(1, "", &key(1))]),
        SignerError::BadField {
            id: 1,
            field: "name"
        }
    );
}

/// Each hex field is refused when it's:
/// - a byte short
/// - a byte long
/// - holding a character that isn't hex
#[test]
fn a_hex_field_of_the_wrong_length_is_refused() {
    let entry = retired(
        signer(1, "piers.rocks", &key(1)),
        json!({ "record": RECORD_URL, "sha256": "00".repeat(32) }),
    );
    for (field, path) in [
        ("public_key", "/public_key"),
        ("proof", "/proof"),
        ("sha256", "/retired/sha256"),
    ] {
        let good = entry.pointer(path).unwrap().as_str().unwrap();
        for bad in [
            good[2..].to_string(),
            format!("{good}00"),
            format!("zz{}", &good[2..]),
        ] {
            let mut entry = entry.clone();
            *entry.pointer_mut(path).unwrap() = json!(bad);
            assert_eq!(
                refusal(vec![entry]),
                SignerError::BadField { id: 1, field },
                "{field} {bad}"
            );
        }
    }
}

#[test]
fn a_public_key_that_isnt_a_curve_point_is_refused() {
    // About half of all encodings aren't a point on the curve.
    let not_a_point = (0..=u8::MAX)
        .map(|n| {
            let mut bytes = [0; 32];
            bytes[0] = n;
            bytes
        })
        .find(|bytes| VerifyingKey::from_bytes(bytes).is_err())
        .unwrap();
    let mut entry = signer(1, "piers.rocks", &key(1));
    entry["public_key"] = json!(hex::encode(not_a_point));
    assert_eq!(
        refusal(vec![entry]),
        SignerError::BadField {
            id: 1,
            field: "public_key"
        }
    );
}

#[test]
fn a_weak_key_is_refused() {
    // The identity point. Its order is 1.
    let mut identity = [0; 32];
    identity[0] = 1;
    let mut entry = signer(1, "piers.rocks", &key(1));
    entry["public_key"] = json!(hex::encode(identity));
    assert_eq!(refusal(vec![entry]), SignerError::WeakKey(1));
}

#[test]
fn a_proof_that_doesnt_verify_is_refused() {
    let mut another_name = signer(1, "piers.rocks", &key(1));
    another_name["proof"] = signer(1, "piers.rock", &key(1))["proof"].clone();
    let mut another_key = signer(1, "piers.rocks", &key(1));
    another_key["proof"] = signer(1, "piers.rocks", &key(2))["proof"].clone();
    for entry in [another_name, another_key] {
        assert_eq!(refusal(vec![entry]), SignerError::BadProof(1));
    }
}

#[test]
fn a_repeated_id_is_refused() {
    assert_eq!(
        refusal(vec![signer(7, "a", &key(1)), signer(7, "b", &key(2))]),
        SignerError::DuplicateId(7)
    );
}

#[test]
fn a_repeated_public_key_is_refused() {
    assert_eq!(
        refusal(vec![signer(1, "a", &key(1)), signer(2, "b", &key(1))]),
        SignerError::DuplicateKey(2)
    );
}

#[test]
fn a_record_without_its_hash_or_a_hash_without_its_record_is_refused() {
    for half in [
        json!({ "record": RECORD_URL }),
        json!({ "sha256": "00".repeat(32) }),
    ] {
        let entry = retired(signer(1, "piers.rocks", &key(1)), half);
        assert_eq!(
            refusal(vec![entry]),
            SignerError::BadField {
                id: 1,
                field: "retired"
            }
        );
    }
}

#[test]
fn a_record_address_that_isnt_https_is_refused() {
    let url = "http://example.com/signatures/1.txt";
    let entry = retired(
        signer(1, "piers.rocks", &key(1)),
        json!({ "record": url, "sha256": "00".repeat(32) }),
    );
    assert_eq!(refusal(vec![entry]), SignerError::NotHttps(url.to_string()));
}

#[test]
fn a_signer_without_manufacturers_is_refused() {
    let mut entry = signer(1, "piers.rocks", &key(1));
    entry.as_object_mut().unwrap().remove("manufacturers");
    assert!(matches!(refusal(vec![entry]), SignerError::Json(_)));
}

/// Each list is refused:
/// - an empty list
/// - a name that isn't a valid `COMMISSIONING_MANUFACTURER` value
/// - `*` beside another entry
#[test]
fn a_bad_manufacturers_list_is_refused() {
    for manufacturers in [
        &[][..],
        &[""],
        &[" piers.rocks"],
        &["piers.rocks "],
        &["piers*rocks"],
        &["**"],
        &["Café"],
        &["piers\trocks"],
        &["piers.rocks", "a*b"],
        &["*", "piers.rocks"],
        &["piers.rocks", "*"],
        &["*", "*"],
    ] {
        let entry = allowing(signer(1, "piers.rocks", &key(1)), manufacturers);
        assert_eq!(
            refusal(vec![entry]),
            SignerError::BadField {
                id: 1,
                field: "manufacturers"
            },
            "{manufacturers:?}"
        );
    }
}

/// Only a piers.rocks key, with an ID from 1 to 255, can allow any
/// manufacturer.
#[test]
fn only_a_piers_rocks_key_allows_any_manufacturer() {
    for id in [1, 255] {
        let entry = allowing(signer(id, "piers.rocks", &key(1)), &["*"]);
        let table = SignerTable::parse(&table(vec![entry])).unwrap();
        assert!(table.get(id).is_some(), "{id}");
    }
    for id in [256, 300, u16::MAX] {
        let entry = allowing(signer(id, "another signer", &key(1)), &["*"]);
        assert_eq!(
            refusal(vec![entry]),
            SignerError::BadField {
                id,
                field: "manufacturers"
            },
            "{id}"
        );
    }
}

/// A listed manufacturer matches byte for byte, without case folding or
/// trimming.
#[test]
fn a_key_allows_only_the_manufacturers_it_lists() {
    let entry = allowing(signer(256, "another signer", &key(1)), &["Acme", "a b"]);
    let table = SignerTable::parse(&table(vec![entry])).unwrap();
    let signer = table.get(256).unwrap();
    for allowed in ["Acme", "a b"] {
        assert!(signer.allows(allowed), "{allowed:?}");
    }
    for refused in [
        "acme", "ACME", " Acme", "Acme ", "Acm", "Acme Ltd", "a  b", "a\tb", "ab", "", "*",
    ] {
        assert!(!signer.allows(refused), "{refused:?}");
    }
}

#[test]
fn a_key_allowing_any_manufacturer_allows_every_name() {
    let entry = allowing(signer(1, "piers.rocks", &key(1)), &["*"]);
    let table = SignerTable::parse(&table(vec![entry])).unwrap();
    let signer = table.get(1).unwrap();
    for name in ["piers.rocks", "onerom.org", "Acme", "*"] {
        assert!(signer.allows(name), "{name:?}");
    }
}

// ---------------------------------------------------------------------------
// Downloading
// ---------------------------------------------------------------------------

/// A download doesn't keep anything from the built-in table. The downloaded
/// table's only signer has an ID the built-in table doesn't use, so a merge
/// would show as an extra signer.
#[tokio::test]
async fn a_download_replaces_the_built_in_table() {
    assert!(SignerTable::built_in().get(1).is_some());
    let downloaded = table(vec![signer(300, "Example signer", &key(1))]);
    let fetch = MockFetch::new()
        .with(POINTER_URL, pointer(TABLE_URL))
        .with(TABLE_URL, downloaded.clone());

    let table = SignerTable::download(&fetch).await.unwrap();

    assert_eq!(table, SignerTable::parse(&downloaded).unwrap());
    assert_eq!(fetch.requested(), [POINTER_URL, TABLE_URL]);
}

#[tokio::test]
async fn a_pointer_with_an_unknown_version_is_refused() {
    let json = json!({ "version": 2, "signing_keys_url": TABLE_URL });
    let fetch = MockFetch::new()
        .with(POINTER_URL, serde_json::to_vec(&json).unwrap())
        .with(TABLE_URL, table(vec![]));

    assert!(matches!(
        SignerTable::download(&fetch).await,
        Err(Error::Signer(SignerError::UnknownVersion(2)))
    ));
    assert!(!fetch.was_requested(TABLE_URL));
}

#[tokio::test]
async fn a_pointer_to_an_http_address_is_refused() {
    let url = "http://example.com/signing-keys.json";
    let fetch = MockFetch::new()
        .with(POINTER_URL, pointer(url))
        .with(url, table(vec![]));

    assert!(matches!(
        SignerTable::download(&fetch).await,
        Err(Error::Signer(SignerError::NotHttps(refused))) if refused == url
    ));
    assert!(!fetch.was_requested(url));
}

#[tokio::test]
async fn a_downloaded_table_with_an_unknown_version_is_refused() {
    let json = json!({ "version": 2 });
    let fetch = MockFetch::new()
        .with(POINTER_URL, pointer(TABLE_URL))
        .with(TABLE_URL, serde_json::to_vec(&json).unwrap());

    assert!(matches!(
        SignerTable::download(&fetch).await,
        Err(Error::Signer(SignerError::UnknownVersion(2)))
    ));
}

#[tokio::test]
async fn a_failed_fetch_carries_its_address() {
    assert!(matches!(
        SignerTable::download(&MockFetch::new()).await,
        Err(Error::Fetch { source, .. }) if source == POINTER_URL
    ));
}

// ---------------------------------------------------------------------------
// Checking an instance
// ---------------------------------------------------------------------------

/// The writes of a fire-24-f instance at row 0x0c0 whose signer is `signer`.
/// `key` signs it.
fn writes(signer: u16, key: &SigningKey) -> Vec<RowWrite> {
    let values =
        CommissioningValues::new(Board::Fire24F, "piers.rocks", "20260926", signer).unwrap();
    let signature = key.sign(&values.message(CHIP_ID)).to_bytes();
    NewCommissioningInstance::new(&values, OTP_COMMISSIONING_AREA_FIRST_ROW)
        .unwrap()
        .writes(&signature)
}

/// A commissioning area holding only `writes`.
fn area(writes: &[RowWrite]) -> CommissioningArea {
    let mut rows =
        vec![
            0;
            usize::from(OTP_COMMISSIONING_AREA_LAST_ROW - OTP_COMMISSIONING_AREA_FIRST_ROW + 1)
        ];
    for write in writes {
        rows[usize::from(write.row - OTP_COMMISSIONING_AREA_FIRST_ROW)] = write.value;
    }
    CommissioningArea::parse(&rows)
}

/// The record line for `area`'s first instance.
fn record_line(area: &CommissioningArea) -> RecordLine {
    RecordLine::new(area.instances()[0].signature().unwrap())
}

/// The verdict on `area`'s first instance against the table `table`.
async fn verdict(
    area: &CommissioningArea,
    table: &[u8],
    fetch: &MockFetch,
) -> Result<Verdict, Error<MockErr>> {
    let table = SignerTable::parse(table).unwrap();
    verify_instance(&area.instances()[0], CHIP_ID, &table, fetch).await
}

#[tokio::test]
async fn a_current_keys_signature_is_verified() {
    let fetch = MockFetch::new();
    let table = table(vec![signer(1, "piers.rocks", &key(1))]);

    let verdict = verdict(&area(&writes(1, &key(1))), &table, &fetch)
        .await
        .unwrap();

    assert_eq!(verdict, Verdict::Verified);
    assert!(verdict.is_accepted());
    assert!(fetch.requested().is_empty());
}

#[tokio::test]
async fn a_retired_keys_signature_in_its_record_is_recorded() {
    let area = area(&writes(2, &key(2)));
    let other = RecordLine::new(&[0; 64]);
    let record = format!("{other}\n\n{}\n", record_line(&area));
    let fetch = MockFetch::new().with(RECORD_URL, record.clone().into_bytes());

    let verdict = verdict(&area, &table_with_record(record.as_bytes()), &fetch)
        .await
        .unwrap();

    assert_eq!(verdict, Verdict::Recorded);
    assert!(verdict.is_accepted());
}

#[tokio::test]
async fn a_retired_keys_signature_missing_from_its_record_is_not_recorded() {
    let area = area(&writes(2, &key(2)));
    let record = format!("{}\n", RecordLine::new(&[0; 64]));
    let fetch = MockFetch::new().with(RECORD_URL, record.clone().into_bytes());

    let verdict = verdict(&area, &table_with_record(record.as_bytes()), &fetch)
        .await
        .unwrap();

    assert_eq!(verdict, Verdict::NotRecorded);
    assert!(!verdict.is_accepted());
}

#[tokio::test]
async fn a_retired_key_without_a_record_is_not_recorded_without_a_fetch() {
    let fetch = MockFetch::new();
    let table = table(vec![retired(signer(2, "piers.rocks", &key(2)), json!({}))]);

    let verdict = verdict(&area(&writes(2, &key(2))), &table, &fetch)
        .await
        .unwrap();

    assert_eq!(verdict, Verdict::NotRecorded);
    assert!(fetch.requested().is_empty());
}

/// A line added after the key was retired changes the file's hash so the file
/// isn't used even though it lists the signature.
#[tokio::test]
async fn a_record_changed_since_retirement_isnt_used() {
    let area = area(&writes(2, &key(2)));
    let fetch = MockFetch::new().with(RECORD_URL, format!("{}\n", record_line(&area)).into_bytes());

    let verdict = verdict(&area, &table_with_record(b""), &fetch)
        .await
        .unwrap();

    assert_eq!(verdict, Verdict::RecordChanged);
    assert!(!verdict.is_accepted());
}

#[tokio::test]
async fn a_signature_by_another_key_is_bad() {
    let table = table(vec![signer(1, "piers.rocks", &key(1))]);

    let verdict = verdict(&area(&writes(1, &key(2))), &table, &MockFetch::new())
        .await
        .unwrap();

    assert_eq!(verdict, Verdict::BadSignature);
}

/// The instance is by piers.rocks and the key allows only onerom.org.
#[tokio::test]
async fn a_manufacturer_the_key_doesnt_allow_isnt_accepted() {
    let fetch = MockFetch::new();
    let entry = allowing(signer(1, "piers.rocks", &key(1)), &["onerom.org"]);

    let verdict = verdict(&area(&writes(1, &key(1))), &table(vec![entry]), &fetch)
        .await
        .unwrap();

    assert_eq!(verdict, Verdict::ManufacturerNotAllowed);
    assert!(!verdict.is_accepted());
    assert!(fetch.requested().is_empty());
}

/// A forged signature is bad whether or not the key allows its manufacturer.
#[tokio::test]
async fn a_signature_by_another_key_is_bad_before_its_manufacturer_is_checked() {
    let entry = allowing(signer(1, "piers.rocks", &key(1)), &["onerom.org"]);

    let verdict = verdict(
        &area(&writes(1, &key(2))),
        &table(vec![entry]),
        &MockFetch::new(),
    )
    .await
    .unwrap();

    assert_eq!(verdict, Verdict::BadSignature);
}

/// A retired key's record file isn't fetched for a manufacturer the key
/// doesn't allow, even where the file lists the signature.
#[tokio::test]
async fn a_retired_key_isnt_looked_up_for_a_manufacturer_it_doesnt_allow() {
    let area = area(&writes(2, &key(2)));
    let record = format!("{}\n", record_line(&area));
    let fetch = MockFetch::new().with(RECORD_URL, record.clone().into_bytes());
    let entry = retired(
        allowing(signer(2, "piers.rocks", &key(2)), &["onerom.org"]),
        json!({ "record": RECORD_URL, "sha256": hex::encode(Sha256::digest(&record)) }),
    );

    let verdict = verdict(&area, &table(vec![entry]), &fetch).await.unwrap();

    assert_eq!(verdict, Verdict::ManufacturerNotAllowed);
    assert!(fetch.requested().is_empty());
}

#[tokio::test]
async fn a_signer_missing_from_the_table_is_unknown() {
    let table = table(vec![signer(1, "piers.rocks", &key(1))]);

    let verdict = verdict(&area(&writes(9, &key(1))), &table, &MockFetch::new())
        .await
        .unwrap();

    assert_eq!(verdict, Verdict::UnknownSigner);
}

#[tokio::test]
async fn an_instance_interrupted_before_its_signature_is_incomplete() {
    // The signature's key row is the last write.
    let mut writes = writes(1, &key(1));
    writes.pop();
    let table = table(vec![signer(1, "piers.rocks", &key(1))]);

    let verdict = verdict(&area(&writes), &table, &MockFetch::new())
        .await
        .unwrap();

    assert_eq!(verdict, Verdict::Incomplete);
}

#[tokio::test]
async fn an_instance_missing_a_commissioning_key_is_invalid() {
    // The board's entry follows the magic and version rows so its key row is
    // the instance's third.
    let mut writes = writes(1, &key(1));
    writes.retain(|write| write.row != OTP_COMMISSIONING_AREA_FIRST_ROW + 2);
    let table = table(vec![signer(1, "piers.rocks", &key(1))]);

    let verdict = verdict(&area(&writes), &table, &MockFetch::new())
        .await
        .unwrap();

    assert_eq!(verdict, Verdict::Invalid);
}

#[tokio::test]
async fn a_malformed_record_line_is_an_error_identifying_it() {
    let area = area(&writes(2, &key(2)));
    let record = format!("{}\n\nnot a record line\n", record_line(&area));
    let fetch = MockFetch::new().with(RECORD_URL, record.clone().into_bytes());

    assert!(matches!(
        verdict(&area, &table_with_record(record.as_bytes()), &fetch).await,
        Err(Error::Signer(SignerError::BadRecord { id: 2, line: 3 }))
    ));
}

#[tokio::test]
async fn a_record_that_isnt_utf8_is_an_error_identifying_its_line() {
    let area = area(&writes(2, &key(2)));
    let record = b"\n\n\xff\n";
    let fetch = MockFetch::new().with(RECORD_URL, record.to_vec());

    assert!(matches!(
        verdict(&area, &table_with_record(record), &fetch).await,
        Err(Error::Signer(SignerError::BadRecord { id: 2, line: 3 }))
    ));
}

#[tokio::test]
async fn a_record_that_cant_be_fetched_is_an_error() {
    let area = area(&writes(2, &key(2)));

    assert!(matches!(
        verdict(&area, &table_with_record(b""), &MockFetch::new()).await,
        Err(Error::Fetch { source, .. }) if source == RECORD_URL
    ));
}

/// The name each verdict takes in JSON.
#[test]
fn verdicts_serialize_in_snake_case() {
    for (verdict, name) in [
        (Verdict::Verified, "verified"),
        (Verdict::Recorded, "recorded"),
        (Verdict::NotRecorded, "not_recorded"),
        (Verdict::RecordChanged, "record_changed"),
        (Verdict::ManufacturerNotAllowed, "manufacturer_not_allowed"),
        (Verdict::BadSignature, "bad_signature"),
        (Verdict::UnknownSigner, "unknown_signer"),
        (Verdict::Incomplete, "incomplete"),
        (Verdict::Invalid, "invalid"),
    ] {
        assert_eq!(serde_json::to_value(verdict).unwrap(), json!(name));
    }
}

/// Downloads the live pointer and table and confirms they still parse. It's
/// ignored by default because it needs the network and tracks a live server.
/// Run it with `cargo test -- --ignored`.
#[tokio::test]
#[ignore = "needs the live images server. Run it with --ignored."]
async fn live_signer_table_still_parses() {
    SignerTable::download(&HttpFetch)
        .await
        .expect("the live pointer and table should parse");
}
