// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Create file operations

use iced::Task;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use rfd::FileDialog;
use std::path::PathBuf;

use onerom_config::chip::CHIP_TYPES;

use crate::app::AppMessage;
use crate::config::Config;
use crate::create::{Create, Message, State};
use crate::studio::{Image, Message as StudioMessage, RuntimeInfo};
use crate::{internal_error, task_from_msg};

/// Show the save file dialog
pub fn save_firmware(create: &mut Create, runtime_info: &RuntimeInfo) -> Task<AppMessage> {
    debug!("Save firmware requested");

    // Check we are not busy
    if create.is_busy() {
        warn!("Busy - skipping save firmware");
        return Task::none();
    }

    // Check we have a selected config - shouldn't be here otherwise
    if runtime_info.selected_config().is_none() {
        internal_error!("SaveFirmware received with no selected config.");
        return Task::none();
    }
    let config = runtime_info.selected_config().unwrap();

    // Update state
    create.state = State::Saving;
    create.set_display_content("Selecting filename...".to_string());

    // Set default filename
    let filename = format!(
        "onerom-{}-{}-{}.bin",
        config.save_filename(),
        create.selected_hw_info.board_name(),
        create.selected_hw_info.mcu_name(),
    );
    Task::future(save_firmware_async(filename))
}

// Async function to show the save file dialog
async fn save_firmware_async(filename: String) -> AppMessage {
    let dialog = FileDialog::new()
        .set_title("Save Firmware Image")
        .set_file_name(filename)
        .add_filter("Binary Files", &["bin"])
        .set_directory(".");
    let path = dialog.save_file();
    Message::SaveFirmwareFilename(path).into()
}

/// Handle the filename entered to save the firmware image
pub fn save_firmware_filename(
    create: &mut Create,
    runtime_info: &RuntimeInfo,
    filename: Option<PathBuf>,
) -> Task<AppMessage> {
    debug!(
        "Save firmware filename received: {}",
        if let Some(f) = &filename {
            format!("{f:?}")
        } else {
            "None".to_string()
        }
    );

    // Check we're in the appropriate state
    if !create.is_saving() {
        internal_error!("SaveFirmwareFilename received while not saving.");
        return Task::none();
    }

    // Check we have a filename
    if filename.is_some() {
        // Kick off the async save operation
        create.set_display_content(format!("Saving firmware to {filename:?}..."));
        let images = runtime_info.image().cloned();
        Task::future(save_firmware_filename_async(filename, images))
    } else {
        // User cancelled
        debug!("Save firmware cancelled by user");
        create.state = State::Idle;
        create.display_content = "Firmware save cancelled.".to_string();
        Task::none()
    }
}

// Filename entered - save image to the filename
async fn save_firmware_filename_async(
    filename: Option<PathBuf>,
    image: Option<Image>,
) -> AppMessage {
    // Handle errors
    if image.is_none() {
        warn!("No image available to save firmware");
        return Message::SaveFirmwareComplete.into();
    }
    if filename.is_none() {
        debug!("Save firmware cancelled by user");
        return Message::SaveFirmwareComplete.into();
    }

    // Write the file
    let image = image.unwrap();
    let filename = filename.unwrap();
    let data = image.full_image();
    match std::fs::write(&filename, data) {
        Ok(_) => {
            debug!("Firmware image saved to {filename:?}");
        }
        Err(e) => {
            error!("Error saving firmware image to {filename:?}: {e}");
        }
    }

    // Send done message
    Message::SaveFirmwareComplete.into()
}

// Handle completion of save firmware operation
pub fn save_firmware_complete(create: &mut Create) -> Task<AppMessage> {
    debug!("Save firmware operation complete");

    if !create.is_saving() {
        internal_error!("SaveFirmwareComplete received while not saving.");
    }

    // Update state
    create.display_content += "\n\nFirmware save complete.";
    create.state = State::Idle;
    Task::none()
}

/// Handle a ROM config being selected
#[allow(clippy::wildcard_enum_match_arm)]
pub fn config_selected(create: &mut Create, config: Config) -> Task<AppMessage> {
    debug!("Config selected: {}", config);

    if create.is_busy() {
        internal_error!("ConfigSelected received while busy.");
    }

    match config {
        Config::SelectLocalFile => {
            // Open file dialog to select local config file
            Task::future(select_local_config_file())
        }
        Config::Build => {
            // Get the selected rom pins from the selected hardware
            let rom_pins: u8 = create
                .selected_hw_info
                .board
                .map_or(24, |board| board.chip_pins());

            // build a vec of rom_types that match the selected rom pins
            let rom_types = CHIP_TYPES
                .iter()
                .filter(|rt| rt.chip_pins() == rom_pins)
                .cloned()
                .collect::<Vec<_>>();

            create.state = State::UserBuilding {
                valid_rom_types: rom_types,
                rom_type: None,
                cs: vec![],
                data: None,
            };
            Task::none()
        }
        config => {
            // Have config details - get Studio to download it
            create.display_content = format!("Loading config: {}...", config.name());
            create.state = State::Loading;
            task_from_msg!(StudioMessage::LoadConfig(config))
        }
    }
}

async fn select_local_config_file() -> AppMessage {
    // Open file dialog to select config file
    let result = FileDialog::new()
        .add_filter("ROM Configuration Files", &["json"])
        .set_title("Select ROM Configuration File")
        .pick_file();

    // Turn into a message
    if let Some(path) = result {
        Message::ConfigSelected(Config::from_local_path(path)).into()
    } else {
        AppMessage::Nop // Silently ignore cancel load file dialog
    }
}

pub fn config_loaded(
    create: &mut Create,
    runtime_info: &RuntimeInfo,
    result: Result<(), String>,
) -> Task<AppMessage> {
    debug!("Config loaded");

    // Check we are in the appropriate state
    if create.state != State::Loading {
        internal_error!("ConfigLoaded received while not loading.");
    }

    // Update state
    create.state = State::Idle;

    // Log and display result
    match result {
        Ok(()) => {
            debug!(
                "Config {} loaded successfully.",
                runtime_info
                    .selected_config()
                    .map(|c| c.name())
                    .unwrap_or("none".to_string())
            );
            create.display_content += &format!(
                "\n\nConfig {} loaded successfully.",
                runtime_info
                    .selected_config()
                    .map(|c| c.name())
                    .unwrap_or("none".to_string())
            );
        }
        Err(e) => {
            warn!("Failed to load config: {}", e);
            create.display_content = format!("Error loading config:\n - {e}");
        }
    }

    Task::none()
}
