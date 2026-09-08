// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `onerom monitor` - watching a running One ROM as it works.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::args;
use crate::utils::check_device_running;
use onerom_cli::cdc::{SILENCE_TIMEOUT, find_port, stream};
use onerom_cli::device::Device;
use onerom_cli::{Error, Options};

pub async fn cmd_log(options: &Options, args: &args::monitor::MonitorLogArgs) -> Result<(), Error> {
    check_device_running(options, args)?;
    let device = options.device.as_ref().unwrap();
    let capture = args.output.as_ref().map(PathBuf::from);
    follow(device, capture, options.verbose).await
}

/// Attach to a One ROM's serial port and print what it sends until it goes
/// away.
///
/// Shared with `onerom program --follow`, which reaches the same place once the
/// One ROM it has just programmed is back on the bus.
///
/// Status goes to stderr, so redirecting stdout captures the One ROM's output
/// on its own.
///
/// The user chose a One ROM, not a serial port, so the port is named only under
/// `--verbose` - where it is there to be handed to another tool - and in the
/// error for failing to open it, where it is what they would act on.
pub async fn follow(device: &Device, capture: Option<PathBuf>, verbose: bool) -> Result<(), Error> {
    let port_name = find_port(device)?;
    if verbose {
        eprintln!("Reading from serial port {port_name}");
    }

    match capture.as_deref() {
        Some(path) => eprintln!(
            "Monitoring log, writing to {} - press Ctrl-C to stop",
            path.display()
        ),
        None => eprintln!("Monitoring log - press Ctrl-C to stop"),
    }

    // Ctrl-C ends the session through stream() rather than through the process,
    // so that what was copied can still be reported. The cost is that an
    // interrupted run exits 0 instead of dying by signal.
    let stop = Arc::new(AtomicBool::new(false));
    let signalled = stop.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signalled.store(true, Ordering::Relaxed);
        }
    });

    // stream() blocks for as long as the session lasts, which is the rest of
    // this command.
    let reported = capture.clone();
    let reader = stop.clone();
    let result = tokio::task::spawn_blocking(move || {
        stream(&port_name, capture.as_deref(), SILENCE_TIMEOUT, &reader)
    })
    .await
    .map_err(|e| Error::Other(format!("Failed to read the log: {e}")))?;

    let copied = result?;

    if stop.load(Ordering::Relaxed) {
        eprintln!("Stopped monitoring the log");
    } else {
        eprintln!("One ROM disconnected");
    }
    if let Some(path) = reported {
        eprintln!("Wrote {copied} bytes to {}", path.display());
    }

    Ok(())
}
