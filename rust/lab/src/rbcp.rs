// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - querying a One ROM in the socket over RBCP
//!
//! The One ROM under test runs the host-control plugin. Lab is the RBCP host,
//! reading the ROM the One ROM serves.

use alloc::string::String;

use onerom_config::hw::Board;
use onerom_metadata::{OTP_STORE_VERSION, OneromOtpKey, metadata_key};
use onerom_rbcp_host::{AsciiField, BootSlot, Bus, Config, Error, ProtocolVersion, Session};

use crate::rom::RomReader;

/// How many times Lab polls the One ROM before it fails.
///
/// A command completed within 45 polls on fire-28-a.
const POLL_LIMIT: u32 = 1_000;

/// What a One ROM reports about itself.
pub struct Identity {
    pub device_type: AsciiField,
    pub device_version: AsciiField,
    pub protocol_version: ProtocolVersion,
    pub boot: BootSlot,
    /// `None` without host-control's Device Info commands.
    pub info: Option<DeviceInfo>,
}

pub struct DeviceInfo {
    /// The firmware's `HW_REV`.
    pub board_type: String,
    /// An `onerom_board_size_t`.
    pub board_size: u32,
    pub chip_id: [u8; 8],
    pub commissioning: Commissioning,
}

pub enum Commissioning {
    No,
    /// A commissioning data version this Lab can't read.
    UnknownVersion(u16),
    Yes(Values),
}

/// A missing value is `None`.
pub struct Values {
    pub board: Option<String>,
    pub manufacturer: Option<String>,
    pub date: Option<String>,
    pub signer: Option<u16>,
    pub signature: Option<[u8; 64]>,
}

/// The RBCP step that failed, and why.
pub type Failure = (&'static str, onerom_rbcp_host::Error);

struct LabBus<'a> {
    reader: &'a mut RomReader,
    mode: u8,
}

impl Bus for LabBus<'_> {
    fn read(&mut self, addr: u32) -> u8 {
        self.reader.read_byte_cycled(addr as usize, self.mode)
    }
}

/// Queries the One ROM in the socket, then puts back the bytes the query
/// changed in the image it serves.
///
/// `reader` must have had `begin_read(mode)` called.
pub fn query(reader: &mut RomReader, board: Board, mode: u8) -> Result<Identity, Failure> {
    let mut bus = LabBus { reader, mode };
    let mut session = Session::enter(&mut bus, config(board)).map_err(|e| ("enter", e))?;
    let device_type = session
        .get_device_type()
        .map_err(|e| ("GET_DEVICE_TYPE", e))?;
    let device_version = session
        .get_device_version()
        .map_err(|e| ("GET_DEVICE_VERSION", e))?;
    let boot = session
        .get_boot_slot_info()
        .map_err(|e| ("GET_BOOT_SLOT_INFO", e))?;
    let protocol_version = session.protocol_version();
    let info = device_info(&mut session)?;

    session.restore_and_exit().map_err(|e| ("restore", e))?;

    Ok(Identity {
        device_type,
        device_version,
        protocol_version,
        boot,
        info,
    })
}

/// `None` where GET_COMMISSIONING_VERSION fails.
fn device_info<B: Bus>(session: &mut Session<'_, B>) -> Result<Option<DeviceInfo>, Failure> {
    let Ok(version) = session.get_commissioning_version() else {
        return Ok(None);
    };
    let mut board_type = [0u8; 256];
    let len = session
        .get_metadata_str(metadata_key::HW_REV, &mut board_type)
        .map_err(|e| ("GET_METADATA_STR", e))?;
    let board_type = text(&board_type[..len.min(board_type.len())]);
    let board_size = session
        .get_metadata_uint(metadata_key::BOARD_SIZE)
        .map_err(|e| ("GET_METADATA_UINT", e))?;
    let mut chip_id = [0u8; 8];
    let len = session
        .get_metadata_array(metadata_key::CHIP_ID, &mut chip_id)
        .map_err(|e| ("GET_METADATA_ARRAY", e))?;
    if len != chip_id.len() {
        return Err(("GET_METADATA_ARRAY", Error::Failed));
    }
    let commissioning = match version {
        0 => Commissioning::No,
        OTP_STORE_VERSION => Commissioning::Yes(values(session)?),
        version => Commissioning::UnknownVersion(version),
    };
    Ok(Some(DeviceInfo {
        board_type,
        board_size,
        chip_id,
        commissioning,
    }))
}

fn values<B: Bus>(session: &mut Session<'_, B>) -> Result<Values, Failure> {
    let mut buf = [0u8; 256];
    let board = value(session, OneromOtpKey::OtpKeyCommissioningBoard, &mut buf)?.map(text);
    let manufacturer = value(
        session,
        OneromOtpKey::OtpKeyCommissioningManufacturer,
        &mut buf,
    )?
    .map(text);
    let date = value(session, OneromOtpKey::OtpKeyCommissioningDate, &mut buf)?.map(text);
    let signer = value(session, OneromOtpKey::OtpKeyCommissioningSigner, &mut buf)?
        .and_then(|v| Some(u16::from_le_bytes(v.try_into().ok()?)));
    let signature = value(session, OneromOtpKey::OtpKeyCommissioningSig, &mut buf)?
        .and_then(|v| v.try_into().ok());
    Ok(Values {
        board,
        manufacturer,
        date,
        signer,
        signature,
    })
}

fn value<'b, B: Bus>(
    session: &mut Session<'_, B>,
    key: OneromOtpKey,
    buf: &'b mut [u8],
) -> Result<Option<&'b [u8]>, Failure> {
    match session.get_commissioning_value(key as u16, buf) {
        Ok(len) => Ok(Some(&buf[..len.min(buf.len())])),
        // A missing key fails.
        Err(Error::Failed) => Ok(None),
        Err(e) => Err(("GET_COMMISSIONING_VALUE", e)),
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Command page 0, with the back-channel on the page above it.
///
/// On a 40-pin board the host-control plugin doesn't observe a ROM's lowest
/// address line - see plugins/user/host-control/README.md.
fn config(board: Board) -> Config {
    let unobserved = u8::from(board.chip_pins() == 40);
    Config {
        command_page: 0,
        bch_start: 0x100 << unobserved,
        bch_size: 32,
        unobserved,
        poll_limit: POLL_LIMIT,
    }
}
