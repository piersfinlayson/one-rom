// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Reads a One ROM Lab from a device or an image.
//!
//! A host reads `onerom_info_t` and branches on `firmware_type`.  For a Lab
//! this crate follows `metadata` and `runtime` with Lab's generated parsers.
//! It also returns the Lab's `onerom_info_t`.
//!
//! It is `no_std` with `alloc` and reads through the same [`Reader`] as
//! onerom-fw-parser.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use airfrog_rpc::io::Reader;
use log::debug;
use onerom_config::mcu::RP235X_BASE_FLASH;
use onerom_lab_metadata::{
    Generations, LAB_METADATA_SIZE, LAB_METADATA_VERSION, LAB_RUNTIME_INFO_MAGIC,
    LAB_RUNTIME_INFO_VERSION, ONEROM_LAB_RUNTIME_INFO_MAGIC_OFFSET, ONEROM_LAB_RUNTIME_INFO_SIZE,
    OneromLabMetadataHeader, OneromLabRuntimeInfo,
};
use onerom_metadata::{
    BUILD_DATE_BUF_LEN, DeviceMemoryView, FirmwareType, MaybeKnown, NewerGeneration,
    ONEROM_INFO_BUILD_DATE_OFFSET, ONEROM_INFO_METADATA_OFFSET, ONEROM_INFO_OFFSET,
    ONEROM_INFO_RUNTIME_OFFSET, ONEROM_INFO_SIZE, ONEROM_INFO_VERSION, OneromInfo, ParseError,
    Pointer, RuntimeAbsence,
};

/// Reads a One ROM Lab.
pub struct LabParser<'a, R: Reader> {
    reader: &'a mut R,
}

impl<'a, R: Reader> LabParser<'a, R> {
    /// A parser reading through `reader`.
    pub fn new(reader: &'a mut R) -> Self {
        Self { reader }
    }

    /// Parses a Lab. Fails if the device isn't a Lab.
    pub async fn parse(&mut self) -> Result<Lab, String> {
        // Lab runs on the RP2350 only.
        self.reader.update_base_address(RP235X_BASE_FLASH);
        let info_addr = RP235X_BASE_FLASH + ONEROM_INFO_OFFSET;

        let mut info_buf = [0u8; ONEROM_INFO_SIZE];
        self.reader
            .read(info_addr, &mut info_buf)
            .await
            .map_err(|_| "Failed to read onerom_info_t".to_string())?;

        // The pointers are bootstrap fields read by hand.
        let build_date_ptr = info_u32(&info_buf, ONEROM_INFO_BUILD_DATE_OFFSET);
        let metadata_ptr = Pointer::new(info_u32(&info_buf, ONEROM_INFO_METADATA_OFFSET));
        let runtime_ptr = Pointer::new(info_u32(&info_buf, ONEROM_INFO_RUNTIME_OFFSET));

        // One ROM's parser reads the header with Lab's two pointers cleared so it
        // never follows them.  It reads firmware_type only from a generation
        // that has one.  It needs the build date.  A date it can't read comes
        // back empty.
        info_clear_u32(&mut info_buf, ONEROM_INFO_METADATA_OFFSET);
        info_clear_u32(&mut info_buf, ONEROM_INFO_RUNTIME_OFFSET);
        let mut build_date_buf = [0u8; BUILD_DATE_BUF_LEN];
        if self
            .reader
            .read(build_date_ptr, &mut build_date_buf)
            .await
            .is_err()
        {
            build_date_buf.fill(0);
        }
        let mut view = DeviceMemoryView::new(&info_buf, info_addr);
        view.add_region(&build_date_buf, build_date_ptr);
        let info = OneromInfo::parse(&view, info_addr, onerom_metadata::Generations::UNKNOWN)
            .map_err(|e| format!("Not a One ROM family image: {e:?}"))?;
        if info.firmware_type != MaybeKnown::Known(FirmwareType::FirmwareTypeLab) {
            return Err(format!(
                "Not a One ROM Lab: firmware type is {}",
                info.firmware_type
            ));
        }

        // Lab's metadata block.  It also holds every string the runtime
        // structure points at.
        let block = match metadata_ptr.addr() {
            Some(addr) => {
                let mut buf = vec![0u8; LAB_METADATA_SIZE as usize];
                match self.reader.read(addr, &mut buf).await {
                    Ok(()) => Ok((addr, buf)),
                    Err(_) => Err(ParseError::OutOfBounds {
                        addr,
                        size: buf.len(),
                    }),
                }
            }
            None => Err(ParseError::NullPointer { field: "metadata" }),
        };

        let metadata = match &block {
            Ok((addr, buf)) => {
                let view = DeviceMemoryView::new(buf, *addr);
                OneromLabMetadataHeader::parse(&view, *addr, Generations::UNKNOWN)
            }
            Err(e) => Err(e.clone()),
        };

        let runtime = match runtime_ptr.addr() {
            Some(addr) => self.parse_runtime(addr, block.as_ref().ok()).await,
            None => Err(RuntimeAbsence::NoPointer),
        };

        Ok(Lab {
            info,
            metadata,
            runtime,
        })
    }

