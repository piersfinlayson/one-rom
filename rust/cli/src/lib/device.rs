// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Device selection logic.
//!
//! Provides a single entry point for resolving a --serial selector (or the
//! implicit single-device case) to a connected One ROM device.

use log::debug;
use nusb::DeviceInfo;
use onerom_config::hw::Board;
use onerom_fw_parser::ParsedDevice;
use wildmatch::WildMatch;

use crate::error::Error;
use crate::usb::enumerate_devices;

/// One ROM device state
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DeviceState {
    Unknown,
    Stopped,
    Running,
    Limp,
}

impl std::fmt::Display for DeviceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state_str = match self {
            DeviceState::Unknown => "Unknown",
            DeviceState::Stopped => "Stopped",
            DeviceState::Running => "Running",
            DeviceState::Limp => "Limp Mode",
        };
        write!(f, "{state_str}")
    }
}

/// A discovered One ROM Fire (RP2350) USB device.
pub struct Device {
    /// USB Vendor ID.
    pub vid: u16,
    /// USB Product ID.
    pub pid: u16,
    /// USB bus identifier.
    pub bus_id: String,
    /// USB device address on the bus.
    pub address: u8,
    /// USB serial number string, if present.
    pub serial: Option<String>,
    /// Underlying nusb device info, retained for opening connections.
    #[allow(unused)]
    pub device_info: DeviceInfo,
    /// One ROM device information, if present on the device
    pub onerom: Option<ParsedDevice>,
    /// Running or stopped.
    pub state: DeviceState,
    /// Whether this device is capable of running One ROM firmware while
    /// plugged into USB
    pub usb_can_run: bool,
}

impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let serial = self.serial.as_deref().unwrap_or("(no serial)");
        let info_str = match self.onerom.as_ref() {
            Some(ParsedDevice::Original(sdrr))
                if sdrr.flash.as_ref().and_then(|f| f.board.as_ref()).is_some() =>
            {
                let info = sdrr.flash.as_ref().unwrap();
                let board = info.board.as_ref().unwrap();
                let fw_version = &info.version;
                format!("One ROM {} - Firmware: {fw_version}", board_label(board))
            }
            Some(ParsedDevice::Schema(onerom)) if onerom.info().is_some() => {
                let info = onerom.info().unwrap();
                let hw_rev = onerom
                    .metadata()
                    .map(|m| m.hw.hw_rev.as_str())
                    .unwrap_or("unknown");
                let fw_version = format!(
                    "v{}.{}.{}",
                    info.major_version, info.minor_version, info.patch_version
                );
                let board_part = match Board::try_from_str(hw_rev) {
                    Some(board) => board_label(&board),
                    None => hw_rev.to_string(),
                };
                format!("One ROM {board_part} - Firmware: {fw_version}")
            }
            _ => "Unknown           - Firmware: n/a  ".to_string(),
        };
        write!(f, "{info_str} State: {} Serial: {serial}", self.state)
    }
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("vid", &format_args!("{:#06x}", self.vid))
            .field("pid", &format_args!("{:#06x}", self.pid))
            .field("bus_id", &self.bus_id)
            .field("address", &self.address)
            .field("serial", &self.serial)
            .finish()
    }
}

impl Device {
    /// Returns whether this is a recognised One ROM device.
    ///
    /// A recognised device has valid One ROM flash or RAM information
    /// available.
    pub fn is_recognised(&self) -> bool {
        self.onerom.as_ref().is_some_and(ParsedDevice::is_recognised)
    }

    pub fn is_running(&self) -> bool {
        self.state == DeviceState::Running
    }

    pub fn usb_can_run(&self) -> bool {
        self.usb_can_run
    }

    pub fn update_onerom(&mut self, onerom: ParsedDevice) {
        self.onerom = Some(onerom);
        self.update_state();
    }

    // Figure out the device state from the presence of the One ROM device
    // information
    fn update_state(&mut self) {
        self.usb_can_run = false;
        self.state = DeviceState::Unknown;

        let Some(onerom) = self.onerom.as_ref() else {
            return;
        };

        match onerom {
            ParsedDevice::Original(sdrr) => {
                if sdrr.flash.is_none() {
                    return;
                };

                if let Some(runtime_info) = &sdrr.ram {
                    self.state = match runtime_info.limp_mode.as_ref() {
                        Some(limp_mode)
                            if *limp_mode != onerom_fw_parser::types::LimpMode::None =>
                        {
                            DeviceState::Limp
                        }
                        _ => DeviceState::Running,
                    }
                } else {
                    self.state = DeviceState::Stopped;
                }
            }
            ParsedDevice::Schema(onerom) => {
                if onerom.info().is_none() {
                    return;
                };

                if let Some(runtime_info) = &onerom.runtime() {
                    self.state = match runtime_info.limp_mode {
                        onerom_metadata::LimpModePattern::LimpModeNone => DeviceState::Running,
                        _ => DeviceState::Limp,
                    }
                } else {
                    self.state = DeviceState::Stopped;
                }
            }
        }

        self.usb_can_run = onerom.is_usb_run_capable();
    }

