// tests/server.rs
//
// Tests for the signing server, with local bare repositories standing in for
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
use onerom_metadata::otp::{CommissioningValues, RecordLine, format_chip_id};
use onerom_signing_server::http::{Server, serve};
use onerom_signing_server::keys::{self, Keys};
use onerom_signing_server::record::{PrivateRecord, PublicRecord};
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

/// Clones `dir`'s `remote` to `dir/name` with a test commit identity.
fn clone(dir: &Path, remote: &str, name: &str) -> PathBuf {
    git(dir, &["clone", "--quiet", remote, name]);
    let clone = dir.join(name);
    git(&clone, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&clone, &["config", "user.name", "Test"]);
    git(&clone, &["config", "user.email", "test@example.invalid"]);
    git(&clone, &["config", "commit.gpgsign", "false"]);
    clone
}

/// Creates the bare repository `name.git` in `dir` with a first commit.
/// Returns its clone `dir/name`, whose branch tracks it.
fn record(dir: &Path, name: &str) -> PathBuf {
    let remote = format!("{name}.git");
    git(
        dir,
        &[
            "init",
            "--quiet",
            "--bare",
            "--initial-branch",
            "main",
            &remote,
        ],
    );
    let record = clone(dir, &remote, name);
    fs::write(record.join("README.md"), "Signatures\n").unwrap();
    git(&record, &["add", "README.md"]);
    git(&record, &["commit", "--quiet", "--message", "Start"]);
    git(
        &record,
        &["push", "--quiet", "--set-upstream", "origin", "main"],
    );
    record
}

/// Pushes a commit writing `text` to `file` to `dir`'s `remote` from another
/// clone.
fn push(dir: &Path, remote: &str, file: &str, text: &str) {
    let other = clone(dir, remote, "other");
    let path = other.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    git(&other, &["add", file]);
    git(&other, &["commit", "--quiet", "--message", "Change"]);
    git(&other, &["push", "--quiet"]);
    fs::remove_dir_all(other).unwrap();
}

/// Makes `remote` refuse pushes until the returned hook is removed.
#[cfg(unix)]
fn refuse_pushes(remote: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let hook = remote.join("hooks/pre-receive");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    hook
}

/// `file` on `remote`'s main branch.
fn show(remote: &Path, file: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(remote)
        .args(["show", &format!("main:{file}")])
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).unwrap())
}

/// The number of commits on `remote`.
fn commits(remote: &Path) -> usize {
    git(remote, &["rev-list", "--count", "main"])
        .trim()
        .parse()
        .unwrap()
}

/// `body` as a dry run.
fn dry_run(body: &str) -> String {
    body.replace('}', r#", "dry_run": true}"#)
}

/// The public record line for `signature`.
fn public_line(signature: &[u8]) -> String {
    RecordLine::new(signature.try_into().unwrap()).to_string()
}

/// The private record line for [`REQUEST`]'s values with the request type
/// `request` and `signature`.
fn private_line(request: &str, signature: &[u8]) -> String {
    let signature: String = signature.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        r#"{{"request":"{request}","chip_id":"DE3F9C232F655B6B","board":"fire-24-f","manufacturer":"piers.rocks","date":"20260926","signature":"{signature}"}}"#
    )
}

/// Copies `key` and `public` into `keys/name`.
fn add_key(keys: &Path, name: &str, key: &str, public: &str) {
    let dir = keys.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::copy(fixture(key), dir.join("key.pem")).unwrap();
    fs::copy(fixture(public), dir.join("public.pem")).unwrap();
}

