// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Contains device's probe handling
//!
//! Uses `probe-rs`.

use airfrog_rpc::io::Reader;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use onerom_app::{FlashPlan, FlashStep, OtpAccess, OtpError};
use onerom_cli::device::{board_size, flash_chips};
use onerom_cli::error::plan_error;
use onerom_cli::usb::{FLASH_BASE, RAM_BASE};
use onerom_config::hw::Board;
use onerom_fw_parser::Parser;
use onerom_metadata::{MaybeKnown, OTP_DATA_BASE, OTP_DATA_RAW_BASE, OneromBoardSize};
use probe_rs::flashing::{DownloadOptions, FlashError, FlashProgress, erase};
use probe_rs::probe::list::Lister;
use probe_rs::probe::{DebugProbeInfo, WireProtocol};
use probe_rs::{Core, Error as ProbeError, MemoryInterface, Permissions, Session};
use std::time::Duration;
use tokio::task::spawn_blocking;

use crate::app::AppMessage;
use crate::device::{
    Address, BoardDetails, Client, Message, UNKNOWN_FLASH_STEP, check_commissioned_board,
};
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
    let fire = hw_info.is_fire();
    let result = spawn_blocking(move || {
        if fire {
            probe_flash_plan(probe.inner().clone(), chip_id, hw_info.board, &data)
        } else {
            probe_flash(probe.inner().clone(), chip_id, address, &data)
                .map_err(|e| format!("Failed to flash firmware: {e}"))
        }
    })
    .await;

    match result {
        Ok(Ok(())) => Message::FlashFirmwareResult(client, Ok(())).into(),
        Ok(Err(log)) => {
            warn!("{log}");
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

/// Read the board size and commissioned board of a Fire device using a probe.
/// Sends Message::BoardDetailsRead when done.  The details are empty for an
/// Ice device.
pub async fn read_board_details_async(
    probe: ProbeType,
    client: Client,
    hw_info: HardwareInfo,
) -> AppMessage {
    let Some(mcu) = hw_info.mcu_variant.filter(|_| hw_info.is_fire()) else {
        return Message::BoardDetailsRead(client, BoardDetails::default()).into();
    };
    let chip_id = mcu.chip_id().to_string();

    let result = spawn_blocking(move || {
        let mut session = open_session(probe.inner().clone(), chip_id)?;
        Ok::<_, ProbeError>(BoardDetails {
            size: read_board_size(&mut session),
            commissioned: read_commissioned_board(&mut session),
        })
    })
    .await;

    let details = match result {
        Ok(Ok(details)) => details,
        Ok(Err(e)) => {
            warn!("Failed to read the board details: {e}");
            BoardDetails::default()
        }
        Err(e) => {
            warn!("Failed to read the board details: {e}");
            BoardDetails::default()
        }
    };
    Message::BoardDetailsRead(client, details).into()
}

// Opens a probe and attaches to the chip
fn open_session(probe: DebugProbeInfo, chip_id: String) -> Result<Session, ProbeError> {
    let mut probe = probe.open()?;
    trace!("Select SWD Protocol");
    probe.select_protocol(WireProtocol::Swd)?;
    trace!("Attach to chip {chip_id}");
    probe.attach(chip_id, Permissions::default())
}

/// The board size of the One ROM on `session` by onerom-cli's rule.  It reads
/// runtime info and OTP through the probe.
fn read_board_size(session: &mut Session) -> Option<MaybeKnown<OneromBoardSize>> {
    futures::executor::block_on(async {
        let parsed = {
            let mut reader = ProbeReader { session };
            Parser::with_base_flash_address(&mut reader, FLASH_BASE, RAM_BASE)
                .parse_device()
                .await
        };
        let onerom = Some(&parsed).filter(|parsed| parsed.is_recognised());
        board_size(onerom, async || {
            let mut otp = ProbeOtp { session };
            onerom_app::read_board_size(&mut otp)
                .await
                .inspect_err(|e| debug!("Couldn't read the board size from OTP: {e}"))
                .ok()
        })
        .await
    })
}

/// The board the current commissioning instance of the One ROM on `session`
/// identifies.  OTP is read through the probe and a failed read is logged.
fn read_commissioned_board(session: &mut Session) -> Option<String> {
    let mut otp = ProbeOtp { session };
    match futures::executor::block_on(onerom_app::read_commissioning(&mut otp)) {
        Ok(area) => area.current()?.board().map(str::to_string),
        Err(e) => {
            warn!("Failed to read the commissioning area: {e}");
            None
        }
    }
}

/// Flashes `data` to a Fire device by the plan for its flash chips.
/// `image_board` is the board `data` is for.  Returns the text to display where
/// it fails.
fn probe_flash_plan(
    probe: DebugProbeInfo,
    chip_id: String,
    image_board: Option<Board>,
    data: &[u8],
) -> Result<(), String> {
    let failed = |e: &dyn std::fmt::Display| format!("Failed to flash firmware: {e}");

    let mut session = open_session(probe, chip_id).map_err(|e| failed(&e))?;

    // An image for another board fails to flash to a commissioned board.
    // Where the commissioning can't be read the flash goes ahead.
    let commissioned = read_commissioned_board(&mut session);
    check_commissioned_board(commissioned.as_deref(), image_board)?;

    let size = read_board_size(&mut session);
    let plan = FlashPlan::new(data, &flash_chips(size))
        .map_err(|e| plan_error(e, data.len(), size).to_string())?;

    // A step this build doesn't know fails before anything is erased.
    #[allow(clippy::wildcard_enum_match_arm)]
    if plan
        .steps()
        .iter()
        .any(|step| !matches!(step, FlashStep::Erase { .. } | FlashStep::Write { .. }))
    {
        return Err(UNKNOWN_FLASH_STEP.to_string());
    }

    for step in plan.steps() {
        match *step {
            FlashStep::Erase { addr, len } => {
                debug!("Erasing {len} bytes at {addr:#010x}");
                let (start, end) = (u64::from(addr), u64::from(addr) + u64::from(len));
                erase(&mut session, &mut FlashProgress::empty(), start, end, false)
                    .map_err(|e| failed(&e))?;
            }
            FlashStep::Write { addr, data } => {
                debug!("Writing {} bytes to {addr:#010x}", data.len());
                let mut loader = session.target().flash_loader();
                loader
                    .add_data(u64::from(addr), data)
                    .map_err(|e| failed(&e))?;
                // The plan erased the flash already.
                let mut options = DownloadOptions::default();
                options.skip_erase = true;
                loader
                    .commit(&mut session, options)
                    .map_err(|e| failed(&e))?;
            }
            _ => return Err(UNKNOWN_FLASH_STEP.to_string()),
        }
    }
    Ok(())
}

/// Reads a device's memory through a probe for the firmware parser.
struct ProbeReader<'a> {
    session: &'a mut Session,
}

impl Reader for ProbeReader<'_> {
    type Error = String;

    async fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), Self::Error> {
        let mut core = self.session.core(0).map_err(|e| e.to_string())?;
        core.read_8(u64::from(addr), buf).map_err(|e| e.to_string())
    }

    fn update_base_address(&mut self, _new_base: u32) {
        // A probe reads absolute addresses.
    }
}