    pub fn get_active_rom_set_index(&self) -> Option<u8> {
        self.onerom.as_ref()?.active_slot_index().map(|i| i as u8)
    }

    /// Returns (rom type label, rom size in bytes) for the active ROM,
    /// if the device is running. Neutral across SDRR and schema devices.
    fn active_rom_facts(&self) -> Option<(String, usize)> {
        if !self.is_running() {
            return None;
        }
        let onerom = self.onerom.as_ref()?;
        let slot = onerom.slots().find(|s| s.active)?;
        let rom = slot.roms().next()?;
        Some((rom.rom_type.into_owned(), rom.size))
    }

    /// Returns the active ROM type label if available.
    pub fn get_active_rom_type(&self) -> Option<String> {
        self.active_rom_facts().map(|(ty, _)| ty)
    }

    /// Returns the active ROM size in bytes if available.
    pub fn get_active_rom_size(&self) -> Option<usize> {
        self.active_rom_facts().map(|(_, size)| size)
    }

    /// Returns whether this device matches the provided serial pattern, which
    /// supports * and ? wildcards
    pub fn matches_serial(&self, pattern: &str) -> bool {
        matches_serial(self.serial.as_deref(), pattern)
    }

    /// Returns a sort key for this device, which sorts first by board type (with
    /// unrecognised devices sorted last) and then by serial number (with devices
    /// with no serial sorted last).
    pub fn sort_key(&self) -> (String, String) {
        let board = self
            .onerom
            .as_ref()
            .and_then(|o| match o {
                ParsedDevice::Original(sdrr) => sdrr
                    .flash
                    .as_ref()
                    .and_then(|f| f.board.as_ref())
                    .map(|b| b.model().to_string()),
                ParsedDevice::Schema(onerom) => onerom.metadata().map(|m| m.hw.hw_rev.clone()),
            })
            .unwrap_or_else(|| "~".to_string()); // sorts after Z
        let serial = self.serial.clone().unwrap_or_else(|| "~".to_string());
        (board, serial)
    }
}

/// Human-readable board identity fragment, e.g. "Fire 24 F".
/// Shared by both Display arms so SDRR and schema devices render identically.
fn board_label(board: &Board) -> String {
    let model = board.model();
    let pins = board.chip_pins();
    // Derive rev from the canonical name, not the raw hw_rev, so legacy
    // aliases normalise to the same output.
    let rev = board
        .name()
        .rsplit_once('-')
        .map(|(_, rev)| rev)
        .unwrap_or("")
        .to_uppercase();
    format!("{model} {pins} {rev}")
}

/// Returns whether a serial number matches a given pattern, which may include
/// wildcards.
pub fn matches_serial(serial: Option<&str>, pattern: &str) -> bool {
    let pattern_upper = pattern.to_uppercase();
    let matcher = WildMatch::new(&pattern_upper);
    serial
        .map(|s| matcher.matches(&s.to_uppercase()))
        .unwrap_or(false)
}

/// Enumerate connected devices and select one based on an optional serial
/// number selector.
///
/// - No selector, one device found: returns that device.
/// - No selector, multiple devices found: returns an error listing serials.
/// - Selector provided: matches against serial number, errors if not found.
pub async fn select_device(
    selector: Option<&str>,
    unrecognised: bool,
    vid_pid: &[(u16, u16)],
) -> Result<Device, Error> {
    let devices = enumerate_devices(unrecognised, vid_pid).await?;

    if devices.is_empty() {
        debug!("No devices found");
        return Err(Error::NoDevices);
    }

    match selector {
        None => {
            if devices.len() > 1 {
                let serials: Vec<String> = devices
                    .iter()
                    .map(|d| d.serial.as_deref().unwrap_or("(no serial)").to_string())
                    .collect();
                debug!("Multiple devices found with no selector: {serials:?}");
                Err(Error::MultipleDevices(serials))
            } else {
                let device = devices.into_iter().next().unwrap();
                debug!("Auto-selected device: {device}");
                Ok(device)
            }
        }
        Some(pattern) => {
            let mut matched: Vec<Device> = devices
                .into_iter()
                .filter(|d| matches_serial(d.serial.as_deref(), pattern))
                .collect();
            match matched.len() {
                0 => Err(Error::DeviceNotFound(pattern.to_string())),
                1 => Ok(matched.remove(0)),
                _ => {
                    let serials: Vec<String> = matched
                        .iter()
                        .map(|d| d.serial.as_deref().unwrap_or("(no serial)").to_string())
                        .collect();
                    debug!("Multiple devices found with selector '{pattern}': {serials:?}");
                    Err(Error::MultipleDevices(serials))
                }
            }
        }
    }
}
