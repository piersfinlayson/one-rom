// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The two ways `hardware commission` and `hardware sign` sign a
//! commissioning instance:
//! - [`SigningServer`] for a key on a signing server
//! - [`KeyFile`] for a private key in a file

use std::path::Path;
use std::time::Duration;

use ed25519_dalek::pkcs8::{ALGORITHM_OID, PrivateKeyInfoRef, SecretDocument};
use ed25519_dalek::{Signer as _, SigningKey};
use log::debug;
use onerom_config::hw::Board;
use onerom_metadata::otp::format_chip_id;
use pkcs8::{EncryptedPrivateKeyInfoRef, pkcs5};
use serde::Serialize;

use crate::Error;
use crate::otp::escape_controls;

/// The longest a request to a signing server may take. The server records a
/// signature in a git repository on GitHub before returning it.
const TIMEOUT: Duration = Duration::from_secs(60);

/// A key on a signing server.
///
/// The key's URL is [`key_url`]: the server's address followed by the API
/// version and the key's ID. The client appends `/public-key` or `/sign` to
/// it. An error shows the key's URL because it identifies the key as well as
/// the server.
pub struct SigningServer {
    /// The server's address as `--signer` gave it.
    address: String,
    id: u16,
    url: String,
    pin: String,
    client: reqwest::Client,
}

/// The body of a `/sign` request.
#[derive(Debug, Serialize)]
struct SignRequest<'a> {
    /// CHIPID as the bootloader's USB serial number shows it.
    chip_id: String,
    board: &'a str,
    manufacturer: &'a str,
    /// The UTC commissioning date as `YYYYMMDD`.
    date: &'a str,
    /// Sign without publicly recording the signature. A request that records
    /// it leaves this out, as a server without the option refuses any field it
    /// doesn't know.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    dry_run: bool,
}

impl SigningServer {
    /// Key `id` on the signing server at `address`. `pin` unlocks it. Refuses
    /// an address that isn't https.
    pub fn new(address: &str, id: u16, pin: &str) -> Result<Self, Error> {
        if !is_https(address) {
            return Err(Error::InvalidArgument(
                "--signer".to_string(),
                format!("'{address}' isn't an https URL"),
            ));
        }
        let url = key_url(address, id);
        let client = reqwest::Client::builder()
            .https_only(true)
            .timeout(TIMEOUT)
            .build()
            .map_err(|_| Error::SigningServerUnreachable(url.clone()))?;
        Ok(Self {
            address: address.to_string(),
            id,
            url,
            pin: pin.to_string(),
            client,
        })
    }

    /// The server's address as `--signer` gave it.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The key's ID.
    pub fn id(&self) -> u16 {
        self.id
    }

    /// The key's 32-byte Ed25519 public key.
    pub async fn public_key(&self) -> Result<[u8; 32], Error> {
        let reply = self.client.get(self.endpoint("public-key")).send().await;
        self.reply(reply).await
    }

    /// The key's signature over the commissioning instance on the chip whose
    /// CHIPID is `chip_id`. The instance holds:
    /// - `board`
    /// - `manufacturer`
    /// - `date`
    ///
    /// `chip_id` holds rows `0x000`–`0x003` read with ECC. Row `0x000` comes
    /// first. `date` is the UTC commissioning date as `YYYYMMDD`.
    ///
    /// The server makes the message itself. The key's ID is the instance's
    /// signer. The caller checks the signature against the message it made.
    ///
    /// The server records the signature before it replies.
    pub async fn sign(
        &self,
        chip_id: [u16; 4],
        board: Board,
        manufacturer: &str,
        date: &str,
    ) -> Result<[u8; 64], Error> {
        self.signature(chip_id, board, manufacturer, date, false)
            .await
    }