/// A server with keys 1 and 2. Its public record's remote is `public.git` in
/// `dir` and its private record's is `private.git`.
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
        let public = record(dir.path(), "public");
        let private = record(dir.path(), "private");
        let server = Server::new(
            Keys::load(&keys).unwrap(),
            PublicRecord::open(public).await.unwrap(),
            PrivateRecord::open(private).await.unwrap(),
        );
        Self { dir, server }
    }

    fn public_remote(&self) -> PathBuf {
        self.dir.path().join("public.git")
    }

    fn private_remote(&self) -> PathBuf {
        self.dir.path().join("private.git")
    }

    /// Key `id`'s public record file on the remote.
    fn public_file(&self, id: u16) -> Option<String> {
        show(&self.public_remote(), &format!("signatures/{id}.txt"))
    }

    /// Key `id`'s private record file on the remote.
    fn private_file(&self, id: u16) -> Option<String> {
        show(&self.private_remote(), &format!("signatures/{id}.jsonl"))
    }

    /// Whether either remote has a file for key `id`.
    fn recorded(&self, id: u16) -> bool {
        self.public_file(id).is_some() || self.private_file(id).is_some()
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

/// A live request's signature:
/// - covers the values' message, with the key's ID as the signer
/// - is added to both records
#[tokio::test]
async fn a_live_request_adds_a_line_to_each_record() {
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
        setup.public_file(1),
        Some(format!("{}\n", public_line(&signature)))
    );
    assert_eq!(
        setup.private_file(1),
        Some(format!("{}\n", private_line("live", &signature)))
    );
}

/// The private record contains a board's canonical name whichever of its names
/// the request used.
#[tokio::test]
async fn the_private_record_contains_the_boards_canonical_name() {
    let setup = Setup::new().await;
    let (status, body) = setup
        .sign(&REQUEST.replace("fire-24-f", "fire-28-usb-a"))
        .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let line = private_line("live", &body).replace("fire-24-f", "fire-28-a");
    assert_eq!(setup.private_file(1), Some(format!("{line}\n")));
}

#[tokio::test]
async fn the_public_commit_message_doesnt_contain_the_chip_id() {
    let setup = Setup::new().await;
    let before = commits(&setup.public_remote());
    let (status, _) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(commits(&setup.public_remote()), before + 1);
    let message = git(
        &setup.public_remote(),
        &["log", "-1", "--format=%B", "main"],
    );
    assert!(!message.contains(&format_chip_id(CHIP_ID)), "{message}");
}

#[tokio::test]
async fn a_repeated_request_adds_nothing() {
    for body in [REQUEST.to_owned(), dry_run(REQUEST)] {
        let setup = Setup::new().await;
        let (_, first) = setup.sign(&body).await;
        let public = commits(&setup.public_remote());
        let private = commits(&setup.private_remote());
        let (status, second) = setup.sign(&body).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(second, first, "{body}");
        assert_eq!(commits(&setup.public_remote()), public, "{body}");
        assert_eq!(commits(&setup.private_remote()), private, "{body}");
    }
}

#[tokio::test]
async fn a_dry_run_adds_only_a_private_line() {
    let setup = Setup::new().await;
    let public = commits(&setup.public_remote());
    let (status, signature) = setup.sign(&dry_run(REQUEST)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&signature)
    );
    assert_eq!(
        setup.private_file(1),
        Some(format!("{}\n", private_line("dry-run", &signature)))
    );
    assert_eq!(setup.public_file(1), None);
    assert_eq!(commits(&setup.public_remote()), public);
}

/// A dry run returns the same signature as a live request. Its private line
/// and the live request's are different lines.
#[tokio::test]
async fn a_dry_run_then_a_live_request_adds_two_private_lines_and_one_public() {
    let setup = Setup::new().await;
    let (_, dry) = setup.sign(&dry_run(REQUEST)).await;
    let (status, live) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dry, live);
    assert_eq!(
        setup.private_file(1),
        Some(format!(
            "{}\n{}\n",
            private_line("dry-run", &live),
            private_line("live", &live)
        ))
    );
    assert_eq!(
        setup.public_file(1),
        Some(format!("{}\n", public_line(&live)))
    );
}

