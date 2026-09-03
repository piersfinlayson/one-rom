// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! A shell hosting both prototype screens in one window.
//!
//! This file is the whole cost of composition: the shared state, a switch, a
//! device picker, message wrapping in both directions, and the one place that
//! tells a screen somebody else changed something it is showing.  Neither
//! screen crate knows this one exists.
//!
//! Run it with `cargo run -p studiov2-shell`.  Either screen still runs on its
//! own — `cargo run -p studiov2-slot-builder`, `cargo run -p
//! studiov2-log-viewer` — against the same [`Shared`] this holds.

mod dev;

use iced::widget::{button, column, container, pick_list, row, rule, text};
use iced::{Alignment, Element, Font, Length, Subscription, Task, window};

use studiov2_generated as generated;
use studiov2_log_viewer::screen as logs;
use studiov2_shared::{Device, Shared, device, style};
use studiov2_slot_builder as builder;

fn main() -> iced::Result {
    iced::application(Shell::boot, Shell::update, Shell::view)
        .title("One ROM Studio v2 - prototype shell")
        .subscription(Shell::subscription)
        .theme(theme)
        .font(builder::FONTS[0])
        .font(builder::FONTS[1])
        .font(builder::FONTS[2])
        .default_font(Font::with_name("Inter"))
        .window_size((1280.0, 860.0))
        .run()
}

/// The theme every screen runs under.
///
/// One window, one theme.  It comes from `studiov2-shared` because both
/// screens draw against the same palette and neither can own it.
fn theme(_state: &Shell) -> iced::Theme {
    style::theme()
}

/// Which screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Showing {
    /// The ROM slot builder.
    Builder,
    /// The log and console panes.
    Logs,
    /// A pane per CLI command, generated from a description of them.
    Commands,
}

/// A device the picker offers, or none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceChoice(Option<Device>);

impl std::fmt::Display for DeviceChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(device) => write!(f, "{device}"),
            None => f.write_str("No device"),
        }
    }
}

/// Everything the shell can be told.
///
/// Three variants of its own, and one per screen.  A screen's own messages
/// travel inside `Builder` and `Logs` and the shell never looks at them.
#[derive(Debug, Clone)]
pub enum Message {
    /// Show a different screen.
    Show(Showing),
    /// The device picker moved.
    DeviceSelected(DeviceChoice),
    /// A message for the builder.
    Builder(builder::Message),
    /// A message for the log screen.
    Logs(logs::Message),
    /// A message for the generated command screen.
    Commands(generated::Message),
    /// A development screenshot arrived.  See [`dev`].
    Shot(window::Screenshot),
}

/// The application.
struct Shell {
    /// The one copy of everything more than one screen reads.
    shared: Shared,
    /// The devices the picker offers.  Which one is selected is in `shared`.
    devices: Vec<DeviceChoice>,
    /// Which screen is showing.
    showing: Showing,
    /// The builder.
    builder: builder::Screen,
    /// The log and console panes.
    logs: logs::Screen,
    /// The generated command panes.
    commands: generated::Screen,
    /// The log revision the shell last told the screens about.
    log_revision: u64,
}

impl Shell {
    /// Builds the shared state, then both screens over it.
    fn boot() -> (Self, Task<Message>) {
        let mut shared = match Shared::stub() {
            Ok(shared) => shared,
            Err(error) => {
                eprintln!("could not open a log store: {error}");
                std::process::exit(1);
            }
        };

        let (builder, builder_task) = builder::Screen::boot(&mut shared);
        // The synthetic source stands in for a device, and the shell has no
        // device attached at boot, so it starts paused.  On its own the log
        // viewer starts it, because measuring it is what that binary is for.
        // The Stream button turns it on here.
        let logs_options = logs::Options {
            streaming: false,
            ..logs::Options::default()
        };
        let (logs, logs_task) = logs::Screen::boot(logs_options, &mut shared);
        let (commands, commands_task) = generated::Screen::boot(&mut shared);

        let devices = std::iter::once(DeviceChoice(None))
            .chain(
                device::attached()
                    .into_iter()
                    .map(|d| DeviceChoice(Some(d))),
            )
            .collect();

        let log_revision = shared.log.revision();

        let shell = Self {
            shared,
            devices,
            showing: Showing::Builder,
            builder,
            logs,
            commands,
            log_revision,
        };

        let mut start = window::latest()
            .and_then(window::gain_focus)
            .chain(builder_task.map(Message::Builder))
            .chain(logs_task.map(Message::Logs))
            .chain(commands_task.map(Message::Commands));

        if dev::shot_path().is_some() {
            start = start.chain(dev::capture());
        }

        (shell, start)
    }

