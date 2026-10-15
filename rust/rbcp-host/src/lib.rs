// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `onerom-rbcp-host` — the host side of the [ROM Bus Control
//! Protocol](https://github.com/piersfinlayson/rom-bus-control-protocol), for
//! talking to a One ROM running the host-control plugin.
//!
//! The host's only operation is a ROM read, provided by the caller through
//! [`Bus`]. Commands are sent as the addresses the host reads, and responses
//! are read back as ROM data from the back-channel region.
//!
//! [`Session::enter`] opens a command-response session. Each other method on
//! [`Session`] sends the RBCP command with the same name.
//!
//! ```ignore
//! let mut session = Session::enter(&mut bus, config)?;
//! let version = session.get_device_version()?;
//! let boot = session.get_boot_slot_info()?;
//! session.restore_and_exit()?;
//! ```
//!
//! # Addresses
//!
//! Every address passed to [`Bus::read`] is a byte offset within the ROM
//! image. A host serving a word-organised ROM as words reads byte N from word
//! N/2, taking D0–D7 for an even N and D8–D15 for an odd one.
//!
//! Command bytes are sent on the address lines the device observes. Where
//! the device doesn't observe the ROM's lowest address lines, consecutive
//! command bytes are further apart. [`Config::unobserved`] is that number of
//! lines, as the specification's "Address Line Presentation" describes.

#![no_std]

/// The response header is 8 bytes. The response data section follows it.
pub const HDR_SIZE: u32 = 8;

/// One ROM's knock. RBCP leaves the knock to the device, and the host uses the
/// same one.
pub const KNOCK: [u8; 6] = *b"!RBCP!";

/// The specification's recommended complete value. Pending is its inverse.
pub const DEFAULT_COMPLETE: u8 = 0xBB;

/// The specification's recommended status-OK value. Failed is its inverse.
pub const DEFAULT_STATUS_OK: u8 = 0xCC;

/// The oldest protocol version this host works with. GET_BOOT_SLOT_INFO and
/// EXIT_CMD_RESP_RESTORE arrived in 0.1.2.
const MIN_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion {
    major: 0,
    minor: 1,
    patch: 2,
};

const TOKEN_LSB: u32 = 2;
const PROGRESS: u32 = 4;
const RESPONSE: u32 = 5;

/// Length of the GET_DEVICE_TYPE and GET_DEVICE_VERSION responses.
const ASCII_FIELD_LEN: usize = 24;

/// Bytes at the start of the back-channel a session writes: the header and
/// the longest response the crate reads.
const WRITTEN_LEN: usize = HDR_SIZE as usize + ASCII_FIELD_LEN;

/// Value of a GET_BOOT_SLOT_INFO field where the device loaded no slot or
/// doesn't know which, and of GET_RAM_SLOT_INFO_ALL's active slot where no
/// slot is active.
const NO_SLOT: u8 = 0xFF;

/// A ROM bus the host reads.
pub trait Bus {
    /// Performs one complete ROM read at `addr` and returns the byte served.
    ///
    /// Every chip select is active during the read and inactive afterwards,
    /// or the device doesn't record it.
    fn read(&mut self, addr: u32) -> u8;
}

/// Where the session puts its command page and back-channel, and how long the
/// host waits for the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Value of the address bits above A7 on every command byte in
    /// command-response mode.
    pub command_page: u16,
    /// Start of the back-channel region, as a byte offset within the image.
    /// It must be 4-byte aligned and off the command page.
    pub bch_start: u32,
    /// Size of the back-channel region in bytes, header included.
    pub bch_size: u16,
    /// Low address lines the device doesn't observe.
    pub unobserved: u8,
    /// How many times the host polls the device before it fails. A poll is
    /// one read, or up to 32 while the back-channel is checked after a
    /// restore. The reset sequence's pauses are this many reads.
    pub poll_limit: u32,
}

/// An RBCP protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl ProtocolVersion {
    /// Whether a host written against `self` works with a device at `device`,
    /// by the specification's "Versioning and Compatibility" rules.
    fn accepts(self, device: ProtocolVersion) -> bool {
        if device.major != self.major {
            return false;
        }
        if self.major == 0 {
            device.minor == self.minor && device.patch >= self.patch
        } else {
            (device.minor, device.patch) >= (self.minor, self.patch)
        }
    }
}

