// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Analyse tab view methods

use iced::widget::{Button, Space, column, row};
use iced::{Element, Length};

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
#[allow(unused_imports)]
use onerom_config::fw::FirmwareVersion;

use crate::analyse::{Analyse, AnalyseState, Message, Source};
use crate::app::AppMessage;
use crate::device::Device;
use crate::hw::HardwareInfo;
use crate::studio::RuntimeInfo;
use crate::style::Style;

/// Default analysis content text
pub const ANALYSIS_TEXT_DEFAULT: &str = "No firmware analysed";

// Button names
const DEVICE_BUTTON_NAME: &str = "Device";
const FILE_BUTTON_NAME: &str = "File";
const SOURCE_DEVICE_BUTTON_NAME: &str = "Detect Device";
const SOURCE_FILE_BUTTON_NAME: &str = "Select File";
const FLASH_BUTTON_NAME: &str = "Flash";

/// Main analyse tab view
pub fn view<'a>(
    analyse: &'a Analyse,
    runtime_info: &'a RuntimeInfo,
    device: &'a Device,
) -> Element<'a, AppMessage> {
    // Get the hardware information - we'll be needing this
    let hw_info = runtime_info.hw_info();

    // Create the top-level buttons row - to select a source
    let buttons = top_level_buttons(analyse, device);

    // Create everything above the analysis content box
    let pre_content = column![
        select_fw_source(),
        buttons,
        Style::horiz_line(),
        fw_content_heading(analyse, hw_info),
    ]
    .spacing(20);

    // Put together the entire content
    column![
        pre_content,
        Space::with_height(Length::Fixed(20.0)),
        Style::container(fw_content(analyse)),
    ]
    .into()
}

// Top level buttons row - source selection and control
fn top_level_buttons<'a>(analyse: &'a Analyse, device: &'a Device) -> Element<'a, AppMessage> {
    // A row with the source selection buttons on the left and the source
    // control button on the right
    row![
        fw_source_buttons(analyse),
        Space::with_width(Length::Fill),
        fw_source_control(analyse, device),
    ]
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

// Firmware source selection buttons
fn fw_source_buttons<'a>(analyse: &'a Analyse) -> Element<'a, AppMessage> {
    // Determine button states based on selected tab
    let is_file_selected = matches!(analyse.selected_source_tab, Source::File);

    // Enable the file button only if not already selected and not busy
    let file_message = if is_file_selected {
        None
    } else {
        if analyse.state.is_idle() {
            Some(Message::SourceSelected(Source::File).into())
        } else {
            None
        }
    };

    // Enable the device button only if not already selected and not busy
    let device_message = if is_file_selected {
        if analyse.state.is_idle() {
            Some(Message::SourceSelected(Source::Device).into())
        } else {
            None
        }
    } else {
        None
    };

    // Create the buttons
    let file_button = Style::text_button_small(FILE_BUTTON_NAME, file_message, is_file_selected);
    let device_button =
        Style::text_button_small(DEVICE_BUTTON_NAME, device_message, !is_file_selected);

    // Stick them in a row
    row![file_button, device_button].spacing(20).into()
}

// Firmware source control buttons - on the right, select file, etc
fn fw_source_control<'a>(analyse: &'a Analyse, device: &'a Device) -> Element<'a, AppMessage> {
    // Show appropriate control based on selected source
    let source_button = match analyse.selected_source_tab {
        Source::Device => fw_source_device_control(analyse, device),
        Source::File => fw_source_file_control(analyse),
    };

    // Create an empty row
    let row = row![];

    // Show flash file if on file source tab, and always the buttons for this
    // source
    if analyse.selected_source_tab == Source::File && analyse.file_contents.is_some() {
        row.push(flash_file_button(analyse, device))
    } else {
        row
    }
    .push_maybe(if device.is_usb_run_capable() {
        Some(reboot_button(analyse, device))
    } else {
        None
    })
    .push(source_button)
    .spacing(20)
    .into()
}

