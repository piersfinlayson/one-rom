// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use iced::widget::Row;
use iced::{Element, Subscription, Task, time};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use std::time::Duration;

use onerom_cli::CliFetch;
use onerom_cli::plugin::{PluginNote, check_config_plugins};
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_config::mcu::Variant as McuVariant;
use onerom_fw::get_rom_files_async;
use onerom_fw::net::{Release, Releases};
use onerom_gen::{Builder, FIRMWARE_SIZE, MAX_METADATA_LEN};

use crate::analyse::Analyse;
use crate::app::AppMessage;
use crate::app::manifest::FirmwareBuildCompatibility;
use crate::config::{
    Config, ConfigManifest, SelectedConfig, download_config_async, generate_built_config,
    load_config_file,
};
use crate::create::{Create, Message as CreateMessage};
use crate::hw::HardwareInfo;
use crate::log::Log;
use crate::style::Style;
use crate::{AppVersion, ManifestType};
use crate::{app_manifest, internal_error, task_from_msg};

const MANIFEST_RETRY_SHORT: Duration = Duration::from_secs(10);
const MANIFEST_RETRY_LONG: Duration = Duration::from_secs(60);

/// Maximum firmware versions this app version can build with.
///
/// Introduced in Studio v0.1.12.
///
/// Studio uses onerom_gen to generate firmware.  That has its own maximum
/// version, which defaults to the current major version.  However, if, for
/// some reason that is unacceptable, it can be overridden for specific
/// Studio version here.  This is normally the result of a mistake in
/// onerom-gen - it claiming it can build versions it cannot.  Probably
/// because we introduced a non-backwards compatible firmware format change,
/// incorrectly without bumping the major version.
pub const MAX_BUILD_FW_VERSIONS: [FirmwareBuildCompatibility; 1] = [FirmwareBuildCompatibility {
    app_version: AppVersion::new(0, 1, 12),
    max_buildable_firmware_version: FirmwareVersion::new(0, 6, 4, 0),
}];

/// Messages for main window
#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(StudioTab),
    HardwareInfo(Option<HardwareInfo>),
    FetchReleases,
    Releases(Releases),
    DownloadRelease(Release, Board, McuVariant),
    ReleaseDownloaded(Result<Vec<u8>, String>),
    ReleaseDoesntExist,
    ClearDownloadedRelease,
    FetchConfigs,
    ConfigManifest(ConfigManifest),
    LoadConfig(Config),
    ConfigLoaded(Result<SelectedConfig, String>),
    ClearDownloadedConfig,
    BuildImage(HardwareInfo),
    BuildImageResult(Result<(Image, String), String>),
    HelpPressed,

    // Used to indicate network is down.  Only returned when trying to fetch
    // configs or releases manifests - not any other files - as those failures
    // might be due to images.onerom.org misconfiguration, rather than network
    // issues.  This assumes releases and configs manifests are never mis-
    // configured.
    DownloadFailed,
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::TabSelected(tab) => write!(f, "TabSelected({tab})"),
            Message::HardwareInfo(info) => write!(f, "HardwareInfo({info:?})"),
            Message::FetchReleases => write!(f, "FetchReleases"),
            Message::Releases(releases) => write!(f, "Releases({})  ", releases.releases_str()),
            Message::DownloadRelease(release, board, mcu) => {
                write!(f, "DownloadRelease({}, {board}, {mcu})", release.version)
            }
            Message::ReleaseDownloaded(result) => match result {
                Ok(data) => write!(f, "ReleaseDownloaded({} bytes)", data.len()),
                Err(_) => write!(f, "ReleaseDownloaded(Err)"),
            },
            Message::ReleaseDoesntExist => write!(f, "ReleaseDoesntExist"),
            Message::ClearDownloadedRelease => write!(f, "ClearDownloadedRelease"),
            Message::FetchConfigs => write!(f, "FetchConfigs"),
            Message::ConfigManifest(configs) => {
                write!(f, "ConfigManifest({})", configs.names_str())
            }
            Message::LoadConfig(config) => write!(f, "LoadConfig({config})"),
            Message::ConfigLoaded(result) => match result {
                Ok(selected) => write!(f, "ConfigLoaded({} bytes)", selected.data.len()),
                Err(_) => write!(f, "ConfigLoaded(Err)"),
            },
            Message::ClearDownloadedConfig => write!(f, "ClearDownloadedConfig"),
            Message::BuildImage(hw) => write!(f, "BuildImage({hw})"),
            Message::BuildImageResult(_) => write!(f, "BuildImageResult"),
            Message::HelpPressed => write!(f, "HelpPressed"),
            Message::DownloadFailed => write!(f, "DownloadFailed"),
        }
    }
}

