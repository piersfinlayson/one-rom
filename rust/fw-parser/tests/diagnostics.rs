// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What the schema parser says about what it could not show.
//!
//! Three cases.  The runtime structure can be missing for four quite
//! different reasons, and each has to arrive as its own.  A structure can
//! carry a generation this build has no field names for, and that has to be
//! said rather than left as a shorter dump.  A field holding a value a fixed
//! list has grown past is neither - it costs that field and leaves the
//! structure whole.

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use onerom_fw_parser::readers::{MemoryReader, RegionKind};
use onerom_fw_parser::{Parser, RuntimeAbsence, SDRR_INFO_FW_OFFSET};
use onerom_metadata::{
    FirmwareType, ONEROM_FAMILY_MAGIC, ONEROM_INFO_FIRMWARE_TYPE_OFFSET as TYPE_OFF,
    ONEROM_INFO_VERSION_OFFSET as VERSION_OFF,
};

const RP235X_FLASH_BASE: u32 = 0x1000_0000;
const RP235X_RAM_BASE: u32 = 0x2008_0000;

/// Where the tests that give a device a runtime structure put it.
const RUNTIME_ADDR: u32 = RP235X_RAM_BASE;

/// A RAM address no test registers a region for, so a read of it fails.
const UNMAPPED_RAM_ADDR: u32 = RP235X_RAM_BASE + 0x1000;

/// The generation `onerom_info_t` and `onerom_runtime_info_t` carry today.
///
/// Taken from the schema rather than written here, so a bump moves the tests
/// with it and the "newer than this build" cases stay newer.
const INFO_GENERATION: u32 = onerom_metadata::ONEROM_INFO_VERSION;
const RUNTIME_GENERATION: u32 = onerom_metadata::RUNTIME_INFO_VERSION;

/// Runs a future to completion.
///
/// Every reader used here is backed by memory and never pends, so a single
/// poll always completes.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("in-memory reader pended"),
    }
}

/// A flash image holding a v0.8.0 `onerom_info_t` at the usual offset.
///
/// `build_date` points at a zero byte inside the image and the metadata
/// pointer is null, so the header parses with the metadata absent.  The
/// firmware type is One ROM's, as One ROM writes it.  The caller says what
/// generation the header claims and where it points for its runtime
/// structure.
fn schema_image(info_generation: u32, runtime_ptr: u32) -> Vec<u8> {
    let mut image = vec![0u8; 0x400];
    let base = SDRR_INFO_FW_OFFSET as usize;
    image[base..base + 4].copy_from_slice(ONEROM_FAMILY_MAGIC.as_bytes());
    image[base + 4..base + 6].copy_from_slice(&0u16.to_le_bytes());
    image[base + 6..base + 8].copy_from_slice(&8u16.to_le_bytes());
    image[base + 8..base + 10].copy_from_slice(&0u16.to_le_bytes());
    // build_date, into the zeroed tail of the image
    image[base + 12..base + 16].copy_from_slice(&(RP235X_FLASH_BASE + 0x300).to_le_bytes());
    image[base + VERSION_OFF..base + VERSION_OFF + 4]
        .copy_from_slice(&info_generation.to_le_bytes());
    image[base + 36..base + 40].copy_from_slice(&runtime_ptr.to_le_bytes());
    image[base + TYPE_OFF..base + TYPE_OFF + 2]
        .copy_from_slice(&(FirmwareType::FirmwareTypeOneRom as u16).to_le_bytes());
    image
}

/// A RAM region holding an `onerom_runtime_info_t` the parser takes.
///
/// Every field is zero bar the magic, the generation, and `bit_mode`, which
/// is set to a width the list does name so the structure reads as a device
/// would have written it.
fn runtime_region(generation: u32) -> Vec<u8> {
    let mut ram = vec![0u8; 64];
    ram[0..4].copy_from_slice(b"sdrr");
    ram[4..8].copy_from_slice(&generation.to_le_bytes());
    ram[BIT_MODE_OFFSET] = 1; // bit_mode: 8-bit
    ram
}

/// Byte offset of `bit_mode` within the runtime structure.
const BIT_MODE_OFFSET: usize = 38;

