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
use onerom_config::mcu::{Rp235xChipId, RpVariant};
use onerom_fw_parser::ParsedDevice;
use onerom_lab_parser::Lab;
use wildmatch::WildMatch;

use crate::Options;
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

// Not non_exhaustive, so a new firmware type fails the build at every match
// that must handle it.
/// A device's firmware, as its own parser read it.
#[derive(Debug)]
pub enum Firmware {
    /// One ROM, from either firmware generation.
    OneRom(ParsedDevice),
    /// One ROM Lab.
    Lab(Lab),
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
    /// The device's firmware, `None` if this build doesn't recognise it.
    pub firmware: Option<Firmware>,
    /// Running or stopped.
    pub state: DeviceState,
    /// Whether this device runs while plugged into USB.
    pub usb_can_run: bool,
    /// The RP2350 chip ID, if it has been read. This is the device's invariant
    /// identity, used to track it across reboots where the USB serial changes
    /// (bootloader mode, or a programmed serial override).
    pub chip_id: Option<Rp235xChipId>,
    /// The RP2350 package variant (RP235xA/RP235xB), if it has been read.
    /// Populated when read from a running device via GET_INFO.
    pub rp_variant: Option<RpVariant>,
}

impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let serial = self.serial.as_deref().unwrap_or("(no serial)");
        let info_str = match self.firmware.as_ref() {
            Some(Firmware::OneRom(ParsedDevice::Original(sdrr)))
                if sdrr.flash.as_ref().and_then(|f| f.board.as_ref()).is_some() =>
            {
                let info = sdrr.flash.as_ref().unwrap();
                let board = info.board.as_ref().unwrap();
                let fw_version = &info.version;
                format!("One ROM {} - Firmware: {fw_version}", board_label(board))
            }
            Some(Firmware::OneRom(ParsedDevice::Schema(onerom))) if onerom.info().is_some() => {
                let info = onerom.info().unwrap();
                let hw_rev = onerom.metadata().map(|m| m.hw.hw_rev.as_str());
                let fw_version = format!(
                    "v{}.{}.{}",
                    info.major_version, info.minor_version, info.patch_version
                );
                format!("One ROM {} - Firmware: {fw_version}", board_part(hw_rev))
            }
            Some(Firmware::Lab(lab)) => {
                let info = &lab.info;
                let fw_version = format!(
                    "v{}.{}.{}",
                    info.major_version, info.minor_version, info.patch_version
                );
                format!(
                    "One ROM Lab {} - Firmware: {fw_version}",
                    lab_board_part(lab)
                )
            }
            Some(Firmware::OneRom(_)) | None => "Unknown           - Firmware: n/a  ".to_string(),
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
    /// Returns whether this build recognises the device's firmware.
    pub fn is_recognised(&self) -> bool {
        self.firmware.is_some()
    }

    pub fn is_running(&self) -> bool {
        self.state == DeviceState::Running
    }

    pub fn usb_can_run(&self) -> bool {
        self.usb_can_run
    }

    pub(crate) fn set_firmware(&mut self, firmware: Option<Firmware>) {
        self.firmware = firmware;
        self.update_state();
    }

    // Figure out the device state from its firmware's runtime information
    #[allow(clippy::wildcard_enum_match_arm)]
    fn update_state(&mut self) {
        self.usb_can_run = false;
        self.state = DeviceState::Unknown;

        let onerom = match self.firmware.as_ref() {
            Some(Firmware::OneRom(onerom)) => onerom,
            Some(Firmware::Lab(lab)) => {
                self.usb_can_run = true;
                self.state = match lab.runtime {
                    Ok(_) => DeviceState::Running,
                    Err(_) => DeviceState::Stopped,
                };
                return;
            }
            None => return,
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
                    // A pattern this build has no name for is still a pattern
                    // the device is blinking, so it counts as limping.
                    self.state = match runtime_info.limp_mode {
                        onerom_metadata::MaybeKnown::Known(
                            onerom_metadata::LimpModePattern::LimpModeNone,
                        ) => DeviceState::Running,
                        _ => DeviceState::Limp,
                    }
                } else {
                    self.state = DeviceState::Stopped;
                }
            }
            ParsedDevice::Lab => return,
            _ => return,
        }

        self.usb_can_run = onerom.is_usb_run_capable();
    }

    /// The One ROM's parse, `None` for other firmware.
    fn onerom(&self) -> Option<&ParsedDevice> {
        match self.firmware.as_ref()? {
            Firmware::OneRom(onerom) => Some(onerom),
            Firmware::Lab(_) => None,
        }
    }

    /// The device's board, from One ROM's metadata or a Lab's structures.
    pub(crate) fn board(&self) -> Option<Board> {
        match self.firmware.as_ref()? {
            Firmware::OneRom(onerom) => onerom.get_board(),
            Firmware::Lab(lab) => lab_hw_rev(lab).and_then(Board::try_from_str),
        }
    }

    pub fn get_active_rom_set_index(&self) -> Option<u8> {
        self.onerom()?.active_slot_index().map(|i| i as u8)
    }

    /// Returns (rom type label, rom size in bytes) for the active ROM,
    /// if the device is running. Neutral across SDRR and schema devices.
    fn active_rom_facts(&self) -> Option<(String, usize)> {
        if !self.is_running() {
            return None;
        }
        let slot = self.onerom()?.slots().find(|s| s.active)?;
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

    /// The verbose one-line MCU / chip-ID summary shown beneath the device
    /// header, e.g. `MCU: RP235xB Chip ID: FC9D67248E8E8023`. Returns `None`
    /// if the chip ID has not been read; the `MCU:` prefix is dropped when the
    /// package variant is unknown.
    pub fn mcu_chip_id_line(&self) -> Option<String> {
        let id = self.chip_id?;
        Some(match self.rp_variant {
            Some(variant) => format!("MCU: {variant} Chip ID: {id}"),
            None => format!("Chip ID: {id}"),
        })
    }

    /// Returns a sort key for this device, which sorts first by board type (with
    /// unrecognised devices sorted last) and then by serial number (with devices
    /// with no serial sorted last).
    pub fn sort_key(&self) -> (String, String) {
        let board = match self.firmware.as_ref() {
            Some(Firmware::OneRom(onerom)) => match onerom {
                ParsedDevice::Original(sdrr) => sdrr
                    .flash
                    .as_ref()
                    .and_then(|f| f.board.as_ref())
                    .map(|b| b.model().to_string()),
                ParsedDevice::Schema(onerom) => onerom.metadata().map(|m| m.hw.hw_rev.clone()),
                ParsedDevice::Lab => None,
                _ => None,
            },
            Some(Firmware::Lab(lab)) => lab_hw_rev(lab).map(str::to_string),
            None => None,
        }
        .unwrap_or_else(|| "~".to_string()); // sorts after Z
        let serial = self.serial.clone().unwrap_or_else(|| "~".to_string());
        (board, serial)
    }
}

