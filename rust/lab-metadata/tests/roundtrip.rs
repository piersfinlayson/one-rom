// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Lab's generated writer against its generated parser.
//!
//! The layout locks at Lab's first release, so what these prove is that the
//! two sides agree on it beforehand - a field written at one offset and read
//! at another passes every other check in the tree.

use onerom_lab_metadata::{DeviceMemoryView, MaybeKnown, Rp235xVariant};
use onerom_lab_metadata::{
    Generations, LAB_METADATA_MAGIC, LAB_METADATA_VERSION, LAB_RUNTIME_INFO_MAGIC,
    LAB_RUNTIME_INFO_VERSION, OneromLabHardwareInfo, OneromLabMetadataHeader, OneromLabRuntimeInfo,
    ParseError, SerializeContext, SerializeError,
};

const METADATA_BASE: u32 = 0x1000_C000;
const METADATA_SIZE: usize = 4096;

/// Compose `header` into a metadata buffer, the three steps a host takes.
fn compose(header: &OneromLabMetadataHeader) -> Result<Vec<u8>, SerializeError> {
    let mut buf = vec![0u8; METADATA_SIZE];
    let generation = header.version;
    header.check_generation(generation)?;
    let mut ctx = SerializeContext::new(METADATA_BASE, generation, &mut buf);
    header.layout(&mut ctx)?;
    header.write(&mut ctx, METADATA_BASE);
    Ok(buf)
}

/// Read a composed buffer back, learning the generation from the bytes rather
/// than being told it.
fn read_back(buf: &[u8]) -> Result<OneromLabMetadataHeader, ParseError> {
    let view = DeviceMemoryView::new(buf, METADATA_BASE);
    OneromLabMetadataHeader::parse(&view, METADATA_BASE, Generations::UNKNOWN)
}

/// The magic as the structure holds it - 15 characters and a terminator.
fn magic() -> [u8; 16] {
    let mut out = [0u8; 16];
    out[..LAB_METADATA_MAGIC.len()].copy_from_slice(LAB_METADATA_MAGIC.as_bytes());
    out
}

fn header() -> OneromLabMetadataHeader {
    OneromLabMetadataHeader {
        magic: magic(),
        version: LAB_METADATA_VERSION,
        hw: OneromLabHardwareInfo {
            hw_rev: Some("fire-40-a".into()),
            rp235x: MaybeKnown::Known(Rp235xVariant::Rp235xb),
        },
    }
}

#[test]
fn a_baked_board_survives_the_round_trip() {
    let buf = compose(&header()).expect("the header should compose");
    let read = read_back(&buf).expect("the buffer should parse");
    assert_eq!(read.version, LAB_METADATA_VERSION);
    assert_eq!(read.hw.hw_rev.as_deref(), Some("fire-40-a"));
    assert_eq!(read.hw.rp235x, MaybeKnown::Known(Rp235xVariant::Rp235xb));
}

#[test]
fn a_board_set_at_runtime_leaves_the_baked_one_absent() {
    let mut h = header();
    h.hw.hw_rev = None;
    let buf = compose(&h).expect("the header should compose");
    let read = read_back(&buf).expect("the buffer should parse");
    assert_eq!(read.hw.hw_rev, None);
}

#[test]
fn the_wrong_magic_is_refused() {
    let mut buf = compose(&header()).expect("the header should compose");
    buf[0] = b'X';
    assert!(matches!(read_back(&buf), Err(ParseError::BadMagic { .. })));
}

/// The runtime structure is parse-only, so there is no writer to round trip
/// against.  What is worth pinning is that its declared size and the bytes the
/// parser walks are the same number.
#[test]
fn the_runtime_structure_reads_its_declared_size() {
    let mut buf = vec![0xffu8; METADATA_SIZE];
    buf[..4].copy_from_slice(LAB_RUNTIME_INFO_MAGIC.as_bytes());
    buf[4..8].copy_from_slice(&LAB_RUNTIME_INFO_VERSION.to_le_bytes());
    buf[8..12]
        .copy_from_slice(&(onerom_lab_metadata::ONEROM_LAB_RUNTIME_INFO_SIZE as u32).to_le_bytes());
    buf[12..16].copy_from_slice(&0u32.to_le_bytes());

    let view = DeviceMemoryView::new(&buf, METADATA_BASE);
    let runtime = OneromLabRuntimeInfo::parse(&view, METADATA_BASE, Generations::UNKNOWN)
        .expect("the buffer should parse");
    assert_eq!(
        runtime.runtime_info_size as usize,
        onerom_lab_metadata::ONEROM_LAB_RUNTIME_INFO_SIZE
    );
    assert_eq!(runtime.hw_rev, None);
}
