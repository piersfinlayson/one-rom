// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! USB device enumeration and transport primitives.
//!
//! Handles discovery of connected One ROM Fire (RP2350) devices via the
//! PICOBOOT protocol.

#[allow(unused_imports)]
use log::{debug, warn};
use onerom_fw_parser::Parser;
use picoboot::{Picoboot, Reader as PicobootReader, Target, usb::Timeouts};
use std::time::Duration;

use crate::Error;
pub use crate::picobootx::LedSubCmd;
use crate::picobootx::{ONEROM_CMD_SET_LED, ONEROM_MAGIC};
use crate::{Device, DeviceState};

/// Flash start address on RP2350.
pub const FLASH_BASE: u32 = 0x1000_0000;
pub const RAM_BASE: u32 = 0x2000_0000;

/// Size of the One ROM metadata region to read from flash.
pub const FLASH_READ_SIZE_KB: u32 = 64;
pub const FLASH_READ_SIZE_BYTES: u32 = FLASH_READ_SIZE_KB * 1024;

pub const DEFAULT_ONEROM_PICOBOOT_TARGETS: [Target; 3] = [
    Target::Rp2350,
    Target::Custom {
        vid: 0x1209,
        pid: 0xF540,
    },
    Target::Custom {
        vid: 0x1209,
        pid: 0xF542,
    },
];

/// Enumerate all connected One ROM Fire (RP2350) devices.
///
/// Returns an empty Vec rather than an error if no devices are found.
pub async fn enumerate_devices(
    unrecognised: bool,
    vid_pid: &[(u16, u16)],
) -> Result<Vec<Device>, Error> {
    // Create the list of targets to use Picoboot to scan for.  We only use
    // the default RP2350 if no custom VID/PID pairs were provided.
    let targets: Vec<Target> = vid_pid
        .iter()
        .map(|&(vid, pid)| Target::Custom { vid, pid })
        .collect();
    let targets = if targets.is_empty() {
        DEFAULT_ONEROM_PICOBOOT_TARGETS.to_vec()
    } else {
        targets
    };

    let device_infos = Picoboot::list_devices(Some(&targets))
        .await
        .map_err(|e| Error::Usb(e.to_string()))?;

    let mut devices = Vec::new();
    for info in device_infos {
        debug!(
            "Found Fire device: {:04x}:{:04x} bus {} addr {}",
            info.vendor_id(),
            info.product_id(),
            info.bus_id(),
            info.device_address(),
        );

        let mut device = Device {
            vid: info.vendor_id(),
            pid: info.product_id(),
            bus_id: info.bus_id().to_owned(),
            address: info.device_address(),
            serial: info.serial_number().map(str::to_owned),
            device_info: info,
            onerom: None,
            state: DeviceState::Unknown,
            usb_can_run: false,
        };

        if let Err(e) = read_device_info(&mut device).await {
            warn!("Failed to read device info on {device:?}: {e}");
        }

        if device.is_recognised() || unrecognised {
            devices.push(device);
        } else {
            debug!("Excluding unrecognised device: {device:?}");
        }
    }

    Ok(devices)
}

async fn get_picoboot(device: &Device, long: bool) -> Result<Picoboot, Error> {
    let mut picoboot = Picoboot::new(device.device_info.clone())
        .await
        .map_err(|e| Error::Usb(e.to_string()))?;

    let timeout = if long {
        // Flash erase can take a long time, so use a longer timeout for all
        // operations when erase is requested.
        Duration::from_secs(20)
    } else {
        Duration::from_millis(2500)
    };
    debug!("Setting PICOBOOT timeouts to {timeout:?} (long={long})");

    picoboot.set_timeouts(Timeouts {
        endpoint: timeout,
        ..Timeouts::default()
    });

    Ok(picoboot)
}

/// Read the first 64KB from flash on a One ROM Fire device.
///
/// Connects to the device via PICOBOOT, reads from the flash start address,
/// and returns the raw bytes. The caller is responsible for parsing the
/// contents.
pub async fn read_device_info(device: &mut Device) -> Result<(), Error> {
    debug!("Reading {FLASH_READ_SIZE_KB}KB from {FLASH_BASE:#010x} on {device}");

    let picoboot = get_picoboot(device, false).await?;
    let mut reader = PicobootReader::new(picoboot).await.map_err(Error::Usb)?;

    // Parse the device information, handling either firmware format.
    let mut parser = Parser::with_base_flash_address(&mut reader, FLASH_BASE, RAM_BASE);
    let onerom = parser.parse_device().await;
    device.update_onerom(onerom);

    Ok(())
}

