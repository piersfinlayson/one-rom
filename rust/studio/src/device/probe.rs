// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Contains device's probe handling
//!
//! Uses `probe-rs`.

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use probe_rs::flashing::FlashError;
use probe_rs::probe::list::Lister;
use probe_rs::probe::{DebugProbeInfo, WireProtocol};
use probe_rs::{Core, Error as ProbeError, MemoryInterface, Permissions};
use std::time::Duration;
use tokio::task::spawn_blocking;

use crate::app::AppMessage;
use crate::device::{Address, Client, Message};
use crate::hw::HardwareInfo;

// Time to wait for core halt operations
const PROBE_CORE_HALT_TIMEOUT: Duration = Duration::from_millis(100);

/// Retrieve the list of connected debug probes.  Sends
/// Message::ProbesDetected when done.
pub async fn get_probe_list_async() -> AppMessage {
    let probes = Lister::new().list_all();
    if !probes.is_empty() {
        let probes: Vec<ProbeType> = probes.into_iter().map(Into::into).collect();
        Message::ProbesDetected(probes).into()
    } else {
        Message::ProbesDetected(Vec::new()).into()
    }
}

/// Wrapper object for DebugProbeInfo.  We use this in objects like pick lists
/// so we have control over how they display
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeType(DebugProbeInfo);

impl std::fmt::Display for ProbeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Your custom display logic here
        write!(
            f,
            "{} ({:04X}:{:04X})",
            self.0.probe_type(),
            self.0.vendor_id,
            self.0.product_id
        )
    }
}

impl From<DebugProbeInfo> for ProbeType {
    fn from(info: DebugProbeInfo) -> Self {
        ProbeType(info)
    }
}

impl ProbeType {
    fn inner(&self) -> &DebugProbeInfo {
        &self.0
    }

    pub fn identifier(&self) -> &str {
        &self.0.identifier
    }

    pub fn serial_number(&self) -> Option<&str> {
        self.0.serial_number.as_deref()
    }
}

/// Read memory from a device using a probe
pub async fn read_async(
    probe: ProbeType,
    client: Client,
    hw_info: HardwareInfo,
    address: Address,
    words: usize,
) -> AppMessage {
    // Get the chip ID
    let chip_id = match hw_info.mcu_variant {
        None => "STM32F411RETx".to_string(),
        Some(mcu) => mcu.chip_id().to_string(),
    };

    // Get the absolute address
    let address = if let Some(address) = address.abs_from_hw_info(&hw_info) {
        address
    } else {
        let log =
            format!("Failed to resolve address for reading {words} words of memory at {address}");
        warn!("{log}");
        return Message::ReadFailed(client, log).into();
    };

    let result = spawn_blocking(move || {
        probe_init_and_operate_on_core(probe.inner().clone(), chip_id, true, |core| {
            let mut buf = vec![0u32; words];
            core.read_32(address as u64, &mut buf)?;
            let bytes: Vec<u8> = buf.iter().flat_map(|w| w.to_le_bytes()).collect();
            Ok(bytes)
        })
    })
    .await;

    match result {
        Ok(Ok(bytes)) => Message::DeviceData(client, bytes).into(),
        Ok(Err(e)) => {
            let log = format!("Failed to read {words} words of memory at {address:#010X}: {e}");
            warn!("{log}");
            debug!("Precise error: {e:?}");
            Message::ReadFailed(client, log).into()
        }
        Err(e) => {
            let log = format!("Failed to read {words} words of memory at {address:#010X}: {e}");
            warn!("{log}");
            debug!("Precise error: {e:?}");
            Message::ReadFailed(client, log).into()
        }
    }
}

/// Flash firmware to a device using a probe
pub async fn flash_async(
    probe: ProbeType,
    hw_info: HardwareInfo,
    client: Client,
    data: Vec<u8>,
) -> AppMessage {
    let chip_id = match hw_info.mcu_variant {
        None => "STM32F411RETx".to_string(),
        Some(mcu) => mcu.chip_id().to_string(),
    };
    let address = match hw_info.mcu_variant {
        None => 0x08000000,
        Some(mcu) => mcu.family().get_flash_base(),
    };
    let result =
        spawn_blocking(move || probe_flash(probe.inner().clone(), chip_id, address, &data)).await;

    match result {
        Ok(Ok(())) => Message::FlashFirmwareResult(client, Ok(())).into(),
        Ok(Err(e)) => {
            let log = format!("Failed to flash firmware: {e}");
            warn!("{log}");
            debug!("Precise error: {e:?}");
            Message::FlashFirmwareResult(client, Err(log)).into()
        }
        Err(e) => {
            let log = format!("Failed to flash firmware: {e}");
            error!("{log}");
            debug!("Precise error: {e:?}");
            Message::FlashFirmwareResult(client, Err(log)).into()
        }
    }
}

// Helper to open a probe, attach to a chip, halt core, and run a closure
fn probe_init_and_operate_on_core<F, R>(
    probe: DebugProbeInfo,
    chip_id: String,
    halt_core: bool,
    f: F,
) -> Result<R, ProbeError>
where
    F: FnOnce(&mut Core) -> Result<R, ProbeError>,
{
    // Open the probe and reset the device
    let mut probe = probe.open()?;
    let probe_name = probe.get_name();
    trace!("Select SWD Protocol");
    probe.select_protocol(WireProtocol::Swd)?;

    // Attach to the target chip
    trace!("Attach to chip {}", chip_id);
    let mut session = probe.attach(chip_id, Permissions::default())?;
    trace!("Get core");
    let mut core = session.core(0)?;

    if halt_core {
        debug!("Halting core using probe {}", probe_name);
        core.halt(PROBE_CORE_HALT_TIMEOUT)?;
    }

    f(&mut core)
}

// Helper to open a probe and session, and run a closure
#[allow(clippy::wildcard_enum_match_arm)]
fn probe_flash(
    probe: DebugProbeInfo,
    chip_id: String,
    load_address: u32,
    data: &[u8],
) -> Result<(), String> {
    let mut probe = probe.open().map_err(|e| e.to_string())?;
    let probe_name = probe.get_name();
    debug!("Flashing firmware using probe {}", probe_name);

    // Initialize the probe and session
    trace!("Select SWD Protocol");
    probe
        .select_protocol(WireProtocol::Swd)
        .map_err(|e| e.to_string())?;

    trace!("Attach to chip {chip_id}");
    let mut session = probe
        .attach(chip_id, Permissions::default())
        .map_err(|e| e.to_string())?;

    trace!("Create flash loader");
    let mut loader = session.target().flash_loader();
    trace!("Add data to flash loader at address {load_address:#X}");
    loader
        .add_data(load_address as u64, data)
        .map_err(|e| e.to_string())?;

    trace!("Commit flash loader");
    match loader.commit(&mut session, probe_rs::flashing::DownloadOptions::default()) {
        Ok(()) => Ok(()),
        Err(e) => {
            match &e {
                FlashError::ResetAndHalt(e) => debug!("FlashError::ResetAndHalt: {e:?}"),
                _ => debug!("FlashError::Unknown: {e:?}"),
            }
            Err(e.to_string())
        }
    }
}