#[tokio::test]
async fn a_new_date_adds_a_second_line() {
    let setup = Setup::new().await;
    setup.sign(REQUEST).await;
    let (status, _) = setup.sign(&REQUEST.replace("20260926", "20260927")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(setup.public_file(1).unwrap().lines().count(), 2);
    assert_eq!(setup.private_file(1).unwrap().lines().count(), 2);
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
    assert!(!setup.recorded(1));
}

#[tokio::test]
async fn a_bad_request_is_refused() {
    let setup = Setup::new().await;
    let bodies = [
        "not JSON".to_owned(),
        REQUEST.replace(r#", "date": "20260926""#, ""),
        REQUEST.replace(r#""date""#, r#""extra": "x", "date""#),
        REQUEST.replace('}', r#", "dry_run": "yes"}"#),
        REQUEST.replace("DE3F9C232F655B6B", "de3f9c232f655b6b"),
        REQUEST.replace("fire-24-f", "fire-99-z"),
        REQUEST.replace("20260926", "2026-09-26"),
        REQUEST.replace("piers.rocks", ""),
        REQUEST.replace("piers.rocks", " piers.rocks"),
        REQUEST.replace("piers.rocks", "piers.rocks "),
        REQUEST.replace("piers.rocks", "piers*rocks"),
        REQUEST.replace("piers.rocks", "Café"),
        REQUEST.replace("piers.rocks", r"piers\trocks"),
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
    assert!(!setup.recorded(1));
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
    assert!(!setup.recorded(2));
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
// The records
// ---------------------------------------------------------------------------

/// A push from another clone is fetched before the server adds a line.
#[tokio::test]
async fn each_record_follows_its_remote() {
    let setup = Setup::new().await;
    for remote in ["public.git", "private.git"] {
        push(setup.dir.path(), remote, "README.md", "Changed\n");
    }
    let (status, _) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::OK);
    assert!(setup.public_file(1).is_some());
    assert!(setup.private_file(1).is_some());
    for remote in [setup.public_remote(), setup.private_remote()] {
        assert_eq!(show(&remote, "README.md").as_deref(), Some("Changed\n"));
    }
}

#[tokio::test]
async fn a_malformed_public_file_isnt_added_to() {
    let setup = Setup::new().await;
    push(
        setup.dir.path(),
        "public.git",
        "signatures/1.txt",
        "not a record line\n",
    );
    let before = commits(&setup.public_remote());
    let (status, _) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(commits(&setup.public_remote()), before);
}

/// Past the first, each bad line is a private line with a value changed or a
/// field added.
#[tokio::test]
async fn a_malformed_private_file_isnt_added_to() {
    let line = private_line("live", &[0; 64]);
    for bad in [
        "not a record line".to_owned(),
        line.replace("live", "maybe"),
        line.replace("DE3F9C232F655B6B", "de3f9c232f655b6b"),
        line.replace("fire-24-f", "fire-99-z"),
        line.replace(&"00".repeat(64), &"00".repeat(63)),
        line.replace('}', r#","extra":"x"}"#),
    ] {
        let setup = Setup::new().await;
        push(
            setup.dir.path(),
            "private.git",
            "signatures/1.jsonl",
            &format!("{bad}\n"),
        );
        let before = commits(&setup.private_remote());
        let (status, _) = setup.sign(REQUEST).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{bad}");
        assert_eq!(commits(&setup.private_remote()), before, "{bad}");
        assert_eq!(setup.public_file(1), None, "{bad}");
    }
}

/// When the public push fails the signature isn't returned and the private
/// live line stays. A retry adds the public line and not a second private
/// line.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_public_push_leaves_the_private_line() {
    let setup = Setup::new().await;
    let hook = refuse_pushes(&setup.public_remote());
    let (status, _) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(setup.public_file(1), None);
    let private = setup.private_file(1);

    fs::remove_file(&hook).unwrap();
    let (status, signature) = setup.sign(REQUEST).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        private,
        Some(format!("{}\n", private_line("live", &signature)))
    );
    assert_eq!(setup.private_file(1), private);
    assert_eq!(
        setup.public_file(1),
        Some(format!("{}\n", public_line(&signature)))
    );
}

/// When the private push fails the signature isn't returned and the public
/// record isn't written, dry run or not.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_private_push_writes_nothing_public() {
    let setup = Setup::new().await;
    refuse_pushes(&setup.private_remote());
    let before = commits(&setup.public_remote());
    for body in [REQUEST.to_owned(), dry_run(REQUEST)] {
        let (status, _) = setup.sign(&body).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    }
    assert_eq!(commits(&setup.public_remote()), before);
    assert!(!setup.recorded(1));
}
