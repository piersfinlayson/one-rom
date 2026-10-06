// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Handles parsing and detecting device information and flashing

use iced::Task;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use onerom_cli::Error as CliError;
use onerom_cli::device::known_board_size;
use onerom_cli::error::image_file_text;
use onerom_cli::otp::{check_board, escape_controls};
#[allow(unused_imports)]
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_config::mcu::Variant as McuVariant;
use onerom_fw_parser::{ParsedDevice, Parser, readers::MemoryReader};
use onerom_gen::FlashChips;
use std::path::PathBuf;

use crate::analyse::{Analyse, AnalyseState, FW_VERSION_METADATA, LoadedFile, Message, Source};
use crate::app::AppMessage;
use crate::device::{Address, BoardDetails, Client, Message as DeviceMessage};
use crate::hw::HardwareInfo;
use crate::studio::Message as StudioMessage;

// The question after a device whose firmware wasn't recognised
const DEVICE_QUESTION: &str = "Are you sure this device is a previously programmed One ROM?";

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
    // ROM firmware is a separate question, answered by is_recognised().  A
    // Lab passes it, and file_device_loaded() turns it away.
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
    if let Some(file) = analyse.file.as_ref() {
        // Get hardware info from firmware file.
        let hw_info = HardwareInfo::from_parsed(&file.fw_info);

        // A Fire image file's slots must match its length.
        if hw_info.is_fire()
            && let Err(e) = file
                .fw_info
                .check_image_file(file.data.len(), FlashChips::first_for(McuVariant::RP2350))
        {
            let text = format!(
                "{}\n  Download or build the image file again.",
                image_file_text(&e)
            );
            warn!("{text}");
            analyse.analysis_content = format!("Firmware flash failed:\n- {text}\n");
            return Task::none();
        }

        // Update state
        analyse.state = AnalyseState::Flashing;
        analyse.analysis_content = format!("Flashing {:?} to device...", file.path);

        // Send flash message to device module
        Task::done(
            DeviceMessage::FlashFirmware {
                client: Client::Analyse,
                hw_info,
                data: file.data.clone(),
            }
            .into(),
        )
    } else {
        analyse.analysis_content = "Cannot flash - no file loaded\n".to_string();
        Task::none()
    }
}

/// Handle firmware flash complete message.  A successful flash sets whether
/// the device stays on USB while running from the file written.  A failed one
/// leaves it unchanged, as a refused flash writes nothing.
pub fn firmware_flash_complete(
    analyse: &mut Analyse,
    result: Result<(), String>,
) -> Task<AppMessage> {
    // Check state
    if analyse.state != AnalyseState::Flashing {
        warn!(
            "Received FlashComplete message while not flashing (state is {})",
            analyse.state
        );
        analyse.analysis_content += "\nReceived unexpected flash complete message.\n";
        return Task::none();
    }

    // Update state
    analyse.state = AnalyseState::Idle;

    // Update analysis content based on result
    match result {
        Ok(()) => {
            analyse.analysis_content += "\nFirmware flash completed successfully.\n";
            let usb_run_capable = analyse
                .file
                .as_ref()
                .is_some_and(|file| file.fw_info.is_usb_run_capable());
            Task::done(DeviceMessage::SetUsbRunCapable(usb_run_capable).into())
        }
        Err(err) => {
            analyse.analysis_content += &format!("\nFirmware flash failed:\n- {err}\n");
            Task::none()
        }
    }
}

/// Attempt to detect connected device.
///
/// If `err` is Some, indicates an error from the previous read attempt,
/// which is logged and displayed.
#[allow(clippy::wildcard_enum_match_arm)]
pub fn detect_device(analyse: &mut Analyse, err: Option<String>) -> Task<AppMessage> {
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
        board_size: None,
        min_board_size: None,
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
    let flash = FlashChips::first_for(mcu);
    let address = Address::Absolute(flash.start);
    let words = (flash.end - flash.start) as usize / 4;
    let hw_info = HardwareInfo {
        board: None,
        model: None,
        mcu_variant: Some(mcu),
        board_size: None,
        min_board_size: None,
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
/// device flash, as parsing is the same process.  `path` is the file's, `None`
/// for device flash.
pub fn file_device_loaded(
    analyse: &mut Analyse,
    result: Result<(ParsedDevice, Vec<u8>), String>,
    path: Option<PathBuf>,
) -> Task<AppMessage> {
    let is_file = path.is_some();

    // The MCU of a failed device read.  Its board may be commissioned.
    let mut failed_mcu = None;

    match result {
        // A Lab is recognised, but Studio works with One ROM alone.
        Ok((ParsedDevice::Lab, _)) => {
            analyse.analysis_content =
                "One ROM Lab found. Studio currently doesn't support Lab.".to_string();
        }

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

            if let Some(path) = path {
                analyse.file = Some(LoadedFile {
                    path,
                    data,
                    fw_info: device.clone(),
                });
            }
            analyse.fw_info = Some(device);
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
                if let AnalyseState::Detecting(state) = &analyse.state {
                    failed_mcu = state.sample_mcu();
                }
                format!("Error loading/parsing device firmware:\n- {err}\n---\n{DEVICE_QUESTION}")
            }
        }
    }

    // Clear state back to idle as we're done reading
    analyse.state = AnalyseState::Idle;

    // Whether the device stays on USB while running comes from reading it,
    // never from a loaded file
    let usb_run_capable_task = if is_file {
        Task::none()
    } else {
        let usb_run_capable = analyse
            .fw_info
            .as_ref()
            .is_some_and(|device| device.is_usb_run_capable());
        Task::done(DeviceMessage::SetUsbRunCapable(usb_run_capable).into())
    };

    // Decide whether to send decoded hardware information to the rest of the
    // app.  Create uses this to pre-populate its own hardware info display.
    // A Fire device's board size and commissioned board are read from it
    // first, including where its firmware wasn't recognised.
    let read_details = |hw_info| {
        Task::done(AppMessage::Device(DeviceMessage::ReadBoardDetails {
            client: Client::Analyse,
            hw_info,
        }))
    };
    let hw_task = match analyse.fw_info.as_ref().map(HardwareInfo::from_parsed) {
        Some(hw_info) if !is_file && hw_info.is_fire() => read_details(hw_info),
        Some(hw_info) => Task::done(StudioMessage::HardwareInfo(Some(hw_info)).into()),
        None if failed_mcu.is_some() => read_details(HardwareInfo {
            mcu_variant: failed_mcu,
            ..HardwareInfo::default()
        }),
        None => Task::none(),
    };
    Task::chain(usb_run_capable_task, hw_task)
}

