// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Create functionality
//!
//! Creates firmware images for selected hardware and configuration.

mod build;
mod file;
mod hw;
mod msg;
mod view;

use iced::keyboard::Key;
use iced::{Element, Subscription, Task, event, keyboard};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::chip::ChipType;
use onerom_config::hw::{Board, BoardSize, Model};
use onerom_config::mcu::{Family, MCU_VARIANTS, Variant as McuVariant};
use onerom_fw::net::{Release, Releases};
use onerom_gen::board_supports_size;

use crate::app::{AppMessage, progress_tick_subscription};
use crate::device::{Client, Device, Message as DeviceMessage};
use crate::hw::HardwareInfo;
use crate::studio::{Message as StudioMessage, RuntimeInfo};
use crate::style::Style;

pub use build::Active;
pub use msg::Message;

/// Create tab internal state
#[derive(Debug, Default, Clone, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    Building,
    Flashing,
    Saving,
    Loading,
    UserBuilding {
        valid_rom_types: Vec<ChipType>,
        rom_type: Option<ChipType>,
        cs: Vec<Option<Active>>,
        data: Option<String>,
    },
    Rebooting,
}

impl State {
    pub const fn is_idle(&self) -> bool {
        matches!(self, State::Idle)
    }

    pub const fn is_busy(&self) -> bool {
        !self.is_idle()
    }

    pub const fn is_user_building(&self) -> bool {
        matches!(self, State::UserBuilding { .. })
    }
}

/// Create tab state
#[derive(Debug, Clone)]
pub struct Create {
    selected_hw_info: HardwareInfo,
    mcu_variants: Option<Vec<McuVariant>>,
    // Whether the selected board size was read from a device
    size_detected: bool,
    display_content: String,
    state: State,
}

impl Default for Create {
    fn default() -> Self {
        Self {
            selected_hw_info: HardwareInfo::default(),
            mcu_variants: None,
            size_detected: false,
            display_content: Self::default_display_content(),
            state: State::Idle,
        }
    }
}

