// tests/server.rs
//
// Tests for the signing server, with a local bare repository standing in for
// GitHub.
//
// The fixtures were generated with OpenSSL 3 as README.md describes:
//
//   openssl genpkey -algorithm ed25519 |
//       openssl pkcs8 -topk8 -scrypt -passout 'pass:test PIN' -out key-1.pem
//   openssl pkey -in key-1.pem -passin 'pass:test PIN' -pubout -out public-1.pem
//
// - key-2.pem was generated the same way. public-2.pem is another key's.
// - unencrypted.pem is from `openssl genpkey -algorithm ed25519`.
// - tls-cert.pem is a self-signed certificate for 127.0.0.1. tls-key.pem is its
//   key.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use ed25519_dalek::{Signature, VerifyingKey};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header::AUTHORIZATION;
use hyper::{Method, Request, StatusCode};
use onerom_config::hw::Board;
use onerom_metadata::otp::{CommissioningValues, RecordLine};
use onerom_signing_server::http::{Server, serve};
use onerom_signing_server::keys::{self, Keys};
use onerom_signing_server::record::Record;
use onerom_signing_server::tls;
use pkcs8::DecodePublicKey;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

const PIN: &str = "test PIN";

/// CHIPID from rows 0x000–0x003 of an RP2350 A4.
const CHIP_ID: [u16; 4] = [0x5b6b, 0x2f65, 0x9c23, 0xde3f];

/// A request to sign fire-24-f's values for [`CHIP_ID`].
const REQUEST: &str = r#"{"chip_id": "DE3F9C232F655B6B", "board": "fire-24-f", "manufacturer": "piers.rocks", "date": "20260926"}"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn public_key(id: u16) -> VerifyingKey {
    let pem = fs::read_to_string(fixture(&format!("public-{id}.pem"))).unwrap();
    VerifyingKey::from_public_key_pem(&pem).unwrap()
}

/// Runs git in `dir` and returns its stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Clones `dir`'s `remote.git` to `dir/name` with a test commit identity.
fn clone(dir: &Path, name: &str) -> PathBuf {
    git(dir, &["clone", "--quiet", "remote.git", name]);
    let clone = dir.join(name);
    git(&clone, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&clone, &["config", "user.name", "Test"]);
    git(&clone, &["config", "user.email", "test@example.invalid"]);
    git(&clone, &["config", "commit.gpgsign", "false"]);
    clone
}

/// Copies `key` and `public` into `keys/name`.
fn add_key(keys: &Path, name: &str, key: &str, public: &str) {
    let dir = keys.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::copy(fixture(key), dir.join("key.pem")).unwrap();
    fs::copy(fixture(public), dir.join("public.pem")).unwrap();
}

/// A server with keys 1 and 2. Its record's remote is `remote.git` in `dir`.
struct Setup {
    dir: TempDir,
    server: Server,
}

impl Setup {
    async fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let keys = dir.path().join("keys");
        add_key(&keys, "1", "key-1.pem", "public-1.pem");
        add_key(&keys, "2", "key-2.pem", "public-2.pem");

        git(
            dir.path(),
            &[
                "init",
                "--quiet",
                "--bare",
                "--initial-branch",
                "main",
                "remote.git",
            ],
        );
        let record = clone(dir.path(), "record");
        fs::write(record.join("README.md"), "Signatures\n").unwrap();
        git(&record, &["add", "README.md"]);
        git(&record, &["commit", "--quiet", "--message", "Start"]);
        git(
            &record,
            &["push", "--quiet", "--set-upstream", "origin", "main"],
        );