/// Byte offset of `current_rom_slot` within the runtime structure, which sits
/// directly after `user_plugin_context`.  The schema owns the layout, so this
/// counts from the constant it exports for the field before.
const CURRENT_ROM_SLOT_OFFSET: usize =
    onerom_metadata::ONEROM_RUNTIME_INFO_USER_PLUGIN_CONTEXT_OFFSET + 4;

/// A parser over a flash image alone, with no RAM registered.
fn flash_only(reader: &mut MemoryReader) -> Parser<'_, MemoryReader> {
    Parser::with_base_flash_address(reader, RP235X_FLASH_BASE, RP235X_RAM_BASE)
}

// ---------------------------------------------------------------------------
// Why the runtime structure is missing
// ---------------------------------------------------------------------------

#[test]
fn a_null_runtime_pointer_says_the_image_is_damaged() {
    let mut reader = MemoryReader::new(schema_image(INFO_GENERATION, 0), RP235X_FLASH_BASE);
    let mut parser = flash_only(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");

    assert!(onerom.runtime().is_none());
    assert_eq!(onerom.runtime_absence(), Some(RuntimeAbsence::NoPointer));
}

#[test]
fn unreadable_runtime_memory_says_it_could_not_be_read() {
    let mut reader = MemoryReader::new(
        schema_image(INFO_GENERATION, UNMAPPED_RAM_ADDR),
        RP235X_FLASH_BASE,
    );
    let mut parser = flash_only(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");

    assert!(onerom.runtime().is_none());
    assert_eq!(onerom.runtime_absence(), Some(RuntimeAbsence::Unreadable));
}

#[test]
fn runtime_memory_without_the_magic_says_the_device_is_not_running() {
    let mut reader = MemoryReader::new(
        schema_image(INFO_GENERATION, RUNTIME_ADDR),
        RP235X_FLASH_BASE,
    );
    // A stopped device's RAM holds whatever it holds, and not the magic.
    reader.add_region(RegionKind::Ram, vec![0u8; 64], RUNTIME_ADDR);
    let mut parser = flash_only(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");

    assert!(onerom.runtime().is_none());
    assert_eq!(onerom.runtime_absence(), Some(RuntimeAbsence::NotRunning));
}

#[test]
fn a_runtime_structure_this_build_cannot_read_says_so() {
    let mut reader = MemoryReader::new(
        schema_image(INFO_GENERATION, RUNTIME_ADDR),
        RP235X_FLASH_BASE,
    );
    // The magic is there, so the loader hands the structure on.  Its active
    // slot points into memory no region covers, and the parser follows that
    // pointer, so the structure behind the magic does not parse.
    let mut ram = runtime_region(RUNTIME_GENERATION);
    ram[CURRENT_ROM_SLOT_OFFSET..CURRENT_ROM_SLOT_OFFSET + 4]
        .copy_from_slice(&UNMAPPED_RAM_ADDR.to_le_bytes());
    reader.add_region(RegionKind::Ram, ram, RUNTIME_ADDR);
    let mut parser = flash_only(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");

    assert!(onerom.runtime().is_none());
    assert_eq!(onerom.runtime_absence(), Some(RuntimeAbsence::Unparsed));
}

/// The structure arrives whole, everything beside the field reads as the
/// device wrote it, and the field itself carries the byte.
#[test]
fn a_value_this_build_has_no_name_for_does_not_cost_the_structure() {
    let mut reader = MemoryReader::new(
        schema_image(INFO_GENERATION, RUNTIME_ADDR),
        RP235X_FLASH_BASE,
    );
    let mut ram = runtime_region(RUNTIME_GENERATION);
    // A bus width no release has ever used.
    ram[BIT_MODE_OFFSET] = 0x37;
    // A neighbour with a value worth checking survived.
    ram[BIT_MODE_OFFSET - 1] = 0x03; // peri_en: USB PLL and ADC
    reader.add_region(RegionKind::Ram, ram, RUNTIME_ADDR);
    let mut parser = flash_only(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");

    assert_eq!(onerom.runtime_absence(), None);
    let runtime = onerom.runtime().expect("the structure should be there");
    assert_eq!(runtime.bit_mode, onerom_metadata::MaybeKnown::Unknown(0x37));
    assert_eq!(runtime.peri_en, 0x03);
    assert_eq!(runtime.version, RUNTIME_GENERATION);
}

#[test]
fn a_running_device_has_nothing_to_explain() {
    let mut reader = MemoryReader::new(
        schema_image(INFO_GENERATION, RUNTIME_ADDR),
        RP235X_FLASH_BASE,
    );
    reader.add_region(
        RegionKind::Ram,
        runtime_region(RUNTIME_GENERATION),
        RUNTIME_ADDR,
    );
    let mut parser = flash_only(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");

    assert!(onerom.runtime().is_some());
    assert_eq!(onerom.runtime_absence(), None);
}

#[test]
fn each_reason_reads_differently() {
    let reasons = [
        RuntimeAbsence::NoPointer,
        RuntimeAbsence::Unreadable,
        RuntimeAbsence::NotRunning,
        RuntimeAbsence::Unparsed,
    ];
    let mut said: Vec<String> = reasons.iter().map(ToString::to_string).collect();
    said.sort();
    said.dedup();
    assert_eq!(said.len(), reasons.len());
}

// ---------------------------------------------------------------------------
// A structure newer than this build knows
// ---------------------------------------------------------------------------

#[test]
fn a_device_of_this_generation_reports_nothing() {
    let mut reader = MemoryReader::new(
        schema_image(INFO_GENERATION, RUNTIME_ADDR),
        RP235X_FLASH_BASE,
    );
    reader.add_region(
        RegionKind::Ram,
        runtime_region(RUNTIME_GENERATION),
        RUNTIME_ADDR,
    );
    let mut parser = flash_only(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");

    assert!(onerom.newer_generations().is_empty());
}

#[test]
fn a_newer_info_structure_is_named_and_still_parsed() {
    let mut reader = MemoryReader::new(
        schema_image(INFO_GENERATION + 1, RUNTIME_ADDR),
        RP235X_FLASH_BASE,
    );
    reader.add_region(
        RegionKind::Ram,
        runtime_region(RUNTIME_GENERATION),
        RUNTIME_ADDR,
    );
    let mut parser = flash_only(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");

    let newer = onerom.newer_generations();
    assert_eq!(newer.len(), 1, "unexpected report: {newer:?}");
    assert_eq!(newer[0].structure, "onerom_info_t");
    assert_eq!(newer[0].device_generation, INFO_GENERATION + 1);
    assert_eq!(newer[0].known_generation, INFO_GENERATION);

    // The fields this build does know are parsed and present regardless.
    let info = onerom.info().expect("info should be present");
    assert_eq!((info.major_version, info.minor_version), (0, 8));
    assert!(onerom.runtime().is_some());
}

#[test]
fn a_newer_runtime_structure_is_named_and_still_parsed() {
    let mut reader = MemoryReader::new(
        schema_image(INFO_GENERATION, RUNTIME_ADDR),
        RP235X_FLASH_BASE,
    );
    reader.add_region(
        RegionKind::Ram,
        runtime_region(RUNTIME_GENERATION + 1),
        RUNTIME_ADDR,
    );
    let mut parser = flash_only(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");

    let newer = onerom.newer_generations();
    assert_eq!(newer.len(), 1, "unexpected report: {newer:?}");
    assert_eq!(newer[0].structure, "onerom_runtime_info_t");
    assert_eq!(newer[0].device_generation, RUNTIME_GENERATION + 1);
    assert_eq!(newer[0].known_generation, RUNTIME_GENERATION);

    // The fields this build does know are parsed and present regardless.
    let runtime = onerom.runtime().expect("runtime should be present");
    assert_eq!(
        runtime.bit_mode,
        onerom_metadata::MaybeKnown::Known(onerom_metadata::BitModes::BitMode8)
    );
}

#[test]
fn a_structure_this_build_did_not_read_is_not_reported() {
    // No runtime region, so there are no runtime bytes to have a generation
    // and the only thing to say about it is that it is missing.
    let mut reader = MemoryReader::new(schema_image(INFO_GENERATION, 0), RP235X_FLASH_BASE);
    let mut parser = flash_only(&mut reader);
    let onerom = block_on(parser.parse_format_schema()).expect("image should parse");

    assert!(onerom.newer_generations().is_empty());
    assert_eq!(onerom.runtime_absence(), Some(RuntimeAbsence::NoPointer));
}
