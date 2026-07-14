// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

//! Crate exposing One ROM metadata types and parsing/serialization logic.
//!
//! The One ROM firmware's metadata is specified by a TOML schema.  It is
//! then processed as part of the build process and both a C header (for the
//! core firmware), and Rust types and parsing/serialization logic is auto-
//! generated from it.  This crate is that Rust code.
//!
//! This crate is designed for use by any tooling that needs to generate
//! One ROM metadata (i.e. building tools like One ROM CLI, Studio and Web),
//! and any tooling that needs to read or manipulate One ROM metadata (the
//! same examples, to process and display information about One ROM firmware
//! files and images stored on devices).  It is `no_std` so it can be used by
//! embedded applications, although `alloc` is required.
//!
//! The majority of the objects are generated from the schema, but some core
//! types and traits are hand-written.
//!
//! There is a key limitation of this crate that embedded callers must be
//! aware of.
//!
//! # Memory constraint
//!
//! All generated `parse` implementations operate on a [`DeviceMemoryView`]:
//! a synchronous, slice-based view over one or more pre-loaded regions of
//! device memory.  Before calling any `parse` function the caller must read
//! the relevant memory regions into buffers and register them with the view.
//!
//! The largest single region is the metadata blob, which is up to
//! [`METADATA_SIZE`] bytes (16 KB).  On an embedded system that reads device
//! memory over a debug interface (e.g. SWD) rather than mapping it directly,
//! those 16 KB must reside in the reader's own RAM simultaneously.  For
//! deeply resource-constrained systems this may be a meaningful allocation.
//!
//! Systems where the target device's flash is memory-mapped (for example, an
//! RP2350 reading its own XIP flash) can create a [`DeviceMemoryView`]
//! directly over the mapped address space with no copying and no additional
//! allocation.
//!
//! A future revision may introduce a lazy, reader-backed view that fetches
//! memory on demand and avoids holding the full metadata blob in RAM.  Until
//! then, callers that cannot afford a 16 KB working buffer should defer use
//! of the generated parsers until that interface is available.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use onerom_config::fw::FirmwareVersion;
use onerom_config::mcu::{RP235X_BASE_FLASH, RP235X_BASE_SRAM, RP235X_END_FLASH, RP235X_END_SRAM};

include!(concat!(env!("OUT_DIR"), "/metadata_generated.rs"));
include!(concat!(env!("OUT_DIR"), "/serialize_generated.rs"));
include!(concat!(env!("OUT_DIR"), "/host_generated.rs"));

mod firmware_overrides_impl;

pub const MIN_SCHEMA_VERSION: FirmwareVersion = FirmwareVersion::new(0, 7, 0, 0);

// ---------------------------------------------------------------------------
// Parse errors
// ---------------------------------------------------------------------------