    /// The signature [`sign`](Self::sign) returns for the same values, without
    /// the server publicly recording it. A server without the dry-run option
    /// refuses the request.
    pub async fn sign_dry_run(
        &self,
        chip_id: [u16; 4],
        board: Board,
        manufacturer: &str,
        date: &str,
    ) -> Result<[u8; 64], Error> {
        self.signature(chip_id, board, manufacturer, date, true)
            .await
    }

    /// The key's signature over the instance. `dry_run` asks the server not to
    /// publicly record it.
    async fn signature(
        &self,
        chip_id: [u16; 4],
        board: Board,
        manufacturer: &str,
        date: &str,
        dry_run: bool,
    ) -> Result<[u8; 64], Error> {
        let request = SignRequest {
            chip_id: format_chip_id(chip_id),
            board: board.name(),
            manufacturer,
            date,
            dry_run,
        };
        let reply = self
            .client
            .post(self.endpoint("sign"))
            .bearer_auth(&self.pin)
            .json(&request)
            .send()
            .await;
        self.reply(reply).await
    }

    /// The URL of the key's `endpoint`.
    fn endpoint(&self, endpoint: &str) -> String {
        format!("{}/{endpoint}", self.url)
    }

    /// The `N` bytes the server replied with.
    async fn reply<const N: usize>(
        &self,
        reply: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<[u8; N], Error> {
        let unreachable = |e: reqwest::Error| {
            debug!(
                "Couldn't reach the signing server at {}: {}",
                self.url,
                chain(&e)
            );
            Error::SigningServerUnreachable(self.url.clone())
        };
        let reply = reply.map_err(unreachable)?;
        let status = reply.status().as_u16();
        let body = reply.bytes().await.map_err(unreachable)?;
        decode_reply(&self.url, status, &body)
    }
}

/// The URL of key `id` on the signing server at `address`, such as
/// `https://HOST/v1/1` for key 1 at `https://HOST`. The address may end with a
/// path, and a slash ending it is ignored.
pub fn key_url(address: &str, id: u16) -> String {
    format!("{}/v1/{id}", address.trim_end_matches('/'))
}

/// `error` followed by each of its sources, separated by colons.
fn chain(error: &dyn std::error::Error) -> String {
    std::iter::successors(Some(error), |e| e.source())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

/// Whether `url` has the https scheme. A scheme is case-insensitive.
pub fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

/// The `N` bytes of a reply from the key at `url` with HTTP status `status`.
///
/// An error status has a one-line text body saying why. The error carries it.
fn decode_reply<const N: usize>(url: &str, status: u16, body: &[u8]) -> Result<[u8; N], Error> {
    if !(200..300).contains(&status) {
        let message = escape_controls(String::from_utf8_lossy(body).trim());
        return Err(Error::SigningServer {
            url: url.to_string(),
            status,
            message,
        });
    }
    body.try_into().map_err(|_| Error::SigningServerReply {
        url: url.to_string(),
        len: body.len(),
        expected: N,
    })
}

/// An Ed25519 private key read from a file.
pub struct KeyFile {
    key: SigningKey,
}

/// The text of the key file at `path`.
fn read_text(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|e| Error::io(path, e))
}

/// RFC 7468's label for an unencrypted PKCS#8 private key.
const PRIVATE_KEY_LABEL: &str = "PRIVATE KEY";

/// RFC 7468's label for an encrypted PKCS#8 private key.
const ENCRYPTED_PRIVATE_KEY_LABEL: &str = "ENCRYPTED PRIVATE KEY";

impl KeyFile {
    /// Whether the PKCS#8 PEM file at `path` holds an encrypted private key.
    pub fn is_encrypted(path: &Path) -> Result<bool, Error> {
        let text = read_text(path)?;
        match SecretDocument::from_pem(&text) {
            Ok((PRIVATE_KEY_LABEL, _)) => Ok(false),
            Ok((ENCRYPTED_PRIVATE_KEY_LABEL, _)) => Ok(true),
            Ok(_) | Err(_) => Err(Error::KeyFileNotPkcs8(path.display().to_string())),
        }
    }