/// What state One ROM should be rebooted into
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebootMode {
    /// Do not reboot
    None,
    /// Stopped is bootloader/BOOTSEL mode
    Stopped { msd: bool },
    /// Running is One ROM in byte serving mode
    Running,
}

impl std::fmt::Display for RebootMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RebootMode::None => write!(f, "none (skip reboot)"),
            RebootMode::Stopped { msd: true } => write!(f, "stopped (MSD enabled)"),
            RebootMode::Stopped { msd: false } => write!(f, "stopped"),
            RebootMode::Running => write!(f, "running"),
        }
    }
}
impl TryFrom<RebootMode> for picoboot::RebootType {
    type Error = Error;

    fn try_from(mode: RebootMode) -> Result<Self, Self::Error> {
        match mode {
            RebootMode::Stopped { msd } => Ok(picoboot::RebootType::Bootsel {
                disable_msd: !msd,
                disable_picoboot: false,
            }),
            RebootMode::Running => Ok(picoboot::RebootType::Normal),
            RebootMode::None => Err(Error::NoReboot),
        }
    }
}

/// Arguments for the reboot method
pub struct RebootArgs {
    /// Type of reboot to perform
    pub mode: RebootMode,

    /// Whether to reboot using "fast" mode (i.e. don't wait for USB device
    /// re-enumeration to take place)
    pub fast: bool,

    /// Whether to check that the device is capable of rebooting into running
    /// mode, before attempting to do so.  Not done for the program command,
    /// but is done for the reboot command.
    pub check_usb_can_run: bool,
}

impl RebootArgs {
    pub fn stopped(msd: bool, fast: bool) -> Self {
        Self {
            mode: RebootMode::Stopped { msd },
            fast,
            check_usb_can_run: false,
        }
    }

    pub fn running(fast: bool, check_usb_can_run: bool) -> Self {
        Self {
            mode: RebootMode::Running,
            fast,
            check_usb_can_run,
        }
    }

    pub fn none() -> Self {
        Self {
            mode: RebootMode::None,
            fast: false,
            check_usb_can_run: false,
        }
    }

    pub fn is_none(&self) -> bool {
        self.mode == RebootMode::None
    }
}

/// Reboot the chosen One ROM
pub async fn reboot(device: &Device, args: &RebootArgs) -> Result<(), Error> {
    // Check we can actually reboot into running mode if requested
    if args.mode == RebootMode::Running && args.check_usb_can_run && !device.usb_can_run() {
        return Err(Error::NoRebootIntoRunning(device.to_string()));
    }

    let mut picoboot = get_picoboot(device, false).await?;

    // Early return Ok(()) if no reboot requested
    let reboot_type = if let Ok(rt) = args.mode.try_into() {
        rt
    } else {
        debug!("No reboot requested, skipping");
        return Ok(());
    };

    const REBOOT_TIMER: Duration = Duration::from_millis(10);
    debug!("Rebooting device {device} with type {reboot_type:?} and timer {REBOOT_TIMER:?}");
    picoboot
        .reboot(reboot_type, REBOOT_TIMER)
        .await
        .map_err(|e| Error::Usb(e.to_string()))?;

    if !args.fast {
        pause_reenumeration().await;
    }

    Ok(())
}

enum MemoryType {
    /// RP2350 bootrom, never writeable
    BootRom,
    /// RP2350 flash, readable at all times, writeable but only through
    /// specific methods
    Flash,
    /// RP2350 physical SRAM, readable and writeable at all times.
    Ram,
    /// Virtual One ROM addresses that are read write at all times when One
    /// ROM is running
    VirtualRw,
}

// A valid One ROM MCU memory region
struct MemoryRegion {
    _name: &'static str,
    start: u32,
    len: u32,
    // true if only accessible when device is in Running state
    mem_type: MemoryType,
}

impl MemoryRegion {
    const fn new(name: &'static str, start: u32, len: u32, mem_type: MemoryType) -> Self {
        Self {
            _name: name,
            start,
            len,
            mem_type,
        }
    }

    fn contains(&self, address: u32, length: u32) -> bool {
        address >= self.start && length <= self.len && address - self.start <= self.len - length
    }
}

const VALID_REGIONS: &[MemoryRegion] = &[
    // 2MB of flash
    MemoryRegion::new("Flash", 0x1000_0000, 0x0020_0000, MemoryType::Flash),
    // 520KB of SRAM
    MemoryRegion::new("SRAM", 0x2000_0000, 0x0008_2000, MemoryType::Ram),
    // 32KB of Boot ROM
    MemoryRegion::new("ROM", 0x0000_0000, 0x0000_8000, MemoryType::BootRom),
    // 512KB of live ROM data
    MemoryRegion::new(
        "Live ROM Image",
        0x9000_0000,
        0x0008_0000,
        MemoryType::VirtualRw,
    ),
];

