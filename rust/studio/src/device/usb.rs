// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Contains device's USB device handling

use dfu_rs::{DEFAULT_USB_TIMEOUT, Device as DfuDevice, DfuType, search_for_dfu};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use onerom_config::Model;
use picoboot::{Picoboot, Target};
use std::time::Duration;

use crate::app::AppMessage;
use crate::device::{Address, Client, Message};
use crate::hw::HardwareInfo;
use crate::internal_error;

pub const FIRE_VID: u16 = 0x1209;
pub const FIRE_BOOT_LOADER_PID: u16 = 0xf540;
pub const FIRE_RUN_PID: u16 = 0xf542;

// Studio can manage:
// - Stock RP2350 MCUs
// - One ROM's custom bootloader VID/PID 1209:f540
// - One ROM's application VID/PID 1209:f542
const FIRE_TARGETS: [Target; 3] = [
    Target::Rp2350,
    Target::Custom {
        vid: FIRE_VID,
        pid: FIRE_BOOT_LOADER_PID,
    },
    Target::Custom {
        vid: FIRE_VID,
        pid: FIRE_RUN_PID,
    },
];

const REBOOT_DELAY: Duration = Duration::from_millis(10);

/// Retrieve the list of connected USB devices.  Sends
/// Message::UsbDevicesDetected when done.
pub async fn get_usb_device_list_async() -> AppMessage {
    let ice_devices = get_ice_list_async().await;
    let fire_devices = get_fire_list_async().await;
    let mut usb_devices = Vec::new();
    if let Some(devices) = ice_devices {
        usb_devices.extend(devices);
    }
    if let Some(devices) = fire_devices {
        usb_devices.extend(devices);
    }
    Message::UsbDevicesDetected(usb_devices).into()
}

// Use dfu_rs::search_for_dfu to get Ice devices
async fn get_ice_list_async() -> Option<Vec<UsbDeviceType>> {
    match search_for_dfu(DEFAULT_USB_TIMEOUT, Some(DfuType::InternalFlash)).await {
        Ok(devices) => {
            // Turn into UsbDeviceType
            let devices = devices
                .into_iter()
                .filter_map(UsbDeviceType::from_dfu)
                .collect();
            Some(devices)
        }
        Err(e) => {
            warn!("Hit error attempting to detect Ice devices:\n  - {}", e);
            None
        }
    }
}

// Use picoboot::list_devices to get Fire devices
async fn get_fire_list_async() -> Option<Vec<UsbDeviceType>> {
    match Picoboot::list_devices(Some(&FIRE_TARGETS)).await {
        Ok(devices) => {
            let mut usb_devices = Vec::new();
            for d in devices {
                let p = Picoboot::new(d)
                    .await
                    .inspect_err(|e| {
                        warn!("Failed to create Picoboot device: {e}");
                    })
                    .ok();
                if let Some(p) = p {
                    usb_devices.push(UsbDeviceType::from_picoboot(p));
                }
            }
            Some(usb_devices)
        }
        Err(e) => {
            warn!("Hit error attempting to detect Fire devices:\n  - {}", e);
            None
        }
    }
}

/// Retrieve the list of connected USB devices after a delay.  Used to give
/// time for the OS to enumerate devices after a reset.
pub async fn get_usb_device_list_delay(duration: Duration) -> AppMessage {
    tokio::time::sleep(duration).await;
    get_usb_device_list_async().await
}

/// A USB device type
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum UsbDeviceType {
    /// An STM32 bootloader
    Ice(DfuDevice),
    /// An RP2350 bootloader
    Fire(Picoboot),
}

impl std::fmt::Display for UsbDeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsbDeviceType::Ice(d) => write!(f, "Ice USB ({})", d.info()),
            UsbDeviceType::Fire(p) => write!(f, "Fire USB ({})", p.info()),
        }
    }
}

impl UsbDeviceType {
    pub fn is_run_capable(&self) -> bool {
        matches!(
            self,
            UsbDeviceType::Fire(p) if p.vid() == FIRE_VID && p.pid() == FIRE_RUN_PID
        )
    }

    pub fn from_dfu(dfu_device: DfuDevice) -> Option<Self> {
        match (dfu_device.info().vid, dfu_device.info().pid) {
            (0x0483, 0xDF11) => Some(UsbDeviceType::Ice(dfu_device)),
            _ => None,
        }
    }

    pub fn from_picoboot(picoboot: Picoboot) -> Self {
        UsbDeviceType::Fire(picoboot)
    }

    pub fn vid(&self) -> u16 {
        match self {
            UsbDeviceType::Ice(d) => d.info().vid(),
            UsbDeviceType::Fire(p) => p.target().vid(),
        }
    }

    pub fn pid(&self) -> u16 {
        match self {
            UsbDeviceType::Ice(d) => d.info().pid(),
            UsbDeviceType::Fire(p) => p.target().pid(),
        }
    }

