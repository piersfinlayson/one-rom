// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

//! The server's HTTP interface.
//!
//! A key's URL is `https://HOST:PORT/v1/ID`, where `ID` is the key's ID.
//! - `GET <key URL>/public-key` returns the key's 32-byte public key.
//! - `POST <key URL>/sign` returns the 64-byte signature of a commissioning
//!   instance once it's recorded. The PIN is provided in `Authorization:
//!   Bearer PIN`. The instance's values are provided as JSON in the request
//!   body.

use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt::Display;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::body::{Body, Bytes};
use hyper::header::{ALLOW, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, WWW_AUTHENTICATE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use log::{debug, error, info, warn};
use onerom_config::hw::Board;
use onerom_metadata::otp::{BuildError, CommissioningValues, parse_chip_id};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};
use tokio_rustls::TlsAcceptor;

use crate::keys::{Keys, SignError, parse_id};
use crate::record::{Added, Record};

/// The largest `/sign` request body. A valid one is under 2KB.
const MAX_BODY: usize = 16 * 1024;

/// The longest a client may take over its TLS handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest a client may take to send a request's headers.
const HEADER_TIMEOUT: Duration = Duration::from_secs(30);

/// The body of a `/sign` request.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignRequest {
    /// The chip's CHIPID, as `format_chip_id` writes it.
    chip_id: String,
    board: String,
    manufacturer: String,
    /// The UTC commissioning date, `YYYYMMDD`.
    date: String,
}

/// The signing server.
pub struct Server {
    keys: Keys,
    record: Record,
    /// Held while a signature is made and recorded so requests are handled one
    /// at a time.
    signing: Mutex<()>,
}

impl Server {
    pub fn new(keys: Keys, record: Record) -> Self {
        Self {
            keys,
            record,
            signing: Mutex::new(()),
        }
    }

    /// The response to `request` from `peer`.
    pub async fn handle<B>(&self, request: Request<B>, peer: SocketAddr) -> Response<Full<Bytes>>
    where
        B: Body<Data = Bytes>,
        B::Error: Into<Box<dyn StdError + Send + Sync>>,
    {
        let (id, endpoint) = match request.uri().path().split('/').collect::<Vec<_>>()[..] {
            ["", "v1", id, endpoint] => (parse_id(id), endpoint),
            _ => return text(StatusCode::NOT_FOUND, "the path doesn't exist"),
        };
        let Some((id, key)) = id.and_then(|id| Some((id, self.keys.get(id)?))) else {
            return text(StatusCode::NOT_FOUND, "the key doesn't exist");
        };
        match (endpoint, request.method()) {
            ("public-key", &Method::GET) => bytes(key.public().as_bytes().to_vec()),
            ("public-key", _) => method_not_allowed("GET"),
            ("sign", &Method::POST) => self.sign(id, request, peer).await,
            ("sign", _) => method_not_allowed("POST"),
            _ => text(StatusCode::NOT_FOUND, "the path doesn't exist"),
        }
    }

