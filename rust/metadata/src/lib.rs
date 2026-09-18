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
// Metadata generations
// ---------------------------------------------------------------------------

/// The metadata generation firmware `version` reads.
///
/// A tool composes at the generation the firmware it is composing for
/// understands, so that firmware never meets metadata newer than itself.
///
/// A `version` newer than every generation in [`METADATA_GENERATIONS`] gets
/// the newest one there - that firmware fills in the rest through its own
/// accessors.  `None` where `version` predates the first generation, which is
/// firmware predating this schema - see [`MIN_SCHEMA_VERSION`].
pub fn metadata_generation_for(version: FirmwareVersion) -> Option<u32> {
    METADATA_GENERATIONS
        .iter()
        .rev()
        .find(|(first, _)| version >= *first)
        .map(|(_, generation)| *generation)
}

// ---------------------------------------------------------------------------
// Parse errors
// ---------------------------------------------------------------------------

/// Errors produced by generated `parse` implementations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    /// The address or the range `addr..addr+size` lies outside all registered
    /// memory regions.
    OutOfBounds { addr: u32, size: usize },
    /// A pointer field that must not be null contained zero.
    /// `field` is the schema field name.
    NullPointer { field: &'static str },
    /// A C string contained bytes that are not valid UTF-8.
    InvalidUtf8,
    /// A C string did not contain the expected magic value
    BadMagic { field: &'static str },
}

// ---------------------------------------------------------------------------
// Fixed-list field values
// ---------------------------------------------------------------------------

/// The value of a field the schema declares as one of a fixed list.
///
/// The lists grow.  A serving algorithm, a limp-mode pattern or a silicon
/// variant added after a host was built reaches that host as a byte it has no
/// name for, and refusing the byte would cost the whole structure it sits in
/// over one field.  So the value is kept as it was found and the field says it
/// is unknown.
///
/// [`MaybeKnown::Unknown`] widens the stored value to `u32` whatever width the
/// list is stored at, and one type serves every such field.  A variable-length
/// structure's own discriminant is the same problem one level up, answered by
/// an `Unknown` arm on each generated algorithm-config enum, carrying the
/// discriminant, the common fields and the parameter bytes.
///
/// # JSON
///
/// A known value serialises by variant name, as the list's own type does.  An
/// unknown one serialises as `{"unknown": 66}`, so a reader can tell the two
/// apart by shape and still has the byte:
///
/// ```json
/// "slot_type": "RomSlotTypeSingleRom"
/// "slot_type": { "unknown": 66 }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaybeKnown<T> {
    /// A value this build has a name for.
    Known(T),
    /// A value this build has no name for, as the device stored it.
    Unknown(u32),
}

impl<T> MaybeKnown<T> {
    /// Returns the value, or `None` where this build has no name for it.
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) => None,
        }
    }

    /// Returns the stored value, or `None` where this build has a name for it.
    pub fn unknown(&self) -> Option<u32> {
        match self {
            Self::Known(_) => None,
            Self::Unknown(raw) => Some(*raw),
        }
    }

    /// Returns `true` if this build has a name for the value.
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known(_))
    }
}

impl<T> From<T> for MaybeKnown<T> {
    fn from(value: T) -> Self {
        Self::Known(value)
    }
}

impl<T: core::fmt::Display> core::fmt::Display for MaybeKnown<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Known(value) => value.fmt(f),
            Self::Unknown(raw) => write!(f, "unknown ({raw:#04x})"),
        }
    }
}

impl<T: serde::Serialize> serde::Serialize for MaybeKnown<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            Self::Known(value) => value.serialize(serializer),
            Self::Unknown(raw) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("unknown", raw)?;
                map.end()
            }
        }
    }
}

/// The two shapes [`MaybeKnown`] takes on the wire.
///
/// serde's untagged handling needs a type to derive against, and the public
/// enum's `Unknown` is a tuple variant - untagged would read a bare number
/// rather than the `{"unknown": …}` shape.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum MaybeKnownRepr<T> {
    Known(T),
    Unknown { unknown: u32 },
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for MaybeKnown<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match MaybeKnownRepr::deserialize(deserializer)? {
            MaybeKnownRepr::Known(value) => Self::Known(value),
            MaybeKnownRepr::Unknown { unknown } => Self::Unknown(unknown),
        })
    }
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
    /// A field newer than the generation being written holds a value other
    /// than the default a reader of that generation uses.
    ///
    /// That generation has nowhere to put the value, and leaving it out would
    /// produce an image that behaves differently from what was asked for.
    FieldTooNew {
        /// The field, as `<struct>.<field>`.
        field: &'static str,
        /// Oldest firmware whose metadata carries the field.
        minimum: FirmwareVersion,
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
/// A field named as another's `count_field` is written from that `Vec`'s
/// length.  Any value set by the caller is ignored.
///
/// ## Generation
/// `root.version` is the metadata generation being written, and it decides
/// which fields go in: one introduced after it is left out, its bytes left at
/// the `0xFF` a device's unwritten flash reads back.  It comes from the header
/// rather than from an argument, so there is one statement of it.  Set it with
/// [`metadata_generation_for`], from the firmware version being composed for.
///
/// Such a field holding anything but its declared default has nowhere to go,
/// so it returns [`SerializeError::FieldTooNew`] rather than dropping the
/// value.
pub fn serialize(
    root: &OneromMetadataHeader,
    base_addr: u32,
    buf: &mut [u8],
) -> Result<(), SerializeError> {
    let generation = root.version;
    // Refuse before anything is written, so a rejected build leaves no
    // half-composed buffer behind.
    root.check_generation(generation)?;
    let mut ctx = SerializeContext::new(base_addr, generation, buf);
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