/// The board a Lab is running as, or the one its image was built for when it
/// isn't running.
fn lab_hw_rev(lab: &Lab) -> Option<&str> {
    match &lab.runtime {
        Ok(runtime) => runtime.hw_rev.as_deref(),
        Err(_) => lab.metadata.as_ref().ok()?.hw.hw_rev.as_deref(),
    }
}

/// The board part of a Lab's line.  A Lab whose board isn't set says so in
/// Lab's own words.
fn lab_board_part(lab: &Lab) -> String {
    let read = lab.runtime.is_ok() || lab.metadata.is_ok();
    match lab_hw_rev(lab) {
        Some(hw_rev) => board_part(Some(hw_rev)),
        None if read => "(board not set)".to_string(),
        None => board_part(None),
    }
}

/// The board part of a device's line: the board's label where `hw_rev` names
/// one, the raw string where it doesn't, and "unknown" where `hw_rev` is
/// missing.
fn board_part(hw_rev: Option<&str>) -> String {
    let Some(hw_rev) = hw_rev else {
        return "unknown".to_string();
    };
    match Board::try_from_str(hw_rev) {
        Some(board) => board_label(&board),
        None => hw_rev.to_string(),
    }
}

/// Human-readable board identity fragment, e.g. "Fire 24 F".
/// Shared by every Display arm so all devices render identically.
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
pub async fn select_device(selector: Option<&str>, options: &Options) -> Result<Device, Error> {
    let devices = enumerate_devices(options).await?;

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

/// Re-select a device by its (invariant) chip ID.
///
/// Used to re-find a device after a state change that may have altered its USB
/// serial - entering the bootloader (where the serial reverts to the chip ID),
/// or programming a serial override. Unlike [`select_device`], this does not
/// rely on the serial string, which is not stable across those transitions.
///
/// If `chip_id` is `None` (the chip ID was never read), this falls back to
/// auto-selecting a single connected device, erroring if more than one is
/// present.
pub async fn select_device_by_chip_id(
    chip_id: Option<Rp235xChipId>,
    options: &Options,
) -> Result<Device, Error> {
    let devices = enumerate_devices(options).await?;

    if devices.is_empty() {
        debug!("No devices found");
        return Err(Error::NoDevices);
    }

    let Some(id) = chip_id else {
        // No chip ID to match on; fall back to single-device auto-select.
        if devices.len() > 1 {
            let serials: Vec<String> = devices
                .iter()
                .map(|d| d.serial.as_deref().unwrap_or("(no serial)").to_string())
                .collect();
            return Err(Error::MultipleDevices(serials));
        }
        return Ok(devices.into_iter().next().unwrap());
    };

    let mut matched: Vec<Device> = devices
        .into_iter()
        .filter(|d| d.chip_id == Some(id))
        .collect();

    match matched.len() {
        0 => Err(Error::DeviceNotFound(id.to_string())),
        1 => Ok(matched.remove(0)),
        // Chip IDs are unique, so more than one match indicates a bug or a
        // read error rather than genuinely duplicate hardware.
        _ => Err(Error::MultipleDevices(vec![id.to_string()])),
    }
}