    pub fn model(&self) -> Model {
        match self {
            UsbDeviceType::Ice(_) => Model::Ice,
            UsbDeviceType::Fire(_) => Model::Fire,
        }
    }
}

/// Read memory from a device using USB DFU
pub async fn read_async(
    usb_device: UsbDeviceType,
    client: Client,
    _hw_info: HardwareInfo,
    address: Address,
    words: usize,
) -> AppMessage {
    let address = address.abs_from_usb_device(&usb_device);

    match usb_device {
        UsbDeviceType::Ice(d) => match d.upload(address, words * 4).await {
            Ok(data) => Message::DeviceData(client, data).into(),
            Err(e) => {
                let log = format!(
                    "Failed to read {words} words of memory at {address:#010X} from Ice USB ({}): {e}",
                    d.info(),
                );
                warn!("{log}");
                Message::ReadFailed(client, log).into()
            }
        },
        UsbDeviceType::Fire(mut p) => match p.read(address, (words * 4) as u32).await {
            Ok(data) => Message::DeviceData(client, data).into(),
            Err(e) => {
                let log = format!(
                    "Failed to read {words} words of memory at {address:#010X} from Fire USB ({}): {e}",
                    p.info(),
                );
                warn!("{log}");
                Message::ReadFailed(client, log).into()
            }
        },
    }
}

/// Flash firmware to a device using USB DFU
pub async fn flash_async(
    usb_device: UsbDeviceType,
    _hw_info: HardwareInfo,
    client: Client,
    data: Vec<u8>,
) -> AppMessage {
    match usb_device {
        UsbDeviceType::Ice(d) => flash_ice_async(d, client, data).await,
        UsbDeviceType::Fire(p) => flash_fire_async(p, client, data).await,
    }
}

async fn flash_ice_async(dfu_device: DfuDevice, client: Client, data: Vec<u8>) -> AppMessage {
    debug!("Erase One ROM USB");
    match dfu_device.mass_erase().await {
        Ok(()) => (),
        Err(e) => {
            let log = format!("Failed to mass erase Ice USB ({}): {e}", dfu_device.info());
            warn!("{log}");
            return Message::FlashFirmwareResult(client, Err(log)).into();
        }
    }
    debug!("Flash firmware to One ROM USB");
    match dfu_device.download(0x08000000, &data).await {
        Ok(()) => {
            debug!(
                "Successfully flashed firmware onto Ice USB ({})",
                dfu_device.info()
            );
            Message::FlashFirmwareResult(client, Ok(())).into()
        }
        Err(e) => {
            let log = format!(
                "Failed to flash firmware to Ice USB ({}): {e}",
                dfu_device.info()
            );
            warn!("{log}");
            Message::FlashFirmwareResult(client, Err(log)).into()
        }
    }
}

async fn flash_fire_async(mut picoboot: Picoboot, client: Client, data: Vec<u8>) -> AppMessage {
    debug!("Flash firmware to Fire USB");
    // Set a timeout to 10s in case a very large flash erase takes a very long time
    picoboot.set_timeouts(picoboot::usb::Timeouts {
        endpoint: Duration::from_secs(20),
        ..picoboot::usb::Timeouts::default()
    });
    match picoboot
        .flash_erase_and_write(picoboot.target().flash_start(), &data)
        .await
    {
        Ok(()) => {
            debug!(
                "Successfully flashed firmware onto Fire USB ({})",
                picoboot.info()
            );
            Message::FlashFirmwareResult(client, Ok(())).into()
        }
        Err(e) => {
            let log = format!(
                "Failed to flash firmware to Fire USB ({}): {e}",
                picoboot.info()
            );
            warn!("{log}");
            Message::FlashFirmwareResult(client, Err(log)).into()
        }
    }
}

pub async fn reboot_async(usb_device: UsbDeviceType, client: Client, stopped: bool) -> AppMessage {
    match usb_device {
        UsbDeviceType::Fire(mut p) => {
            let reboot_type = if stopped {
                picoboot::RebootType::Bootsel {
                    disable_msd: true,
                    disable_picoboot: false,
                }
            } else {
                picoboot::RebootType::Normal
            };
            match p.reboot(reboot_type, REBOOT_DELAY).await {
                Ok(()) => Message::RebootDeviceResult(client, Ok(())).into(),
                Err(e) => {
                    let log = format!("Failed to reboot Fire USB ({}): {e}", p.info());
                    warn!("{log}");
                    Message::RebootDeviceResult(client, Err(log)).into()
                }
            }
        }
        _ => {
            let log = "Attempted to reboot non-Fire device";
            internal_error!("{log}");
            Message::RebootDeviceResult(client, Err(log.into())).into()
        }
    }
}