/// Tabs for main window
#[derive(Debug, Default, Clone, PartialEq)]
pub enum StudioTab {
    #[default]
    Analyse,
    Create,
    Log,
}

impl std::fmt::Display for StudioTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StudioTab::Create => write!(f, "Create"),
            StudioTab::Analyse => write!(f, "Analyse"),
            StudioTab::Log => write!(f, "Log"),
        }
    }
}

impl StudioTab {
    /// Get the tab name
    pub fn name(&self) -> &str {
        match self {
            StudioTab::Create => Create::top_level_button_name(),
            StudioTab::Analyse => Analyse::top_level_button_name(),
            StudioTab::Log => Log::top_level_button_name(),
        }
    }

    /// Create the tab buttons
    ///
    /// Returns a row of buttons
    pub fn buttons(active: &StudioTab, serious_errors: bool) -> Element<'_, AppMessage> {
        let mut buttons = Vec::new();
        for tab in [StudioTab::Analyse, StudioTab::Create, StudioTab::Log] {
            let active = *active == tab;
            let on_press = if active {
                None
            } else {
                Some(AppMessage::Studio(Message::TabSelected(tab.clone())))
            };
            let button = if serious_errors && tab == StudioTab::Log {
                Style::error_button(tab.name(), on_press, active)
            } else {
                Style::text_button(tab.name(), on_press, active)
            };
            buttons.push(button.into());
        }

        // Add the buttons to a row, with spacing between them
        Row::with_children(buttons).spacing(20).into()
    }
}

/// Image built by Studio
#[derive(Debug, Clone)]
pub struct Image {
    firmware: Vec<u8>,

    metadata: Vec<u8>,

    roms: Vec<u8>,
}

impl std::fmt::Display for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Image({}/{}/{}",
            self.firmware.len(),
            self.metadata.len(),
            self.roms.len()
        )
    }
}

#[allow(dead_code)]
impl Image {
    /// Returns just the required portion of the firmware image
    pub fn firmware_skinny(&self) -> &[u8] {
        &self.firmware
    }

    /// Returns the length of the firmware portion
    pub fn firmware_len(&self) -> usize {
        self.firmware.len()
    }

    /// Returns the maximum firmware length (48KB)
    pub const fn max_firmware_len() -> usize {
        FIRMWARE_SIZE
    }

    /// Returns offset from start of flash the firmware is located
    pub const fn firmware_offset() -> usize {
        0
    }

    /// Returns the full firmware portion padded to 48KB
    pub fn firmware_full(&self) -> Vec<u8> {
        let mut fw = vec![0xFF_u8; Self::max_firmware_len()];
        let skinny = self.firmware_skinny();
        fw[..skinny.len()].copy_from_slice(skinny);
        fw
    }

    /// Returns the metadata portion
    pub fn metadata_skinny(&self) -> &[u8] {
        &self.metadata
    }

    /// Returns the length of the metadata portion
    pub fn metadata_len(&self) -> usize {
        self.metadata.len()
    }

    /// Returns maximum metadata length (16KB)
    pub const fn max_metadata_len() -> usize {
        MAX_METADATA_LEN
    }