/// Shares a detected device's hardware information, with the board size and
/// commissioned board read from it.  The commissioned board replaces the
/// firmware's.
pub fn board_details_read(analyse: &mut Analyse, details: BoardDetails) -> Task<AppMessage> {
    let commissioned = details.commissioned.as_deref();
    let board_size = known_board_size(details.size);

    let hw_info = if let Some(device) = analyse.fw_info.as_ref() {
        let hw_info = HardwareInfo::from_parsed(device);
        if let Some(board) = hw_info.board
            && let Err(CliError::CommissionedBoardMismatch {
                commissioned,
                image,
            }) = check_board(commissioned, board, false)
        {
            analyse.analysis_content = format!(
                "This One ROM is commissioned as {commissioned}, but its firmware is for {image}.\n{}",
                analyse.analysis_content
            );
        }
        HardwareInfo {
            board_size,
            ..hw_info
        }
    } else {
        // The device's firmware wasn't recognised, so only a commissioned
        // board identifies it
        let Some(commissioned) = commissioned else {
            return Task::none();
        };
        analyse.analysis_content = analyse.analysis_content.replace(
            DEVICE_QUESTION,
            &format!(
                "This One ROM is commissioned as {}.",
                escape_controls(commissioned)
            ),
        );
        HardwareInfo {
            board_size,
            ..HardwareInfo::default()
        }
    };

    let hw_info = match commissioned.and_then(Board::try_from_str) {
        Some(board) => HardwareInfo {
            board: Some(board),
            model: Some(board.model()),
            mcu_variant: hw_info.mcu_variant.or(Some(McuVariant::RP2350)),
            ..hw_info
        },
        None if hw_info.board.is_none() => return Task::none(),
        None => hw_info,
    };
    Task::done(StudioMessage::HardwareInfo(Some(hw_info)).into())
}

pub fn stop_device(analyse: &mut Analyse) -> Task<AppMessage> {
    debug!("Stopping device");
    reboot_device(analyse, true)
}

pub fn run_device(analyse: &mut Analyse) -> Task<AppMessage> {
    debug!("Running device");
    reboot_device(analyse, false)
}

// A reboot from the File view leaves the analysis and loaded file in place,
// so the file can still be flashed.  From the Device view the device is
// analysed again once it has rebooted.
fn reboot_device(analyse: &mut Analyse, stopped: bool) -> Task<AppMessage> {
    let reboot_task = Task::done(AppMessage::Device(DeviceMessage::RebootDevice {
        client: Client::Analyse,
        stopped,
    }));
    if analyse.selected_source_tab == Source::File {
        analyse.state = AnalyseState::Rebooting;
        analyse.analysis_content += &format!("\n{}", analyse.state.content());
        reboot_task
    } else {
        Task::chain(analyse.start_analysis(AnalyseState::Rebooting), reboot_task)
    }
}

