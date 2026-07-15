// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

pub mod manifest;

use iced::alignment::Vertical::Bottom;
use iced::time::Instant;
use iced::widget::{Space, Stack, column, row};
use iced::{Element, Length, Subscription, Task};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;

use crate::analyse::{Analyse, Message as AnalyseMessage};
use crate::create::{Create, Message as CreateMessage};
use crate::device::{Device, Message as DeviceMessage, get_devices_startup};
use crate::log::{Level, Log, LogEntry, Message as LogMessage};
use crate::studio::{Message as StudioMessage, RuntimeInfo, Studio, StudioTab};
use crate::style::{Message as StyleMessage, Style};
use crate::update_app_manifest;

// How often to tick progress updates
const PROGRESS_TICK_INTERVAL: Duration = Duration::from_millis(500);

/// Produces a subscription emitting a progress tick message at a regular
/// interval.
///
/// Takes a message *constructor*, not a message value.  `Subscription::map()`
/// asserts at runtime that the closure handed to it is non-capturing: iced
/// identifies subscriptions across update cycles by hashing them, and a
/// closure holding captured state has no stable identity to hash.  Capturing
/// a message value here and cloning it therefore panics for every message
/// type that isn't zero-sized.
///
/// A caller-supplied `|_| Message::ProgressTick` captures nothing, so it is
/// zero-sized and satisfies the assert whatever the message type.
pub fn progress_tick_subscription<T, F>(tick: F) -> Subscription<T>
where
    T: 'static,
    F: Fn(Instant) -> T + Clone + Send + 'static,
{
    iced::time::every(PROGRESS_TICK_INTERVAL).map(tick)
}

// How often to re-read the application manifest
const MANIFEST_REFRESH_INTERVAL: Duration = Duration::from_mins(5);

/// Kicks off any startup tasks for the app
///
/// - Select the default top-level tab
/// - Fetch One ROM releases from the network
pub fn startup_task() -> Task<AppMessage> {
    Task::batch([
        Task::done(AppMessage::Log(LogMessage::AddEntry(LogEntry::new(
            Level::Info,
            "One ROM Studio started".to_string(),
        )))),
        Task::done(AppMessage::UpdateManifest(true)),
        Task::run(get_devices_startup(), |msg| msg),
    ])
}

/// Top level Message enum - container for all sub-module messages
#[derive(Debug, Clone)]
pub enum AppMessage {
    /// Analyse pane messages
    Analyse(AnalyseMessage),

    /// Create pane messages
    Create(CreateMessage),

    /// Device related messages (debug probe and USB operations)
    Device(DeviceMessage),

    /// Log related messages
    Log(LogMessage),

    /// Studio (main application) messages.  May be used for high level
    /// operations, and also for when a single message may need to have
    /// effects on multiple sub-modules.
    Studio(StudioMessage),

    /// Style related messages
    Style(StyleMessage),

    /// Help related messages
    Help(bool),

    /// No-operation message, where it's easier to return a message than none
    /// at all (for example in match arms).
    Nop,

    /// Update the manifest from disk/network.  Argument is whether being run
    /// at startup - if so, trigger release/config manifest fetches afterwards.
    UpdateManifest(bool),
    ManifestUpdated(bool),
}

impl std::fmt::Display for AppMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppMessage::Analyse(msg) => write!(f, "Analyse::{msg}"),
            AppMessage::Create(msg) => write!(f, "Create::{msg}"),
            AppMessage::Device(msg) => write!(f, "Device::{msg}"),
            AppMessage::Log(msg) => write!(f, "Log::{msg}"),
            AppMessage::Studio(msg) => write!(f, "Studio::{msg}"),
            AppMessage::Style(msg) => write!(f, "Style::{msg}"),
            AppMessage::Help(flag) => write!(f, "Help({flag})"),
            AppMessage::Nop => write!(f, "Nop"), // Write Nop message
            AppMessage::UpdateManifest(flag) => write!(f, "UpdateManifest({flag})"),
            AppMessage::ManifestUpdated(flag) => write!(f, "ManifestUpdated({flag})"),
        }
    }
}

/// Main application structure
pub struct App<'a> {
    create: Create,
    analyse: Analyse,
    device: Device,
    log: Log,
    studio: Studio,
    style: Style<'a>,
    help: bool,
}

impl<'a> Default for App<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> App<'a> {
    pub fn new() -> Self {
        Self {
            create: Create::new(),
            analyse: Analyse::new(),
            device: Device::new(),
            style: Style::new(),
            log: Log::new(),
            studio: Studio::new(),
            help: false,
        }
    }

    fn runtime_info(&self) -> &RuntimeInfo {
        self.studio.runtime_info()
    }

    pub fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        let runtime_info = self.runtime_info().clone();

