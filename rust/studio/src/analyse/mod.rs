// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Analyse functionality
//!
//! Analyse parses a device or firmware file's contents and displays
//! information about it.

mod device;
mod file;
mod msg;
mod view;

use iced::{Element, Subscription, Task};
use std::path::PathBuf;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
#[allow(unused_imports)]
use onerom_config::fw::FirmwareVersion;
use onerom_fw_parser::ParsedDevice;

use crate::app::{AppMessage, progress_tick_subscription};
use crate::device::Device;
use crate::studio::{Message as StudioMessage, RuntimeInfo};

pub use device::DetectState;
pub use msg::Message;

// First firmware version with explicit metadata support, requiring only the
// first 64KB of firwmare to be read for full parsing.
const FW_VERSION_METADATA: FirmwareVersion = FirmwareVersion::new(0, 5, 0, 0);

/// Firmware source
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Device,
    #[default]
    File,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Device => write!(f, "Device"),
            Source::File => write!(f, "File"),
        }
    }
}

/// Analyse tab state
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum AnalyseState {
    #[default]
    Idle,
    Loading,
    Detecting(DetectState),
    Flashing,
    Rebooting,
}

impl std::fmt::Display for AnalyseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnalyseState::Idle => write!(f, "Idle"),
            AnalyseState::Loading => write!(f, "Loading"),
            AnalyseState::Detecting(state) => write!(f, "Detecting ({})", state),
            AnalyseState::Flashing => write!(f, "Flashing"),
            AnalyseState::Rebooting => write!(f, "Rebooting"),
        }
    }
}

impl AnalyseState {
    /// Check if the analyse tab is busy
    pub fn is_busy(&self) -> bool {
        !self.is_idle()
    }

    /// Check if the analyse tab is idle
    pub fn is_idle(&self) -> bool {
        matches!(self, AnalyseState::Idle)
    }

    /// Get content text for the current state, used in content "window"
    pub fn content(&self) -> String {
        match self {
            AnalyseState::Idle => view::ANALYSIS_TEXT_DEFAULT.to_string(),
            AnalyseState::Loading => "Loading firmware...".to_string(),
            AnalyseState::Detecting(state) => format!("Trying to detect One ROM {state} ..."),
            AnalyseState::Flashing => "Flashing firmware...".to_string(),
            AnalyseState::Rebooting => "Rebooting device...".to_string(),
        }
    }
}

/// Analyse tab state and content
#[derive(Debug, Clone)]
pub struct Analyse {
    // Overall state
    state: AnalyseState,

    // Currently selected source
    selected_source_tab: Source,

    // Analysis content (text window)
    analysis_content: String,

    // Loaded firmware information.
    //
    // Holds either firmware generation - pre-v0.7.0 (original) or v0.7.0+
    // (schema).  Only ever populated from a recognised One ROM: an
    // unrecognised parse is reported as an error and leaves this None.
    fw_info: Option<ParsedDevice>,

    // Loaded firmware file path
    fw_file: Option<PathBuf>,

    // Loaded firmware file contents
    file_contents: Option<Vec<u8>>,
}

impl Default for Analyse {
    fn default() -> Self {
        Self {
            state: Default::default(),
            analysis_content: view::ANALYSIS_TEXT_DEFAULT.to_string(),
            selected_source_tab: Default::default(),
            fw_info: Default::default(),
            fw_file: Default::default(),
            file_contents: Default::default(),
        }
    }
}

impl Analyse {
    /// Top-level button name for analyse tab
    pub const fn top_level_button_name() -> &'static str {
        "Analyse"
    }

    /// Constructor
    pub fn new() -> Self {
        Self::default()
    }

    /// Analyse Message handler
    pub fn update(&mut self, _runtime_info: &RuntimeInfo, message: Message) -> Task<AppMessage> {
        msg::message(self, _runtime_info, message)
    }

    /// Check if the analyse tab is busy performing an operation
    fn is_busy(&self) -> bool {
        self.state.is_busy()
    }

    // Handle progress tick message when the analyse tab is busy
    fn progress_tick(&mut self) {
        if self.is_busy() {
            self.analysis_content += "."
        }
    }

    // Clear hardware info in the main app
    fn clear_hw_info(&self) -> Task<AppMessage> {
        Task::done(StudioMessage::HardwareInfo(None).into())
    }

    // Start analysis process - sends message to clear previous hw info in the
    // main app
    fn start_analysis(&mut self, state: AnalyseState) -> Task<AppMessage> {
        self.state = state;
        self.analysis_content += &self.state.content().to_string();
        self.fw_info = None;
        self.file_contents = None;
        self.clear_hw_info()
    }

    /// Render the analyse tab view
    pub fn view<'a>(
        &'a self,
        runtime_info: &'a RuntimeInfo,
        device: &'a Device,
    ) -> Element<'a, AppMessage> {
        view::view(self, runtime_info, device)
    }

    /// Handle Analyse subscriptions
    pub fn subscription(&self) -> Subscription<Message> {
        if self.is_busy() {
            // Constructor, not a value - see progress_tick_subscription().
            progress_tick_subscription(|_| Message::ProgressTick)
        } else {
            Subscription::none()
        }
    }
}