impl core::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Why a command didn't succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The token didn't change. The device didn't receive the command or
    /// discarded it.
    NotReceived,
    /// The token changed but progress never reached complete.
    NeverCompleted,
    /// The device completed the command and reported failure.
    Failed,
    /// The device's protocol version isn't one this host works with.
    Version(ProtocolVersion),
    /// The back-channel didn't return to the bytes it held before the session.
    NotRestored,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotReceived => f.write_str("command not received"),
            Self::NeverCompleted => f.write_str("command received but never completed"),
            Self::Failed => f.write_str("device reported failure"),
            Self::Version(v) => write!(
                f,
                "device implements RBCP {v}, which this host doesn't support (minimum {MIN_PROTOCOL_VERSION})"
            ),
            Self::NotRestored => f.write_str("image not restored"),
        }
    }
}

/// A null-terminated ASCII response field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsciiField([u8; ASCII_FIELD_LEN]);

impl AsciiField {
    /// The bytes before the first null.
    pub fn as_bytes(&self) -> &[u8] {
        let len = self.0.iter().position(|&b| b == 0).unwrap_or(self.0.len());
        &self.0[..len]
    }
}

/// The GET_BOOT_SLOT_INFO response. A field is `None` where the device loaded
/// no slot at boot or doesn't know which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootSlot {
    pub flash_slot: Option<u8>,
    pub ram_slot: Option<u8>,
}

/// A command-response session.
///
/// A session dropped without [`Session::restore_and_exit`], as on a failure,
/// restores the back-channel the same way.
pub struct Session<'b, B: Bus> {
    bus: &'b mut B,
    config: Config,
    complete: u8,
    status_ok: u8,
    protocol_version: ProtocolVersion,
    /// The start of the back-channel as it was before the session.
    original: [u8; WRITTEN_LEN],
    /// Set once the session has ended, so dropping it sends nothing.
    ended: bool,
}

impl<'b, B: Bus> Session<'b, B> {
    /// Resets the device's RBCP state, enters command-response mode and reads
    /// the device's protocol version.
    ///
    /// Where the version check fails, the device leaves command-response mode
    /// and the back-channel isn't restored.
    pub fn enter(bus: &'b mut B, config: Config) -> Result<Self, Error> {
        let mut session = Session {
            bus,
            config,
            complete: DEFAULT_COMPLETE,
            status_ok: DEFAULT_STATUS_OK,
            protocol_version: MIN_PROTOCOL_VERSION,
            original: [0; WRITTEN_LEN],
            ended: false,
        };
        session.reset();

        for i in 0..session.original_len() {
            session.original[i] = session.bus.read(config.bch_start + i as u32);
        }

        // The device reports progress and response by writing these two
        // values, so each has to differ from what the image already holds
        // there.
        session.complete = unlike(session.read_hdr(PROGRESS), DEFAULT_COMPLETE);
        session.status_ok = unlike(session.read_hdr(RESPONSE), DEFAULT_STATUS_OK);

        let page = config.command_page.to_le_bytes();
        let start = config.bch_start.to_le_bytes();
        let size = config.bch_size.to_le_bytes();
        let args = [
            page[0],
            page[1],
            start[0],
            start[1],
            start[2],
            size[0],
            size[1],
            session.complete,
            session.status_ok,
        ];
        session.issue(true, group::CONTROL, control::ENTER_CMD_RESP, &args)?;

        session.issue(false, group::READ, read::GET_PROTOCOL_VERSION, &[])?;
        let mut v = [0u8; 3];
        session.read_data(&mut v);
        session.protocol_version = ProtocolVersion {
            major: v[0],
            minor: v[1],
            patch: v[2],
        };
        if !MIN_PROTOCOL_VERSION.accepts(session.protocol_version) {
            // The restore uses commands the device may not have, so the device
            // is only taken out of command-response mode.
            session.ended = true;
            session.reset();
            return Err(Error::Version(session.protocol_version));
        }
        Ok(session)
    }

