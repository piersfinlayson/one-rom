//! One ROM Protocol - One ROM Lab support
//!
//! Used by both the One ROM Lab firmware and host tools to communicate.
//!
//! Uses `airfrog-rpc` for the underlying RPC transport.
//!
//! The host can retrieve the RAM metada using `onerom-fw-parser` which provides
//! the RAM channel addresses required for RPC communication.
//!
//! See `airfrog::firmware::onerom_lab` for example host usage.

// Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_database::{RomEntry, RomType};

use crate::Error;

/// Commands supported by One ROM Lab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Command {
    /// Response with a Pong.  No data follows.
    Ping = 0x0000_0000,

    /// Trigger a read of the connected ROM.  No data follows.
    ReadRom = 0x0000_0001,

    /// Trigger a read of a set of raw data associated with the last ROM read.
    /// Data follows - see [`GetRawData`].
    GetRawData = 0x0000_0002,

    /// Unknown command, do not use.  No data follows.
    Unknown = 0xFFFF_FFFF,
}

impl From<u32> for Command {
    fn from(value: u32) -> Self {
        match value {
            0x0000_0000 => Command::Ping,
            0x0000_0001 => Command::ReadRom,

            _ => Command::Unknown,
        }
    }
}

impl From<Command> for u32 {
    fn from(cmd: Command) -> Self {
        cmd as u32
    }
}

impl Command {
    pub fn size() -> usize {
        core::mem::size_of::<Self>()
    }

    pub fn as_bytes(&self) -> [u8; 4] {
        (*self as u32).to_le_bytes()
    }
}

/// Responses from One ROM Lab to Commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Response {
    /// Ping response
    Pong = 0x0000_0000,

    /// ReadRom successful response.  Following this word, is the ROM metadata
    /// as a sequence of bytes.  See [`LabRomEntry`].
    /// - Name of the ROM, followed by 0
    /// - Part number of the ROM, followed by 0
    /// - 32-bit wrapping checksum of the ROM, little endian encoded
    /// - 20 byte SHA1 digest of the ROM
    RomEntry = 0x0000_0001,

    /// ROM (probably) connected but not recognised.  No data follows
    RomNotRecognised = 0x0000_0002,

    /// One ROM Lab hit an error
    Error = 0x8000_0000,

    /// One ROM Lab did not detect a ROM connected, but it may have been
    /// unrecognised
    NoRom = 0x8000_0001,

    Unknown = 0xFFFF_FFFF,
}

impl Response {
    pub const fn size() -> usize {
        core::mem::size_of::<Self>()
    }

    pub fn to_bytes(&self, buf: &mut [u8]) {
        let value = *self as u32;
        buf[..4].copy_from_slice(&value.to_le_bytes());
    }
}

