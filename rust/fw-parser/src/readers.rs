// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! onerom-fw-parser
//!
//! Contains standard re-usable reader implementations for parsing SDRR firmware

use crate::Reader;

#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec, vec::Vec};

/// A reader that operates on an in-memory firmware image.
///
/// This is the standard reader for PC applications that can load the entire
/// firmware file into memory. It handles translation between absolute memory
/// addresses and offsets within the firmware data.
///
/// This is unlikely to be appropriate for embedded applications, which
/// should implement their own `Reader` trait to read from flash or other
/// memory directly.
///
/// # Example
///
/// ```rust
/// # async fn test() -> Result<(), Box<dyn std::error::Error>> {
/// use onerom_fw_parser::{Parser, readers::MemoryReader};
///
/// // Load firmware file
/// let firmware_data = std::fs::read("firmware.bin")?;
///
/// // Create reader starting at STM32F4 flash base
/// let mut reader = MemoryReader::new(firmware_data, 0x08000000);
///
/// // Parse the firmware
/// let mut parser = Parser::new(&mut reader);
/// let info = parser.parse().await;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }
/// ```
#[derive(Debug)]
pub struct MemoryReader {
    regions: Vec<MemoryRegion>,
}

#[derive(Debug)]
struct MemoryRegion {
    kind: RegionKind,
    base_address: u32,
    data: Vec<u8>,
}

/// The type of memory region
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RegionKind {
    Flash,
    Ram,
}

impl MemoryReader {
    /// Create a new memory reader.
    ///
    /// # Arguments
    ///
    /// * `data` - The complete firmware image data
    /// * `base_address` - The base address where this firmware would be loaded
    ///   in the target device (typically `0x08000000` for STM32F4)
    pub fn new(data: Vec<u8>, base_address: u32) -> Self {
        Self {
            regions: vec![MemoryRegion {
                kind: RegionKind::Flash,
                data,
                base_address,
            }],
        }
    }

    /// Create a memory reader from a specific region (e.g. RAM)
    pub fn new_of_kind(kind: RegionKind, data: Vec<u8>, base_address: u32) -> Self {
        Self {
            regions: vec![MemoryRegion {
                kind,
                data,
                base_address,
            }],
        }
    }

    /// Add a memory region to the reader. This allows the reader to handle multiple
    /// regions (e.g. flash and RAM) in a single reader instance.
    pub fn add_region(&mut self, kind: RegionKind, data: Vec<u8>, base_address: u32) {
        self.regions.push(MemoryRegion {
            kind,
            base_address,
            data,
        });
    }
}

impl Reader for MemoryReader {
    type Error = String;

    async fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), Self::Error> {
        let end_addr = addr
            .checked_add(buf.len() as u32)
            .ok_or_else(|| format!("Address overflow at 0x{:08X}", addr))?;

        let region = self
            .regions
            .iter()
            .find(|r| {
                let region_end = r.base_address.saturating_add(r.data.len() as u32);
                addr >= r.base_address && end_addr <= region_end
            })
            .ok_or_else(|| format!("No region covers 0x{:08X}..0x{:08X}", addr, end_addr))?;

        let offset = (addr - region.base_address) as usize;
        buf.copy_from_slice(&region.data[offset..offset + buf.len()]);
        Ok(())
    }

    /// Updates the base address for any flash regions
    fn update_base_address(&mut self, new_base: u32) {
        if let Some(r) = self
            .regions
            .iter_mut()
            .find(|r| r.kind == RegionKind::Flash)
        {
            r.base_address = new_base;
        }
    }
}