    /// Reads a PKCS#8 PEM Ed25519 private key from `path`. `pin` decrypts an
    /// encrypted key. An unencrypted key refuses one.
    ///
    /// `openssl genpkey -algorithm ed25519` writes an unencrypted key.
    pub fn read(path: &Path, pin: Option<&str>) -> Result<Self, Error> {
        let text = read_text(path)?;
        let name = path.display().to_string();
        let (label, document) =
            SecretDocument::from_pem(&text).map_err(|_| Error::KeyFileNotPkcs8(name.clone()))?;
        match (label, pin) {
            (PRIVATE_KEY_LABEL, None) => Self::from_der(document.as_bytes(), &name),
            (PRIVATE_KEY_LABEL, Some(_)) => Err(Error::InvalidArgument(
                "--pin".to_string(),
                format!("Key file {name} isn't encrypted"),
            )),
            (ENCRYPTED_PRIVATE_KEY_LABEL, Some(pin)) => Self::decrypt(&document, pin, &name),
            (ENCRYPTED_PRIVATE_KEY_LABEL, None) => Err(Error::KeyFileEncrypted(name)),
            _ => Err(Error::KeyFileNotPkcs8(name)),
        }
    }

    /// The key in `document`, an encrypted PKCS#8 key, decrypted with `pin`.
    /// `name` is its file's.
    fn decrypt(document: &SecretDocument, pin: &str, name: &str) -> Result<Self, Error> {
        let unsupported = || Error::KeyFileEncryptionUnsupported(name.to_string());
        // The pkcs8 crate refuses to parse some encryption it doesn't support,
        // such as PBES2 with triple DES.
        let info =
            EncryptedPrivateKeyInfoRef::try_from(document.as_bytes()).map_err(|_| unsupported())?;
        let decrypted = info.decrypt(pin).map_err(|e| {
            let supported = !matches!(
                e,
                pkcs8::Error::EncryptedPrivateKey(
                    pkcs5::Error::UnsupportedAlgorithm { .. }
                        | pkcs5::Error::AlgorithmParametersInvalid { .. }
                        | pkcs5::Error::NoPbes1CryptSupport
                )
            );
            if supported {
                Error::KeyFileWrongPin(name.to_string())
            } else {
                unsupported()
            }
        })?;
        let key = Self::from_der(decrypted.as_bytes(), name);
        // A wrong PIN can decrypt to bytes that aren't a key.
        if let Err(Error::KeyFileNotPkcs8(_)) = key {
            return Err(Error::KeyFileWrongPin(name.to_string()));
        }
        key
    }

    /// The Ed25519 key in `der`, an unencrypted PKCS#8 key. `name` is its
    /// file's.
    fn from_der(der: &[u8], name: &str) -> Result<Self, Error> {
        let not_pkcs8 = |_| Error::KeyFileNotPkcs8(name.to_string());
        let info = PrivateKeyInfoRef::try_from(der).map_err(not_pkcs8)?;
        if info.algorithm.oid != ALGORITHM_OID {
            return Err(Error::KeyFileNotEd25519(name.to_string()));
        }
        let key = SigningKey::try_from(info).map_err(not_pkcs8)?;
        Ok(Self { key })
    }

    /// The key's 32-byte Ed25519 public key.
    pub fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    /// The key's Ed25519 signature over `message`.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.key.sign(message).to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ed25519_dalek::pkcs8::EncodePrivateKey;
    use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;

    /// CHIPID from rows 0x000–0x003 of an RP2350 A4.
    const CHIP_ID: [u16; 4] = [0x5b6b, 0x2f65, 0x9c23, 0xde3f];

    const URL: &str = "https://example.invalid/v1/1";

    const ADDRESS: &str = "https://example.invalid";

