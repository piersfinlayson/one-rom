// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
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
//! and any tooling that needs to read or manipualte One ROM metadata (the
//! same examples, to process and display information about One ROM firmware
//! files and images stored on devices).  It is `no_std` so it can be used by
//! embedded applications, although `alloc` is required.
//!
//! The majority of the objects are generated from the schema, but some core
//! types and traits are hand-written.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

include!(concat!(env!("OUT_DIR"), "/metadata_generated.rs"));
include!(concat!(env!("OUT_DIR"), "/serialize_generated.rs"));

// ---------------------------------------------------------------------------
// Parse errors
// ---------------------------------------------------------------------------

/// Errors produced by generated `parse` implementations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The address or the range `addr..addr+size` lies outside the viewed
    /// region.
    OutOfBounds { addr: u32, size: usize },
    /// A pointer field that must not be null contained zero.
    /// `field` is the schema field name.
    NullPointer { field: &'static str },
    /// An enum discriminant value was not recognised.
    UnknownDiscriminant { type_name: &'static str, value: u32 },
    /// A C string contained bytes that are not valid UTF-8.
    InvalidUtf8,
}

// ---------------------------------------------------------------------------
// FirmwareView
// ---------------------------------------------------------------------------

/// A read-only view over a firmware or metadata byte slice.
///
/// All absolute flash addresses in the binary are resolved relative to
/// `base`, which must equal the load address of `data[0]`.
///
/// Generated `parse` implementations receive a `&FirmwareView` and an
/// absolute flash address; this type translates addresses to slice offsets
/// and provides typed reads.
pub struct FirmwareView<'a> {
    data: &'a [u8],
    base: u32,
}

impl<'a> FirmwareView<'a> {
    /// Construct a view over `data` loaded at flash address `base`.
    pub fn new(data: &'a [u8], base: u32) -> Self {
        Self { data, base }
    }

    // -------------------------------------------------------------------------
    // Primitive reads
    // -------------------------------------------------------------------------

    /// Read a `u8` at the given flash address.
    pub fn read_u8(&self, addr: u32) -> Result<u8, ParseError> {
        let off = self.offset(addr, 1)?;
        Ok(self.data[off])
    }

    /// Read a little-endian `u16` at the given flash address.
    pub fn read_u16_le(&self, addr: u32) -> Result<u16, ParseError> {
        let off = self.offset(addr, 2)?;
        Ok(u16::from_le_bytes([self.data[off], self.data[off + 1]]))
    }

    /// Read a little-endian `u32` at the given flash address.
    pub fn read_u32_le(&self, addr: u32) -> Result<u32, ParseError> {
        let off = self.offset(addr, 4)?;
        Ok(u32::from_le_bytes([
            self.data[off],
            self.data[off + 1],
            self.data[off + 2],
            self.data[off + 3],
        ]))
    }

    /// Read exactly `N` bytes at the given flash address into a fixed array.
    pub fn read_bytes<const N: usize>(&self, addr: u32) -> Result<[u8; N], ParseError> {
        let off = self.offset(addr, N)?;
        let mut buf = [0u8; N];
        buf.copy_from_slice(&self.data[off..off + N]);
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
    /// Used for non-nullable `cstr_ptr` fields.  If the stored pointer is
    /// zero the call will return `OutOfBounds` (zero is below any valid
    /// flash base address); use `read_cstr_opt` for nullable fields.
    pub fn read_cstr(&self, addr: u32) -> Result<String, ParseError> {
        let ptr = self.read_u32_le(addr)?;
        self.follow_cstr(ptr)
    }

    /// Read the 32-bit pointer at `addr`.  Returns `Ok(None)` if the pointer
    /// is null or 0xFFFF_FFFF; otherwise follows it and returns the null
    /// terminated string.
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

    /// Return a sub-slice of `len` bytes starting at flash address `addr`.
    ///
    /// The returned slice borrows from the original data with lifetime `'a`,
    /// not from this borrow of `self`, so callers can collect into a `Vec`
    /// without tying the view's borrow to the result.
    pub fn slice_at(&self, addr: u32, len: usize) -> Result<&'a [u8], ParseError> {
        let off = self.offset(addr, len)?;
        Ok(&self.data[off..off + len])
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Translate a flash address to a slice index, checking that `size` bytes
    /// are available from that offset.
    fn offset(&self, addr: u32, size: usize) -> Result<usize, ParseError> {
        let off = addr
            .checked_sub(self.base)
            .map(|o| o as usize)
            .ok_or(ParseError::OutOfBounds { addr, size })?;
        if off.saturating_add(size) > self.data.len() {
            return Err(ParseError::OutOfBounds { addr, size });
        }
        Ok(off)
    }

    /// Follow a raw (non-zero) pointer value and read the null-terminated
    /// UTF-8 string it points to.
    fn follow_cstr(&self, ptr: u32) -> Result<String, ParseError> {
        let start = self.offset(ptr, 1)?;
        let remaining = &self.data[start..];
        let len = remaining
            .iter()
            .position(|&b| b == 0)
            .ok_or(ParseError::OutOfBounds {
                addr: ptr,
                // Signal that we exhausted the viewed region without finding
                // a null terminator.
                size: remaining.len() + 1,
            })?;
        let s = core::str::from_utf8(&remaining[..len]).map_err(|_| ParseError::InvalidUtf8)?;
        Ok(String::from(s))
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
/// before calling; the serializer copies them verbatim.
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