/// Errors produced by generated `parse` implementations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The address or the range `addr..addr+size` lies outside all registered
    /// memory regions.
    OutOfBounds { addr: u32, size: usize },
    /// A pointer field that must not be null contained zero.
    /// `field` is the schema field name.
    NullPointer { field: &'static str },
    /// An enum discriminant value was not recognised.
    UnknownDiscriminant { type_name: &'static str, value: u32 },
    /// A C string contained bytes that are not valid UTF-8.
    InvalidUtf8,
    /// A C string did not contain the expected magic value
    BadMagic { field: &'static str },
}

// ---------------------------------------------------------------------------
// Pointer type
// ---------------------------------------------------------------------------

/// A 32-bit firmware pointer, normalised at construction time.
///
/// Both `0x0000_0000` and `0xFFFF_FFFF` are treated as null/absent sentinels,
/// matching the convention used throughout the OneROM firmware and metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Pointer {
    /// Null or absent pointer (raw value was `0` or `0xFFFF_FFFF`).
    Null,
    /// A non-null 32-bit address.
    Addr32(u32),
}

impl Pointer {
    /// Construct a [`Pointer`] from a raw `u32`, treating `0` and `0xFFFF_FFFF`
    /// as [`Pointer::Null`].
    pub fn new(raw: u32) -> Self {
        match raw {
            0 | 0xFFFF_FFFF => Self::Null,
            a => Self::Addr32(a),
        }
    }

    /// Returns `true` if this pointer is null.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns the raw address, or `None` if null.
    pub fn addr(&self) -> Option<u32> {
        if let Self::Addr32(a) = self {
            Some(*a)
        } else {
            None
        }
    }

    /// Returns the raw `u32` address, or `0` for [`Pointer::Null`].
    /// Used when serialising pointer fields back to binary.
    pub fn raw(&self) -> u32 {
        match self {
            Self::Null => 0,
            Self::Addr32(a) => *a,
        }
    }

    /// Returns `true` if the address falls within the RP235x XIP flash region.
    pub fn is_flash(&self) -> bool {
        matches!(self, Self::Addr32(a) if matches!(a, RP235X_BASE_FLASH..=RP235X_END_FLASH))
    }

    /// Returns `true` if the address falls within the RP235x SRAM region.
    pub fn is_sram(&self) -> bool {
        matches!(self, Self::Addr32(a) if matches!(a, RP235X_BASE_SRAM..=RP235X_END_SRAM))
    }
}

// ---------------------------------------------------------------------------
// DeviceMemoryView
// ---------------------------------------------------------------------------

/// A read-only view over one or more regions of device memory.
///
/// All absolute addresses in the binary are resolved by searching registered
/// regions in the order they were added.  This allows a single view to cover
/// discontiguous memory — for example, a flash info header, a separately
/// loaded metadata blob, and a RAM region — without requiring callers to
/// allocate a single contiguous buffer spanning the full address range.
///
/// Generated `parse` implementations receive a `&DeviceMemoryView` and an
/// absolute address; this type translates addresses to slice offsets and
/// provides typed reads.
///
/// # Construction
///
/// Use [`DeviceMemoryView::new`] for a single region, then [`add_region`] for
/// each additional region:
///
/// ```rust
/// # use onerom_metadata::DeviceMemoryView;
/// let flash: &[u8] = &[0u8; 256];
/// let ram:   &[u8] = &[0u8; 64];
///
/// let mut view = DeviceMemoryView::new(flash, 0x1000_0000);
/// view.add_region(ram, 0x2000_0000);
/// ```
///
/// [`add_region`]: DeviceMemoryView::add_region
pub struct DeviceMemoryView<'a> {
    regions: Vec<(&'a [u8], u32)>,
}

impl<'a> DeviceMemoryView<'a> {
    /// Construct a view with a single initial memory region.
    ///
    /// # Arguments
    ///
    /// * `data`  - Byte slice containing the region's data.
    /// * `base`  - Absolute address of the first byte of `data`.
    pub fn new(data: &'a [u8], base: u32) -> Self {
        Self {
            regions: alloc::vec![(data, base)],
        }
    }

    /// Add an additional memory region to the view.
    ///
    /// Regions are searched in insertion order; the first region whose address
    /// range covers the requested address is used.  Overlapping regions are
    /// permitted but may produce unexpected results if they disagree on
    /// overlapping bytes.
    ///
    /// # Arguments
    ///
    /// * `data`  - Byte slice containing the region's data.
    /// * `base`  - Absolute address of the first byte of `data`.
    pub fn add_region(&mut self, data: &'a [u8], base: u32) {
        self.regions.push((data, base));
    }

    // -------------------------------------------------------------------------
    // Primitive reads
    // -------------------------------------------------------------------------

    /// Read a `u8` at the given absolute address.
    pub fn read_u8(&self, addr: u32) -> Result<u8, ParseError> {
        let (data, off) = self.region_for(addr, 1)?;
        Ok(data[off])
    }

    /// Read a little-endian `u16` at the given absolute address.
    pub fn read_u16_le(&self, addr: u32) -> Result<u16, ParseError> {
        let (data, off) = self.region_for(addr, 2)?;
        Ok(u16::from_le_bytes([data[off], data[off + 1]]))
    }

    /// Read a little-endian `u32` at the given absolute address.
    pub fn read_u32_le(&self, addr: u32) -> Result<u32, ParseError> {
        let (data, off) = self.region_for(addr, 4)?;
        Ok(u32::from_le_bytes([
            data[off],
            data[off + 1],
            data[off + 2],
            data[off + 3],
        ]))
    }

    /// Read exactly `N` bytes at the given absolute address into a fixed array.
    pub fn read_bytes<const N: usize>(&self, addr: u32) -> Result<[u8; N], ParseError> {
        let (data, off) = self.region_for(addr, N)?;
        let mut buf = [0u8; N];
        buf.copy_from_slice(&data[off..off + N]);
        Ok(buf)
    }

    // -------------------------------------------------------------------------
    // Pointer and string reads
    // -------------------------------------------------------------------------

    /// Read the raw 32-bit pointer value stored at `addr` without following it.
    pub fn read_ptr(&self, addr: u32) -> Result<u32, ParseError> {
        self.read_u32_le(addr)
    }

    /// Read the 32-bit pointer at `addr`, follow it, and return the
    /// null-terminated UTF-8 string it points to.
    ///
    /// Used for non-nullable `cstr_ptr` fields.
    pub fn read_cstr(&self, addr: u32) -> Result<String, ParseError> {
        let ptr = self.read_u32_le(addr)?;
        self.follow_cstr(ptr)
    }

    /// Read the 32-bit pointer at `addr`.  Returns `Ok(None)` if the pointer
    /// is null (`0` or `0xFFFF_FFFF`); otherwise follows it and returns the
    /// null-terminated string.
    ///
    /// Used for nullable `cstr_ptr` fields.
    pub fn read_cstr_opt(&self, addr: u32) -> Result<Option<String>, ParseError> {
        let ptr = self.read_u32_le(addr)?;
        if ptr == 0 || ptr == 0xFFFF_FFFF {
            Ok(None)
        } else {
            self.follow_cstr(ptr).map(Some)
        }
    }

    /// Return a sub-slice of `len` bytes starting at absolute address `addr`.
    ///
    /// The returned slice borrows from the original region data with lifetime
    /// `'a`, so callers can collect into a `Vec` without tying the view's
    /// borrow to the result.
    pub fn slice_at(&self, addr: u32, len: usize) -> Result<&'a [u8], ParseError> {
        for &(data, base) in &self.regions {
            #[allow(clippy::collapsible_if)]
            if let Some(off) = addr.checked_sub(base).map(|o| o as usize) {
                if off.saturating_add(len) <= data.len() {
                    return Ok(&data[off..off + len]);
                }
            }
        }
        Err(ParseError::OutOfBounds { addr, size: len })
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Find the region covering `addr..addr+size` and return `(data, offset)`.
    fn region_for(&self, addr: u32, size: usize) -> Result<(&[u8], usize), ParseError> {
        for &(data, base) in &self.regions {
            #[allow(clippy::collapsible_if)]
            if let Some(off) = addr.checked_sub(base).map(|o| o as usize) {
                if off.saturating_add(size) <= data.len() {
                    return Ok((data, off));
                }
            }
        }
        Err(ParseError::OutOfBounds { addr, size })
    }

    /// Follow a raw (non-null) pointer and read the null-terminated UTF-8
    /// string it points to, searching all registered regions.
    fn follow_cstr(&self, ptr: u32) -> Result<String, ParseError> {
        for &(data, base) in &self.regions {
            #[allow(clippy::collapsible_if)]
            if let Some(start) = ptr.checked_sub(base).map(|o| o as usize) {
                if start < data.len() {
                    let remaining = &data[start..];
                    let len =
                        remaining
                            .iter()
                            .position(|&b| b == 0)
                            .ok_or(ParseError::OutOfBounds {
                                addr: ptr,
                                size: remaining.len() + 1,
                            })?;
                    let s = core::str::from_utf8(&remaining[..len])
                        .map_err(|_| ParseError::InvalidUtf8)?;
                    return Ok(String::from(s));
                }
            }
        }
        Err(ParseError::OutOfBounds { addr: ptr, size: 1 })
    }
}

// ---------------------------------------------------------------------------
// Serialize errors
// ---------------------------------------------------------------------------

/// Errors produced by the two-phase serializer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SerializeError {
    /// The output buffer or metadata region is too small to hold the
    /// serialized objects.
    Overflow,
    /// A `Vec` field's length exceeds the range of the corresponding
    /// binary count field (e.g. > 255 for a `u8` count).
    CountOverflow {
        /// Name of the count field that would overflow.
        field: &'static str,
    },
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Serialize `root` into `buf` starting at flash address `base_addr`.
///
/// ## Buffer
/// `buf` is filled with `0xFF` on entry; only the serialized object bytes
/// are written over it.  `buf` may be any length ≥ the total serialized
/// output.  Use [`METADATA_SIZE`] bytes to cover the full metadata region.
///
/// ## Base address
/// Use [`METADATA_BASE`] as `base_addr` for production metadata images.
///
/// ## opaque_ptr fields
/// Fields such as `OneromRomSlot::data` store raw flash addresses pointing
/// to data outside the metadata region.  Set them to the correct value
/// before calling; the serializer copies them verbatim via [`Pointer::raw`].
///
/// ## Derived count fields
/// `OneromMetadataHeader::rom_slot_count` and `OneromRomSlot::rom_count`
/// are written from the corresponding `Vec` length.  Any value set by the
/// caller is ignored.
pub fn serialize(
    root: &OneromMetadataHeader,
    base_addr: u32,
    buf: &mut [u8],
) -> Result<(), SerializeError> {
    let mut ctx = SerializeContext::new(base_addr, buf);
    // Phase 1: assign flash addresses to every reachable object.
    root.layout(&mut ctx)?;
    // Phase 2: write bytes.  Root is always at base_addr.
    root.write(&mut ctx, base_addr);
    Ok(())
}

// ---------------------------------------------------------------------------
// Utils
// ---------------------------------------------------------------------------

/// Escape a string for embedding inside a C string literal.
///
/// Escapes `"` → `\"` and `\` → `\\`. NUL bytes are rejected because
/// `cstr_ptr` fields are null-terminated C strings and a NUL would
/// silently truncate the value at the C level.
pub fn escape_c_string(s: &str) -> alloc::string::String {
    let mut out = alloc::string::String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\0' => panic!("NUL byte in C string literal (field value: {:?})", s),
            c => out.push(c),
        }
    }
    out
}