    /// Key `id`'s signature of the values in `request`, returned once it's
    /// recorded.
    async fn sign<B>(&self, id: u16, request: Request<B>, peer: SocketAddr) -> Response<Full<Bytes>>
    where
        B: Body<Data = Bytes>,
        B::Error: Into<Box<dyn StdError + Send + Sync>>,
    {
        let Some(pin) = bearer(request.headers()).map(str::to_owned) else {
            return unauthorized("the PIN must be provided in Authorization: Bearer PIN");
        };
        let body = match Limited::new(request.into_body(), MAX_BODY).collect().await {
            Ok(body) => body.to_bytes(),
            Err(error) if error.is::<LengthLimitError>() => {
                return text(StatusCode::PAYLOAD_TOO_LARGE, "the request exceeds 16KB");
            }
            Err(error) => {
                return text(
                    StatusCode::BAD_REQUEST,
                    format!("can't read the request: {error}"),
                );
            }
        };
        let request: SignRequest = match serde_json::from_slice(&body) {
            Ok(request) => request,
            Err(error) => {
                return text(
                    StatusCode::BAD_REQUEST,
                    format!("the request isn't valid: {error}"),
                );
            }
        };
        let Some(chip_id) = parse_chip_id(&request.chip_id) else {
            return text(
                StatusCode::BAD_REQUEST,
                "chip_id isn't 16 uppercase hex digits",
            );
        };
        let Some(board) = Board::try_from_str(&request.board) else {
            return text(
                StatusCode::BAD_REQUEST,
                format!("unknown board {}", request.board),
            );
        };
        let values = match CommissioningValues::new(board, &request.manufacturer, &request.date, id)
        {
            Ok(values) => values,
            Err(BuildError::EmptyManufacturer) => {
                return text(StatusCode::BAD_REQUEST, "manufacturer is empty");
            }
            Err(BuildError::BadDate) => {
                return text(StatusCode::BAD_REQUEST, "date isn't 8 digits, YYYYMMDD");
            }
            Err(BuildError::DoesNotFit) => {
                return text(
                    StatusCode::BAD_REQUEST,
                    "the values don't fit the commissioning area",
                );
            }
            // An ID is never 0, and values have no first row.
            Err(error @ (BuildError::BadSigner | BuildError::BadFirstRow)) => {
                error!("key {id}: unexpected {error:?}");
                return text(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
            }
        };
        let message = values.message(chip_id);

        let _turn = self.signing.lock().await;
        let Some(key) = self.keys.get(id).cloned() else {
            return text(StatusCode::NOT_FOUND, "the key doesn't exist");
        };
        let signature = match tokio::task::spawn_blocking(move || key.sign(&pin, &message)).await {
            Ok(Ok(signature)) => signature.to_bytes(),
            Ok(Err(SignError::WrongPin)) => {
                warn!("{peer}: incorrect PIN for key {id}");
                return unauthorized("the PIN is incorrect");
            }
            Ok(Err(SignError::Mismatch)) => {
                error!("key {id}: key.pem and public.pem are different keys");
                return text(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "the key's files don't match",
                );
            }
            Ok(Err(SignError::BadSignature)) => {
                error!("key {id}: a signature didn't verify");
                return text(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "the signature didn't verify",
                );
            }
            Err(error) => {
                error!("key {id}: signing failed: {error}");
                return text(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
            }
        };

        match self.record.add(id, chip_id, &signature).await {
            Ok(added) => {
                let recorded = match added {
                    Added::New => "recorded",
                    Added::AlreadyThere => "already recorded",
                };
                info!(
                    "{peer}: key {id} signed {} for {}, {}, {}, {recorded}",
                    request.chip_id, request.board, request.manufacturer, request.date
                );
                bytes(signature.to_vec())
            }
            Err(error) => {
                error!("key {id}: {error}");
                text(
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!(
                        "the record can't be written, so the signature isn't returned: {error}"
                    ),
                )
            }
        }
    }
}

/// Serves `server` over TLS on `listener`.
pub async fn serve(listener: TcpListener, tls: TlsAcceptor, server: Arc<Server>) {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                warn!("accept failed: {error}");
                sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let tls = tls.clone();
        let server = server.clone();
        tokio::spawn(async move {
            let stream = match timeout(HANDSHAKE_TIMEOUT, tls.accept(stream)).await {
                Ok(Ok(stream)) => stream,
                Ok(Err(error)) => return debug!("{peer}: TLS handshake failed: {error}"),
                Err(_) => return debug!("{peer}: TLS handshake timed out"),
            };
            let service = service_fn(|request| {
                let server = server.clone();
                async move { Ok::<_, Infallible>(server.handle(request, peer).await) }
            });
            if let Err(error) = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_TIMEOUT)
                .serve_connection(TokioIo::new(stream), service)
                .await
            {
                debug!("{peer}: {error}");
            }
        });
    }
}

/// The PIN in `headers`' `Authorization: Bearer PIN`.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let (scheme, pin) = headers.get(AUTHORIZATION)?.to_str().ok()?.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("Bearer") && !pin.is_empty()).then_some(pin)
}

fn response(
    status: StatusCode,
    content_type: &'static str,
    body: Vec<u8>,
) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(body)));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

fn bytes(body: Vec<u8>) -> Response<Full<Bytes>> {
    response(StatusCode::OK, "application/octet-stream", body)
}

/// A response whose body is `message` on one line.
fn text(status: StatusCode, message: impl Display) -> Response<Full<Bytes>> {
    response(
        status,
        "text/plain; charset=utf-8",
        format!("{message}\n").into_bytes(),
    )
}

fn unauthorized(message: &str) -> Response<Full<Bytes>> {
    let mut response = text(StatusCode::UNAUTHORIZED, message);
    response
        .headers_mut()
        .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

fn method_not_allowed(allow: &'static str) -> Response<Full<Bytes>> {
    let mut response = text(
        StatusCode::METHOD_NOT_ALLOWED,
        "the method isn't valid for the path",
    );
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allow));
    response
}