    /// Returns offset from start of flash the metadata is located
    pub const fn metadata_offset() -> usize {
        FIRMWARE_SIZE
    }

    /// Returns the full metadata portion padded to 16KB
    pub fn metadata_full(&self) -> Vec<u8> {
        let mut md = vec![0xFF_u8; Self::max_metadata_len()];
        let skinny = self.metadata_skinny();
        md[..skinny.len()].copy_from_slice(skinny);
        md
    }

    /// Returns the ROMs portion
    pub fn roms(&self) -> &[u8] {
        &self.roms
    }

    /// Returns the length of the ROMs portion
    pub fn roms_len(&self) -> usize {
        self.roms.len()
    }

    /// Returns offset from start of flash the ROMs are located
    pub const fn roms_offset() -> usize {
        FIRMWARE_SIZE + MAX_METADATA_LEN
    }

    /// Returns full image as would be flashed to device
    pub fn full_image(&self) -> Vec<u8> {
        let mut image = Vec::new();
        image.extend_from_slice(&self.firmware_full());
        image.extend_from_slice(&self.metadata_full());
        image.extend_from_slice(&self.roms);
        image
    }

    /// Returns size of full image as would be flashed to device
    pub fn full_image_len(&self) -> usize {
        Self::max_firmware_len() + Self::max_metadata_len() + self.roms_len()
    }
}

/// Network state
#[derive(Debug, Default, Clone)]
pub enum NetworkState {
    #[default]
    Untested,
    Online,
    Offline,
}

impl NetworkState {
    pub fn is_offline(&self) -> bool {
        matches!(self, NetworkState::Offline)
    }
}

/// Contains information retrieved/computed at runtime
#[derive(Debug, Clone, Default)]
pub struct RuntimeInfo {
    // One ROM releases retrieved from network
    releases: Option<Releases>,

    // Detected or selected hardware info
    hw_info: Option<HardwareInfo>,

    // Downloaded firmware image
    firmware: Option<Vec<u8>>,

    // Selected firmware
    selected_firmware: Option<Release>,

    // Available configs
    config_manifest: Option<ConfigManifest>,

    // Selected config
    selected_config: Option<SelectedConfig>,

    // Built image
    image: Option<Image>,

    // Network state
    network_state: NetworkState,
}

impl RuntimeInfo {
    pub fn network_online(&mut self) {
        if self.network_state.is_offline() {
            info!("Network is online");
        }
        self.network_state = NetworkState::Online;
    }

    pub fn network_offline(&mut self) {
        if !self.network_state.is_offline() {
            warn!("Network is offline");
        }
        self.network_state = NetworkState::Offline;
    }

    pub fn is_offline(&self) -> bool {
        self.network_state.is_offline()
    }

    pub fn releases(&self) -> Option<&Releases> {
        self.releases.as_ref()
    }

    fn set_releases(&mut self, releases: Releases) {
        self.releases = Some(releases);
    }

    fn set_configs(&mut self, configs: ConfigManifest) {
        self.config_manifest = Some(configs);
    }

    pub fn hw_info(&self) -> Option<&HardwareInfo> {
        self.hw_info.as_ref()
    }

    fn set_hw_info(&mut self, hw_info: Option<HardwareInfo>) {
        self.hw_info = hw_info;
    }

    #[allow(dead_code)]
    pub fn firmware(&self) -> Option<&Vec<u8>> {
        self.firmware.as_ref()
    }

    pub fn firmware_len(&self) -> Option<usize> {
        self.firmware.as_ref().map(|f| f.len())
    }

    pub fn selected_config_len(&self) -> Option<usize> {
        self.selected_config.as_ref().map(|c| c.data.len())
    }

    fn set_firmware(&mut self, firmware: Vec<u8>) {
        self.firmware = Some(firmware);
    }

    fn clear_firmware(&mut self) {
        self.clear_selected_firmware();
    }

    pub fn firmware_selected(&self) -> bool {
        self.selected_firmware.is_some()
    }