pub fn device_reboot_complete(
    analyse: &mut Analyse,
    result: Result<(), String>,
) -> Task<AppMessage> {
    match result {
        Ok(()) if analyse.selected_source_tab == Source::File => {
            analyse.analysis_content += "\nDevice rebooted successfully.";
            analyse.state = AnalyseState::Idle;
            Task::none()
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::RuntimeInfo;
    use futures::StreamExt;
    use iced_runtime::Action;
    use onerom_fw_parser::Sdrr;

    // Handles `msg` and returns the messages its Task sends
    fn update(analyse: &mut Analyse, msg: Message) -> Vec<AppMessage> {
        let task = analyse.update(&RuntimeInfo::default(), msg);
        let Some(stream) = iced_runtime::task::into_stream(task) else {
            return Vec::new();
        };
        let outputs = stream.filter_map(|action| async move {
            if let Action::Output(msg) = action {
                Some(msg)
            } else {
                None
            }
        });
        futures::executor::block_on(outputs.collect())
    }

    // Firmware without the USB plugin
    fn firmware() -> ParsedDevice {
        ParsedDevice::Original(Sdrr {
            flash: None,
            ram: None,
        })
    }

    fn load_file(analyse: &mut Analyse) -> Vec<AppMessage> {
        let result = Ok((firmware(), vec![0; 16]));
        update(analyse, Message::FileLoaded(PathBuf::from("a.bin"), result))
    }

    fn sets_usb_run_capable(msgs: &[AppMessage]) -> Option<bool> {
        msgs.iter().find_map(|msg| {
            if let AppMessage::Device(DeviceMessage::SetUsbRunCapable(capable)) = msg {
                Some(*capable)
            } else {
                None
            }
        })
    }

    /// Stop and Run on the File view reboot the device and leave the
    /// analysis and loaded file in place, whether or not the reboot works.
    #[test]
    fn a_reboot_on_the_file_view_keeps_the_loaded_file() {
        let failed = Err("reboot failed".to_string());
        for (msg, result) in [
            (Message::StopDevice, Ok(())),
            (Message::RunDevice, Ok(())),
            (Message::StopDevice, failed),
        ] {
            let stopped = matches!(msg, Message::StopDevice);
            let mut analyse = Analyse::new();
            load_file(&mut analyse);

            let sent = update(&mut analyse, msg);
            assert!(sent.iter().any(|msg| matches!(
                msg,
                AppMessage::Device(DeviceMessage::RebootDevice { stopped: s, .. }) if *s == stopped
            )));
            assert!(
                !sent
                    .iter()
                    .any(|msg| matches!(msg, AppMessage::Studio(StudioMessage::HardwareInfo(_))))
            );
            assert!(analyse.file.is_some());

            update(&mut analyse, Message::DeviceRebootComplete(result));
            assert!(analyse.state.is_idle());
            assert!(analyse.file.is_some());
            assert!(analyse.fw_info.is_some());
        }
    }

    /// Stop on the Device view analyses the device again once it has
    /// rebooted, and the loaded file is discarded.
    #[test]
    fn a_reboot_on_the_device_view_analyses_the_device_again() {
        let mut analyse = Analyse::new();
        load_file(&mut analyse);
        update(&mut analyse, Message::SourceSelected(Source::Device));

        update(&mut analyse, Message::StopDevice);
        assert!(analyse.file.is_none());

        let sent = update(&mut analyse, Message::DeviceRebootComplete(Ok(())));
        assert!(matches!(analyse.state, AnalyseState::Detecting(_)));
        assert!(
            sent.iter()
                .any(|msg| matches!(msg, AppMessage::Device(DeviceMessage::ReadDevice { .. })))
        );
    }

    /// Reading a device sets whether it stays on USB while running.  Loading
    /// a file doesn't.
    #[test]
    fn loading_a_file_leaves_run_capability_alone() {
        let mut analyse = Analyse::new();
        assert_eq!(sets_usb_run_capable(&load_file(&mut analyse)), None);

        let result = Ok((firmware(), vec![0; 16]));
        let sent = update(&mut analyse, Message::DeviceLoaded(result));
        assert_eq!(sets_usb_run_capable(&sent), Some(false));
    }

    /// A successful flash sets whether the device stays on USB while running
    /// from the file written.  A failed flash leaves it unchanged.
    #[test]
    fn a_flash_sets_run_capability_from_the_file() {
        for (result, expected) in [(Ok(()), Some(false)), (Err("failed".to_string()), None)] {
            let mut analyse = Analyse::new();
            load_file(&mut analyse);
            update(&mut analyse, Message::FlashFirmware);
            assert_eq!(analyse.state, AnalyseState::Flashing);

            let sent = update(&mut analyse, Message::FlashComplete(result));
            assert_eq!(sets_usb_run_capable(&sent), expected);
            assert!(analyse.file.is_some());
        }
    }

    /// A commissioned board whose firmware wasn't recognised displays its
    /// commissioned board in place of the question.
    #[test]
    fn a_commissioned_board_without_firmware_says_what_it_is() {
        let mut analyse = Analyse::new();
        let failed = |analyse: &Analyse| analyse.analysis_content.contains(DEVICE_QUESTION);
        analyse.analysis_content = format!("Blank device detected\n---\n{DEVICE_QUESTION}");

        let _ = board_details_read(&mut analyse, BoardDetails::default());
        assert!(failed(&analyse));

        let details = BoardDetails {
            size: None,
            commissioned: Some("fire-24-f".to_string()),
        };
        let _ = board_details_read(&mut analyse, details);
        assert!(!failed(&analyse));
        assert!(analyse.analysis_content.contains("fire-24-f"));
    }
}