    /// From `openssl genpkey -algorithm ed25519 | openssl pkcs8 -topk8
    /// -scrypt -passout pass:test`.
    const ENCRYPTED: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----
MIGbMFcGCSqGSIb3DQEFDTBKMCkGCSsGAQQB2kcECzAcBBCygZFSouxgAoz03h2W
pWvSAgJAAAIBCAIBATAdBglghkgBZQMEASoEEI5DlJCcGZ+k1qjMlzDf9qIEQESu
mDfuv/uA/EVN4DQZajeb6QaS3jYuKexSVklfDT0DMynvVcfZ5BTWXojk99Ph/N2o
Mx/cjBQLObAI6LuJncI=
-----END ENCRYPTED PRIVATE KEY-----
";

    /// From `openssl genpkey -algorithm ed25519 -aes-256-cbc -pass pass:test`,
    /// which uses PBKDF2.
    const AES_256_CBC: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----
MIGjMF8GCSqGSIb3DQEFDTBSMDEGCSqGSIb3DQEFDDAkBBAYnOcgR1/91c4OSvzz
/bdOAgIIADAMBggqhkiG9w0CCQUAMB0GCWCGSAFlAwQBKgQQtYOVsGxrdHxsT8Me
XPRAWARA3o3b/1JPnVc11wD+BdUnwbRCNGCH7xW0cvBmbb3aZ6Uo6osUriPXuxOe
cW6PWdA8Ys1DPfqADJZEWn5jjTMisw==
-----END ENCRYPTED PRIVATE KEY-----
";

    /// From `openssl genpkey -algorithm ed25519 | openssl pkcs8 -topk8 -v1
    /// PBE-MD5-DES -passout pass:test -provider legacy -provider default`.
    const PBES1: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----
MFcwGwYJKoZIhvcNAQUDMA4ECDOcgymcx8QvAgIIAAQ4Gf4s+PWJj+uviCpH2fEf
elx0zT7/O11DuJzzHIqInF2PhTr4nNIWsbXT7oe3ZQj5Li678VcAnxo=
-----END ENCRYPTED PRIVATE KEY-----
";

    /// From `openssl genpkey -algorithm ed25519 | openssl pkcs8 -topk8 -v2
    /// des3 -passout pass:test`.
    const TRIPLE_DES: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----
MIGSMFYGCSqGSIb3DQEFDTBJMDEGCSqGSIb3DQEFDDAkBBB2dNHHr2Ce+tl3Hu8+
9eJWAgIIADAMBggqhkiG9w0CCQUAMBQGCCqGSIb3DQMHBAjAIWHxO0J6eAQ4u33P
M/wjG6bMtiF0yPF8rwQbaldBNoEeFSm5tDFALO6pcxpafKTWdXXb1Hdlfk8lV2oX
W0G87dU=
-----END ENCRYPTED PRIVATE KEY-----
";

    /// From `openssl genpkey -algorithm x25519`.
    const X25519: &str = "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VuBCIEINDV7XQ6z4iHvwHgUk9TIoZT4A/thmf2z0Agx0bZyelf
-----END PRIVATE KEY-----
";

    /// From `openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256`.
    const P256: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgSdnk/0fIpjdrJrqr
SlmbDWYPwkDuw0eu2dVahTAFkDahRANCAAQygyWZl7meGBtGp6jwBmgf/YixAvEq
iMcga2y1OrfxMbb+71DBh9AOyszeZljXjWv/ywweOfVl3rsEdSIEHNPO
-----END PRIVATE KEY-----
";

    /// From `openssl ecparam -name prime256v1 -genkey -noout`. It holds the
    /// key in SEC1's form rather than PKCS#8's.
    const SEC1: &str = "-----BEGIN EC PRIVATE KEY-----
MHcCAQEEIGEIocVWPwj1BAlEJLcrTQoNgImnc+XOgFv8Bf6W8ao2oAoGCCqGSM49
AwEHoUQDQgAEMfwPYY2jNfIL20Jk9SIVBova4JSqtULVECJwAJQbpx+jFxFBTVRf
OdKR4pVGo/fYus95rivSHhST/Q5sFz6k0g==
-----END EC PRIVATE KEY-----
";

