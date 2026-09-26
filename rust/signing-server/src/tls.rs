// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

//! The server's TLS certificate.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::pem::{self, PemObject};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// Why the certificate can't be used.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("can't read {path}: {source}")]
    Pem { path: PathBuf, source: pem::Error },
    #[error("{path} doesn't contain a certificate")]
    NoCertificate { path: PathBuf },
    #[error("the TLS certificate and key can't be used: {0}")]
    Rustls(#[from] tokio_rustls::rustls::Error),
}

/// Accepts TLS connections with the certificate chain in `cert` and its
/// private key in `key`. Both are PEM files.
pub fn acceptor(cert: &Path, key: &Path) -> Result<TlsAcceptor, Error> {
    let pem_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| Error::Pem { path, source }
    };
    let certs = CertificateDer::pem_file_iter(cert)
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(pem_error(cert))?;
    if certs.is_empty() {
        return Err(Error::NoCertificate { path: cert.into() });
    }
    let key = PrivateKeyDer::from_pem_file(key).map_err(pem_error(key))?;
    let config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