        // Log non-log no-op messages
        match &message {
            AppMessage::Nop | AppMessage::Log(_) => {} // Do not trace log Nops
            m => trace!("{m}"),
        }
        match message {
            AppMessage::Analyse(fw_msg) => self.analyse.update(&runtime_info, fw_msg),
            AppMessage::Device(dev_msg) => self.device.update(&runtime_info, dev_msg),
            AppMessage::Create(prog_msg) => {
                self.create.update(&runtime_info, &self.device, prog_msg)
            }
            AppMessage::Studio(studio_msg) => self.studio.update(studio_msg),
            AppMessage::Log(log_msg) => self.log.update(&runtime_info, log_msg),
            AppMessage::Style(style_msg) => self.style.update(style_msg).map(|m| m.into()),
            AppMessage::Help(flag) => {
                self.help = flag;
                Task::none()
            }
            AppMessage::Nop => Task::none(), // Do nothing with Nop messages
            AppMessage::UpdateManifest(flag) => Task::future(update_app_manifest(flag)),
            AppMessage::ManifestUpdated(flag) => {
                // `flag` inicates whether this update was run at startup
                match flag {
                    true => Task::batch([
                        Task::done(AppMessage::Studio(StudioMessage::FetchReleases)),
                        Task::done(AppMessage::Studio(StudioMessage::FetchConfigs)),
                    ]),
                    false => Task::none(),
                }
            }
        }
    }

    pub fn view(&self) -> Element<'_, AppMessage> {
        let runtime_info = self.runtime_info();

        let serious_errors = self.log.serious_errors_occurred();
        let top_left_corner = column![
            Style::text_studio_h1(),
            self.studio.top_level_buttons(serious_errors),
        ]
        .spacing(20);

        let top_right_corner = self.device.view(&self.style);

        let top_row = row![
            column![top_left_corner, Space::with_height(5.0)].width(Length::FillPortion(5)),
            top_right_corner.width(Length::FillPortion(4)),
        ]
        .align_y(Bottom)
        .height(110)
        .spacing(20);

        let content_row = match self.studio.active_tab() {
            StudioTab::Analyse => self.analyse.view(runtime_info, &self.device),
            StudioTab::Create => self.create.view(runtime_info, &self.device, &self.style),
            StudioTab::Log => self.log.view(runtime_info),
        };

        let main_view = column![
            top_row,
            Space::with_height(20.0),
            Style::horiz_line(),
            Space::with_height(20.0),
            content_row,
            Style::blank_space(),
            Style::horiz_line(),
            Space::with_height(20.0),
            self.style.footer(runtime_info),
        ]
        .padding([20, 20]);

        if self.help {
            let help_overlay = self.device.help_overlay();
            let overlay = Style::overlay_container(help_overlay);

            Stack::new().push(main_view).push(overlay).into()
        } else {
            main_view.into()
        }
    }

    pub fn subscription(&self) -> Subscription<AppMessage> {
        let manifest_reread =
            iced::time::every(MANIFEST_REFRESH_INTERVAL).map(|_| AppMessage::UpdateManifest(false));

        Subscription::batch(vec![
            manifest_reread,
            self.studio.subscription().map(AppMessage::Studio),
            self.analyse.subscription().map(AppMessage::Analyse),
            self.create.subscription().map(AppMessage::Create),
            self.device.subscription().map(AppMessage::Device),
            self.log.subscription(),
        ])
    }
}

impl From<StudioMessage> for AppMessage {
    fn from(msg: StudioMessage) -> Self {
        AppMessage::Studio(msg)
    }
}

impl From<CreateMessage> for AppMessage {
    fn from(msg: CreateMessage) -> Self {
        AppMessage::Create(msg)
    }
}

impl From<DeviceMessage> for AppMessage {
    fn from(msg: DeviceMessage) -> Self {
        AppMessage::Device(msg)
    }
}

impl From<AnalyseMessage> for AppMessage {
    fn from(msg: AnalyseMessage) -> Self {
        AppMessage::Analyse(msg)
    }
}

impl From<StyleMessage> for AppMessage {
    fn from(msg: StyleMessage) -> Self {
        AppMessage::Style(msg)
    }
}

impl From<LogMessage> for AppMessage {
    fn from(msg: LogMessage) -> Self {
        AppMessage::Log(msg)
    }
}

// For each variant type
impl From<CreateMessage> for Option<AppMessage> {
    fn from(msg: CreateMessage) -> Self {
        Some(AppMessage::Create(msg))
    }
}

impl From<DeviceMessage> for Option<AppMessage> {
    fn from(msg: DeviceMessage) -> Self {
        Some(AppMessage::Device(msg))
    }
}

impl From<StudioMessage> for Option<AppMessage> {
    fn from(msg: StudioMessage) -> Self {
        Some(AppMessage::Studio(msg))
    }
}

impl From<AnalyseMessage> for Option<AppMessage> {
    fn from(msg: AnalyseMessage) -> Self {
        Some(AppMessage::Analyse(msg))
    }
}

impl From<StyleMessage> for Option<AppMessage> {
    fn from(msg: StyleMessage) -> Self {
        Some(AppMessage::Style(msg))
    }
}

impl From<LogMessage> for Option<AppMessage> {
    fn from(msg: LogMessage) -> Self {
        Some(AppMessage::Log(msg))
    }
}

/// Studio version information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl Default for Version {
    fn default() -> Self {
        Self {
            major: env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap_or(0),
            minor: env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0),
            patch: env!("CARGO_PKG_VERSION_PATCH").parse().unwrap_or(0),
        }
    }
}

impl Version {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}