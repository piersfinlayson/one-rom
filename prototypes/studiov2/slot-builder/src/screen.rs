// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The ROM slot builder screen.
//!
//! Everything the builder needs that only the builder needs is in [`Screen`].
//! The device, the log and the built image are not: they are in
//! [`Shared`], because a programmer screen and an analysis screen want the
//! same three things and there can only be one of each.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use iced::widget::text_editor;
use onerom_config::fw::FirmwareVersion;
use studiov2_shared::{Image, Shared, store::Store};

use crate::build::{self, Built, Request, Usage};
use crate::catalog::{
    self, BoardChoice, ChipChoice, FormatChoice, PluginChoice, PluginSlot, PolarityChoice,
    SizingChoice,
};
use crate::error::{self, Error};
use crate::slot::{self, Slot};
use crate::ui;

/// How many lines of the shared log the builder's own pane shows.
///
/// The pane is a tail, not the log.  Anyone wanting the whole thing goes to
/// the log screen, which holds a bigger window over the same file.
const TAIL_LINES: usize = 200;

/// A firmware version the builder offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionChoice(pub FirmwareVersion);

impl std::fmt::Display for VersionChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "v{}.{}.{}",
            self.0.major(),
            self.0.minor(),
            self.0.patch()
        )
    }
}

/// Everything the user can do.
#[derive(Debug, Clone)]
pub enum Message {
    /// A One ROM type was chosen.
    BoardSelected(BoardChoice),
    /// A firmware version was chosen.
    VersionSelected(VersionChoice),
    /// The system plugin picker moved.
    SystemPlugin(PluginSlot),
    /// The user plugin picker moved.
    UserPlugin(PluginSlot),
    /// The help section was opened or closed.
    ToggleHelp,
    /// Add a slot to the end of the list.
    AddSlot,
    /// Remove the slot at this index.
    RemoveSlot(usize),
    /// Move the slot at this index to that one.
    MoveSlot(usize, isize),
    /// Open the file dialog for this slot.
    PickFile(usize),
    /// The dialog closed with a file.
    FilePicked(usize, PathBuf),
    /// The slot's file format changed.
    FormatSelected(usize, FormatChoice),
    /// The slot's Intel HEX load address was edited.
    LoadAddressEdited(usize, String),
    /// The slot's emulated chip changed.
    ChipSelected(usize, ChipChoice),
    /// The slot's size handling changed.
    SizingSelected(usize, SizingChoice),
    /// One of the slot's chip-select polarities changed.
    PolaritySelected(usize, usize, PolarityChoice),
    /// The slot's label was edited.
    LabelEdited(usize, String),
    /// Build the image.
    Build,
    /// The build finished.
    BuildFinished(Result<Built, Arc<Error>>),
    /// Save the built image.
    Save,
    /// The save finished.
    SaveFinished(Result<PathBuf, Arc<Error>>),
    /// The log pane was interacted with — it is read-only, so only selection
    /// and scrolling reach the content.
    Log(text_editor::Action),
    /// A development screenshot arrived. See [`crate::dev`].
    Screenshot(iced::window::Screenshot),

    /// Somebody outside this screen appended to the shared log.
    LogGrew,
    /// The selected device changed.
    DeviceChanged,
}

/// The builder's whole state.
pub struct Screen {
    /// Every board the builder offers.
    pub boards: Vec<BoardChoice>,
    /// The chosen board, if any.
    pub board: Option<BoardChoice>,
    /// The chosen board's chip types, recomputed only when the board changes.
    pub chips: Vec<ChipChoice>,
    /// The firmware versions on offer.
    pub versions: Vec<VersionChoice>,
    /// The chosen firmware version.
    pub version: Option<VersionChoice>,
    /// The slot list, in boot-select order.
    pub slots: Vec<Slot>,
    /// The system plugins found in the tree.
    pub system_plugins: Vec<PluginChoice>,
    /// The user plugins found in the tree.
    pub user_plugins: Vec<PluginChoice>,
    /// The chosen system plugin.
    pub system_plugin: Option<PluginChoice>,
    /// The chosen user plugin.
    pub user_plugin: Option<PluginChoice>,
    /// The file formats `onerom-gen` decodes.
    pub formats: Vec<FormatChoice>,
    /// The ways an image is fitted to its chip.
    pub sizings: Vec<SizingChoice>,
    /// A signature of the form at the moment of the build, so an edit since
    /// makes the image stale.
    ///
    /// The image itself is in [`Shared::image`], because the programmer and
    /// analysis screens want it too.  What is here is the builder's own note
    /// of what it asked for.
    pub built_from: Option<String>,
    /// Whether a build is in flight.
    pub building: bool,
    /// The message under the build buttons.
    pub status: String,
    /// Whether the help section is open.
    pub help_open: bool,
    /// The last few lines of the shared log, for the pane at the foot of the
    /// page.
    ///
    /// A cache of [`Shared::log`] and not a second copy of it: nothing is
    /// ever written here that was not written there first, and
    /// [`Screen::refresh_log`] brings it up to date whenever the log moves on.
    pub log_tail: text_editor::Content,
    /// The log revision `log_tail` was built from.
    log_revision: u64,
    /// The lines of the shared log `log_tail` holds.
    tail: Range<usize>,
}