fn check_memory_range(
    device: &Device,
    address: u32,
    length: u32,
    write: bool,
    flash_writes_allowed: bool,
) -> Result<(), Error> {
    for region in VALID_REGIONS {
        if region.contains(address, length) {
            return match region.mem_type {
                MemoryType::BootRom => {
                    if write {
                        Err(Error::MemoryNotWriteable)
                    } else {
                        Ok(())
                    }
                }
                MemoryType::Flash => {
                    if !write || flash_writes_allowed {
                        Ok(())
                    } else {
                        Err(Error::MemoryNotWriteable)
                    }
                }
                MemoryType::Ram => Ok(()),
                MemoryType::VirtualRw => {
                    if device.is_running() {
                        Ok(())
                    } else {
                        Err(Error::MemoryDeviceNotRunning)
                    }
                }
            };
        }
    }
    Err(Error::InvalidMemoryRange(address, length))
}

/// Read bytes from device memory
pub async fn read_memory(device: &Device, address: u32, length: u32) -> Result<Vec<u8>, Error> {
    check_memory_range(device, address, length, false, false)?;

    let mut picoboot = get_picoboot(device, false).await?;

    picoboot
        .read(address, length)
        .await
        .map_err(|e| Error::Usb(e.to_string()))
}

/// Write bytes to device memory.
///
/// Flash writes are not permitted via this path — use the update subcommands
/// for persistent flash writes. SRAM and virtual (live ROM) addresses are
/// both accepted.
pub async fn write_memory(device: &Device, address: u32, data: &[u8]) -> Result<(), Error> {
    check_memory_range(device, address, data.len() as u32, true, false)?;

    let mut picoboot = get_picoboot(device, false).await?;

    picoboot
        .write(address, data)
        .await
        .map_err(|e| Error::Usb(e.to_string()))
}

/// Erase and write firmware to device flash.
pub async fn flash_program(device: &Device, data: &[u8]) -> Result<(), Error> {
    let mut picoboot = get_picoboot(device, true).await?;

    picoboot
        .flash_erase_and_write(FLASH_BASE, data)
        .await
        .map_err(|e| Error::Usb(e.to_string()))
}

/// Read firmware from device flash for verification.
pub async fn flash_program_read(device: &Device, size: u32) -> Result<Vec<u8>, Error> {
    let mut picoboot = get_picoboot(device, false).await?;

    picoboot
        .flash_read(FLASH_BASE, size)
        .await
        .map_err(|e| Error::Usb(e.to_string()))
}

/// Erase a region of device flash.
///
/// Both `offset` and `size` are relative to `FLASH_BASE` and must be
/// multiples of 4096 (one flash sector).
pub async fn flash_erase(device: &Device, offset: u32, size: u32) -> Result<(), Error> {
    const SECTOR_SIZE: u32 = 4096;

    if !offset.is_multiple_of(SECTOR_SIZE) {
        return Err(Error::Other(format!(
            "offset {offset:#x} is not sector-aligned (must be a multiple of {SECTOR_SIZE:#x})"
        )));
    }
    if size == 0 || !size.is_multiple_of(SECTOR_SIZE) {
        return Err(Error::Other(format!(
            "size {size:#x} must be a non-zero multiple of {SECTOR_SIZE:#x}"
        )));
    }

    let address = FLASH_BASE + offset;
    check_memory_range(device, address, size, true, true)?;

    let mut picoboot = get_picoboot(device, true).await?;

    picoboot
        .flash_erase(address, size)
        .await
        .map_err(|e| Error::Usb(e.to_string()))
}

/// Sleep for a short time to allow the device to disconnect and reappear
/// after a reboot.
async fn pause_reenumeration() {
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
}

/// Set the status LED on a One ROM device.
pub async fn set_led(device: &Device, led_id: u8, sub_cmd: LedSubCmd) -> Result<(), Error> {
    let mut args = [0u8; 16];
    args[0] = led_id;
    args[1] = sub_cmd as u8;

    let cmd = picoboot::PicobootXCmd::new(ONEROM_MAGIC, ONEROM_CMD_SET_LED, 0x10, 0, args);

    let mut picoboot = get_picoboot(device, false).await?;

    picoboot
        .send_picobootx_cmd(cmd, None)
        .await
        .map(|_| ())
        .map_err(|e| Error::Usb(e.to_string()))
}
