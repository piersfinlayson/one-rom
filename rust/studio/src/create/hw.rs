// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Create hardware selection operations

use iced::Task;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use crate::app::AppMessage;
use crate::create::{Create, State};
use crate::device::{Client, Message as DeviceMessage};
use crate::studio::RuntimeInfo;
use crate::{internal_error, task_from_msgs};

/// Detect the hardware connected to the device
pub fn detect_hardware(_create: &mut Create) -> Task<AppMessage> {
    info!("Detect hardware requested - not implemented");
    Task::none()
}

/// Handle detected hardware information
pub fn detected_hardware_info(create: &mut Create, runtime_info: &RuntimeInfo) -> Task<AppMessage> {
    // Check we have some hardware info
    if runtime_info.hw_info().is_none() {
        trace!("No hardware info available");
        create.size_detected = false;
        return Task::none();
    }

    // Handle the detected hardware info, piece by piece
    let hw_info = runtime_info.hw_info().unwrap();
    debug!("Incoming detected hardware info: {hw_info}");

    if let Some(model) = hw_info.model {
        create.model_selected(model);
        true
    } else {
        false
    };

    let msg1 = if create.has_model()
        && let Some(board) = hw_info.board
    {
        create.board_selected(runtime_info, board)
    } else {
        None
    };

    // A size read from a device replaces the selected size where the board
    // supports it.  Otherwise the image's minimum size does.
    create.size_detected = false;
    if create.selected_hw_info.board_size.is_some()
        && let Some(board) = create.selected_hw_info.board
    {
        let sizes = Create::board_sizes(board);
        if let Some(size) = hw_info.board_size
            && sizes.contains(&size)
        {
            create.board_size_selected(size);
            create.size_detected = true;
        } else if let Some(min) = hw_info.min_board_size
            && sizes.contains(&min)
        {
            create.board_size_selected(min);
        }
    }

    let msg2 = if create.has_board()
        && let Some(mcu) = hw_info.mcu_variant
    {
        create.mcu_selected(mcu);
        create.select_latest_release(runtime_info.releases())
    } else {
        None
    };

    // Return combined tasks
    task_from_msgs!([msg1, msg2])
}

/// Flash the firmware to the connected device
pub fn flash_firmware(create: &mut Create, runtime_info: &RuntimeInfo) -> Task<AppMessage> {
    debug!("Flash firmware requested");
    if !create.is_busy() {
        match runtime_info.image().map(|imgs| imgs.full_image()) {
            Some(fw) => {
                create.state = State::Flashing;
                create.set_display_content("Flashing firmware...");
                Task::done(
                    DeviceMessage::FlashFirmware {
                        client: Client::Create,
                        hw_info: create.selected_hw_info,
                        data: fw,
                    }
                    .into(),
                )
            }
            None => {
                create.set_display_content("No firmware image available to flash.");
                Task::none()
            }
        }
    } else {
        warn!("Busy - skipping flash firmware");
        Task::none()
    }
}

/// Handle the result of a flash firmware operation.  A successful flash sets
/// whether the device stays on USB while running from the image written.  A
/// failed one leaves it unchanged, as a refused flash writes nothing.
pub fn flash_firmware_result(
    create: &mut Create,
    runtime_info: &RuntimeInfo,
    result: Result<(), String>,
) -> Task<AppMessage> {
    debug!(
        "Flash firmware result received: {}",
        if result.is_ok() { "OK" } else { "Error" }
    );

    // Check state
    if !create.is_busy() {
        internal_error!("FlashFirmwareResult received while not busy.");
    }
    if !create.is_flashing() {
        internal_error!("FlashFirmwareResult received while not flashing.");
    }

    // Update state to idle
    create.state = State::Idle;

    // Handle result
    match result {
        Ok(_) => {
            create.display_content = "Firmware flashed successfully.".to_string();
            let usb_run_capable = runtime_info
                .image()
                .is_some_and(|image| image.is_usb_run_capable());
            Task::done(DeviceMessage::SetUsbRunCapable(usb_run_capable).into())
        }
        Err(e) => {
            // Each further line sits beneath the first line's text
            let e = e.replace('\n', "\n  ");
            create.display_content = format!("Error flashing firmware:\n  - {e}");
            Task::none()
        }
    }
}