    /// Reads the runtime structure at `addr` and the strings it points at from
    /// `block`.
    async fn parse_runtime(
        &mut self,
        addr: u32,
        block: Option<&(u32, Vec<u8>)>,
    ) -> Result<OneromLabRuntimeInfo, RuntimeAbsence> {
        let mut buf = [0u8; ONEROM_LAB_RUNTIME_INFO_SIZE];
        if self.reader.read(addr, &mut buf).await.is_err() {
            return Err(RuntimeAbsence::Unreadable);
        }
        if !buf[ONEROM_LAB_RUNTIME_INFO_MAGIC_OFFSET..]
            .starts_with(LAB_RUNTIME_INFO_MAGIC.as_bytes())
        {
            return Err(RuntimeAbsence::NotRunning);
        }

        let mut view = DeviceMemoryView::new(&buf, addr);
        if let Some((block_addr, block)) = block {
            view.add_region(block, *block_addr);
        }
        OneromLabRuntimeInfo::parse(&view, addr, Generations::UNKNOWN).map_err(|e| {
            debug!("Lab runtime structure at {addr:#010X} did not parse: {e:?}");
            RuntimeAbsence::Unparsed
        })
    }
}

/// A Lab's structures or why each is missing.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Lab {
    /// Lab's `onerom_info_t`, with `metadata` and `runtime` left `None`.
    pub info: OneromInfo,
    /// Includes the board the image was built for.
    pub metadata: Result<OneromLabMetadataHeader, ParseError>,
    /// Includes the board Lab is running as. Missing on a stopped Lab.
    pub runtime: Result<OneromLabRuntimeInfo, RuntimeAbsence>,
}

impl Lab {
    /// Structures this parser is too old to read in full, `onerom_info_t` included.
    pub fn newer_generations(&self) -> Vec<NewerGeneration> {
        let mut found = Vec::new();
        if self.info.version > ONEROM_INFO_VERSION {
            found.push(NewerGeneration {
                structure: "onerom_info_t",
                device_generation: self.info.version,
                known_generation: ONEROM_INFO_VERSION,
            });
        }
        if let Ok(metadata) = &self.metadata
            && metadata.version > LAB_METADATA_VERSION
        {
            found.push(NewerGeneration {
                structure: "onerom_lab_metadata_header_t",
                device_generation: metadata.version,
                known_generation: LAB_METADATA_VERSION,
            });
        }
        if let Ok(runtime) = &self.runtime
            && runtime.version > LAB_RUNTIME_INFO_VERSION
        {
            found.push(NewerGeneration {
                structure: "onerom_lab_runtime_info_t",
                device_generation: runtime.version,
                known_generation: LAB_RUNTIME_INFO_VERSION,
            });
        }
        found
    }
}

/// Read a little-endian `u32` out of the header.
///
/// `offset` is one of the schema's `ONEROM_INFO_*_OFFSET` constants and the
/// buffer is a whole header so the read is always inside it.
fn info_u32(buf: &[u8; ONEROM_INFO_SIZE], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

/// Zero a `u32` pointer field in a working copy of the header.
fn info_clear_u32(buf: &mut [u8; ONEROM_INFO_SIZE], offset: usize) {
    buf[offset..offset + 4].fill(0);
}