    pub fn selected_firmware(&self) -> Option<&Release> {
        self.selected_firmware.as_ref()
    }

    fn set_selected_firmware(&mut self, release: Release) {
        self.selected_firmware = Some(release);
    }

    fn clear_selected_firmware(&mut self) {
        self.selected_firmware = None;
        self.firmware = None;
    }

    pub fn config_manifest(&self) -> Option<&ConfigManifest> {
        self.config_manifest.as_ref()
    }

    pub fn clear_config(&mut self) {
        self.selected_config = None;
    }

    pub fn config_selected(&self) -> bool {
        self.selected_config.is_some()
    }

    pub fn selected_config(&self) -> Option<&SelectedConfig> {
        self.selected_config.as_ref()
    }

    pub fn set_selected_config(&mut self, selected: SelectedConfig) {
        self.selected_config = Some(selected.clone());

        if let Some(manifest) = &mut self.config_manifest {
            if selected.config.is_file() {
                // If the selected config is a local file, we need to ensure
                // it's at the start of the manifest
                manifest.update_local_file(selected.config);
            } else {
                // If the selected config is not a local file, clear out any
                // local file
                manifest.remove_local_file();
            }
        }

        // Clear out any built image
        self.image = None;
    }

    fn clear_selected_config(&mut self) {
        self.selected_config = None;
    }

    /// Returns reference to image
    pub fn image(&self) -> Option<&Image> {
        self.image.as_ref()
    }

    pub fn built_firmware_len(&self) -> Option<usize> {
        self.image().map(|imgs| imgs.firmware_len())
    }

    pub fn built_metadata_len(&self) -> Option<usize> {
        self.image().map(|imgs| imgs.metadata_len())
    }

    pub fn built_roms_len(&self) -> Option<usize> {
        self.image().map(|imgs| imgs.roms_len())
    }

    pub fn built_full_image_len(&self) -> Option<usize> {
        self.image().map(|imgs| imgs.full_image_len())
    }
}

/// Main application state
#[derive(Debug, Default, Clone)]
pub struct Studio {
    active_tab: StudioTab,
    runtime_info: RuntimeInfo,
}

impl Studio {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_tab(&self) -> &StudioTab {
        &self.active_tab
    }

    pub fn runtime_info(&self) -> &RuntimeInfo {
        &self.runtime_info
    }

    pub fn update(&mut self, message: Message) -> Task<AppMessage> {
        match message {
            Message::TabSelected(tab) => {
                self.active_tab = tab;
                Task::none()
            }
            Message::HardwareInfo(info) => {
                self.runtime_info.set_hw_info(info);

                // Share with Create
                task_from_msg!(CreateMessage::DetectedHardwareInfo)
            }
            Message::FetchReleases => Task::future(Self::fetch_releases_async()),
            Message::Releases(releases) => {
                self.download_succeeded();
                self.runtime_info.set_releases(releases.clone());
                Task::done(CreateMessage::ReleasesUpdated.into())
            }
            Message::DownloadRelease(release, board, mcu) => {
                self.download_release(release, board, mcu)
            }
            Message::ReleaseDownloaded(result) => {
                match &result {
                    Ok(data) => {
                        self.download_succeeded();
                        self.runtime_info.set_firmware(data.clone());
                    }
                    Err(_) => {
                        self.download_failed();
                        self.runtime_info.clear_firmware();
                    }
                };
                let result = result.map(drop);
                task_from_msg!(CreateMessage::ReleaseDowloaded(result))
            }
            Message::ReleaseDoesntExist => {
                self.runtime_info.clear_firmware();
                task_from_msg!(CreateMessage::ReleaseDowloaded(Err(
                    "Requested firmware release does not exist".to_string()
                )))
            }
            Message::ClearDownloadedRelease => {
                self.download_succeeded();
                self.runtime_info.clear_firmware();
                Task::none()
            }
            Message::FetchConfigs => Task::future(Self::fetch_configs_async(
                self.runtime_info.selected_config().cloned(),
            )),
            Message::ConfigManifest(configs) => {
                self.download_succeeded();
                self.runtime_info.set_configs(configs.clone());
                Task::done(CreateMessage::ConfigsUpdated.into())
            }
            Message::LoadConfig(config) => self.load_config(config),
            Message::ConfigLoaded(result) => {
                let create_result = match result {
                    Ok(selected) => {
                        self.download_succeeded();
                        self.runtime_info.set_selected_config(selected);
                        Ok(())
                    }
                    Err(e) => {
                        self.download_failed();
                        self.runtime_info.clear_selected_config();
                        Err(e)
                    }
                };
                Task::done(CreateMessage::ConfigLoaded(create_result).into())
            }
            Message::ClearDownloadedConfig => {
                self.runtime_info.clear_config();
                Task::none()
            }
            Message::BuildImage(hw_info) => {
                Task::future(Self::build_image_async(hw_info, self.runtime_info.clone()))
            }
            Message::BuildImageResult(result) => {
                let msg = match result {
                    Ok((image, desc)) => {
                        self.runtime_info.image = Some(image);
                        CreateMessage::BuildImageResult(Ok(desc))
                    }
                    Err(e) => {
                        warn!("Failed to build image: {e}");
                        CreateMessage::BuildImageResult(Err(e))
                    }
                };
                Task::done(msg.into())
            }
            Message::HelpPressed => self.help_pressed(),
            Message::DownloadFailed => {
                self.download_failed();
                Task::none()
            }
        }
    }