fn reboot_button<'a>(analyse: &'a Analyse, device: &'a Device) -> Button<'a, AppMessage> {
    let (content, message) = if device.is_running() {
        (
            "Stop",
            if analyse.state.is_idle() {
                Some(Message::StopDevice.into())
            } else {
                None
            },
        )
    } else {
        (
            "Run",
            if analyse.state.is_idle() {
                Some(Message::RunDevice.into())
            } else {
                None
            },
        )
    };

    Style::text_button_small(content, message, analyse.state.is_idle())
}

// Heading for firmware source selection
fn select_fw_source() -> Element<'static, AppMessage> {
    row![Style::text_h3("Select Firmware Source")].into()
}

// Heading for analysis content window.
fn fw_content_heading<'a>(
    analyse: &'a Analyse,
    hw_info: Option<&'a HardwareInfo>,
) -> Element<'a, AppMessage> {
    // Create the actual heading
    let heading = Style::text_h3("Analysis");

    if let Some(hw_info) = hw_info {
        // Add hardware information as we have some
        let version = analyse.fw_info.as_ref().and_then(|device| device.version());
        let metadata = analyse
            .fw_info
            .as_ref()
            .map_or(Some(false), |device| device.metadata_present());
        let info_row = Style::hw_info_row(
            version,
            metadata,
            hw_info.model,
            hw_info.board,
            hw_info.mcu_variant,
            false,
        );

        row![heading, Space::with_width(Length::Fill), info_row,]
            .align_y(iced::alignment::Vertical::Center)
    } else {
        row![heading]
    }
    .into()
}

// Flash firmware button
fn flash_file_button<'a>(analyse: &'a Analyse, device: &'a Device) -> Button<'a, AppMessage> {
    // Only highlight if ready to flash
    let highlighted = analyse.state.is_idle()
        && device.is_ready()
        && analyse.fw_info.is_some()
        && !device.is_running();

    // Figure out whether the button will do anything (based on highlighted
    // state)
    let message = if analyse.state.is_idle() && device.is_ready() && analyse.fw_info.is_some() {
        Some(Message::FlashFirmware.into())
    } else {
        None
    };

    // Button content changes based on state
    let content = if analyse.state.is_idle() {
        FLASH_BUTTON_NAME
    } else {
        "Flashing..."
    };

    // Create the button
    Style::text_button_small(content, message, highlighted)
}

// Device source control button
fn fw_source_device_control<'a>(
    analyse: &'a Analyse,
    device: &'a Device,
) -> Button<'a, AppMessage> {
    // Only highlight if ready to detect
    let highlighted = analyse.state.is_idle() && device.is_ready();

    // Figure out whether the button will do anything (based on highlighted
    // state)
    let message = if analyse.state.is_idle() && device.is_ready() {
        Some(AppMessage::Analyse(Message::DetectDevice))
    } else {
        None
    };

    // Button content changes based on state
    let content = if analyse.state.is_idle() {
        SOURCE_DEVICE_BUTTON_NAME
    } else {
        "Detecting..."
    };

    // Create the button
    Style::text_button_small(content, message, highlighted)
}

// File source control button
fn fw_source_file_control<'a>(analyse: &'a Analyse) -> Button<'a, AppMessage> {
    // Only enable this button if file not being loaded
    let file_control_message = if analyse.state.is_idle() {
        Some(Message::SelectFile.into())
    } else {
        None
    };

    // Button content changes based on state
    let content = if analyse.state != AnalyseState::Loading {
        SOURCE_FILE_BUTTON_NAME
    } else {
        "Loading..."
    };

    // Highlight if idle
    let highlight = analyse.state.is_idle();

    // Create the button
    Style::text_button_small(content, file_control_message, highlight)
}

// Firmware analysis content box - shows whatever analysis text we have stored
fn fw_content<'a>(analyse: &'a Analyse) -> Element<'a, AppMessage> {
    Style::box_scrollable_text(&analyse.analysis_content, 350.0, true).into()
}