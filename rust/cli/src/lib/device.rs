// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Device selection logic.
//!
//! Provides a single entry point for resolving a --serial selector (or the
//! implicit single-device case) to a connected One ROM device.

use log::debug;
use nusb::DeviceInfo;
use onerom_app::device_board_size;
use onerom_config::hw::{Board, BoardSize};
use onerom_config::mcu::{Rp235xChipId, RpVariant, Variant};
use onerom_fw_parser::{ParseError, ParsedDevice};
use onerom_gen::FlashChips;
use onerom_lab_parser::Lab;
use onerom_metadata::{MaybeKnown, OneromBoardSize, USB_BOOTLOADER_PID, USB_BOOTLOADER_VID};
use picoboot::{PICOBOOT_PID_RP2350, PICOBOOT_VID};
use wildmatch::WildMatch;

use crate::Options;
use crate::error::Error;
use crate::otp::{Commissioning, board_size_text};
use crate::usb::enumerate_devices;

pub use onerom_app::known_board_size;

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
    /// The RP2350 package variant (RP235xA/RP235xB), if it has been read, as
    /// [`ChipInfo::package`](crate::usb::ChipInfo::package) describes it.
    pub rp_variant: Option<RpVariant>,
    /// The device's commissioning area. Enumeration reads it.
    pub commissioning: Commissioning,
    /// The board's size, as [`Device::board_size()`] describes it.
    pub(crate) board_size: Option<MaybeKnown<OneromBoardSize>>,
    /// The firmware parser's reasons, where this build doesn't recognise the
    /// device's firmware.
    pub(crate) unrecognised_firmware_reasons: Vec<ParseError>,
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
                format!(
                    "One ROM {}{} - Firmware: {fw_version}",
                    board_label(board),
                    size_suffix(self.board_size)
                )
            }
            Some(Firmware::OneRom(ParsedDevice::Schema(onerom))) if onerom.info().is_some() => {
                let info = onerom.info().unwrap();
                let hw_rev = onerom.metadata().map(|m| m.hw.hw_rev.as_str());
                let fw_version = format!(
                    "v{}.{}.{}",
                    info.major_version, info.minor_version, info.patch_version
                );
                format!(
                    "One ROM {}{} - Firmware: {fw_version}",
                    board_part(hw_rev),
                    size_suffix(self.board_size)
                )
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
            Some(Firmware::OneRom(_)) | None => match self.commissioned_hw_rev() {
                Some(hw_rev) => format!(
                    "One ROM {}{} - Firmware: n/a  ",
                    board_part(Some(hw_rev)),
                    size_suffix(self.board_size)
                ),
                None => "Unknown           - Firmware: n/a  ".to_string(),
            },
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
    /// Returns whether this build recognises the device's firmware or its OTP
    /// holds a current commissioning instance.
    pub fn is_recognised(&self) -> bool {
        self.firmware.is_some() || self.commissioning.current().is_some()
    }

    pub fn is_running(&self) -> bool {
        self.state == DeviceState::Running
    }

    pub fn usb_can_run(&self) -> bool {
        self.usb_can_run
    }

    /// The firmware parser's reasons for not recognising the device's
    /// firmware. Empty where this build recognises it, and where the parser
    /// didn't provide a reason, as for erased flash.
    pub fn unrecognised_firmware_reasons(&self) -> &[ParseError] {
        &self.unrecognised_firmware_reasons
    }

    /// Sets the device's firmware, or the parser's reasons for not
    /// recognising it.
    pub(crate) fn set_firmware(&mut self, firmware: Result<Firmware, Vec<ParseError>>) {
        (self.firmware, self.unrecognised_firmware_reasons) = match firmware {
            Ok(firmware) => (Some(firmware), Vec::new()),
            Err(reasons) => (None, reasons),
        };
        self.update_state();
    }

    // Figure out the device state from its firmware's runtime information
    #[allow(clippy::wildcard_enum_match_arm)]
    fn update_state(&mut self) {
        self.usb_can_run = false;
        // A device on a bootloader's USB ID is stopped even where its flash
        // doesn't hold firmware this build recognises. Runtime info found in
        // RAM below overrides that.
        self.state = if is_bootloader(self.vid, self.pid) {
            DeviceState::Stopped
        } else {
            DeviceState::Unknown
        };

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
    pub(crate) fn onerom(&self) -> Option<&ParsedDevice> {
        match self.firmware.as_ref()? {
            Firmware::OneRom(onerom) => Some(onerom),
            Firmware::Lab(_) => None,
        }
    }

    /// The device's commissioned board or else the board its firmware is for.
    /// The commissioned board comes first because firmware from v0.8.0 stays in
    /// the bootloader where its board differs from the commissioned board.
    pub(crate) fn board(&self) -> Option<Board> {
        self.commissioned_board().or_else(|| self.firmware_board())
    }

    /// The board the device's firmware is for. One ROM's metadata or a Lab's
    /// structures hold it.
    pub fn firmware_board(&self) -> Option<Board> {
        match self.firmware.as_ref()? {
            Firmware::OneRom(onerom) => onerom.get_board(),
            Firmware::Lab(lab) => lab_hw_rev(lab).and_then(Board::try_from_str),
        }
    }

    /// The board the device's current commissioning instance holds. `None`
    /// where this build doesn't know it.
    pub fn commissioned_board(&self) -> Option<Board> {
        self.commissioned_hw_rev().and_then(Board::try_from_str)
    }

    /// The board name the device's current commissioning instance holds,
    /// whether or not this build knows the board.
    fn commissioned_hw_rev(&self) -> Option<&str> {
        self.commissioning.current()?.board()
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

    /// The line shown beneath the device's line giving its board size, e.g.
    /// `Board size: L`. `None` where the size isn't known.
    pub fn board_size_line(&self) -> Option<String> {
        let size = self.board_size?;
        Some(format!("Board size: {}", board_size_text(size)))
    }

    /// The board's size, from runtime info while One ROM runs and from OTP
    /// while it's stopped or where runtime info doesn't record it. `None` for
    /// One ROM Lab and where neither could be read.
    pub fn board_size(&self) -> Option<MaybeKnown<OneromBoardSize>> {
        self.board_size
    }

    /// The device's flash chips, from its board size. A board whose size
    /// isn't known has the first chip alone.
    pub fn flash_chips(&self) -> FlashChips {
        flash_chips(self.board_size)
    }

    /// Returns a sort key for this device, which sorts first by board type (with
    /// unrecognised devices sorted last) and then by serial number (with devices
    /// with no serial sorted last). The board type is the commissioned board's,
    /// or else the firmware's.
    pub fn sort_key(&self) -> (String, String) {
        let firmware = match self.firmware.as_ref() {
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
        };
        let board = self
            .commissioned_hw_rev()
            .map(str::to_string)
            .or(firmware)
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

/// The board size of a One ROM. `onerom` is its firmware's parse, `None` where
/// this build doesn't recognise its firmware.
///
/// It's the size runtime info records where One ROM is running and records
/// one. Otherwise it's the size `read_otp` reads from OTP. Where that read fails
/// it's what runtime info records. It's `None` for One ROM Lab and where neither
/// can be read.
pub async fn board_size(
    onerom: Option<&ParsedDevice>,
    read_otp: impl AsyncFnOnce() -> Option<OneromBoardSize>,
) -> Option<MaybeKnown<OneromBoardSize>> {
    if matches!(onerom, Some(ParsedDevice::Lab)) {
        return None;
    }
    let runtime = onerom.and_then(ParsedDevice::runtime_board_size);
    device_board_size(runtime, || read_otp()).await
}

/// The flash chips of a board whose recorded size is `size`. A board whose
/// size isn't known has the first chip alone because every board has it.
pub fn flash_chips(size: Option<MaybeKnown<OneromBoardSize>>) -> FlashChips {
    let size = known_board_size(size).unwrap_or(BoardSize::M);
    FlashChips::new(Variant::RP2350, size)
}

/// What follows the board in a device's line. `(L)` on an L board, `(other)`
/// on one that's neither M nor L, including a size this build doesn't know,
/// and nothing on an M board or where the size isn't known.
fn size_suffix(size: Option<MaybeKnown<OneromBoardSize>>) -> &'static str {
    match size {
        Some(MaybeKnown::Known(OneromBoardSize::BoardSizeL)) => " (L)",
        Some(MaybeKnown::Known(OneromBoardSize::BoardSizeOther) | MaybeKnown::Unknown(_)) => {
            " (other)"
        }
        Some(MaybeKnown::Known(
            OneromBoardSize::BoardSizeM | OneromBoardSize::BoardSizeUnknown,
        ))
        | None => "",
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

/// Returns whether `vid` and `pid` are a bootloader's USB ID, either the stock
/// RP2350 bootloader's or a commissioned One ROM's.
fn is_bootloader(vid: u16, pid: u16) -> bool {
    matches!(
        (vid, pid),
        (PICOBOOT_VID, PICOBOOT_PID_RP2350) | (USB_BOOTLOADER_VID, USB_BOOTLOADER_PID)
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_whose_size_isnt_known_has_the_first_chip_alone() {
        use OneromBoardSize::{BoardSizeL, BoardSizeM, BoardSizeOther, BoardSizeUnknown};
        let l = flash_chips(Some(MaybeKnown::Known(BoardSizeL)));
        assert_eq!(l.second(), Some(0x1100_0000..0x1120_0000));
        for size in [
            Some(MaybeKnown::Known(BoardSizeM)),
            Some(MaybeKnown::Known(BoardSizeUnknown)),
            Some(MaybeKnown::Known(BoardSizeOther)),
            Some(MaybeKnown::Unknown(3)),
            None,
        ] {
            let chips = flash_chips(size);
            assert_eq!(chips.first(), l.first(), "{size:?}");
            assert_eq!(chips.second(), None, "{size:?}");
        }
    }

    /// A One ROM that isn't running has the size OTP configures, and none
    /// where OTP can't be read.
    #[tokio::test]
    async fn a_one_rom_that_isnt_running_has_its_otp_size() {
        let l = board_size(None, async || Some(OneromBoardSize::BoardSizeL)).await;
        assert_eq!(l, Some(MaybeKnown::Known(OneromBoardSize::BoardSizeL)));
        assert_eq!(board_size(None, async || None).await, None);
    }

    /// One ROM Lab doesn't have a board size, so OTP isn't read.
    #[tokio::test]
    async fn one_rom_lab_doesnt_have_a_size() {
        let size = board_size(Some(&ParsedDevice::Lab), async || {
            panic!("OTP read for One ROM Lab")
        })
        .await;
        assert_eq!(size, None);
    }

    #[test]
    fn the_device_line_marks_only_l_and_other_sizes() {
        use OneromBoardSize::{BoardSizeL, BoardSizeM, BoardSizeOther, BoardSizeUnknown};
        // `(L)` is the form agreed for the device line, so it's quoted.
        assert_eq!(size_suffix(Some(MaybeKnown::Known(BoardSizeL))), " (L)");
        // A size that's neither M nor L carries the same suffix, whether this
        // build knows it or not.
        let other = size_suffix(Some(MaybeKnown::Known(BoardSizeOther)));
        assert!(!other.is_empty());
        assert_ne!(other, " (L)");
        assert_eq!(size_suffix(Some(MaybeKnown::Unknown(3))), other);
        // M, and a size that isn't known, carry none.
        for size in [
            Some(MaybeKnown::Known(BoardSizeM)),
            Some(MaybeKnown::Known(BoardSizeUnknown)),
            None,
        ] {
            assert_eq!(size_suffix(size), "", "{size:?}");
        }
    }

    #[test]
    fn only_a_bootloader_usb_id_means_stopped() {
        use onerom_metadata::{USB_PLUGIN_PID, USB_PLUGIN_VID};
        assert!(is_bootloader(PICOBOOT_VID, PICOBOOT_PID_RP2350));
        assert!(is_bootloader(USB_BOOTLOADER_VID, USB_BOOTLOADER_PID));
        // One ROM is running on the USB plugin's ID, even where this build
        // can't read its firmware.
        assert!(!is_bootloader(USB_PLUGIN_VID, USB_PLUGIN_PID));
    }
}
