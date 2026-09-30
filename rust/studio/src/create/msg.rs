// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Create Message handling

use iced::Task;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use std::path::PathBuf;

use onerom_config::chip::ChipType;
use onerom_config::hw::{Board, BoardSize, Model};
use onerom_config::mcu::Variant as McuVariant;
use onerom_fw::net::Release;

use crate::app::AppMessage;
use crate::config::Config;
use crate::create::Create;
use crate::create::build::{build_image, build_image_result, build_json_config_from_state};
use crate::create::file::{
    config_loaded, config_selected, save_firmware, save_firmware_complete, save_firmware_filename,
};
use crate::create::hw::{
    detect_hardware, detected_hardware_info, flash_firmware, flash_firmware_result,
};
use crate::device::Device;
use crate::studio::RuntimeInfo;
use crate::task_from_msg;

use super::State;
use super::build::{Active, select_cs_active, select_data_vec, select_rom_type};

/// Create Messages
#[derive(Debug, Clone)]
pub enum Message {
    // Hardware and firmware release picklist values changed
    BoardSelected(Board),
    BoardSizeSelected(BoardSize),
    ModelSelected(Model),
    McuSelected(McuVariant),
    ReleaseSelected(Release),
    ReleaseDowloaded(Result<(), String>),

    // Detect hardware button operation.
    // Information can be detected via connected device using Analyse tab
    DetectHardware,
    DetectedHardwareInfo,

    // Releases and configs have been updated (from network)
    ReleasesUpdated,
    ConfigsUpdated,

    // ROM config has been selected via pick list
    ConfigSelected(Config),
    ConfigLoaded(Result<(), String>),
    ReloadConfig,

    // Build image
    BuildImage,
    BuildImageResult(Result<String, String>),

    // Save the firmware image as a file.
    // - SaveFirmware - save button pressed
    // - SaveFirmwareFilename(Option<PathBuf>) - filename selected (or
    //   cancelled)
    // - SaveFirmwareComplete - save operation complete
    SaveFirmware,
    SaveFirmwareFilename(Option<PathBuf>),
    SaveFirmwareComplete,

    // Flash firmware
    KeyFlashFirmware,
    FlashFirmware,
    FlashFirmwareResult(Result<(), String>),

    // Progress tick from subscription during operation
    ProgressTick,

    // User building custom configuration
    BuildingSelectChipType(ChipType),
    BuildingSelectCsActive(usize, Active),
    BuildingSelectDataVec(Vec<u8>),
    BuildingComplete,
    BuildingCancelled,

    StopDevice,
    RunDevice,
    DeviceRebootComplete(Result<(), String>),
}

