// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - querying a One ROM in the socket over RBCP
//!
//! The One ROM under test runs the host-control plugin. Lab is the RBCP host,
//! reading the ROM the One ROM serves.

use onerom_config::hw::Board;
use onerom_rbcp_host::{AsciiField, BootSlot, Bus, Config, ProtocolVersion, Session};

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

    session.restore_and_exit().map_err(|e| ("restore", e))?;

    Ok(Identity {
        device_type,
        device_version,
        protocol_version,
        boot,
    })
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