    /// Routes a message to whichever screen owns it.
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Show(showing) => {
                self.showing = showing;
                Task::none()
            }

            Message::DeviceSelected(choice) => {
                self.shared.device = choice.0;
                let task = self
                    .builder
                    .update(builder::Message::DeviceChanged, &mut self.shared)
                    .map(Message::Builder)
                    .chain(
                        self.logs
                            .update(logs::Message::DeviceChanged, &mut self.shared)
                            .map(Message::Logs),
                    )
                    .chain(
                        self.commands
                            .update(generated::Message::DeviceChanged, &mut self.shared)
                            .map(Message::Commands),
                    );
                self.spread(task)
            }

            Message::Builder(message) => {
                let task = self
                    .builder
                    .update(message, &mut self.shared)
                    .map(Message::Builder);
                self.spread(task)
            }

            Message::Logs(message) => {
                let task = self
                    .logs
                    .update(message, &mut self.shared)
                    .map(Message::Logs);
                self.spread(task)
            }

            Message::Commands(message) => {
                let task = self
                    .commands
                    .update(message, &mut self.shared)
                    .map(Message::Commands);
                self.spread(task)
            }

            Message::Shot(shot) => {
                if let Some(path) = dev::shot_path()
                    && let Err(error) = dev::write_png(&path, &shot)
                {
                    eprintln!("screenshot: {error}");
                }
                iced::exit()
            }
        }
    }

    /// Tells both screens the log grew, when it did.
    ///
    /// A screen showing the log holds a widget-side window over it, and a
    /// widget cannot be refreshed from `view`.  The screen that did the
    /// appending has already caught up, and answers this with nothing.
    ///
    /// This is the whole cross-screen notification mechanism.  It is here
    /// rather than in either screen because neither may reach the other.
    fn spread(&mut self, task: Task<Message>) -> Task<Message> {
        let revision = self.shared.log.revision();
        if revision == self.log_revision {
            return task;
        }
        self.log_revision = revision;

        task.chain(
            self.builder
                .update(builder::Message::LogGrew, &mut self.shared)
                .map(Message::Builder),
        )
        .chain(
            self.logs
                .update(logs::Message::LogGrew, &mut self.shared)
                .map(Message::Logs),
        )
        .chain(
            self.commands
                .update(generated::Message::LogGrew, &mut self.shared)
                .map(Message::Commands),
        )
    }

    /// Draws the chrome, and whichever screen is showing inside it.
    fn view(&self) -> Element<'_, Message> {
        let body = match self.showing {
            Showing::Builder => self.builder.view(&self.shared).map(Message::Builder),
            Showing::Logs => self.logs.view(&self.shared).map(Message::Logs),
            Showing::Commands => self.commands.view(&self.shared).map(Message::Commands),
        };

        column![
            self.chrome(),
            rule::horizontal(1).style(style::divider),
            body
        ]
        .spacing(8)
        .into()
    }

    /// The screen switch, the device picker and what the image says.
    fn chrome(&self) -> Element<'_, Message> {
        let selected = DeviceChoice(self.shared.device.clone());

        let image = match &self.shared.image {
            Some(image) => format!("image: {} ({} bytes)", image.name, image.len()),
            None => "image: none built".to_owned(),
        };

        container(
            row![
                tab("Builder", Showing::Builder, self.showing),
                tab("Logs", Showing::Logs, self.showing),
                tab("Commands", Showing::Commands, self.showing),
                iced::widget::Space::new().width(Length::Fill),
                text(image).size(style::NOTE).style(style::dim),
                pick_list(
                    self.devices.as_slice(),
                    Some(selected),
                    Message::DeviceSelected,
                )
                .text_size(style::BODY)
                .padding([6, 10])
                .style(style::picker),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([10, 16])
        .style(style::panel)
        .width(Length::Fill)
        .into()
    }

    /// What the shell listens to.
    ///
    /// The log screen's subscriptions run whether or not it is showing: the
    /// device does not stop talking because the user looked at the builder.
    fn subscription(&self) -> Subscription<Message> {
        self.logs.subscription().map(Message::Logs)
    }
}

/// A screen-switch button.
fn tab(label: &str, screen: Showing, current: Showing) -> Element<'_, Message> {
    let style = if screen == current {
        style::gold_button
    } else {
        style::icon_button
    };

    button(text(label).size(style::BODY))
        .style(style)
        .padding([8, 16])
        .on_press(Message::Show(screen))
        .into()
}