    fn key_file(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.pem");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    #[test]
    fn a_key_file_signs_with_its_key() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
        let (_dir, path) = key_file(&pem);
        assert!(!KeyFile::is_encrypted(&path).unwrap());
        let file = KeyFile::read(&path, None).unwrap();
        assert_eq!(file.public_key(), key.verifying_key().to_bytes());
        let signature = ed25519_dalek::Signature::from_bytes(&file.sign(b"message"));
        assert!(
            key.verifying_key()
                .verify_strict(b"message", &signature)
                .is_ok()
        );
    }

    /// Whether `file`'s key signs a message that its public key verifies.
    fn signs(file: &KeyFile) -> bool {
        let key = ed25519_dalek::VerifyingKey::from_bytes(&file.public_key()).unwrap();
        let signature = ed25519_dalek::Signature::from_bytes(&file.sign(b"message"));
        key.verify_strict(b"message", &signature).is_ok()
    }

    #[test]
    fn an_encrypted_key_file_is_read_with_its_pin() {
        for text in [ENCRYPTED, AES_256_CBC] {
            let (_dir, path) = key_file(text);
            assert!(KeyFile::is_encrypted(&path).unwrap());
            assert!(signs(&KeyFile::read(&path, Some("test")).unwrap()));
        }
    }

    #[test]
    fn an_encrypted_key_file_needs_its_pin() {
        for text in [ENCRYPTED, AES_256_CBC] {
            let (_dir, path) = key_file(text);
            let name = path.display().to_string();
            assert!(matches!(
                KeyFile::read(&path, None),
                Err(Error::KeyFileEncrypted(file)) if file == name
            ));
            let error = KeyFile::read(&path, Some("wrong")).err().unwrap();
            assert!(
                matches!(&error, Error::KeyFileWrongPin(file) if *file == name),
                "{error}"
            );
        }
    }

    #[test]
    fn an_unencrypted_key_file_refuses_a_pin() {
        let pem = SigningKey::from_bytes(&[7; 32])
            .to_pkcs8_pem(LineEnding::LF)
            .unwrap();
        let (_dir, path) = key_file(&pem);
        assert!(matches!(
            KeyFile::read(&path, Some("test")),
            Err(Error::InvalidArgument(..))
        ));
    }

    /// Encryption that the pkcs8 crate doesn't decrypt isn't reported as a
    /// wrong PIN.
    #[test]
    fn unsupported_encryption_is_refused() {
        for text in [PBES1, TRIPLE_DES] {
            let (_dir, path) = key_file(text);
            assert!(KeyFile::is_encrypted(&path).unwrap());
            let error = KeyFile::read(&path, Some("test")).err().unwrap();
            assert!(
                matches!(error, Error::KeyFileEncryptionUnsupported(_)),
                "{error}"
            );
        }
    }