impl Create {
    /// Name of the button to select Create
    pub const fn top_level_button_name() -> &'static str {
        "Create"
    }

    // Default content for display window
    fn default_display_content() -> String {
        "Image not yet built...".to_string()
    }

    /// Instantiation method
    pub fn new() -> Self {
        Self::default()
    }

    /// Is the create tab ready for operations?
    #[allow(dead_code)]
    pub fn is_ready(&self) -> bool {
        self.state.is_idle()
    }

    // Internal state methods
    #[allow(dead_code)]
    fn is_idle(&self) -> bool {
        self.state.is_idle()
    }
    fn is_busy(&self) -> bool {
        self.state.is_busy()
    }
    fn is_building(&self) -> bool {
        matches!(self.state, State::Building)
    }
    fn is_flashing(&self) -> bool {
        matches!(self.state, State::Flashing)
    }
    fn is_saving(&self) -> bool {
        matches!(self.state, State::Saving)
    }

    /// Main Create Message handling function
    pub fn update(
        &mut self,
        runtime_info: &RuntimeInfo,
        device: &Device,
        message: Message,
    ) -> Task<AppMessage> {
        msg::message(self, runtime_info, device, message)
    }

    // Update progress display
    fn progress_tick(&mut self) {
        if self.is_busy() {
            self.display_content += "."
        }
    }

    // Set display in the content window
    fn set_display_content(&mut self, content: impl ToString) {
        self.display_content = content.to_string();
    }

    fn select_latest_release(&mut self, releases: Option<&Releases>) -> Option<AppMessage> {
        // Only select latest if hardware is fully selected
        if !self.hardware_selected() {
            return None;
        }
        let board = self.selected_hw_info.board.as_ref().unwrap();
        let mcu = self.selected_hw_info.mcu_variant.as_ref().unwrap();

        if let Some(releases) = releases {
            let latest = releases.latest();
            let latest = releases.release_from_string(latest);
            if let Some(r) = latest
                && r.supports_hw(board, mcu)
            {
                self.select_release(r.clone())
            } else {
                debug!("No latest release found in releases for this hardware");
                None
            }
        } else {
            warn!("Release updated but no releases found for hardware");
            None
        }
    }

    fn select_release(&mut self, release: Release) -> Option<AppMessage> {
        // Download the release
        if let Some(board) = self.selected_hw_info.board
            && let Some(mcu) = self.selected_hw_info.mcu_variant
        {
            Some(AppMessage::Studio(StudioMessage::DownloadRelease(
                release, board, mcu,
            )))
        } else {
            warn!("Board or MCU not selected, cannot download firmware");
            None
        }
    }

    fn has_model(&self) -> bool {
        self.selected_hw_info.model.is_some()
    }
    fn has_board(&self) -> bool {
        self.selected_hw_info.board.is_some()
    }
    #[allow(dead_code)]
    fn has_mcu(&self) -> bool {
        self.selected_hw_info.mcu_variant.is_some()
    }

    fn model_selected(&mut self, model: Model) {
        self.selected_hw_info.model = Some(model);
        self.selected_hw_info.board = None;
        self.selected_hw_info.mcu_variant = None;
        // The board size stays for board_selected to keep or replace
        self.mcu_variants = None;
    }

    // The board sizes `board` supports, smallest first
    fn board_sizes(board: Board) -> Vec<BoardSize> {
        BoardSize::supported_values()
            .iter()
            .copied()
            .filter(|&size| board_supports_size(board, size))
            .collect()
    }

    fn board_selected(&mut self, runtime_info: &RuntimeInfo, board: Board) -> Option<AppMessage> {
        self.selected_hw_info.board = Some(board);

        // Board Size is offered only for a board supporting more than M.  The
        // selected size is kept where the new board supports it.
        let sizes = Self::board_sizes(board);
        self.selected_hw_info.board_size = if sizes.len() > 1 {
            self.selected_hw_info
                .board_size
                .filter(|size| sizes.contains(size))
                .or(Some(BoardSize::M))
        } else {
            None
        };

        let mut vars = Vec::new();
        for var in MCU_VARIANTS {
            if board.mcu_family() == var.family() {
                vars.push(*var);
            }
        }
        self.mcu_variants = Some(vars);

        // Special case the Fire boards
        if board.mcu_family() == Family::Rp2350 {
            self.mcu_selected(McuVariant::RP2350);
            self.select_latest_release(runtime_info.releases())
        } else {
            Some(self.clear_mcu())
        }
    }

    fn board_size_selected(&mut self, size: BoardSize) {
        self.selected_hw_info.board_size = Some(size);
    }

    fn mcu_selected(&mut self, mcu: McuVariant) {
        self.selected_hw_info.mcu_variant = Some(mcu);
    }

    fn clear_mcu(&mut self) -> AppMessage {
        self.selected_hw_info.mcu_variant = None;
        StudioMessage::ClearDownloadedRelease.into()
    }

    fn hardware_selected(&self) -> bool {
        self.selected_hw_info.is_complete()
    }

    fn ready_to_build(&self, runtime_info: &RuntimeInfo) -> bool {
        self.hardware_selected()
            && runtime_info.firmware_selected()
            && runtime_info.config_selected()
    }

    /// Create tab view function
    pub fn view<'a>(
        &'a self,
        runtime_info: &'a RuntimeInfo,
        device: &Device,
        style: &'a Style,
    ) -> Element<'a, AppMessage> {
        view::view(self, runtime_info, device, style)
    }

    /// Create tab subscription function
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![];

        if self.is_busy() {
            // Constructor, not a value - see progress_tick_subscription().
            subs.push(progress_tick_subscription(|_| Message::ProgressTick))
        }

        #[allow(clippy::collapsible_if)]
        subs.push(event::listen_with(|event, status, _id| {
            if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Character(ref c),
                ..
            }) = event
            {
                if status == event::Status::Ignored && c.as_ref() == "f" {
                    return Some(Message::KeyFlashFirmware);
                }
            }
            None
        }));

        Subscription::batch(subs)
    }

    pub fn stop_device(&mut self) -> Task<AppMessage> {
        self.state = State::Rebooting;
        self.set_display_content("Rebooting device...");
        Task::done(AppMessage::Device(DeviceMessage::RebootDevice {
            client: Client::Create,
            stopped: true,
        }))
    }

    pub fn run_device(&mut self) -> Task<AppMessage> {
        self.state = State::Rebooting;
        self.set_display_content("Rebooting device...");
        Task::done(AppMessage::Device(DeviceMessage::RebootDevice {
            client: Client::Create,
            stopped: false,
        }))
    }

    pub fn reboot_complete(&mut self, result: Result<(), String>) -> Task<AppMessage> {
        self.state = State::Idle;
        match result {
            Ok(()) => self.set_display_content("Device rebooted successfully."),
            Err(e) => self.set_display_content(format!("Device reboot failed: {e}")),
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(name: &str) -> Board {
        Board::try_from_str(name).unwrap()
    }

    /// Board Size is offered for a board supporting a second flash chip and
    /// keeps its value across boards where it's still offered.
    #[test]
    fn board_size_is_kept_where_still_offered() {
        let runtime_info = RuntimeInfo::default();
        let mut create = Create::new();
        create.model_selected(Model::Fire);

        create.board_selected(&runtime_info, board("fire-40-a"));
        assert_eq!(create.selected_hw_info.board_size, Some(BoardSize::M));
        create.board_size_selected(BoardSize::L);

        create.board_selected(&runtime_info, board("fire-40-b"));
        assert_eq!(create.selected_hw_info.board_size, Some(BoardSize::L));

        create.board_selected(&runtime_info, board("fire-24-f"));
        assert_eq!(create.selected_hw_info.board_size, None);

        create.board_selected(&runtime_info, board("fire-40-a"));
        assert_eq!(create.selected_hw_info.board_size, Some(BoardSize::M));
    }

    #[test]
    fn selecting_the_model_keeps_board_size() {
        let runtime_info = RuntimeInfo::default();
        let mut create = Create::new();
        create.model_selected(Model::Fire);
        create.board_selected(&runtime_info, board("fire-40-a"));
        create.board_size_selected(BoardSize::L);

        create.model_selected(Model::Fire);
        create.board_selected(&runtime_info, board("fire-40-a"));
        assert_eq!(create.selected_hw_info.board_size, Some(BoardSize::L));
    }

    fn fire_40_a(board_size: Option<BoardSize>, min_board_size: Option<BoardSize>) -> HardwareInfo {
        HardwareInfo {
            board: Some(board("fire-40-a")),
            model: Some(Model::Fire),
            mcu_variant: Some(McuVariant::RP2350),
            board_size,
            min_board_size,
        }
    }

    /// Passes `hw_info` to `create` as Studio does.
    fn detected(create: &mut Create, hw_info: HardwareInfo) {
        let mut runtime_info = RuntimeInfo::default();
        runtime_info.set_hw_info(Some(hw_info));
        let _ = hw::detected_hardware_info(create, &runtime_info);
    }

    #[test]
    fn a_board_sets_board_size() {
        let mut create = Create::new();
        for size in [BoardSize::L, BoardSize::M] {
            detected(&mut create, fire_40_a(Some(size), Some(BoardSize::M)));
            assert_eq!(create.selected_hw_info.board_size, Some(size));
            assert!(create.size_detected);
        }
    }

    #[test]
    fn an_image_sets_board_size_to_its_minimum() {
        let mut create = Create::new();
        detected(&mut create, fire_40_a(Some(BoardSize::L), None));
        for size in [BoardSize::M, BoardSize::L] {
            detected(&mut create, fire_40_a(None, Some(size)));
            assert_eq!(create.selected_hw_info.board_size, Some(size));
            assert!(!create.size_detected);
        }
    }
}
