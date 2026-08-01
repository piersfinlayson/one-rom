// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Handles parsing and detecting device information and flashing

use iced::Task;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
#[allow(unused_imports)]
use onerom_config::fw::FirmwareVersion;
use onerom_config::mcu::Variant as McuVariant;
use onerom_fw_parser::{ParsedDevice, Parser, readers::MemoryReader};

use crate::analyse::{Analyse, AnalyseState, FW_VERSION_METADATA, Message};
use crate::app::AppMessage;
use crate::device::{Address, Client, Message as DeviceMessage};
use crate::hw::HardwareInfo;
use crate::studio::Message as StudioMessage;

/// Detect device state machine statuses
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum DetectState {
    /// Trying One ROM Ice - first step
    #[default]
    Ice,

    /// Trying One ROM Fire - second step
    Fire,

    /// Re-reading device flash after initial read, used when firmware is pre-
    /// v0.5.0 and more than 64KB of flash is needed to parse fully.
    Reread(McuVariant, FirmwareVersion),

    /// Detection complete
    Done,
}

impl std::fmt::Display for DetectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DetectState::Ice => write!(f, "Ice"),
            DetectState::Fire => write!(f, "Fire"),
            DetectState::Reread(_, _) => write!(f, "Reread"),
            DetectState::Done => write!(f, "Done"),
        }
    }
}

impl DetectState {
    /// Get the next state in the detection process
    pub fn next(&self) -> Self {
        match self {
            DetectState::Ice => DetectState::Fire,
            DetectState::Fire => DetectState::Done,
            DetectState::Done => DetectState::Done,
            DetectState::Reread(_, _) => DetectState::Done,
        }
    }

    /// Check if the detection process is complete
    pub fn is_done(&self) -> bool {
        matches!(self, DetectState::Done)
    }

    /// Get a sample MCU variant for this detection state.
    /// We assume a specific STM32 MCU - doesn't matter which one as we're
    /// just readig common stuff, like flash base - and the chip ID will work
    /// for all.
    pub fn sample_mcu(&self) -> Option<McuVariant> {
        match self {
            DetectState::Ice => Some(McuVariant::F411RE),
            DetectState::Fire => Some(McuVariant::RP2350),
            DetectState::Reread(mcu, _) => Some(*mcu),
            DetectState::Done => None,
        }
    }

    pub fn flash_base(&self) -> Address {
        match self.sample_mcu() {
            Some(mcu) => Address::Absolute(mcu.family().get_flash_base()),
            None => Address::FlashStart,
        }
    }
}

// Receive device data and process it
pub async fn handle_device_data(data: Vec<u8>) -> AppMessage {
    let data_len = data.len();

    // Before proceeding, check if the entire data is 0xFF - this indicates
    // a blank flash
    if data.iter().all(|&b| b == 0xFF) {
        // There's no point in trying a longer (>64KB) read because we
        // don't know precisely what sort of device is being used, and
        // hence how much flash it has.  We'll assume it's entirely blank.
        debug!("Read flash data ({data_len} bytes) is all 0xFF - indicating blank flash");
        return Message::DeviceLoaded(Err("Blank device detected".to_string())).into();
    }

    // We always pass in 0x08000000 as the parser's base address even if
    // RP2350 - parser will figure out what it's looking at.
    //
    // parse_device() detects the firmware generation and is infallible: it
    // returns a ParsedDevice for any input, so whether this is actually One
    // ROM firmware is a separate question, answered by is_recognised().
    let device = {
        let mut reader = MemoryReader::new(data.clone(), 0x08000000);
        let mut parser = Parser::new(&mut reader);
        parser.parse_device().await
    };

    if !device.is_recognised() {
        debug!("Device data contains no recognisable One ROM firmware information");
        return Message::DeviceLoaded(Err("No One ROM firmware information found".to_string()))
            .into();
    }

    // The parse succeeded, but we may still need to read and parse from the
    // device again - first time around we only read 64KB of flash, and in
    // pre-v0.5.0 firmware, often more than this is needed.
    if data_len > (64 * 1024) {
        // We read more than 64KB, so whatever happened just return the
        // result
        debug!("Firmware data length > 64KB ({data_len} bytes), so not re-reading");
        return Message::DeviceLoaded(Ok((device, data))).into();
    }

    // Decide whether a full-flash re-read is needed.  Only original-format
    // firmware can require one: schema-format firmware is v0.7.0+, and keeps
    // its metadata within the first 64KB by construction.
    let reread = reread_required(&device);

    match reread {
        Some((mcu, version)) => Message::RereadDevice(mcu, version).into(),
        None => Message::DeviceLoaded(Ok((device, data))).into(),
    }
}

