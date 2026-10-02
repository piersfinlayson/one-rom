// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Fixtures for tests:
//! - a board
//! - two signing keys
//! - a signer table
//! - a commissioning plan
//! - firmware images
//!
//! The board is onerom-app's in-memory OTP.
//!
//! [`holds`] and the checks beside it find the values in a command's output.

use std::path::{Path, PathBuf};

use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::{Signer as _, SigningKey};
use onerom_app::{
    BoardSize, LocalFetch, MemoryOtp, Plan, Request, RequestDate, SignerTable, prepare,
};
use onerom_cli::usb::{FLASH_BASE, FLASH_READ_SIZE_BYTES};
use onerom_config::hw::Board;
use onerom_gen::FIRMWARE_SIZE;
use onerom_metadata::METADATA_BASE;
use onerom_metadata::otp::pico_otp::ecc_encode;
use serde_json::json;

use crate::args::hardware::{HardwareCommissionArgs, today};

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

/// A manufacturer. It's also the name of its key, [`acme_key`], and the one
/// manufacturer that key may sign.
const ACME: &str = "Acme Retro";

/// [`ACME`]'s signing key.
fn acme_key() -> SigningKey {
    SigningKey::from_bytes(&[4; 32])
}

/// [`acme_key`]'s ID in a signer table.
const ACME_ID: u16 = 256;

/// The PIN [`acme_key_file`] is encrypted with.
pub const ACME_PIN: &str = "1234";

/// [`acme_key`] in a PKCS#8 PEM file encrypted as docs/COMMISSIONING.md makes
/// one. From `openssl pkcs8 -topk8 -scrypt -passout pass:1234` given the key
/// unencrypted.
const ACME_PEM: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----
MIGbMFcGCSqGSIb3DQEFDTBKMCkGCSsGAQQB2kcECzAcBBBe88c+n4GnMVi0qGYs
l3MbAgJAAAIBCAIBATAdBglghkgBZQMEASoEEHU9MAx6R73um/Jzd18ugsAEQJ1L
Nt5hgzjFa46Bpq4GGvBCWEmg10zfTCHoVRuwMQRMtC28Qiu8lKNVX6mOlppKAY5W
qx9HGrusGew4/buAwfA=
-----END ENCRYPTED PRIVATE KEY-----
";

/// [`acme_key`] in the file `acme.pem`, encrypted with [`ACME_PIN`].
pub fn acme_key_file() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acme.pem");
    std::fs::write(&path, ACME_PEM).unwrap();
    (dir, path)
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

/// Zeroed flash holding One ROM's header for v0.`minor`.0. Its build date
/// pointer is `build_date` and its metadata pointer `metadata`.
pub fn header_image(minor: u16, build_date: u32, metadata: u32) -> Vec<u8> {
    use onerom_metadata::{
        FirmwareType, ONEROM_FAMILY_MAGIC, ONEROM_INFO_BUILD_DATE_OFFSET,
        ONEROM_INFO_FIRMWARE_TYPE_OFFSET, ONEROM_INFO_MAGIC_OFFSET, ONEROM_INFO_METADATA_OFFSET,
        ONEROM_INFO_MINOR_VERSION_OFFSET, ONEROM_INFO_OFFSET, ONEROM_INFO_VERSION,
        ONEROM_INFO_VERSION_OFFSET,
    };
    let mut image = vec![0; FLASH_READ_SIZE_BYTES as usize];
    let header = &mut image[ONEROM_INFO_OFFSET as usize..];
    let mut put = |offset: usize, bytes: &[u8]| {
        header[offset..offset + bytes.len()].copy_from_slice(bytes);
    };
    put(ONEROM_INFO_MAGIC_OFFSET, ONEROM_FAMILY_MAGIC.as_bytes());
    put(ONEROM_INFO_MINOR_VERSION_OFFSET, &minor.to_le_bytes());
    put(ONEROM_INFO_BUILD_DATE_OFFSET, &build_date.to_le_bytes());
    put(
        ONEROM_INFO_VERSION_OFFSET,
        &ONEROM_INFO_VERSION.to_le_bytes(),
    );
    put(ONEROM_INFO_METADATA_OFFSET, &metadata.to_le_bytes());
    let one_rom = FirmwareType::FirmwareTypeOneRom as u16;
    put(ONEROM_INFO_FIRMWARE_TYPE_OFFSET, &one_rom.to_le_bytes());
    image
}

