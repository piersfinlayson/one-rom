// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `hardware validate`.

use onerom_app::{BoardSize, LocalFetch, MemoryOtp, SignerTable, read_commissioning};

use super::{device, failed, json_boards, newer_boards, size_of};
use crate::hardware::{SigningKeys, built_in_warning, validate_otp};
use crate::test_board::{Files, commissioned_board, table, table_allowing};

/// The lines `hardware validate` prints for `otp`, with the downloaded
/// signer table.
async fn validate(otp: &mut MemoryOtp, board: &str, verbose: bool) {
    let files = Files(Vec::new());
    let keys = (&table(None), SigningKeys::Downloaded);
    validate_with(otp, board, verbose, keys, &files).await;
}

/// The lines `hardware validate` prints for `otp` checked against a signer
/// table from the source `keys` identifies. `fetch` fetches a retired key's
/// record file. The built-in table follows the warning the CLI prints when the
/// download fails.
async fn validate_with<F: LocalFetch<Error = onerom_fw::Error>>(
    otp: &mut MemoryOtp,
    board: &str,
    verbose: bool,
    keys: (&SignerTable, SigningKeys),
    fetch: &F,
) {
    let warning = (keys.1 == SigningKeys::BuiltIn).then(download_warning);
    validate_warned(otp, board, verbose, keys, fetch, warning.as_deref()).await;
}

/// [`validate_with`] with `warning` as the warning the CLI prints before it
/// uses the built-in table.
async fn validate_warned<F: LocalFetch<Error = onerom_fw::Error>>(
    otp: &mut MemoryOtp,
    board: &str,
    verbose: bool,
    keys: (&SignerTable, SigningKeys),
    fetch: &F,
    warning: Option<&str>,
) {
    println!(
        "$ onerom hardware validate{}",
        if verbose { " --verbose" } else { "" }
    );
    println!("~ {}", device(board, size_of(otp).await));
    for text in warning.into_iter().flat_map(str::lines) {
        println!("~ {text}");
    }
    let mut out = Vec::new();
    let result = validate_otp(otp, keys, fetch, (false, verbose), &mut out).await;
    print!("{}", String::from_utf8(out).unwrap());
    if let Err(e) = result {
        failed(e);
    }
}

/// The pointer holding the current signer table's address.
const POINTER_URL: &str = "https://images.onerom.org/signers.json";

/// The warning `signer_table` prints when images.onerom.org can't be
/// reached, written by `built_in_warning`.
fn download_warning() -> String {
    // The warning doesn't show reqwest's error so any one stands in for it.
    let error = reqwest::Client::new().get("").build().unwrap_err();
    let error = onerom_fw::Error::network(POINTER_URL.to_string(), error);
    built_in_warning(&onerom_app::Error::fetch(POINTER_URL, error))
}

/// `validate` without and with `--verbose` on `otp`, whose firmware is for
/// `board`.
async fn both(otp: &mut MemoryOtp, board: &str) {
    validate(otp, board, false).await;
    println!();
    validate(otp, board, true).await;
}

#[tokio::test]
async fn a_commissioned_m_board() {
    let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
    both(&mut otp, "fire-24-f").await;
}

#[tokio::test]
async fn a_commissioned_l_board() {
    let mut otp = commissioned_board("fire-40-a", BoardSize::L).await;
    both(&mut otp, "fire-40-a").await;
}

/// Each state `--verbose` shows an instance in.
#[tokio::test]
async fn instance_states() {
    for (name, mut otp) in super::instance_states().await {
        println!("### {name}");
        validate(&mut otp, "fire-24-f", true).await;
        println!();
    }
}

/// Data from a newer version. First an area holding version 2, then an
/// instance holding an unknown key.
#[tokio::test]
async fn newer_data() {
    for (name, mut otp) in newer_boards() {
        println!("### {name}");
        validate(&mut otp, "fire-24-f", false).await;
        println!();
    }
}

/// The URL of the retired key's record file.
const RECORD_URL: &str = "https://example.invalid/signatures/1.txt";

/// A signer table in which the test key is retired with a record file at
/// [`RECORD_URL`] whose SHA-256 is `record`'s.
fn retired_table(record: &[u8]) -> SignerTable {
    use sha2::{Digest, Sha256};
    table(Some(serde_json::json!({
        "record": RECORD_URL,
        "sha256": hex::encode(Sha256::digest(record)),
    })))
}

/// The record file line for `otp`'s current instance.
async fn record_line(otp: &mut MemoryOtp) -> String {
    use onerom_metadata::otp::RecordLine;
    let area = read_commissioning(otp).await.unwrap();
    let signature = area.current().unwrap().signature().unwrap();
    let line = RecordLine::new(signature);
    format!("{line}\n")
}

/// A fetch that fails as the CLI's does when the server answers 404.
struct NotFound;

impl LocalFetch for NotFound {
    type Error = onerom_fw::Error;

    async fn fetch(&self, source: &str) -> Result<Vec<u8>, Self::Error> {
        Err(onerom_fw::Error::Http {
            url: source.to_string(),
            status: 404,
        })
    }
}

/// Prints `### case`, then `validate` on `otp` without and with `--verbose`.
async fn validate_case<F: LocalFetch<Error = onerom_fw::Error>>(
    otp: &mut MemoryOtp,
    case: &str,
    keys: (&SignerTable, SigningKeys),
    fetch: &F,
) {
    println!("### {case}");
    validate_with(otp, "fire-24-f", false, keys, fetch).await;
    println!();
    validate_with(otp, "fire-24-f", true, keys, fetch).await;
    println!();
}