impl Screen {
    /// The app, plus whatever the screenshot hook asks for.
    pub fn boot(shared: &mut Shared) -> (Self, Task<Message>) {
        let mut screen = Self::new(shared);
        if crate::dev::shot_path().is_some() {
            crate::dev::apply_setup(&mut screen, shared);
            return (screen, crate::dev::capture());
        }
        (screen, Task::none())
    }

    /// The initial state: one empty slot, no board, the USB plugin selected
    /// where one is present.
    fn new(shared: &mut Shared) -> Self {
        let plugins = catalog::plugins();
        let system_plugins: Vec<PluginChoice> =
            plugins.iter().filter(|p| p.system).cloned().collect();
        let user_plugins: Vec<PluginChoice> =
            plugins.iter().filter(|p| !p.system).cloned().collect();
        let system_plugin = system_plugins.first().cloned();

        let mut app = Self {
            boards: catalog::boards(),
            board: None,
            chips: Vec::new(),
            versions: firmware_versions(),
            version: None,
            slots: vec![Slot::default()],
            system_plugins,
            user_plugins,
            system_plugin,
            user_plugin: None,
            formats: catalog::formats(),
            sizings: catalog::sizings(),
            built_from: None,
            building: false,
            status: String::new(),
            help_open: true,
            log_tail: text_editor::Content::new(),
            log_revision: u64::MAX,
            tail: 0..0,
        };
        app.version = app.versions.last().copied();
        app.adopt_device(shared);
        app.note(shared, "Ready. Pick a One ROM type to begin.");
        app
    }

    /// Takes the board from the selected device, where there is one.
    ///
    /// A device on the bus already answers the first question the form asks,
    /// so asking it again would be rude.  A device the builder has no board
    /// for leaves the picker alone rather than clearing it.
    fn adopt_device(&mut self, shared: &Shared) {
        let Some(device) = shared.device.as_ref() else {
            return;
        };

        let Some(board) = self
            .boards
            .iter()
            .find(|board| board.0.name() == device.board)
            .copied()
        else {
            return;
        };

        self.select_board(board);
    }

    /// Applies a board choice to the form.
    fn select_board(&mut self, board: BoardChoice) {
        self.board = Some(board);
        self.chips = catalog::chip_types(board.0);

        // Slots are user intent: trim to what the jumpers can select and
        // drop a chip type this board cannot serve, but keep everything
        // else exactly as it was.
        self.slots.truncate(catalog::max_slots(board.0));
        let chips = self.chips.clone();
        for slot in &mut self.slots {
            if let Some(chip) = slot.chip
                && !chips.iter().any(|offered| offered.alias == chip.alias)
            {
                slot.chip = None;
            }
        }
    }

    /// How the flash fills up under the current form.
    pub fn usage(&self) -> Usage {
        build::usage(&self.slots, self.plugins().len())
    }

    /// The plugins that would go into a build, system first.
    pub fn plugins(&self) -> Vec<PluginChoice> {
        self.system_plugin
            .iter()
            .chain(self.user_plugin.iter())
            .cloned()
            .collect()
    }

    /// Whether Build has everything it needs.
    pub fn can_build(&self) -> bool {
        self.board.is_some()
            && self.version.is_some()
            && !self.building
            && self.slots.iter().all(Slot::is_complete)
            && !self.usage().over()
            && !(self.user_plugin.is_some() && self.system_plugin.is_none())
    }

    /// Whether the built image still matches the form on screen.
    pub fn build_is_current(&self, shared: &Shared) -> bool {
        shared.image.is_some() && self.built_from.as_deref() == Some(&self.signature())
    }

    /// Everything a build reads, as one string, so an edit anywhere makes the
    /// held image stale.
    fn signature(&self) -> String {
        match self.request() {
            Some(request) => build::config_json(&request).unwrap_or_default(),
            None => String::new(),
        }
    }

    /// The build request the form describes, or `None` while it is incomplete.
    pub(crate) fn request(&self) -> Option<Request> {
        Some(Request {
            board: self.board?.0,
            version: self.version?.0,
            slots: self.slots.clone(),
            plugins: self.plugins(),
        })
    }

    /// Adds a line to the shared log, and to this screen's tail of it.
    fn note(&mut self, shared: &mut Shared, line: &str) {
        if let Err(error) = shared.log.append(&[Arc::from(line)]) {
            // A log that cannot be written to is not worth failing a build
            // over, and the status line is about to say something anyway.
            eprintln!("log: {error}");
            return;
        }
        self.refresh_log(&shared.log);
    }

