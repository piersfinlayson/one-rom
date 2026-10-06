// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Analyse file handling methods
//!
//! Processing the parsed firmware is handled in `device.rs` as it's mostly
//! the same logic whether loading from file or device.

use iced::Task;
use rfd::FileDialog;
use std::path::PathBuf;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use onerom_cli::image::parse_firmware;
#[allow(unused_imports)]
use onerom_config::fw::FirmwareVersion;
use onerom_fw_parser::ParsedDevice;

use crate::analyse::{Analyse, AnalyseState, Message};
use crate::app::AppMessage;

/// Load a file based on user selection
pub fn load_file(analyse: &mut Analyse, path: Option<PathBuf>) -> Task<AppMessage> {
    if let Some(path) = path {
        // User selected a file

        // First task is to start the analysis - this has impacts on other
        // areas, hence a task
        let start_analysis_task = analyse.start_analysis(AnalyseState::Loading);

        // Create task to load the file asynchronously
        let load_file_task = Task::perform(load_file_async(path.clone()), move |result| {
            AppMessage::Analyse(Message::FileLoaded(path.clone(), result))
        });

        // Return a batch of tasks - i.e. run both in parallel
        Task::batch([start_analysis_task, load_file_task])
    } else {
        // User cancelled file selection, just ignore
        Task::none()
    }
}

// Actual file load routine
async fn load_file_async(path: PathBuf) -> Result<(ParsedDevice, Vec<u8>), String> {
    // Check we have a valid file
    if !path.exists() || !path.is_file() {
        return Err("File does not exist or is a directory".to_string());
    }

    // Read in the file
    let data = std::fs::read(path).map_err(|e| e.to_string())?;

    // Parse it as the CLI does so a file that uses the second flash chip is
    // read at the second chip's address.
    //
    // Parsing is infallible. It returns a ParsedDevice for any input, so
    // whether this is actually One ROM firmware is a separate question,
    // answered by is_recognised().  A Lab passes it, and file_device_loaded()
    // turns it away.
    let device = parse_firmware(&data).await;

    if !device.is_recognised() {
        debug!("Parsed file contains no recognisable One ROM firmware information");
        return Err("No One ROM firmware information found".to_string());
    }

    Ok((device, data))
}

/// Show firmware file chooser dialog
pub fn fw_file_chooser() -> Task<AppMessage> {
    Task::perform(
        async {
            FileDialog::new()
                .add_filter("firmware", &["bin"])
                .pick_file()
        },
        |path| Message::FileSelected(path).into(),
    )
}
