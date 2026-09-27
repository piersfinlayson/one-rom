// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The two ways `hardware commission` signs a commissioning instance:
//! - [`SigningServer`] for a key on a signing server
//! - [`KeyFile`] for a private key in a file

use std::path::Path;
use std::time::Duration;

use ed25519_dalek::pkcs8::{ALGORITHM_OID, PrivateKeyInfoRef, SecretDocument};
use ed25519_dalek::{Signer as _, SigningKey};
use onerom_config::hw::Board;
use onerom_metadata::otp::format_chip_id;
use serde::Serialize;

use crate::Error;
use crate::otp::escape_controls;

/// The longest a request to a signing server may take. The server records a
/// signature in a git repository on GitHub before returning it.
const TIMEOUT: Duration = Duration::from_secs(60);

/// A key on a signing server.
///
/// The key's URL is the server's address followed by the API version and the
/// key's ID. Key 1's URL on a server at HOST is `https://HOST/v1/1`. The client
/// appends `/public-key` or `/sign` to it.
pub struct SigningServer {
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
}

impl SigningServer {
    /// The key at `url`. `pin` unlocks it. Refuses a URL that isn't https.
    pub fn new(url: &str, pin: &str) -> Result<Self, Error> {
        if !is_https(url) {
            return Err(Error::InvalidArgument(
                "--signer".to_string(),
                format!("'{url}' isn't an https URL"),
            ));
        }
        let client = reqwest::Client::builder()
            .https_only(true)
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| Error::Network(url.to_string(), e.to_string()))?;
        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            pin: pin.to_string(),
            client,
        })
    }

    /// The key's 32-byte Ed25519 public key.
    pub async fn public_key(&self) -> Result<[u8; 32], Error> {
        let url = self.endpoint("public-key");
        let reply = self.client.get(&url).send().await;
        self.reply(&url, reply).await
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
    pub async fn sign(
        &self,
        chip_id: [u16; 4],
        board: Board,
        manufacturer: &str,
        date: &str,
    ) -> Result<[u8; 64], Error> {
        let url = self.endpoint("sign");
        let request = SignRequest {
            chip_id: format_chip_id(chip_id),
            board: board.name(),
            manufacturer,
            date,
        };
        let reply = self
            .client
            .post(&url)
            .bearer_auth(&self.pin)
            .json(&request)
            .send()
            .await;
        self.reply(&url, reply).await
    }

    /// The URL of the key's `endpoint`.
    fn endpoint(&self, endpoint: &str) -> String {
        format!("{}/{endpoint}", self.url)
    }

    /// The `N` bytes the server replied with.
    async fn reply<const N: usize>(
        &self,
        url: &str,
        reply: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<[u8; N], Error> {
        let network = |e: reqwest::Error| Error::Network(url.to_string(), e.to_string());
        let reply = reply.map_err(network)?;
        let status = reply.status().as_u16();
        let body = reply.bytes().await.map_err(network)?;
        decode_reply(url, status, &body)
    }
}

/// Whether `url` has the https scheme. A scheme is case-insensitive.
fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

/// The `N` bytes of a reply from `url` with HTTP status `status`.
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

/// RFC 7468's label for an unencrypted PKCS#8 private key.
const PRIVATE_KEY_LABEL: &str = "PRIVATE KEY";

/// RFC 7468's label for an encrypted PKCS#8 private key.
const ENCRYPTED_PRIVATE_KEY_LABEL: &str = "ENCRYPTED PRIVATE KEY";

impl KeyFile {
    /// Reads an unencrypted PKCS#8 PEM Ed25519 private key from `path`.
    /// `openssl genpkey -algorithm ed25519` writes one.
    pub fn read(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        let name = || path.display().to_string();
        let (label, document) =
            SecretDocument::from_pem(&text).map_err(|_| Error::KeyFileNotPkcs8(name()))?;
        match label {
            PRIVATE_KEY_LABEL => {}
            ENCRYPTED_PRIVATE_KEY_LABEL => return Err(Error::KeyFileEncrypted(name())),
            _ => return Err(Error::KeyFileNotPkcs8(name())),
        }
        let info = PrivateKeyInfoRef::try_from(document.as_bytes())
            .map_err(|_| Error::KeyFileNotPkcs8(name()))?;
        if info.algorithm.oid != ALGORITHM_OID {
            return Err(Error::KeyFileNotEd25519(name()));
        }
        let key = SigningKey::try_from(info).map_err(|_| Error::KeyFileNotPkcs8(name()))?;
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

    const URL: &str = "https://example.invalid/v1/1/sign";

    /// From `openssl genpkey -algorithm ed25519 | openssl pkcs8 -topk8
    /// -scrypt -passout pass:test`.
    const ENCRYPTED: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----
MIGbMFcGCSqGSIb3DQEFDTBKMCkGCSsGAQQB2kcECzAcBBCygZFSouxgAoz03h2W
pWvSAgJAAAIBCAIBATAdBglghkgBZQMEASoEEI5DlJCcGZ+k1qjMlzDf9qIEQESu
mDfuv/uA/EVN4DQZajeb6QaS3jYuKexSVklfDT0DMynvVcfZ5BTWXojk99Ph/N2o
Mx/cjBQLObAI6LuJncI=
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
        let file = KeyFile::read(&path).unwrap();
        assert_eq!(file.public_key(), key.verifying_key().to_bytes());
        let signature = ed25519_dalek::Signature::from_bytes(&file.sign(b"message"));
        assert!(
            key.verifying_key()
                .verify_strict(b"message", &signature)
                .is_ok()
        );
    }

    #[test]
    fn an_encrypted_key_file_is_refused() {
        let (_dir, path) = key_file(ENCRYPTED);
        assert!(matches!(
            KeyFile::read(&path),
            Err(Error::KeyFileEncrypted(_))
        ));
    }

    #[test]
    fn a_key_that_isnt_ed25519_is_refused() {
        for text in [X25519, P256] {
            let (_dir, path) = key_file(text);
            assert!(
                matches!(KeyFile::read(&path), Err(Error::KeyFileNotEd25519(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn a_file_that_isnt_a_pkcs8_private_key_is_refused() {
        for text in [SEC1, "not a key", ""] {
            let (_dir, path) = key_file(text);
            assert!(
                matches!(KeyFile::read(&path), Err(Error::KeyFileNotPkcs8(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn a_missing_key_file_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            KeyFile::read(&dir.path().join("missing.pem")),
            Err(Error::Io(_))
        ));
    }

    /// The server refuses a body with any other field.
    #[test]
    fn a_sign_request_has_the_servers_four_fields() {
        let request = SignRequest {
            chip_id: format_chip_id(CHIP_ID),
            board: "fire-24-f",
            manufacturer: "piers.rocks",
            date: "20260926",
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "chip_id": "DE3F9C232F655B6B",
                "board": "fire-24-f",
                "manufacturer": "piers.rocks",
                "date": "20260926",
            })
        );
    }

    #[test]
    fn only_an_https_url_is_accepted() {
        assert!(SigningServer::new("https://example.invalid/v1/1", "pin").is_ok());
        assert!(SigningServer::new("HTTPS://example.invalid/v1/1", "pin").is_ok());
        for url in ["http://example.invalid/v1/1", "example.invalid/v1/1", ""] {
            assert!(
                matches!(
                    SigningServer::new(url, "pin"),
                    Err(Error::InvalidArgument(..))
                ),
                "{url}"
            );
        }
    }

    #[test]
    fn an_endpoint_follows_the_keys_url() {
        let server = SigningServer::new("https://example.invalid/v1/1", "pin").unwrap();
        assert_eq!(server.endpoint("sign"), "https://example.invalid/v1/1/sign");
        let server = SigningServer::new("https://example.invalid/v1/1/", "pin").unwrap();
        assert_eq!(
            server.endpoint("public-key"),
            "https://example.invalid/v1/1/public-key"
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
        for (status, sentence) in [
            (400, "refused the request"),
            (401, "refused the PIN"),
            (404, "doesn't have a key at that URL"),
            (503, "couldn't record the signature"),
            (500, "HTTP status 500"),
        ] {
            let error = decode_reply::<64>(URL, status, b"the server's reason\n").unwrap_err();
            let Error::SigningServer {
                status: s,
                ref message,
                ..
            } = error
            else {
                panic!("{status}: {error}");
            };
            assert_eq!(s, status);
            assert_eq!(message, "the server's reason");
            let text = error.to_string();
            assert!(text.contains(sentence), "{status}: {text}");
            assert!(text.contains(URL), "{status}: {text}");
            assert!(text.contains("the server's reason"), "{status}: {text}");
        }
    }

    #[test]
    fn an_error_message_has_its_control_characters_escaped() {
        let error = decode_reply::<32>(URL, 400, b"bad\x1b[2J value").unwrap_err();
        assert!(error.to_string().contains("bad\\u{1b}[2J value"), "{error}");
    }
}
