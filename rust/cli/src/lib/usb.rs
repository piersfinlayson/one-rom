// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! USB device enumeration and transport primitives.
//!
//! Handles discovery of connected One ROM Fire (RP2350) devices via the
//! PICOBOOT protocol.

#[allow(unused_imports)]
use log::{debug, warn};
use onerom_config::mcu::{Rp235xChipId, RpVariant};
use onerom_fw_parser::Parser;
use picoboot::{
    Picoboot, PicobootCmd, PicobootCmdId, Reader as PicobootReader, Target, usb::Timeouts,
};
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
            chip_id: None,
            rp_variant: None,
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

/// RP2350 chip identity and package variant, read from a device via GET_INFO.
#[derive(Debug, Clone, Copy)]
pub struct ChipInfo {
    /// The device's invariant chip ID.
    pub chip_id: Rp235xChipId,
    /// The package variant, present when the response carried a recognised
    /// `package_sel`.
    pub package: Option<RpVariant>,
}

/// Read the RP2350 chip ID and package variant via the picoboot `GET_INFO`
/// (`SYS` / `CHIP_INFO`) command, served by both the running picobootx stack
/// and the stock bootrom.
///
/// Connects the supplied handle (if not already connected) and resets the
/// PICOBOOT interface before issuing the command, mirroring the crate's own
/// read/write paths: the stock bootrom can leave the bulk endpoint halted after
/// a prior operation, and without a reset the command write stalls.
///
/// The command args are the packed `pb_get_info_args_t`: `info_type = SYS`
/// (byte 0), three reserved bytes, then the `param0` flags word (`CHIP_INFO`)
/// at byte offset 4. `bCmdSize` is `0x10`. `dTransferLength` must be at least
/// the response size and a multiple of 4 — the stock bootrom STALLs the
/// endpoint if the buffer is too small to hold its reply — so we request 32
/// bytes, a little over the largest response we can receive.
///
/// The response is a self-describing word array: word 0 is the count of words
/// that follow. Since we request only `CHIP_INFO`, its three data words
/// (`package_sel`, `device_id_low`, `device_id_high`) are always the last
/// three of those words. Locating them relative to the count word handles both
/// layouts seen in the field:
///
/// - the stock RP2350 bootrom returns `[count=4, flags, package_sel, lo, hi]`;
/// - picobootx (running) currently returns `[count=3, package_sel, lo, hi]`,
///   omitting the returned-flags word (a picobootx bug; once fixed it will
///   return `count=4` like the bootrom, which this parser also accepts).
///
/// `package_sel` yields the package variant; an unrecognised value is warned
/// and returned as `None`, without failing the chip-ID read.
pub async fn read_chip_info(pb: &mut Picoboot) -> Result<ChipInfo, Error> {
    const PB_INFO_SYS: u8 = 0x01;
    const CHIP_INFO_FLAG: u32 = 0x0000_0001;
    const RESP_BYTES: u32 = 32;

    let conn = pb.connect().await.map_err(|e| Error::Usb(e.to_string()))?;
    conn.reset_interface()
        .await
        .map_err(|e| Error::Usb(e.to_string()))?;

    let mut args = [0u8; 16];
    args[0] = PB_INFO_SYS;
    args[4..8].copy_from_slice(&CHIP_INFO_FLAG.to_le_bytes());
    let cmd = PicobootCmd::new(PicobootCmdId::GetInfo, 0x10, RESP_BYTES, args);

    let resp = conn
        .send_cmd(cmd, None)
        .await
        .map_err(|e| Error::Usb(e.to_string()))?;

    let word = |i: usize| u32::from_le_bytes([resp[i], resp[i + 1], resp[i + 2], resp[i + 3]]);

    // Word 0 is the count of words that follow; the three CHIP_INFO data words
    // are the last of them, starting at word `count - 2`. Need the count word
    // plus at least those three data words.
    let count = if resp.len() >= 4 { word(0) as usize } else { 0 };
    if count < 3 || resp.len() < (count + 1) * 4 {
        return Err(Error::Usb(format!(
            "GET_INFO CHIP_INFO returned {} bytes with count {count}; too short",
            resp.len()
        )));
    }
    let data = (count - 2) * 4;
    let package_sel = word(data);
    let package = RpVariant::from_package_sel(package_sel);
    if package.is_none() {
        warn!("Unrecognised RP2350 package_sel {package_sel:#x} in CHIP_INFO");
    }
    Ok(ChipInfo {
        chip_id: Rp235xChipId::from_chip_info([package_sel, word(data + 4), word(data + 8)]),
        package,
    })
}

/// Read the first 64KB from flash on a One ROM Fire device.
///
/// Connects to the device via PICOBOOT, reads from the flash start address,
/// and returns the raw bytes. The caller is responsible for parsing the
/// contents.
pub async fn read_device_info(device: &mut Device) -> Result<(), Error> {
    debug!("Reading {FLASH_READ_SIZE_KB}KB from {FLASH_BASE:#010x} on {device}");

    // Parse the device's flash first, to establish its state and recognition.
    let picoboot = get_picoboot(device, false).await?;
    let onerom = {
        let mut reader = PicobootReader::new(picoboot).await.map_err(Error::Usb)?;
        let mut parser = Parser::with_base_flash_address(&mut reader, FLASH_BASE, RAM_BASE);
        parser.parse_device().await
    };
    device.update_onerom(onerom);

    // Read the chip ID - the device's invariant identity - and package variant.
    let (chip_id, rp_variant) = resolve_chip_id(device).await;
    device.chip_id = chip_id;
    device.rp_variant = rp_variant;

    Ok(())
}

/// Determine a device's RP2350 chip ID and, where available, its package
/// variant.
///
/// The chip ID and package are read directly via GET_INFO, which both the
/// running picobootx stack and the stock bootrom serve. GET_INFO is preferred
/// over the USB serial because a running device may present a serial-number
/// override, whereas the true chip ID never changes. If GET_INFO fails for any
/// reason, fall back to the serial, which is the chip ID in hex whenever the
/// device is not presenting an override (notably in the bootloader). The
/// package variant is only available from GET_INFO, so it is `None` on the
/// serial fallback path.
async fn resolve_chip_id(device: &Device) -> (Option<Rp235xChipId>, Option<RpVariant>) {
    match read_device_chip_info(device).await {
        Ok(info) => (Some(info.chip_id), info.package),
        Err(e) => {
            warn!("GET_INFO failed on {device}, falling back to serial: {e}");
            let chip_id = device
                .serial
                .as_deref()
                .and_then(Rp235xChipId::from_hex_serial);
            (chip_id, None)
        }
    }
}

/// Open a fresh picoboot handle to a discovered device and read its chip info.
async fn read_device_chip_info(device: &Device) -> Result<ChipInfo, Error> {
    let mut picoboot = get_picoboot(device, false).await?;
    read_chip_info(&mut picoboot).await
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