    /// The protocol version the device reported when the session was entered.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub fn get_device_type(&mut self) -> Result<AsciiField, Error> {
        self.ascii_field(read::GET_DEVICE_TYPE)
    }

    pub fn get_device_version(&mut self) -> Result<AsciiField, Error> {
        self.ascii_field(read::GET_DEVICE_VERSION)
    }

    pub fn get_boot_slot_info(&mut self) -> Result<BootSlot, Error> {
        self.issue(false, group::READ, read::GET_BOOT_SLOT_INFO, &[])?;
        let mut r = [0u8; 2];
        self.read_data(&mut r);
        let slot = |s| (s != NO_SLOT).then_some(s);
        Ok(BootSlot {
            flash_slot: slot(r[0]),
            ram_slot: slot(r[1]),
        })
    }

    /// Puts back the back-channel bytes the session changed, ends the
    /// session and checks the bytes read back as they did before it.
    pub fn restore_and_exit(mut self) -> Result<(), Error> {
        self.ended = true;
        self.restore()
    }

    /// The specification's way of putting back the back-channel: SLOT_POKE for
    /// the bytes past the header, then EXIT_CMD_RESP_RESTORE for the header.
    fn restore(&mut self) -> Result<(), Error> {
        self.issue(false, group::READ, read::GET_RAM_SLOT_INFO_ALL, &[])?;
        let mut info = [0u8; 2];
        self.read_data(&mut info);
        let slot = info[1];
        if slot == NO_SLOT {
            return Err(Error::NotRestored);
        }

        let hdr = HDR_SIZE as usize;
        for i in hdr..self.original_len() {
            let addr = self.config.bch_start + i as u32;
            if self.bus.read(addr) != self.original[i] {
                let a = addr.to_le_bytes();
                let args = [self.original[i], a[0], a[1], a[2], slot];
                self.issue(false, group::MODIFY, modify::SLOT_POKE, &args)?;
            }
        }

        let count = hdr.min(self.original_len());
        let mut args = [0u8; 9];
        args[..count].copy_from_slice(&self.original[..count]);
        args[8] = count as u8;
        self.send(group::CONTROL, control::EXIT_CMD_RESP_RESTORE, &args);

        for _ in 0..self.config.poll_limit {
            if (0..self.original_len())
                .all(|i| self.bus.read(self.config.bch_start + i as u32) == self.original[i])
            {
                return Ok(());
            }
        }
        Err(Error::NotRestored)
    }

    /// How many bytes of `original` are saved: no more than the back-channel
    /// holds.
    fn original_len(&self) -> usize {
        WRITTEN_LEN.min(usize::from(self.config.bch_size))
    }

    fn ascii_field(&mut self, cmd: u8) -> Result<AsciiField, Error> {
        self.issue(false, group::READ, cmd, &[])?;
        let mut field = [0u8; ASCII_FIELD_LEN];
        self.read_data(&mut field);
        Ok(AsciiField(field))
    }

    /// The specification's "Host Polling Sequence", with the knock in front
    /// where `knock` is set.
    fn issue(&mut self, knock: bool, group: u8, cmd: u8, args: &[u8]) -> Result<(), Error> {
        let before = self.read_hdr(TOKEN_LSB);
        if knock {
            for b in KNOCK {
                self.send_byte(b);
            }
        }
        self.send(group, cmd, args);

        if !self.poll(TOKEN_LSB, |t| t != before) {
            return Err(Error::NotReceived);
        }
        let complete = self.complete;
        if !self.poll(PROGRESS, |p| p == complete) {
            return Err(Error::NeverCompleted);
        }
        if self.read_hdr(RESPONSE) != self.status_ok {
            return Err(Error::Failed);
        }
        Ok(())
    }

    /// The specification's recommended reset sequence, from "Communication
    /// Initiation - Resetting the Device".
    fn reset(&mut self) {
        for _ in 0..5 {
            self.send_reset();
        }
        self.pause();
        self.send_reset();
        self.pause();
        for b in KNOCK {
            self.send_byte(b);
        }
        self.send_reset();
        self.pause();
    }

    fn send_reset(&mut self) {
        self.send_byte(group::RESET);
        self.send_byte(reset::RBCP_RESET);
    }

    /// Reads the progress byte, which is off the command page and so isn't a
    /// command byte. Repeated, it doesn't form a knock.
    fn pause(&mut self) {
        for _ in 0..self.config.poll_limit {
            self.read_hdr(PROGRESS);
        }
    }

    fn poll(&mut self, offset: u32, mut done: impl FnMut(u8) -> bool) -> bool {
        (0..self.config.poll_limit).any(|_| done(self.read_hdr(offset)))
    }

    fn send(&mut self, group: u8, cmd: u8, args: &[u8]) {
        self.send_byte(group);
        self.send_byte(cmd);
        for &a in args {
            self.send_byte(a);
        }
    }

    fn send_byte(&mut self, byte: u8) {
        let observed = (u32::from(self.config.command_page) << 8) | u32::from(byte);
        self.bus.read(observed << self.config.unobserved);
    }

    fn read_hdr(&mut self, offset: u32) -> u8 {
        self.bus.read(self.config.bch_start + offset)
    }

    fn read_data(&mut self, out: &mut [u8]) {
        let base = self.config.bch_start + HDR_SIZE;
        for (addr, b) in (base..).zip(out.iter_mut()) {
            *b = self.bus.read(addr);
        }
    }
}

impl<B: Bus> Drop for Session<'_, B> {
    fn drop(&mut self) {
        if !self.ended {
            let _ = self.restore();
        }
    }
}

/// `default`, or its inverse where `current` already holds `default`.
fn unlike(current: u8, default: u8) -> u8 {
    if current == default {
        !default
    } else {
        default
    }
}

// The protocol's command groups and commands, from the specification's
// "Command Reference".