/// Base firmware for v0.`minor`.0: One ROM's header alone, pointing at the
/// metadata region.
pub fn base_firmware(minor: u16) -> Vec<u8> {
    let mut image = header_image(minor, FLASH_BASE + 0x300, METADATA_BASE);
    image.truncate(FIRMWARE_SIZE);
    image
}

/// Flash holding a fire-24-e's v0.6.0 firmware header, the layout from before
/// v0.7.0, without ROM sets. Its pin and extra info structures are zeros.
pub fn original_image() -> Vec<u8> {
    use onerom_metadata::{ONEROM_INFO_MAGIC, ONEROM_INFO_OFFSET};
    // Offsets in sdrr_info_t.
    const MINOR_VERSION: usize = 6;
    const BUILD_DATE: usize = 12;
    const HW_REV: usize = 24;
    const MCU_LINE: usize = 28;
    const MCU_STORAGE: usize = 30;
    const PINS: usize = 48;
    const EXTRA: usize = 56;
    // stm_line_t's value for an RP2350, and stm_storage_t's for its 2MB.
    const RP2350: u16 = 5;
    const STORAGE_2MB: u16 = 7;
    let mut image = vec![0; 0x1000];
    let mut put = |at: u32, bytes: &[u8]| {
        let at = at as usize;
        image[at..at + bytes.len()].copy_from_slice(bytes);
    };
    put(0x300, b"Jan  1 2026 00:00:00\0");
    put(0x340, b"fire-24-e\0");
    let header = ONEROM_INFO_OFFSET;
    put(header, ONEROM_INFO_MAGIC.as_bytes());
    put(header + MINOR_VERSION as u32, &6u16.to_le_bytes());
    put(
        header + BUILD_DATE as u32,
        &(FLASH_BASE + 0x300).to_le_bytes(),
    );
    put(header + HW_REV as u32, &(FLASH_BASE + 0x340).to_le_bytes());
    put(header + MCU_LINE as u32, &RP2350.to_le_bytes());
    put(header + MCU_STORAGE as u32, &STORAGE_2MB.to_le_bytes());
    put(header + PINS as u32, &(FLASH_BASE + 0x400).to_le_bytes());
    put(header + EXTRA as u32, &(FLASH_BASE + 0x800).to_le_bytes());
    image
}

/// The size of each 27C400's image in [`image_file`].
pub const IMAGE_27C400: usize = 512 * 1024;

/// The config for `sets` single 27C400 chip sets. Set n's image is `n.bin` in
/// `dir`.
pub fn config_27c400(sets: usize, dir: &Path) -> String {
    let sets: Vec<String> = (0..sets)
        .map(|n| {
            let file = dir.join(format!("{n}.bin"));
            format!(
                r#"{{ "type": "single", "chips": [{{ "file": {}, "type": "27C400" }}] }}"#,
                json!(file)
            )
        })
        .collect();
    format!(
        r#"{{ "version": 1, "description": "Test", "chip_sets": [{}] }}"#,
        sets.join(", ")
    )
}

/// An image file built for a `size` fire-40-a with v0.8.0 firmware from
/// [`base_firmware`]. It holds `sets` single 27C400 chip sets.
pub fn image_file(size: BoardSize, sets: usize) -> Vec<u8> {
    use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
    use onerom_config::mcu::{Family, Variant};
    use onerom_gen::{Builder, FileData};

    let version = FirmwareVersion::new(0, 8, 0, 0);
    let dir = Path::new("rom");
    let mut builder =
        Builder::from_json(version, Family::Rp2350, &config_27c400(sets, dir)).unwrap();
    for n in 0..sets {
        builder
            .add_file(FileData::new(n, vec![n as u8; IMAGE_27C400]))
            .unwrap();
    }
    let props = FirmwareProperties::new(
        version,
        Board::Fire40A,
        Variant::RP2350,
        ServeAlg::Default,
        false,
    )
    .unwrap()
    .with_board_size(size);
    let (metadata, rom_data) = builder.build(props).unwrap();
    onerom_fw::assemble_firmware(base_firmware(8), Some(metadata), Some(rom_data)).unwrap()
}