/// Reads a device's OTP through a probe from the OTP read aliases.
struct ProbeOtp<'a> {
    session: &'a mut Session,
}

impl ProbeOtp<'_> {
    fn read_word(&mut self, addr: u32) -> Result<u32, OtpError> {
        let transport = |e: ProbeError| OtpError::Transport(e.to_string());
        let mut core = self.session.core(0).map_err(transport)?;
        core.read_word_32(u64::from(addr)).map_err(transport)
    }
}

impl OtpAccess for ProbeOtp<'_> {
    async fn read_ecc(&mut self, row: u16, count: u16) -> Result<Vec<u16>, OtpError> {
        (u32::from(row)..u32::from(row) + u32::from(count))
            .map(|row| {
                // Two rows to each word, the even row in the low half.
                let word = self.read_word(OTP_DATA_BASE + 4 * (row >> 1))?;
                Ok(if row & 1 == 1 { word >> 16 } else { word } as u16)
            })
            .collect()
    }

    async fn read_raw(&mut self, row: u16, count: u16) -> Result<Vec<u32>, OtpError> {
        (u32::from(row)..u32::from(row) + u32::from(count))
            .map(|row| {
                // One row to each word in bits 23:0.
                Ok(self.read_word(OTP_DATA_RAW_BASE + 4 * row)? & 0x00FF_FFFF)
            })
            .collect()
    }

    async fn write_ecc(&mut self, _row: u16, _value: u16) -> Result<(), OtpError> {
        Err(OtpError::Transport(OTP_READ_ONLY.to_string()))
    }

    async fn write_raw(&mut self, _row: u16, _value: u32) -> Result<(), OtpError> {
        Err(OtpError::Transport(OTP_READ_ONLY.to_string()))
    }
}

// Studio only reads OTP through a probe.
const OTP_READ_ONLY: &str = "Studio doesn't write OTP through a probe";
