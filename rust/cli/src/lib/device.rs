// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Device selection logic.
//!
//! Provides a single entry point for resolving a --serial selector (or the
//! implicit single-device case) to a connected One ROM device.

use log::debug;
use nusb::DeviceInfo;
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
        let info_str = if let Some(onerom) = self.onerom.as_ref()
            && let Some(sdrr) = onerom.as_original()
            && let Some(info) = sdrr.flash.as_ref()
            && let Some(board) = info.board.as_ref()
        {
            let model = board.model().to_string();
            let chip_pins = board.chip_pins();
            let hw_rev = board
                .name()
                .rsplit_once('-')
                .map(|(_, rev)| rev)
                .unwrap_or("(unknown revision)")
                .to_uppercase();
            let fw_version = &info.version;
            format!("One ROM {model} {chip_pins} {hw_rev} - Firmware: {fw_version}")
        } else {
            "Unknown           - Firmware: n/a  ".to_string()
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
        self.onerom.as_ref().map(|o| match o {
            ParsedDevice::Original(sdrr) => sdrr.flash.is_some() || sdrr.ram.is_some(),
            ParsedDevice::Schema(onerom) => onerom.info().is_some(),
        }).unwrap_or(false)
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

        let Some(onerom) = self.onerom.as_ref() else { return; };

        match onerom {
            ParsedDevice::Original(sdrr) => {
                let Some(flash) = sdrr.flash.as_ref() else { return; };

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

                self.usb_can_run = flash.is_usb_run_capable();
            }
            ParsedDevice::Schema(onerom) => {
                if onerom.info().is_none() {
                    return;
                }

                if onerom.runtime().is_some() {
                    self.state = DeviceState::Running;
                } else {
                    self.state = DeviceState::Stopped;
                }
                // usb_can_run not yet determinable from schema format
            }
        }
    }

    pub fn get_active_rom_set_index(&self) -> Option<u8> {
        self.onerom.as_ref().and_then(|o| match o {
            ParsedDevice::Original(sdrr) => {
                sdrr.ram.as_ref().map(|ram| ram.rom_set_index)
            }
            ParsedDevice::Schema(_) => None,
        })
    }

    /// Returns the active ROM set if available.
    pub fn get_active_rom_set(&self) -> Option<&onerom_fw_parser::SdrrRomSet> {
        let sdrr = self.onerom.as_ref().and_then(|o| o.as_original())?;
        let flash_info = sdrr.flash.as_ref()?;
        let active_set_index = self.get_active_rom_set_index()? as usize;
        flash_info.rom_sets.get(active_set_index)
    }

    /// Returns the active ROM type if available.
    pub fn get_active_rom_type(&self) -> Option<onerom_fw_parser::SdrrRomType> {
        if !self.is_running() {
            return None;
        }
        self.get_active_rom_set()
            .and_then(|set| set.roms.first())
            .map(|rom| rom.rom_type)
    }

    /// Returns the active ROM size if available
    pub fn get_active_rom_size(&self) -> Option<usize> {
        self.get_active_rom_type()
            .map(|rom_type| rom_type.rom_size())
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
            .and_then(|o| o.as_original())
            .and_then(|s| s.flash.as_ref())
            .and_then(|f| f.board.as_ref())
            .map(|b| b.model().to_string())
            .unwrap_or_else(|| "~".to_string()); // sorts after Z
        let serial = self.serial.clone().unwrap_or_else(|| "~".to_string());
        (board, serial)
    }
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