/// `image`, an [`image_file`], with slot `slot`'s data pointer changed to
/// `addr`. The ROM data stays where it was.
pub async fn move_slot(mut image: Vec<u8>, slot: usize, addr: u32) -> Vec<u8> {
    let parsed = onerom_cli::image::parse_firmware(&image).await;
    let from = parsed
        .as_schema()
        .and_then(|onerom| onerom.metadata())
        .and_then(|metadata| metadata.rom_slots.get(slot))
        .and_then(|slot| slot.data.addr())
        .expect("the slot's data address");
    // The pointer is the only aligned word holding the slot's address.
    let words: Vec<usize> = (0..image.len())
        .step_by(4)
        .filter(|&at| image[at..].starts_with(&from.to_le_bytes()))
        .collect();
    let [at] = words[..] else {
        panic!("{from:#010x} is in the image {} times", words.len());
    };
    image[at..at + 4].copy_from_slice(&addr.to_le_bytes());
    image
}

/// A fire-40-b commissioned as M by [`ACME`] on today's date, the date
/// `hardware commission` takes without `--date`. [`acme_key`] signs it as
/// signer 256.
pub async fn acme_board() -> MemoryOtp {
    let mut otp = blank_board();
    let request = Request {
        board: Board::try_from_str("fire-40-b").unwrap(),
        size: BoardSize::M,
        manufacturer: ACME.to_string(),
        date: RequestDate::Today(today()),
        signer: ACME_ID,
        force: false,
    };
    let prepared = prepare(&mut otp, &request).await.unwrap();
    let signature = acme_key().sign(prepared.message()).to_bytes();
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

/// A signer table holding [`key`] as signer 1, which may sign any
/// manufacturer, and [`acme_key`] as signer 256. `retired` is key 1's
/// `retired` field.
pub fn table(retired: Option<serde_json::Value>) -> SignerTable {
    signer_table(retired, &["*"])
}

/// A signer table holding [`key`] as current signer 1, which may sign only
/// `manufacturers`, and [`acme_key`] as signer 256.
pub fn table_allowing(manufacturers: &[&str]) -> SignerTable {
    signer_table(None, manufacturers)
}

/// A signer table holding [`key`] as signer 1 and [`acme_key`] as signer 256.
/// `retired` is key 1's `retired` field and `manufacturers` its
/// `manufacturers` field.
fn signer_table(retired: Option<serde_json::Value>, manufacturers: &[&str]) -> SignerTable {
    let mut signer = signer_entry(&key(), 1, SIGNER_NAME, manufacturers);
    if let Some(retired) = retired {
        signer["retired"] = retired;
    }
    let acme = signer_entry(&acme_key(), ACME_ID, ACME, &[ACME]);
    let table = json!({ "version": 1, "signers": [signer, acme] });
    SignerTable::parse(table.to_string().as_bytes()).unwrap()
}

/// A signer table's entry for `key` as signer `id` named `name`.
/// `manufacturers` is its `manufacturers` field.
fn signer_entry(
    key: &SigningKey,
    id: u16,
    name: &str,
    manufacturers: &[&str],
) -> serde_json::Value {
    let proof = key.sign(&[b"onerom-signer-v1".as_slice(), name.as_bytes()].concat());
    json!({
        "id": id,
        "name": name,
        "public_key": hex::encode(key.verifying_key().to_bytes()),
        "proof": hex::encode(proof.to_bytes()),
        "manufacturers": manufacturers,
    })
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
        validate: false,
        inspect_otp: false,
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