// Determine whether a parsed device needs its full flash re-reading to be
// parsed completely, returning the MCU variant and firmware version needed to
// perform that read.
fn reread_required(device: &ParsedDevice) -> Option<(McuVariant, FirmwareVersion)> {
    // Schema-format firmware never needs a second read.
    let Some(sdrr) = device.as_original() else {
        trace!("Schema-format firmware - 64KB read is sufficient");
        return None;
    };

    // No flash information means there's nothing to re-read on the strength
    // of.  is_recognised() may still have passed on the RAM information alone.
    let info = sdrr.flash.as_ref()?;

    if info.version >= FW_VERSION_METADATA || info.parse_errors.is_empty() {
        // Firmware is v0.5.0 or later, so 64KB read is sufficient, or we
        // parsed everything OK anyway
        trace!("Firmware is v0.5.0 or later, or parsed successfully");
        return None;
    }

    // The MCU info wasn't decoded.  This is worrying, and means we can't
    // confidently predict the size, so just return as is.
    let Some(mcu) = info.mcu_variant else {
        info!(
            "MCU variant {} {} not detected during firmware decode, cannot re-read full flash",
            info.stm_line, info.stm_storage
        );
        return None;
    };

    // Ready to re-read full flash
    Some((mcu, info.version))
}

/// Flash firmware to device
pub fn flash_firmware(analyse: &mut Analyse) -> Task<AppMessage> {
    // Check if busy
    if analyse.state.is_busy() {
        warn!(
            "Cannot flash firmware - Analyse tab is busy ({})",
            analyse.state
        );
        analyse.analysis_content += "\nCannot flash firmware - Analyse tab is busy.\n";
        return Task::none();
    }

    // Check if we have firmware data to flash
    if let Some(device_fw_data) = analyse.file_contents.as_ref()
        && let Some(filename) = analyse.fw_file.as_ref()
    {
        // Get hardware info from firmware file.
        let hw_info = analyse
            .fw_info
            .as_ref()
            .map(HardwareInfo::from_parsed)
            .unwrap_or_default();

        // Update state
        analyse.state = AnalyseState::Flashing;
        analyse.analysis_content = format!("Flashing {filename:?} to device...");

        // Send flash message to device module
        Task::done(
            DeviceMessage::FlashFirmware {
                client: Client::Analyse,
                hw_info,
                data: device_fw_data.clone(),
            }
            .into(),
        )
    } else {
        analyse.analysis_content = "Cannot flash - no file loaded\n".to_string();
        Task::none()
    }
}

/// Handle firmware flash complete message
pub fn firmware_flash_complete(analyse: &mut Analyse, result: Result<(), String>) {
    // Check state
    if analyse.state != AnalyseState::Flashing {
        warn!(
            "Received FlashComplete message while not flashing (state is {})",
            analyse.state
        );
        analyse.analysis_content += "\nReceived unexpected flash complete message.\n";
        return;
    }

    // Update state
    analyse.state = AnalyseState::Idle;

    // Update analysis content based on result
    match result {
        Ok(()) => {
            analyse.analysis_content += "\nFirmware flash completed successfully.\n";
        }
        Err(err) => {
            analyse.analysis_content += &format!("\nFirmware flash failed:\n- {err}\n");
        }
    }
}

/// Attempt to detect connected device.
///
/// If `err` is Some, indicates an error from the previous read attempt,
/// which is logged and displayed.
#[allow(clippy::wildcard_enum_match_arm)]
pub fn detect_device(analyse: &mut Analyse, err: Option<String>) -> Task<AppMessage> {
    // Clear out previous firmware info and file contents
    analyse.file_contents = None;

    // If there was an error from the previous read attempt, log it
    if let Some(err) = err {
        analyse.fw_info = None;
        analyse.analysis_content += &format!("\nError reading from device:\n- {err}\n");
    }

    // Move onto next detection state
    let new_state = match &analyse.state {
        AnalyseState::Detecting(state) => AnalyseState::Detecting(state.next()),
        _ => AnalyseState::Detecting(DetectState::default()),
    };
    let detect_state = match new_state.clone() {
        AnalyseState::Detecting(state) => state,
        _ => unreachable!(),
    };

    // Check if detection is done
    if detect_state.is_done() {
        analyse.fw_info = None;
        analyse.analysis_content += "---\nDevice detection failed - neither One ROM Ice nor One ROM Fire hardware detected.\nHave you connected the probe to the One ROM correctly, and does the One ROM have power?";
        analyse.state = AnalyseState::Idle;
        return Task::none();
    }

    // Actually do a detection, based on current (new) state.  First, get the
    // Task to start analysis display update
    let start_analysis_task = analyse.start_analysis(new_state);

    // Produce the hardware info for this detection attempt
    let hw_info = HardwareInfo {
        board: None,
        model: None,
        mcu_variant: detect_state.sample_mcu(),
    };

    // Produce the Task to read device flash
    let read_device_task = Task::done(AppMessage::Device(DeviceMessage::ReadDevice {
        client: Client::Analyse,
        hw_info,
        address: detect_state.flash_base(),
        words: 65536 / 4,
    }));

    // Chain the two tasks together
    Task::chain(start_analysis_task, read_device_task)
}