// Main Create Message handling function
pub fn message(
    create: &mut Create,
    runtime_info: &RuntimeInfo,
    device: &Device,
    msg: Message,
) -> Task<AppMessage> {
    match msg {
        // Hardware and firmware release picklist values changed
        Message::ModelSelected(model) => {
            debug!("Model selected: {}", model.name());
            create.model_selected(model);
            Task::none()
        }
        Message::BoardSelected(board) => {
            debug!("Board selected: {}", board.name());
            task_from_msg!(create.board_selected(runtime_info, board))
        }
        Message::BoardSizeSelected(size) => {
            debug!("Board size selected: {size}");
            create.board_size_selected(size);
            create.size_detected = false;
            Task::none()
        }
        Message::McuSelected(mcu) => {
            debug!("MCU selected: {}", mcu);
            create.mcu_selected(mcu);
            task_from_msg!(create.select_latest_release(runtime_info.releases()))
        }
        Message::ReleaseSelected(release) => {
            debug!("Firmware release selected: {}", release.version);
            task_from_msg!(create.select_release(release))
        }
        Message::ReleaseDowloaded(result) => {
            debug!("Firmware release downloaded");
            match result {
                Ok(()) => debug!("Release download succeeded"),
                Err(e) => warn!("Release download failed: {e}"),
            }
            Task::none()
        }

        // Detect hardware button operation.
        Message::DetectHardware => detect_hardware(create),
        Message::DetectedHardwareInfo => detected_hardware_info(create, runtime_info),

        // Releases and configs have been updated (from network)
        Message::ReleasesUpdated => {
            let releases = runtime_info.releases();

            // Select the latest firmware, unless one is already selected
            if create.hardware_selected() && runtime_info.selected_firmware().is_none() {
                task_from_msg!(create.select_latest_release(releases))
            } else {
                Task::none()
            }
        }
        Message::ConfigsUpdated => Task::none(),

        // ROM config has been selected via pick list
        Message::ConfigSelected(config) => config_selected(create, config),
        Message::ConfigLoaded(result) => config_loaded(create, runtime_info, result),
        Message::ReloadConfig => {
            debug!("Reloading configuration");
            let config = runtime_info.selected_config();
            if let Some(config) = config {
                config_selected(create, config.config.clone())
            } else {
                debug!("Reload config requested but no config selected");
                Task::none()
            }
        }

        // Build image
        Message::BuildImage => build_image(create, runtime_info),
        Message::BuildImageResult(result) => build_image_result(create, result, runtime_info),

        // Save the firmware image as a file.
        Message::SaveFirmware => save_firmware(create, runtime_info),
        Message::SaveFirmwareFilename(filename) => {
            save_firmware_filename(create, runtime_info, filename)
        }
        Message::SaveFirmwareComplete => save_firmware_complete(create),

        // Flash firmware
        Message::KeyFlashFirmware => {
            if runtime_info.image().is_some()
                && !create.is_building()
                && !create.is_busy()
                && device.is_ready()
            {
                flash_firmware(create, runtime_info)
            } else {
                trace!("Flash firmware requested but device not ready or create busy");
                Task::none()
            }
        }
        Message::FlashFirmware => flash_firmware(create, runtime_info),
        Message::FlashFirmwareResult(result) => flash_firmware_result(create, result),

        // Progress tick from subscription during operation
        Message::ProgressTick => {
            create.progress_tick();
            Task::none()
        }

        // User building custom configuration
        Message::BuildingSelectChipType(rom_type) => select_rom_type(create, rom_type),
        Message::BuildingSelectCsActive(index, active) => select_cs_active(create, index, active),
        Message::BuildingSelectDataVec(data) => select_data_vec(create, data),
        Message::BuildingComplete => {
            debug!("User completed custom configuration build");
            // Create a Built config from the create state
            let task = build_json_config_from_state(create);
            create.state = State::Idle;
            task
        }
        Message::BuildingCancelled => {
            debug!("User cancelled custom configuration build");
            create.state = State::Idle;
            create.set_display_content("Custom configuration build cancelled.");
            Task::none()
        }

        Message::StopDevice => create.stop_device(),
        Message::RunDevice => create.run_device(),
        Message::DeviceRebootComplete(result) => create.reboot_complete(result),
    }
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::BoardSelected(board) => write!(f, "BoardSelected({})", board.name()),
            Message::BoardSizeSelected(size) => write!(f, "BoardSizeSelected({size})"),
            Message::ModelSelected(model) => write!(f, "ModelSelected({})", model.name()),
            Message::McuSelected(mcu) => write!(f, "McuSelected({mcu})"),
            Message::ReleaseSelected(release) => {
                write!(f, "ReleaseSelected({})", release.version)
            }
            Message::ReleaseDowloaded(result) => match result {
                Ok(()) => write!(f, "ReleaseDowloaded(Ok)"),
                Err(e) => write!(f, "ReleaseDowloaded(Err({e}))"),
            },

            Message::DetectHardware => write!(f, "DetectHardware"),
            Message::DetectedHardwareInfo => write!(f, "DetectedHardwareInfo"),

            Message::ReleasesUpdated => write!(f, "ReleasesUpdated"),
            Message::ConfigsUpdated => write!(f, "ConfigsUpdated"),

            Message::ConfigSelected(name) => write!(f, "ConfigSelected({})", name),
            Message::ConfigLoaded(result) => match result {
                Ok(()) => write!(f, "ConfigLoaded(Ok)"),
                Err(e) => write!(f, "ConfigLoaded(Err({e}))"),
            },
            Message::ReloadConfig => write!(f, "ReloadConfig"),

            Message::BuildImage => write!(f, "BuildImage"),
            Message::BuildImageResult(result) => {
                write!(f, "BuildImageResult({:?})", result)
            }

            Message::SaveFirmware => write!(f, "SaveFirmware"),
            Message::SaveFirmwareFilename(filename) => {
                write!(f, "SaveFirmwareFilename({:?})", filename)
            }
            Message::SaveFirmwareComplete => write!(f, "SaveFirmwareComplete"),

            Message::KeyFlashFirmware => write!(f, "KeyFlashFirmware"),
            Message::FlashFirmware => write!(f, "FlashFirmware"),
            Message::FlashFirmwareResult(result) => {
                write!(f, "FlashFirmwareResult({:?})", result)
            }

            Message::ProgressTick => write!(f, "ProgressTick"),

            Message::BuildingSelectChipType(rom_type) => {
                write!(f, "BuildingSelectChipType({rom_type})")
            }
            Message::BuildingSelectCsActive(index, active) => {
                write!(f, "BuildingSelectCsActive({}, {})", index, active)
            }
            Message::BuildingSelectDataVec(data) => {
                write!(f, "BuildingSelectDataVec(len={})", data.len())
            }
            Message::BuildingComplete => write!(f, "BuildingComplete"),
            Message::BuildingCancelled => write!(f, "BuildingCancelled"),

            Message::StopDevice => write!(f, "StopDevice"),
            Message::RunDevice => write!(f, "RunDevice"),
            Message::DeviceRebootComplete(result) => match result {
                Ok(()) => write!(f, "DeviceRebootComplete(Ok)"),
                Err(e) => write!(f, "DeviceRebootComplete(Err({e}))"),
            },
        }
    }
}
