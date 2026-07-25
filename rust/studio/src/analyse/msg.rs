// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Analyse Message handling

use iced::Task;
use std::path::PathBuf;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
#[allow(unused_imports)]
use onerom_config::fw::FirmwareVersion;
use onerom_config::mcu::Variant as McuVariant;
use onerom_fw_parser::ParsedDevice;

use crate::analyse::device::{
    detect_device, device_reboot_complete, file_device_loaded, firmware_flash_complete,
    flash_firmware, handle_device_data, reread_device, run_device, stop_device,
};
use crate::analyse::file::{fw_file_chooser, load_file};
use crate::analyse::{Analyse, Source};
use crate::app::AppMessage;
use crate::studio::RuntimeInfo;

/// Analyse tab messages
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Message {
    /// Source (File/Device) selected
    SourceSelected(Source),

    // Handle file
    SelectFile,
    FileSelected(Option<PathBuf>),
    FileLoaded(Result<(ParsedDevice, Vec<u8>), String>),

    // Handle device
    DetectDevice,
    DeviceLoaded(Result<(ParsedDevice, Vec<u8>), String>),
    DeviceData(Vec<u8>),
    ReadFailed(String),
    RereadDevice(McuVariant, FirmwareVersion),

    // Handle flashing
    FlashFirmware,
    FlashComplete(Result<(), String>),

    // Progress tick
    ProgressTick,

    StopDevice,
    RunDevice,
    DeviceRebootComplete(Result<(), String>),
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::SourceSelected(tab) => write!(f, "SourceSelected({:?})", tab),
            Message::SelectFile => write!(f, "SelectFile"),
            Message::FileSelected(_) => write!(f, "FileSelected(...)"),
            Message::FileLoaded(_) => write!(f, "FileLoaded(...)"),
            Message::DetectDevice => write!(f, "DetectDevice"),
            Message::DeviceLoaded(_) => write!(f, "DeviceLoaded(...)"),
            Message::DeviceData(_) => write!(f, "DeviceData(...)"),
            Message::ReadFailed(err) => write!(f, "ReadFailed({err})"),
            Message::RereadDevice(_, _) => write!(f, "RereadDevice"),
            Message::FlashFirmware => write!(f, "FlashFirmware"),
            Message::FlashComplete(_) => write!(f, "FlashComplete(...)"),
            Message::ProgressTick => write!(f, "ProgressTick"),
            Message::StopDevice => write!(f, "StopDevice"),
            Message::RunDevice => write!(f, "RunDevice"),
            Message::DeviceRebootComplete(_) => write!(f, "DeviceRebootComplete(...)"),
        }
    }
}

/// Analyse Message handler
pub fn message(
    analyse: &mut Analyse,
    _runtime_info: &RuntimeInfo,
    msg: Message,
) -> Task<AppMessage> {
    match msg {
        // User has seledcted a firmware source
        Message::SourceSelected(tab) => {
            analyse.selected_source_tab = tab;
            Task::none()
        }

        // Handle file operations
        Message::SelectFile => {
            debug!("Selecting firmware file");
            fw_file_chooser()
        }
        Message::FileSelected(path) => {
            debug!("Firmware file selected: {:?}", path);
            load_file(analyse, path)
        }
        Message::FileLoaded(result) => {
            debug!(
                "Firmware file loaded: {}",
                if result.is_ok() { "OK" } else { "Error" }
            );
            file_device_loaded(analyse, result, true)
        }

        // Handle device operations
        Message::DetectDevice => {
            debug!("Starting device detection");
            // Clear out previous analysis content
            analyse.analysis_content = String::new();
            detect_device(analyse, None)
        }
        Message::DeviceLoaded(result) => {
            debug!(
                "Device firmware loaded: {}",
                if result.is_ok() { "OK" } else { "Error" }
            );
            file_device_loaded(analyse, result, false)
        }
        Message::DeviceData(data) => {
            debug!("Device data received: {} bytes", data.len());
            Task::future(handle_device_data(data))
        }
        Message::ReadFailed(err) => {
            debug!("Device read failed: {}", err);
            // Move onto trying to detect next device type
            detect_device(analyse, Some(err))
        }
        Message::RereadDevice(mcu, fw_version) => {
            debug!(
                "Re-reading device flash for MCU variant {} with fw v{}.{}.{}",
                mcu,
                fw_version.major(),
                fw_version.minor(),
                fw_version.patch()
            );
            Task::done(reread_device(analyse, mcu, fw_version))
        }

        // Handle flashing
        Message::FlashFirmware => {
            debug!("Flashing firmware to device");
            flash_firmware(analyse)
        }
        Message::FlashComplete(result) => {
            debug!(
                "Firmware flash complete: {}",
                if result.is_ok() { "OK" } else { "Error" }
            );
            firmware_flash_complete(analyse, result);
            Task::none()
        }

        // Handle progress tick
        Message::ProgressTick => {
            analyse.progress_tick();
            Task::none()
        }

        Message::StopDevice => stop_device(analyse),
        Message::RunDevice => run_device(analyse),
        Message::DeviceRebootComplete(result) => device_reboot_complete(analyse, result),
    }
}