    fn download_failed(&mut self) {
        self.runtime_info.network_offline()
    }

    fn download_succeeded(&mut self) {
        self.runtime_info.network_online()
    }

    fn help_pressed(&self) -> Task<AppMessage> {
        Task::none()
    }

    async fn build_image_async(hw_info: HardwareInfo, runtime_info: RuntimeInfo) -> AppMessage {
        // Check we have firmware and config
        let firmware = if let Some(fw) = runtime_info.firmware() {
            fw.clone()
        } else {
            warn!("No firmware downloaded, cannot build image");
            return CreateMessage::BuildImageResult(Err("No firmware downloaded.\n  This is likely due to an internal error.  Please report it.".to_string()))
                .into();
        };

        let config = if let Some(cfg) = runtime_info.selected_config() {
            cfg.clone()
        } else {
            warn!("No config downloaded, cannot build image");
            return CreateMessage::BuildImageResult(Err("No config downloaded.\n  This is likely due to an internal error.  Please report it.".to_string())).into();
        };

        // Turn config into string
        let config_str = match String::from_utf8(config.data) {
            Ok(s) => s,
            Err(e) => {
                warn!("Config is not valid UTF-8: {}", e);
                return CreateMessage::BuildImageResult(Err(
                    "The supplied config is not valid UTF-8.".to_string(),
                ))
                .into();
            }
        };

        // Create image builder from config
        let fw_ver = if let Some(release) = runtime_info.selected_firmware() {
            if let Ok(fw) = release.firmware_version() {
                fw
            } else {
                warn!("Cannot get firmware version from selected release, cannot build image");
                return CreateMessage::BuildImageResult(Err(
                    "Cannot get firmware version from selected release.\n  This is likely due to an internal error.  Please report it.".to_string(),
                ))
                .into();
            }
        } else {
            warn!("Cannot get firmware version from hardware info, cannot build image");
            return CreateMessage::BuildImageResult(Err(
                "Cannot get firmware version from hardware info.\n  This is likely due to an internal error.  Please report it.".to_string(),
            ))
            .into();
        };

        // Check firmware build compatibility
        let manifest = app_manifest();
        let max_buildable = manifest.max_buildable_firmware();
        #[allow(clippy::collapsible_if)]
        if let Some(max) = max_buildable {
            if fw_ver > *max {
                warn!(
                    "Firmware version {fw_ver} is too new for this app version to build (max buildable: {max})"
                );
                return CreateMessage::BuildImageResult(Err(
                    format!("Firmware version {fw_ver} is too new for this app version to build.\nMaximum buildable firmware version: {max}\n  Update the app to build with newer firmware versions.")
                ))
                .into();
            }
        }

        let mcu_fam = if let Some(mcu) = hw_info.mcu_variant {
            mcu.family()
        } else {
            warn!("Cannot get MCU family from hardware info, cannot build image");
            return CreateMessage::BuildImageResult(Err(
                "Cannot get MCU family from hardware info.\n  This is likely due to an internal error.  Please report it.".to_string(),
            ))
            .into();
        };

        let mut builder = match Builder::from_json(fw_ver, mcu_fam, &config_str) {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to create image builder from config: {e:?}");
                return CreateMessage::BuildImageResult(Err(e.to_string())).into();
            }
        };