impl From<u32> for Response {
    fn from(value: u32) -> Self {
        match value {
            0x0000_0000 => Response::Pong,
            0x0000_0001 => Response::RomEntry,
            0x0000_0002 => Response::RomNotRecognised,
            0x8000_0000 => Response::Error,
            0x8000_0001 => Response::NoRom,
            _ => Response::Unknown,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct GetRawData {
    /// The type of ROM to get information for
    rom_type: RomType,
}

impl GetRawData {
    const fn binary_size() -> usize {
        RomType::binary_size()
    }

    pub fn from_buffer(buf: &[u8]) -> Result<Self, Error> {
        // Get RomType
        let rom_type_size = Self::binary_size();
        if buf.len() < rom_type_size {
            return Err(Error::BufferTooSmall);
        }
        let rom_type = RomType::from_bytes(&buf[0..rom_type_size])?;

        Ok(Self { rom_type })
    }

    pub fn to_buffer(&self) -> Result<Vec<u8>, Error> {
        let mut pos = 0;

        let size = Self::binary_size() + Command::size();
        let mut buf = vec![0; size];

        // Write Response code
        if size < Command::size() {
            return Err(Error::BufferTooSmall);
        }
        let rsp_u32 = Command::GetRawData as u32;
        buf[pos..pos + 4].copy_from_slice(&rsp_u32.to_le_bytes());
        pos += 4;

        // Write RomType
        let rom_type_size = RomType::binary_size();
        if size < pos + rom_type_size {
            return Err(Error::BufferTooSmall);
        }
        self.rom_type.to_bytes(&mut buf[pos..pos + rom_type_size])?;

        Ok(buf)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LabRomType {
    #[serde(rename = "ROM Type")]
    rom_type: RomType,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct RomRawData {
    #[serde(rename = "ROM Type")]
    rom_type: RomType,
    #[serde(rename = "Checksum")]
    checksum: u32,
    #[serde(rename = "SHA1 Digest")]
    sha1: [u8; 20],
}

impl RomRawData {
    const fn binary_size() -> usize {
        RomType::binary_size() + 4 + 20
    }

    pub fn from_buffer(buf: &[u8]) -> Result<Self, Error> {
        let mut pos = 0;

        // Read RomType
        let rom_type_size = RomType::binary_size();
        if buf.len() < pos + rom_type_size {
            return Err(Error::BufferTooSmall);
        }
        let rom_type = RomType::from_bytes(&buf[pos..pos + rom_type_size])?;
        pos += rom_type_size;

        // Read 32-bit checksum (little endian)
        if buf.len() < pos + 4 {
            return Err(Error::BufferTooSmall);
        }
        let checksum = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
        pos += 4;

        // Read 20-byte SHA1
        if buf.len() < pos + 20 {
            return Err(Error::BufferTooSmall);
        }
        let mut sha1 = [0u8; 20];
        sha1.copy_from_slice(&buf[pos..pos + 20]);

        Ok(Self {
            rom_type,
            checksum,
            sha1,
        })
    }

    pub fn to_buffer(&self) -> Result<Vec<u8>, Error> {
        let mut pos = 0;

        let size = Self::binary_size();
        let mut buf = vec![0; size];

        // Write RomType
        let rom_type_size = RomType::binary_size();
        if size < pos + rom_type_size {
            return Err(Error::BufferTooSmall);
        }
        self.rom_type.to_bytes(&mut buf[pos..pos + rom_type_size])?;
        pos += rom_type_size;

        // Write 32-bit checksum (little endian)
        if size < pos + 4 {
            return Err(Error::BufferTooSmall);
        }
        buf[pos..pos + 4].copy_from_slice(&self.checksum.to_le_bytes());
        pos += 4;

        // Write 20-byte SHA1
        if size < pos + 20 {
            return Err(Error::BufferTooSmall);
        }
        buf[pos..pos + 20].copy_from_slice(&self.sha1);

        Ok(buf)
    }
}

/// Response data for RomMetadata
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LabRomEntry {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Part Number")]
    part_number: String,
    #[serde(rename = "Checksum")]
    checksum: u32,
    #[serde(rename = "SHA1 Digest")]
    sha1: [u8; 20],
}

impl LabRomEntry {
    /// Get RomMetadata from the appropriate response
    pub fn from_buffer(buf: &[u8]) -> Result<Self, Error> {
        let mut pos = 0;

        // Get Response code
        if buf.len() < 4 {
            return Err(Error::BufferTooSmall);
        }
        let rsp_u32 = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let response: Response = rsp_u32.into();
        pos += 4;
        match response {
            Response::RomEntry => (), // Continue
            Response::NoRom => Err(Error::NoRom)?,
            Response::RomNotRecognised => Err(Error::RomNotRecognised)?,
            _ => {
                warn!("Unexpected response code for RomMetadata: {rsp_u32:#010X} {response:?}");
                Err(Error::InvalidResponse)?
            }
        }

        // Parse name (null-terminated string)
        let name_end = buf[pos..].iter().position(|&b| b == 0).ok_or_else(|| {
            warn!("Name string not null-terminated");
            Error::InvalidData
        })?;
        let name = String::from_utf8(buf[pos..pos + name_end].to_vec()).map_err(|_| {
            warn!("Invalid UTF-8 in name");
            Error::InvalidData
        })?;
        pos += name_end + 1; // Skip null terminator

        // Parse part number (null-terminated string)
        let part_end = buf[pos..].iter().position(|&b| b == 0).ok_or_else(|| {
            warn!("Part number string not null-terminated");
            Error::InvalidData
        })?;
        let part_number = String::from_utf8(buf[pos..pos + part_end].to_vec()).map_err(|_| {
            warn!("Invalid UTF-8 in part number");
            Error::InvalidData
        })?;
        pos += part_end + 1; // Skip null terminator

        // Parse 32-bit checksum (little endian)
        if buf.len() < pos + 4 {
            warn!("Buffer too short for checksum");
            return Err(Error::BufferTooSmall);
        }
        let checksum = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
        pos += 4;

        // Parse 20-byte SHA1
        if buf.len() < pos + 20 {
            warn!("Buffer too short for SHA1 Digest");
            return Err(Error::BufferTooSmall);
        }
        let mut sha1 = [0u8; 20];
        sha1.copy_from_slice(&buf[pos..pos + 20]);

        Ok(LabRomEntry {
            name,
            part_number,
            checksum,
            sha1,
        })
    }

    fn buf_size(&self) -> usize {
        Response::size() + self.name.len() + 1 + self.part_number.len() + 1 + 4 + 20
    }

    pub fn to_buffer(&self) -> Result<Vec<u8>, Error> {
        let mut pos = 0;

        let size = self.buf_size();
        let mut buf = vec![0; size];

        // Write Response code
        if size < 4 {
            return Err(Error::BufferTooSmall);
        }
        let rsp_u32 = Response::RomEntry as u32;
        buf[pos..pos + 4].copy_from_slice(&rsp_u32.to_le_bytes());
        pos += 4;

        // Write name (null-terminated string)
        let name_bytes = self.name.as_bytes();
        if size < pos + name_bytes.len() + 1 {
            return Err(Error::BufferTooSmall);
        }
        buf[pos..pos + name_bytes.len()].copy_from_slice(name_bytes);
        pos += name_bytes.len();
        buf[pos] = 0; // Null terminator
        pos += 1;

        // Write part number (null-terminated string)
        let part_bytes = self.part_number.as_bytes();
        if size < pos + part_bytes.len() + 1 {
            return Err(Error::BufferTooSmall);
        }
        buf[pos..pos + part_bytes.len()].copy_from_slice(part_bytes);
        pos += part_bytes.len();
        buf[pos] = 0; // Null terminator
        pos += 1;

        // Write 32-bit checksum (little endian)
        if size < pos + 4 {
            return Err(Error::BufferTooSmall);
        }
        buf[pos..pos + 4].copy_from_slice(&self.checksum.to_le_bytes());
        pos += 4;

        // Write 20-byte SHA1
        if size < pos + 20 {
            return Err(Error::BufferTooSmall);
        }
        buf[pos..pos + 20].copy_from_slice(&self.sha1);

        Ok(buf)
    }
}

impl From<RomEntry> for LabRomEntry {
    fn from(entry: RomEntry) -> Self {
        LabRomEntry {
            name: entry.name().to_string(),
            part_number: entry.part().to_string(),
            checksum: entry.sum(),
            sha1: *entry.sha1(),
        }
    }
}