        let server = Server::new(
            Keys::load(&keys).unwrap(),
            Record::open(record).await.unwrap(),
        );
        Self { dir, server }
    }

    fn remote(&self) -> PathBuf {
        self.dir.path().join("remote.git")
    }

    /// Key `id`'s record file on the remote.
    fn record_file(&self, id: u16) -> Option<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.remote())
            .args(["show", &format!("main:signatures/{id}.txt")])
            .output()
            .unwrap();
        output
            .status
            .success()
            .then(|| String::from_utf8(output.stdout).unwrap())
    }

    /// The number of commits on the remote.
    fn commits(&self) -> usize {
        git(&self.remote(), &["rev-list", "--count", "main"])
            .trim()
            .parse()
            .unwrap()
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        authorization: Option<&str>,
        body: &str,
    ) -> (StatusCode, Vec<u8>) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(authorization) = authorization {
            request = request.header(AUTHORIZATION, authorization);
        }
        let request = request
            .body(Full::new(Bytes::from(body.to_owned())))
            .unwrap();
        let response = self
            .server
            .handle(request, "127.0.0.1:1".parse().unwrap())
            .await;
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, body.to_vec())
    }

    /// Requests key 1's signature of `body` with the correct PIN.
    async fn sign(&self, body: &str) -> (StatusCode, Vec<u8>) {
        self.send(
            Method::POST,
            "/v1/1/sign",
            Some(&format!("Bearer {PIN}")),
            body,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

#[test]
fn keys_load_from_their_id_directories_and_files_are_ignored() {
    let dir = TempDir::new().unwrap();
    add_key(dir.path(), "1", "key-1.pem", "public-1.pem");
    add_key(dir.path(), "65535", "key-1.pem", "public-1.pem");
    fs::write(dir.path().join("README"), "").unwrap();
    let keys = Keys::load(dir.path()).unwrap();
    assert_eq!(keys.ids().collect::<Vec<_>>(), [1, 65535]);
}

#[test]
fn a_directory_whose_name_isnt_a_key_id_is_refused() {
    for name in ["0", "01", "65536", "one"] {
        let dir = TempDir::new().unwrap();
        add_key(dir.path(), name, "key-1.pem", "public-1.pem");
        assert!(
            matches!(Keys::load(dir.path()), Err(keys::Error::BadId { .. })),
            "{name}"
        );
    }
}

#[test]
fn an_unencrypted_key_is_refused() {
    let dir = TempDir::new().unwrap();
    add_key(dir.path(), "1", "unencrypted.pem", "public-1.pem");
    assert!(matches!(
        Keys::load(dir.path()),
        Err(keys::Error::NotEncrypted { .. })
    ));
}

#[test]
fn a_directory_without_keys_is_refused() {
    let dir = TempDir::new().unwrap();
    assert!(matches!(
        Keys::load(dir.path()),
        Err(keys::Error::NoKeys { .. })
    ));
}

// ---------------------------------------------------------------------------
// The HTTP interface
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_public_key_is_its_32_bytes() {
    let setup = Setup::new().await;
    let (status, body) = setup.send(Method::GET, "/v1/1/public-key", None, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, public_key(1).as_bytes());
}

#[tokio::test]
async fn an_unknown_key_or_path_is_not_found() {
    let setup = Setup::new().await;
    for path in [
        "/v1/3/public-key",
        "/v1/0/public-key",
        "/v1/01/public-key",
        "/v2/1/public-key",
        "/v1/1/other",
        "/v1/1/public-key/",
        "/1/public-key",
        "/",
    ] {
        let (status, _) = setup.send(Method::GET, path, None, "").await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn a_wrong_method_is_refused() {
    let setup = Setup::new().await;
    let (status, _) = setup.send(Method::POST, "/v1/1/public-key", None, "").await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let (status, _) = setup.send(Method::GET, "/v1/1/sign", None, "").await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

/// A signature:
/// - covers the values' message, with the key's ID as the signer
/// - is recorded on the remote in a commit for the CHIPID
#[tokio::test]
async fn a_signature_is_recorded_and_returned() {
    let setup = Setup::new().await;
    let (status, body) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let signature: [u8; 64] = body.try_into().unwrap();
    let message = CommissioningValues::new(Board::Fire24F, "piers.rocks", "20260926", 1)
        .unwrap()
        .message(CHIP_ID);
    public_key(1)
        .verify_strict(&message, &Signature::from_bytes(&signature))
        .unwrap();
    assert_eq!(
        setup.record_file(1),
        Some(format!("{}\n", RecordLine::new(CHIP_ID, &signature)))
    );
    assert_eq!(
        git(&setup.remote(), &["log", "-1", "--format=%s", "main"]),
        "Key 1: DE3F9C232F655B6B\n"
    );
}

#[tokio::test]
async fn a_repeated_request_adds_nothing() {
    let setup = Setup::new().await;
    let (_, first) = setup.sign(REQUEST).await;
    let commits = setup.commits();
    let (status, second) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second, first);
    assert_eq!(setup.commits(), commits);
}

#[tokio::test]
async fn a_new_date_adds_a_second_line() {
    let setup = Setup::new().await;
    setup.sign(REQUEST).await;
    let (status, _) = setup.sign(&REQUEST.replace("20260926", "20260927")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(setup.record_file(1).unwrap().lines().count(), 2);
}

#[tokio::test]
async fn a_missing_or_wrong_pin_is_refused() {
    let setup = Setup::new().await;
    for authorization in [
        None,
        Some("Bearer not the PIN".to_owned()),
        Some(format!("Basic {PIN}")),
        Some("Bearer ".to_owned()),
    ] {
        let (status, _) = setup
            .send(
                Method::POST,
                "/v1/1/sign",
                authorization.as_deref(),
                REQUEST,
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{authorization:?}");
    }
    assert_eq!(setup.record_file(1), None);
}

#[tokio::test]
async fn a_bad_request_is_refused() {
    let setup = Setup::new().await;
    let bodies = [
        "not JSON".to_owned(),
        REQUEST.replace(r#", "date": "20260926""#, ""),
        REQUEST.replace(r#""date""#, r#""extra": "x", "date""#),
        REQUEST.replace("DE3F9C232F655B6B", "de3f9c232f655b6b"),
        REQUEST.replace("fire-24-f", "fire-99-z"),
        REQUEST.replace("20260926", "2026-09-26"),
        REQUEST.replace("piers.rocks", ""),
        REQUEST.replace("piers.rocks", &"x".repeat(1941)),
    ];
    for body in &bodies {
        let (status, response) = setup.sign(body).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{body}: {}",
            String::from_utf8_lossy(&response)
        );
    }
    let (status, _) = setup
        .sign(&REQUEST.replace("piers.rocks", &"x".repeat(20_000)))
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(setup.record_file(1), None);
}

#[tokio::test]
async fn a_key_whose_files_differ_is_refused() {
    let setup = Setup::new().await;
    let (status, _) = setup
        .send(
            Method::POST,
            "/v1/2/sign",
            Some(&format!("Bearer {PIN}")),
            REQUEST,
        )
        .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(setup.record_file(2), None);
}

/// The server serves the public key over TLS.
#[tokio::test]
async fn the_server_answers_over_tls() {
    let Setup { dir: _dir, server } = Setup::new().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let acceptor = tls::acceptor(&fixture("tls-cert.pem"), &fixture("tls-key.pem")).unwrap();
    tokio::spawn(serve(listener, acceptor, Arc::new(server)));

    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_file(fixture("tls-cert.pem")).unwrap())
        .unwrap();
    let config = ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let stream = TcpStream::connect(address).await.unwrap();
    let mut stream = TlsConnector::from(Arc::new(config))
        .connect(ServerName::try_from("127.0.0.1").unwrap(), stream)
        .await
        .unwrap();
    stream
        .write_all(b"GET /v1/1/public-key HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    assert!(response.ends_with(public_key(1).as_bytes()));
}

// ---------------------------------------------------------------------------
// The record
// ---------------------------------------------------------------------------

/// A push from another clone is fetched before the server adds a line.
#[tokio::test]
async fn the_record_follows_the_remote() {
    let setup = Setup::new().await;
    let other = clone(setup.dir.path(), "other");
    fs::write(other.join("README.md"), "Changed\n").unwrap();
    git(
        &other,
        &["commit", "--quiet", "--all", "--message", "Change"],
    );
    git(&other, &["push", "--quiet"]);
    let (status, _) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::OK);
    assert!(setup.record_file(1).is_some());
    assert_eq!(
        git(&setup.remote(), &["show", "main:README.md"]),
        "Changed\n"
    );
}

#[tokio::test]
async fn a_malformed_record_file_isnt_added_to() {
    let setup = Setup::new().await;
    let other = clone(setup.dir.path(), "other");
    fs::create_dir(other.join("signatures")).unwrap();
    fs::write(other.join("signatures/1.txt"), "not a record line\n").unwrap();
    git(&other, &["add", "signatures"]);
    git(&other, &["commit", "--quiet", "--message", "Break"]);
    git(&other, &["push", "--quiet"]);
    let commits = setup.commits();
    let (status, _) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(setup.commits(), commits);
}

/// When a push fails the signature isn't returned. The failed push doesn't
/// affect the next request.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_push_returns_no_signature() {
    use std::os::unix::fs::PermissionsExt;

    let setup = Setup::new().await;
    let hook = setup.remote().join("hooks/pre-receive");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let (status, _) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(setup.record_file(1), None);

    fs::remove_file(&hook).unwrap();
    let (status, _) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(setup.record_file(1).unwrap().lines().count(), 1);
}