        // Get ROM files we need to download.  Cache them so that if we're asked to download the
        // same file again (for example zip with multiple extracts) we don't redownload it.
        //
        // Should implement in fw::get_rom_files_async copying existing get_rom_files()
        match get_rom_files_async(&mut builder).await {
            Ok(()) => (),
            Err(e) => {
                warn!("Failed to get ROM files: {e:?}");
                return CreateMessage::BuildImageResult(Err(e.to_string())).into();
            }
        }

        // A config may name a plugin, which reaches the image without ever
        // having been selected against the images server's manifest.  The
        // binary header declares only a minimum firmware version, so a plugin
        // withdrawn for a *newer* firmware - which hard faults the device on
        // boot - is caught only here.
        match check_config_plugins(&builder, &fw_ver, &CliFetch).await {
            Ok(notes) => {
                for note in notes {
                    match note {
                        PluginNote::Checked { name, version } => {
                            debug!("Plugin '{name}' v{version} is compatible with firmware")
                        }
                        PluginNote::Unofficial { source } => {
                            debug!("Plugin {source} is not an official plugin, cannot be checked")
                        }
                        PluginNote::Unchecked { source, error } => warn!(
                            "Could not check plugin {source} for firmware compatibility: {error}"
                        ),
                    }
                }
            }
            Err(e) => {
                warn!("Plugin incompatible with selected firmware: {e}");
                return CreateMessage::BuildImageResult(Err(e.to_string())).into();
            }
        }

        // Get firmware version
        let fw = match runtime_info.selected_firmware() {
            Some(fw) => fw,
            None => {
                warn!("No selected firmware, cannot build image");
                return CreateMessage::BuildImageResult(Err("No selected firmware.\n  This is likely due to an internal error.  Please report it.".to_string()))
                    .into();
            }
        };

        // Build the firmware properties
        let props = match hw_info.firmware_properties(fw) {
            Some(p) => p,
            None => {
                warn!("Cannot get firmware properties, cannot build image");
                return CreateMessage::BuildImageResult(Err(
                    "Cannot get firmware properties.\n  This is likely due to an internal error.  Please report it.".to_string()
                ))
                .into();
            }
        };

        // Build the image
        let (metadata, roms) = match builder.build(props) {
            Ok((md, roms)) => (md, roms),
            Err(e) => {
                warn!("Failed to build image: {e:?}");
                return CreateMessage::BuildImageResult(Err(e.to_string())).into();
            }
        };

        // Store image
        let image = Image {
            firmware,
            metadata,
            roms,
        };
        let total_len = image.full_image_len();
        let fw_len = image.firmware_len();
        let md_len = image.metadata_len();
        let roms_len = image.roms_len();

        // Get description
        let desc = builder.description();

        debug!(
            "Built image: total={total_len} bytes, firmware={fw_len} bytes, metadata={md_len} bytes, roms={roms_len} bytes"
        );