/// Reread device flash after initial read, as this was an older firmware
/// needing a full flash read
pub fn reread_device(
    analyse: &mut Analyse,
    mcu: McuVariant,
    fw_version: FirmwareVersion,
) -> AppMessage {
    // Indicate we're rereading
    debug!(
        "Re-reading full flash for MCU variant {} with fw v{}.{}.{}",
        mcu,
        fw_version.major(),
        fw_version.minor(),
        fw_version.patch()
    );
    analyse.analysis_content += &format!(
        "\nRe-reading full flash from {mcu} based device with firmware v{}.{}.{}...",
        fw_version.major(),
        fw_version.minor(),
        fw_version.patch()
    );
    analyse.state = AnalyseState::Detecting(DetectState::Reread(mcu, fw_version));

    // Build the message re-read the flash (and re-parse).  We now have the
    // MCU variant, so can get the full flash size - this is what we need to
    // read.
    let address = Address::Absolute(mcu.family().get_flash_base());
    let words = mcu.flash_storage_bytes() / 4;
    let hw_info = HardwareInfo {
        board: None,
        model: None,
        mcu_variant: Some(mcu),
    };

    // Send read message to device module to read the flash
    DeviceMessage::ReadDevice {
        client: Client::Analyse,
        hw_info,
        address,
        words,
    }
    .into()
}

/// Common function to handle firmware being loaded from either a file or
/// device flash, as parsing is the same process
pub fn file_device_loaded(
    analyse: &mut Analyse,
    result: Result<(ParsedDevice, Vec<u8>), String>,
    is_file: bool,
) -> Task<AppMessage> {
    match result {
        // The actual read and parse succeeded, and found a One ROM
        Ok((device, data)) => {
            // Turn the parsed device into JSON
            let json = serde_json::to_string_pretty(&device).map_err(|e| e.to_string());

            // Handle JSON parse result, updating analysis content (the window
            // display) accordingly
            analyse.analysis_content = match json {
                Ok(j) => j,
                Err(e) => format!("Error serializing info to JSON: {}", e),
            };

            // Store firmware info and file contents
            analyse.fw_info = Some(device);
            analyse.file_contents = if is_file { Some(data) } else { None };
        }

        // The read failed, or what we read wasn't a One ROM
        Err(err) => {
            // Update analysis content with error message, depending on whether
            // this was a file or device read
            analyse.fw_info = None;
            analyse.analysis_content = if is_file {
                format!(
                    "Error loading/parsing file:\n- {}\n---\nAre you sure this is a valid One ROM firmware .bin file?",
                    err,
                )
            } else {
                format!(
                    "Error loading/parsing device firmware:\n- {}\n---\nAre you sure this device is a previously programmed One ROM?",
                    err,
                )
            }
        }
    }

    // Clear state back to idle as we're done reading
    analyse.state = AnalyseState::Idle;

    // Figure out whether this device is capable of running firmware while
    // connected via USB.  We'll likely extend this to other properties in
    // future
    let usb_run_capable = analyse
        .fw_info
        .as_ref()
        .is_some_and(|device| device.is_usb_run_capable());
    let usb_run_capable_task = Task::done(AppMessage::Device(DeviceMessage::SetUsbRunCapable(
        usb_run_capable,
    )));

    // Decide whether to send decoded hardware information to the rest of the
    // app.  Create uses this to pre-populate its own hardware info display.
    match share_hw_info(analyse) {
        Some(msg) => Task::chain(usb_run_capable_task, Task::done(msg)),
        None => usb_run_capable_task,
    }
}

// Decide whether to share decoded hardware info with rest of app
fn share_hw_info(analyse: &mut Analyse) -> Option<AppMessage> {
    // We have some information so share it
    analyse.fw_info.as_ref().map(|device| {
        AppMessage::Studio(StudioMessage::HardwareInfo(Some(
            HardwareInfo::from_parsed(device),
        )))
    })
}

pub fn stop_device(analyse: &mut Analyse) -> Task<AppMessage> {
    debug!("Stopping device");
    let start_task = analyse.start_analysis(AnalyseState::Rebooting);
    let reboot_task = Task::done(AppMessage::Device(DeviceMessage::RebootDevice {
        client: Client::Analyse,
        stopped: true,
    }));
    Task::chain(start_task, reboot_task)
}

pub fn run_device(analyse: &mut Analyse) -> Task<AppMessage> {
    debug!("Running device");
    let start_task = analyse.start_analysis(AnalyseState::Rebooting);
    let reboot_task = Task::done(AppMessage::Device(DeviceMessage::RebootDevice {
        client: Client::Analyse,
        stopped: false,
    }));
    Task::chain(start_task, reboot_task)
}

pub fn device_reboot_complete(
    analyse: &mut Analyse,
    result: Result<(), String>,
) -> Task<AppMessage> {
    match result {
        Ok(()) => {
            analyse.analysis_content += "\nDevice rebooted successfully, re-analysing...";
            detect_device(analyse, None)
        }
        Err(e) => {
            analyse.analysis_content += &format!("\nDevice reboot failed: {e}");
            analyse.state = AnalyseState::Idle;
            Task::none()
        }
    }
}