/// Each result `validate` shows for a board the test key signed.
#[tokio::test]
async fn results() {
    use SigningKeys::{BuiltIn, Downloaded};
    let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
    let record = record_line(&mut otp).await.into_bytes();
    let file = |text: &[u8]| Files(vec![(RECORD_URL.to_string(), text.to_vec())]);
    // A record file listing another board's signature.
    let other = b"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\n";
    let empty = SignerTable::parse(br#"{"version": 1, "signers": []}"#).unwrap();
    let none = Files(Vec::new());

    let case = "1: the signer isn't in the table";
    validate_case(&mut otp, case, (&empty, Downloaded), &none).await;

    let case = "2: the key is retired and its record file lists the signature";
    let keys = (&retired_table(&record), Downloaded);
    validate_case(&mut otp, case, keys, &file(&record)).await;

    let case = "3: the key is retired without a record file";
    let keys = (&table(Some(serde_json::json!({}))), Downloaded);
    validate_case(&mut otp, case, keys, &none).await;

    let case = "4: the key is retired and its record file doesn't list the signature";
    let keys = (&retired_table(other), Downloaded);
    validate_case(&mut otp, case, keys, &file(other)).await;

    let case = "5: the key is retired and its record file changed since retirement";
    let keys = (&retired_table(other), Downloaded);
    validate_case(&mut otp, case, keys, &file(&record)).await;

    let case = "6: the key is retired and its record file can't be fetched (HTTP 404)";
    let keys = (&retired_table(&record), Downloaded);
    validate_case(&mut otp, case, keys, &NotFound).await;

    let case = "7: the signer table can't be downloaded so the built-in one is used (the test table stands in for it)";
    validate_case(&mut otp, case, (&table(None), BuiltIn), &none).await;

    let case =
        "8: the key doesn't allow the manufacturer, piers.rocks (key 1 may sign only onerom.org)";
    let keys = (&table_allowing(&["onerom.org"]), Downloaded);
    validate_case(&mut otp, case, keys, &none).await;
}

/// The downloaded signer table is refused because key 300, outside
/// piers.rocks's IDs, allows any manufacturer. The CLI warns and uses the
/// built-in table, which the test table stands in for.
#[tokio::test]
async fn a_downloaded_table_refused_for_its_manufacturers() {
    use ed25519_dalek::Signer as _;
    let table_url = "https://example.invalid/signing-keys.json";
    let key = ed25519_dalek::SigningKey::from_bytes(&[3; 32]);
    let name = "another signer";
    let proof = key.sign(&[b"onerom-signer-v1".as_slice(), name.as_bytes()].concat());
    let refused = serde_json::json!({ "version": 1, "signers": [{
        "id": 300,
        "name": name,
        "public_key": hex::encode(key.verifying_key().to_bytes()),
        "proof": hex::encode(proof.to_bytes()),
        "manufacturers": ["*"],
    }]});
    let pointer = serde_json::json!({ "version": 1, "signing_keys_url": table_url });
    let files = Files(vec![
        (POINTER_URL.to_string(), pointer.to_string().into_bytes()),
        (table_url.to_string(), refused.to_string().into_bytes()),
    ]);
    let error = SignerTable::download(&files).await.unwrap_err();

    let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
    let keys = (&table(None), SigningKeys::BuiltIn);
    let warning = built_in_warning(&error);
    let none = Files(Vec::new());
    validate_warned(&mut otp, "fire-24-f", false, keys, &none, Some(&warning)).await;
}

/// The lines `hardware validate --json` prints for `otp` checked against a
/// signer table from the source `keys` identifies. `fetch` fetches a retired
/// key's record file. The CLI doesn't print the device line with `--json`.
async fn validate_json<F: LocalFetch<Error = onerom_fw::Error>>(
    otp: &mut MemoryOtp,
    keys: (&SignerTable, SigningKeys),
    fetch: &F,
) {
    println!("$ onerom hardware validate --json");
    let mut out = Vec::new();
    let result = validate_otp(otp, keys, fetch, (true, false), &mut out).await;
    print!("{}", String::from_utf8(out).unwrap());
    if let Err(e) = result {
        failed(e);
    }
}

/// `--json` for each board, then for three signer tables.
#[tokio::test]
async fn json() {
    let none = Files(Vec::new());
    for (name, mut otp) in json_boards().await {
        println!("### {name}");
        validate_json(&mut otp, (&table(None), SigningKeys::Downloaded), &none).await;
        println!();
    }

    let mut otp = commissioned_board("fire-24-f", BoardSize::M).await;
    let record = record_line(&mut otp).await.into_bytes();
    let empty = SignerTable::parse(br#"{"version": 1, "signers": []}"#).unwrap();
    println!("### the signer isn't in the table");
    validate_json(&mut otp, (&empty, SigningKeys::Downloaded), &none).await;
    println!();
    println!("### the key is retired and its record file can't be fetched (HTTP 404)");
    let retired = retired_table(&record);
    validate_json(&mut otp, (&retired, SigningKeys::Downloaded), &NotFound).await;
    println!();
    println!(
        "### the key doesn't allow the manufacturer, piers.rocks (key 1 may sign only onerom.org)"
    );
    let refusing = table_allowing(&["onerom.org"]);
    validate_json(&mut otp, (&refusing, SigningKeys::Downloaded), &none).await;
}
