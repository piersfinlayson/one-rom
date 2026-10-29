// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `onerom monitor` - watching a running One ROM as it works.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::args;
use crate::utils::check_device_running;
use onerom_cli::cdc::{SILENCE_TIMEOUT, find_port, stream};
use onerom_cli::device::Device;
use onerom_cli::ram_log;
use onerom_cli::{Error, Options};

pub async fn cmd_log(options: &Options, args: &args::monitor::MonitorLogArgs) -> Result<(), Error> {
    check_device_running(options, args)?;
    let device = options.device.as_ref().unwrap();
    let capture = args.output.as_ref().map(PathBuf::from);
    follow(device, capture, options.verbose).await
}

pub(crate) enum Source {
    Serial(String),
    Ram,
}

/// Print what a One ROM logs until it disconnects.
///
/// Status goes to stderr so stdout contains only the One ROM's output.
///
/// The user chose a One ROM so the serial port is identified only under
/// `--verbose` and in the error for failing to open it.
pub async fn follow(device: &Device, capture: Option<PathBuf>, verbose: bool) -> Result<(), Error> {
    let source = if ram_log::read_log_from_ram(device) {
        Source::Ram
    } else {
        Source::Serial(find_port(device, 1)?)
    };
    if verbose {
        eprintln!("{}", source_line(&source));
    }

    eprintln!("{}", monitoring_line(capture.as_deref()));

    // Ctrl-C ends the stream so what was copied can still be reported. An
    // interrupted run then exits 0.
    let stop = Arc::new(AtomicBool::new(false));
    let signalled = stop.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signalled.store(true, Ordering::Relaxed);
        }
    });

    let copied = match source {
        Source::Serial(port_name) => {
            let capture = capture.clone();
            let reader = stop.clone();
            tokio::task::spawn_blocking(move || {
                stream(&port_name, capture.as_deref(), SILENCE_TIMEOUT, &reader)
            })
            .await
            .map_err(|e| Error::Other(format!("Failed to read the log: {e}")))??
        }
        Source::Ram => ram_log::stream(device, capture.as_deref(), &stop).await?,
    };

    eprintln!("{}", ended_line(stop.load(Ordering::Relaxed)));
    if let Some(path) = capture {
        eprintln!("{}", wrote_line(copied, &path));
    }

    Ok(())
}

pub(crate) fn source_line(source: &Source) -> String {
    match source {
        Source::Serial(port_name) => format!("Reading from serial port {port_name}"),
        Source::Ram => {
            "Reading logs from RAM as a plugin has exclusive use of the serial port".to_string()
        }
    }
}

pub(crate) fn monitoring_line(capture: Option<&Path>) -> String {
    match capture {
        Some(path) => format!(
            "Monitoring log, writing to {} - press Ctrl-C to stop",
            path.display()
        ),
        None => "Monitoring log - press Ctrl-C to stop".to_string(),
    }
}

pub(crate) fn ended_line(stopped: bool) -> &'static str {
    if stopped {
        "Stopped monitoring the log"
    } else {
        "One ROM disconnected"
    }
}

pub(crate) fn wrote_line(copied: u64, path: &Path) -> String {
    format!("Wrote {copied} bytes to {}", path.display())
}
