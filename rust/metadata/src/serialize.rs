// src/serialize.rs
//
// Hand-written serialization support for One ROM metadata.
//
// The bulk of the implementation is in the generated file
// ($OUT_DIR/serialize_generated.rs), which provides:
//   - METADATA_BASE and METADATA_SIZE constants
//   - SerializeContext struct and impl
//   - Per-type layout(), layout_sub_objects(), and write() methods
//
// This file provides SerializeError and the public serialize() entry point.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

// Bring crate-root types (generated structs, enums, etc.) into scope so
// the generated serialize code can reference them by bare name.
#[allow(unused_imports)]
use crate::*;

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
// Generated: SerializeContext, schema constants, and per-type impls.
// ---------------------------------------------------------------------------

include!(concat!(env!("OUT_DIR"), "/serialize_generated.rs"));

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