    /// Brings the tail pane up to date if the shared log has moved on.
    ///
    /// Cheap when nothing changed, which is what lets the shell call it after
    /// every message without thinking about who wrote what.
    ///
    /// Where the pane already holds part of what it now needs, the new lines
    /// are pasted on and the ones that fell off the top are deleted, rather
    /// than the widget being handed a fresh buffer.  A fresh buffer re-shapes
    /// every line the pane holds, and a talking device would have it re-shaping
    /// two hundred of them twenty times a second to show ten new ones.
    pub fn refresh_log(&mut self, log: &Store) {
        if self.log_revision == log.revision() {
            return;
        }
        self.log_revision = log.revision();

        let wanted = log.len().saturating_sub(TAIL_LINES)..log.len();
        let slides = !self.tail.is_empty()
            && wanted.start >= self.tail.start
            && wanted.start <= self.tail.end
            && wanted.end >= self.tail.end;

        let text = match log.text(if slides {
            self.tail.end..wanted.end
        } else {
            wanted.clone()
        }) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("log: {error}");
                return;
            }
        };

        if !slides {
            self.log_tail = text_editor::Content::with_text(&text);
            self.tail = wanted;
            return;
        }

        if !text.is_empty() {
            self.log_tail
                .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
            self.log_tail
                .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                    Arc::new(format!("\n{text}")),
                )));
        }

        let leaving = wanted.start - self.tail.start;
        if leaving > 0 {
            // iced gives a `Content` no trim, so the departing lines are
            // selected from the top and deleted.
            self.log_tail.move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 0 },
                selection: Some(text_editor::Position {
                    line: leaving,
                    column: 0,
                }),
            });
            self.log_tail
                .perform(text_editor::Action::Edit(text_editor::Edit::Delete));
        }

        self.tail = wanted;
    }

    pub fn update(&mut self, message: Message, shared: &mut Shared) -> Task<Message> {
        match message {
            Message::BoardSelected(board) => {
                self.select_board(board);
                self.note(
                    shared,
                    &format!(
                        "{board}: {} chip types, {} slots addressable.",
                        self.chips.len(),
                        catalog::max_slots(board.0)
                    ),
                );
                Task::none()
            }
            Message::VersionSelected(version) => {
                self.version = Some(version);
                Task::none()
            }
            Message::SystemPlugin(plugin) => {
                self.system_plugin = plugin.0;
                Task::none()
            }
            Message::UserPlugin(plugin) => {
                self.user_plugin = plugin.0;
                Task::none()
            }
            Message::AddSlot => {
                let limit = self.board.map_or(0, |board| catalog::max_slots(board.0));
                if self.slots.len() < limit {
                    let slot = Slot::after(self.slots.last());
                    self.slots.push(slot);
                }
                Task::none()
            }
            Message::RemoveSlot(index) => {
                if self.slots.len() > 1 && index < self.slots.len() {
                    self.slots.remove(index);
                }
                Task::none()
            }
            Message::MoveSlot(from, to) => {
                slot::move_slot(&mut self.slots, from, to);
                Task::none()
            }
            Message::PickFile(index) => Task::future(async {
                rfd::AsyncFileDialog::new()
                    .add_filter("ROM images", &["bin", "rom", "hex", "ihex", "ihx", "mcs"])
                    .pick_file()
                    .await
            })
            .and_then(move |handle| {
                Task::done(Message::FilePicked(index, handle.path().to_owned()))
            }),
            Message::FilePicked(index, path) => {
                if let Some(slot) = self.slots.get_mut(index) {
                    slot.format = Slot::format_for(&path);
                    slot.file = Some(path);
                }
                Task::none()
            }
            Message::FormatSelected(index, format) => {
                if let Some(slot) = self.slots.get_mut(index) {
                    slot.format = format.0;
                }
                Task::none()
            }
            Message::LoadAddressEdited(index, value) => {
                if let Some(slot) = self.slots.get_mut(index) {
                    slot.load_address = value;
                }
                Task::none()
            }
            Message::ChipSelected(index, chip) => {
                if let Some(slot) = self.slots.get_mut(index) {
                    slot.chip = Some(chip);
                }
                Task::none()
            }
            Message::SizingSelected(index, sizing) => {
                if let Some(slot) = self.slots.get_mut(index) {
                    slot.sizing = sizing.0;
                }
                Task::none()
            }
            Message::PolaritySelected(index, line, polarity) => {
                if let Some(slot) = self.slots.get_mut(index)
                    && let Some(current) = slot.polarities.get_mut(line)
                {
                    *current = polarity;
                }
                Task::none()
            }
            Message::LabelEdited(index, value) => {
                if let Some(slot) = self.slots.get_mut(index) {
                    slot.label = value;
                }
                Task::none()
            }
            Message::Build => {
                let Some(request) = self.request() else {
                    return Task::none();
                };
                self.building = true;
                shared.image = None;
                self.built_from = Some(self.signature());
                self.status = "Building...".to_owned();
                self.note(shared, "Building...");

                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || build::run(request))
                            .await
                            .map_err(|join| Error::Read {
                                path: PathBuf::new(),
                                source: std::io::Error::other(join.to_string()),
                            })
                            .and_then(|result| result)
                            .map_err(Arc::new)
                    },
                    Message::BuildFinished,
                )
            }
            Message::BuildFinished(result) => {
                self.building = false;
                match result {
                    Ok(built) => {
                        let image = self.published(&built);
                        self.status = format!(
                            "Built {} of metadata and images.",
                            catalog::kb(image.len() as u64)
                        );
                        let line = format!("Build complete: {}", built.description);
                        self.note(shared, &line);
                        shared.image = Some(image);
                    }
                    Err(error) => {
                        self.built_from = None;
                        self.status = error::chain(error.as_ref());
                        let line = format!("Build failed: {}", error::chain(error.as_ref()));
                        self.note(shared, &line);
                    }
                }
                Task::none()
            }
            Message::Save => {
                let Some(image) = shared.image.clone() else {
                    return Task::none();
                };
                let name = image.name.clone();

                Task::future(async move {
                    rfd::AsyncFileDialog::new()
                        .set_file_name(name)
                        .save_file()
                        .await
                })
                .and_then(move |handle| {
                    let path = handle.path().to_owned();
                    let bytes = Arc::clone(&image.bytes);
                    Task::perform(
                        async move {
                            let target = path.clone();
                            std::fs::write(&target, bytes.as_slice())
                                .map(|()| target)
                                .map_err(|source| Arc::new(Error::Write { path, source }))
                        },
                        Message::SaveFinished,
                    )
                })
            }
            Message::SaveFinished(result) => {
                match result {
                    Ok(path) => {
                        self.status = format!("Saved to {}", path.display());
                        let line = format!("Saved {}", path.display());
                        self.note(shared, &line);
                    }
                    Err(error) => {
                        self.status = error::chain(error.as_ref());
                        let line = error::chain(error.as_ref());
                        self.note(shared, &line);
                    }
                }
                Task::none()
            }
            Message::ToggleHelp => {
                self.help_open = !self.help_open;
                Task::none()
            }
            Message::Screenshot(shot) => {
                if let Some(path) = crate::dev::shot_path()
                    && let Err(error) = crate::dev::write_png(&path, &shot)
                {
                    eprintln!("screenshot: {error}");
                }
                iced::exit()
            }
            Message::Log(action) => {
                // Read-only: selection, scrolling and copying pass through,
                // edits do not.
                if !action.is_edit() {
                    self.log_tail.perform(action);
                }
                Task::none()
            }

            Message::LogGrew => {
                self.refresh_log(&shared.log);
                Task::none()
            }

            Message::DeviceChanged => {
                self.adopt_device(shared);
                let line = match shared.device.as_ref() {
                    Some(device) => format!("Device: {device}."),
                    None => "No device selected.".to_owned(),
                };
                self.note(shared, &line);
                Task::none()
            }
        }
    }

    pub fn view<'a>(&'a self, shared: &'a Shared) -> iced::Element<'a, Message> {
        ui::page(self, shared)
    }

    /// What leaves the builder when a build succeeds.
    ///
    /// `Built` carries `onerom-gen`'s two buffers.  What every other screen
    /// wants is one image and a name for it, so that is what is published.
    fn published(&self, built: &Built) -> Image {
        let name = match self.board.zip(self.version) {
            Some((board, version)) => build::suggested_name(board.0, version.0),
            None => "onerom.bin".to_owned(),
        };

        Image {
            name,
            description: built.description.clone(),
            bytes: Arc::new(built.bytes()),
        }
    }
}

/// The firmware versions the builder offers.
///
/// FAKED: the shipped page reads `releases.json` from `images.onerom.org`.
/// This prototype does no network I/O, so the list is written out here and only
/// the v2 window it is filtered against is real.
fn firmware_versions() -> Vec<VersionChoice> {
    [(0, 7, 0), (0, 7, 1), (0, 7, 2)]
        .into_iter()
        .map(|(major, minor, patch)| FirmwareVersion::new(major, minor, patch, 0))
        .filter(|version| {
            *version >= onerom_gen::MIN_SUPPORTED_FIRMWARE_VERSION_V2
                && *version <= onerom_gen::MAX_SUPPORTED_FIRMWARE_VERSION_V2
        })
        .map(VersionChoice)
        .collect()
}