pub mod group {
    pub const CONTROL: u8 = 0x00;
    pub const READ: u8 = 0x01;
    pub const MODIFY: u8 = 0x02;
    pub const NV_STORAGE: u8 = 0x03;
    pub const PIPES: u8 = 0x04;
    pub const AUX: u8 = 0x05;
    pub const LED: u8 = 0x06;
    pub const RESET: u8 = 0xAA;
}

pub mod control {
    pub const NOP: u8 = 0x00;
    pub const ENTER_CMD_RESP: u8 = 0x01;
    pub const EXIT_CMD_RESP_ACK: u8 = 0x02;
    pub const EXIT_CMD_RESP_SILENT: u8 = 0x03;
    pub const SWITCH_AND_EXIT: u8 = 0x04;
    pub const LOAD_AND_EXIT: u8 = 0x05;
    pub const EXIT_CMD_RESP_RESTORE: u8 = 0x06;
    pub const REBOOT: u8 = 0x07;
}

pub mod read {
    pub const GET_FLASH_SLOT_COUNT: u8 = 0x00;
    pub const GET_FLASH_SLOT_INFO: u8 = 0x01;
    pub const GET_FLASH_SLOT_INFO_ALL: u8 = 0x02;
    pub const GET_RAM_SLOT_INFO_ALL: u8 = 0x03;
    pub const GET_DEVICE_TYPE: u8 = 0x04;
    pub const GET_DEVICE_VERSION: u8 = 0x05;
    pub const GET_PROTOCOL_VERSION: u8 = 0x06;
    pub const SLOT_PEEK: u8 = 0x07;
    pub const GET_BOOT_SLOT_INFO: u8 = 0x08;
}

pub mod modify {
    pub const SLOT_POKE: u8 = 0x00;
    pub const SWITCH_SLOT: u8 = 0x01;
    pub const LOAD_SLOT: u8 = 0x02;
    pub const SLOT_POKE_ALL_BYTE: u8 = 0x03;
}

pub mod nv {
    pub const GET_NV_CAPABILITY: u8 = 0x00;
    pub const NV_PEEK: u8 = 0x01;
    pub const NV_POKE_BEGIN: u8 = 0x02;
    pub const NV_POKE: u8 = 0x03;
    pub const NV_POKE_COMMIT: u8 = 0x04;
    pub const NV_POKE_DISCARD: u8 = 0x05;
    pub const NV_POKE_COMMIT_BYTE: u8 = 0x06;
}

pub mod pipes {
    pub const GET_PIPE_CAPABILITY: u8 = 0x00;
    pub const GET_PIPE_INFO: u8 = 0x01;
    pub const PIPE_WRITE: u8 = 0x02;
    pub const PIPE_READ: u8 = 0x03;
}

pub mod aux {
    pub const GET_AUX_CAPABILITY: u8 = 0x00;
    pub const GET_AUX_GROUP_INFO: u8 = 0x01;
    pub const GET_AUX_PIN_INFO: u8 = 0x02;
    pub const SET_AUX: u8 = 0x03;
    pub const SET_AUX_AND_EXIT: u8 = 0x04;
    pub const SET_AUX_SWITCH_EXIT: u8 = 0x05;
}

pub mod led {
    pub const GET_LED_CAPABILITY: u8 = 0x00;
    pub const GET_LED_INFO: u8 = 0x01;
    pub const GET_LED_MODE_INFO: u8 = 0x02;
    pub const SET_LED: u8 = 0x03;
}

pub mod reset {
    pub const RBCP_RESET: u8 = 0xAA;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_rules() {
        let v = |major, minor, patch| ProtocolVersion {
            major,
            minor,
            patch,
        };
        assert!(MIN_PROTOCOL_VERSION.accepts(v(0, 1, 2)));
        assert!(MIN_PROTOCOL_VERSION.accepts(v(0, 1, 3)));
        assert!(!MIN_PROTOCOL_VERSION.accepts(v(0, 1, 1)));
        assert!(!MIN_PROTOCOL_VERSION.accepts(v(0, 2, 0)));
        assert!(!MIN_PROTOCOL_VERSION.accepts(v(1, 1, 2)));
        assert!(v(1, 2, 0).accepts(v(1, 3, 0)));
        assert!(!v(1, 2, 3).accepts(v(1, 2, 2)));
    }

    #[test]
    fn unlike_avoids_current() {
        assert_eq!(unlike(0x00, DEFAULT_COMPLETE), DEFAULT_COMPLETE);
        assert_eq!(unlike(DEFAULT_COMPLETE, DEFAULT_COMPLETE), 0x44);
        assert_eq!(unlike(DEFAULT_STATUS_OK, DEFAULT_STATUS_OK), 0x33);
    }
}