    #[test]
    fn a_key_that_isnt_ed25519_is_refused() {
        for text in [X25519, P256] {
            let (_dir, path) = key_file(text);
            assert!(
                matches!(KeyFile::read(&path, None), Err(Error::KeyFileNotEd25519(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn a_file_that_isnt_a_pkcs8_private_key_is_refused() {
        for text in [SEC1, "not a key", ""] {
            let (_dir, path) = key_file(text);
            assert!(
                matches!(KeyFile::read(&path, None), Err(Error::KeyFileNotPkcs8(_))),
                "{text}"
            );
            assert!(
                matches!(KeyFile::is_encrypted(&path), Err(Error::KeyFileNotPkcs8(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn a_missing_key_file_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            KeyFile::read(&dir.path().join("missing.pem"), None),
            Err(Error::Io(_))
        ));
    }

    /// A request that records has the four fields every server accepts. A
    /// server refuses a body with a field it doesn't know.
    #[test]
    fn a_sign_request_has_the_servers_four_fields() {
        let request = |dry_run| SignRequest {
            chip_id: format_chip_id(CHIP_ID),
            board: "fire-24-f",
            manufacturer: "piers.rocks",
            date: "20260926",
            dry_run,
        };
        let mut expected = serde_json::json!({
            "chip_id": "DE3F9C232F655B6B",
            "board": "fire-24-f",
            "manufacturer": "piers.rocks",
            "date": "20260926",
        });
        assert_eq!(serde_json::to_value(request(false)).unwrap(), expected);

        expected["dry_run"] = true.into();
        assert_eq!(serde_json::to_value(request(true)).unwrap(), expected);
    }

    #[test]
    fn only_an_https_address_is_accepted() {
        assert!(SigningServer::new(ADDRESS, 1, "pin").is_ok());
        assert!(SigningServer::new("HTTPS://example.invalid", 1, "pin").is_ok());
        for address in ["http://example.invalid", "example.invalid", ""] {
            assert!(
                matches!(
                    SigningServer::new(address, 1, "pin"),
                    Err(Error::InvalidArgument(..))
                ),
                "{address}"
            );
        }
    }

    /// A key's URL is the server's address, the API version and the key's ID.
    #[test]
    fn a_keys_url_follows_the_servers_address() {
        assert_eq!(key_url(ADDRESS, 1), URL);
        assert_eq!(key_url("https://example.invalid/", 1), URL);
        assert_eq!(
            key_url("https://example.invalid:8443/sign/", 65535),
            "https://example.invalid:8443/sign/v1/65535"
        );
        // The address keeps its case.
        assert_eq!(
            key_url("HTTPS://Example.invalid", 2),
            "HTTPS://Example.invalid/v1/2"
        );
    }

    #[test]
    fn an_endpoint_follows_the_keys_url() {
        let server = SigningServer::new(ADDRESS, 1, "pin").unwrap();
        assert_eq!(server.endpoint("sign"), "https://example.invalid/v1/1/sign");
        let server = SigningServer::new("https://example.invalid/", 2, "pin").unwrap();
        assert_eq!(
            server.endpoint("public-key"),
            "https://example.invalid/v1/2/public-key"
        );
    }

    #[test]
    fn a_reply_of_the_right_length_is_accepted() {
        assert_eq!(decode_reply::<32>(URL, 200, &[7; 32]).unwrap(), [7; 32]);
        assert_eq!(decode_reply::<64>(URL, 200, &[9; 64]).unwrap(), [9; 64]);
    }

    #[test]
    fn a_reply_of_the_wrong_length_is_refused() {
        for (len, expected) in [(0, 32), (31, 32), (33, 32), (63, 64), (65, 64)] {
            let body = vec![0; len];
            let error = match expected {
                32 => decode_reply::<32>(URL, 200, &body).map(|_| ()),
                _ => decode_reply::<64>(URL, 200, &body).map(|_| ()),
            }
            .unwrap_err();
            assert!(
                matches!(
                    error,
                    Error::SigningServerReply { len: l, expected: e, .. } if l == len && e == expected
                ),
                "{error}"
            );
        }
    }

    #[test]
    fn each_error_status_is_reported_with_the_servers_message() {
        for status in [400, 401, 404, 503, 500] {
            let error = decode_reply::<64>(URL, status, b"the server's reason\n").unwrap_err();
            let Error::SigningServer {
                url,
                status: s,
                message,
            } = error
            else {
                panic!("{status}: {error}");
            };
            assert_eq!(url, URL);
            assert_eq!(s, status);
            assert_eq!(message, "the server's reason");
        }
    }

    #[test]
    fn an_error_message_has_its_control_characters_escaped() {
        let error = decode_reply::<32>(URL, 400, b"bad\x1b[2J value").unwrap_err();
        assert!(
            matches!(&error, Error::SigningServer { message, .. } if message == "bad\\u{1b}[2J value"),
            "{error}"
        );
    }
}
