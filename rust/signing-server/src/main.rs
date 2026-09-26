// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use std::error::Error;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use log::{error, info};
use tokio::net::TcpListener;

use onerom_signing_server::http::{Server, serve};
use onerom_signing_server::keys::Keys;
use onerom_signing_server::record::Record;
use onerom_signing_server::tls;

/// Signs One ROM commissioning instances.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Directory containing a subdirectory for each key. Each subdirectory
    /// contains key.pem and public.pem.
    #[arg(long, value_name = "DIR")]
    keys: PathBuf,

    /// The server's clone of the record repository.
    #[arg(long, value_name = "DIR")]
    record: PathBuf,

    /// The TLS certificate chain in PEM.
    #[arg(long, value_name = "FILE")]
    tls_cert: PathBuf,

    /// The TLS certificate's private key in PEM.
    #[arg(long, value_name = "FILE")]
    tls_key: PathBuf,

    /// The address and port to listen on.
    #[arg(long, value_name = "ADDRESS", default_value = "0.0.0.0:8443")]
    listen: SocketAddr,
}

#[tokio::main]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), Box<dyn Error>> {
    let keys = Keys::load(&args.keys)?;
    let ids: Vec<_> = keys.ids().map(|id| id.to_string()).collect();
    info!("keys {}", ids.join(", "));
    let record = Record::open(args.record).await?;
    let tls = tls::acceptor(&args.tls_cert, &args.tls_key)?;
    let listener = TcpListener::bind(args.listen).await?;
    info!("listening on {}", args.listen);
    serve(listener, tls, Arc::new(Server::new(keys, record))).await;
    Ok(())
}