        Message::BuildImageResult(Ok((image, desc))).into()
    }

    async fn fetch_releases_async() -> AppMessage {
        let url = app_manifest()
            .manifest_url(ManifestType::FirmwareRelease)
            .to_string();
        match Releases::from_network_async_url(&url).await {
            Ok(releases) => AppMessage::Studio(Message::Releases(releases)),
            Err(e) => {
                warn!("Failed to fetch releases from network\n  - {e}");
                Message::DownloadFailed.into()
            }
        }
    }

    async fn fetch_configs_async(selected: Option<SelectedConfig>) -> AppMessage {
        match ConfigManifest::from_network_async(selected).await {
            Ok(configs) => AppMessage::Studio(Message::ConfigManifest(configs)),
            Err(e) => {
                warn!("Failed to fetch configs from network\n  - {e}");
                Message::DownloadFailed.into()
            }
        }
    }

    fn download_release(
        &mut self,
        release: Release,
        board: Board,
        mcu: McuVariant,
    ) -> Task<AppMessage> {
        self.runtime_info.clear_selected_firmware();

        // Check we have Releases
        let releases = if let Some(releases) = self.runtime_info.releases() {
            releases.clone()
        } else {
            error!("No releases available in Studio, cannot download");
            return Task::none();
        };

        // Get the firmware version
        let Ok(fw_ver) = release.firmware_version() else {
            warn!("No firmware version {release} found, cannot download");
            return Task::none();
        };

        // Set the selected firmware
        self.runtime_info.set_selected_firmware(release.clone());

        // Download the firmware
        Task::future(Self::download_release_async(releases, fw_ver, board, mcu))
    }

    fn load_config(&mut self, config: Config) -> Task<AppMessage> {
        self.runtime_info.clear_selected_config();

        let task = match &config {
            Config::Network { .. } => Task::future(download_config_async(config.clone())),
            Config::File { .. } => Task::done(load_config_file(config.clone())),
            Config::SelectLocalFile => {
                internal_error!("SelectLocalFile should be handled before Studio");
                return Task::none();
            }
            Config::Build => {
                internal_error!("Build should be handled before Studio");
                return Task::none();
            }
            Config::Built { .. } => Task::done(generate_built_config(config.clone())),
        };

        // Set the selected config
        self.runtime_info.set_selected_config(config.into());

        // Download the config
        task
    }

    async fn download_release_async(
        releases: Releases,
        fw_ver: FirmwareVersion,
        board: Board,
        mcu: McuVariant,
    ) -> AppMessage {
        // Download the firmware
        let result = match releases
            .download_firmware_async(&fw_ver, &board, &mcu)
            .await
        {
            Ok(data) => Ok(data),
            Err(onerom_fw::Error::ReleaseNotFound) => {
                trace!("Release {fw_ver:?} does not exist for {board} {mcu}");
                return Message::ReleaseDoesntExist.into();
            }
            Err(onerom_fw::Error::Http { url, status }) => {
                info!(
                    "Failed to download release {fw_ver:?} for {board} {mcu} from {url}: HTTP error {status}"
                );
                return Message::ReleaseDoesntExist.into();
            }
            Err(e) => {
                let log = format!("Failed to download release {fw_ver} for {board} {mcu}:\n  {e}");
                warn!("{log}");
                Err(log)
            }
        };

        Message::ReleaseDownloaded(result).into()
    }

    pub fn top_level_buttons(&self, serious_errors: bool) -> iced::Element<'_, AppMessage> {
        StudioTab::buttons(self.active_tab(), serious_errors)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let check_releases_duration = if self.runtime_info.releases().is_some() {
            MANIFEST_RETRY_LONG
        } else {
            MANIFEST_RETRY_SHORT
        };
        let check_configs_duration = if self.runtime_info.config_manifest().is_some() {
            MANIFEST_RETRY_LONG
        } else {
            MANIFEST_RETRY_SHORT
        };

        Subscription::batch(vec![
            time::every(check_releases_duration).map(|_| Message::FetchReleases),
            time::every(check_configs_duration).map(|_| Message::FetchConfigs),
        ])
    }
}
