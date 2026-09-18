// tests/generation_roundtrip.rs
//
// The generated writer and the generated parser, run against each other with
// a generation-gated field of every kind between them.
//
// No field of the shipped schema carries a generation marker, so this runs
// against build/gating_fixture.toml, whose Rust the build script generates
// into OUT_DIR.  Nothing else includes it.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

extern crate alloc;

use onerom_config::fw::FirmwareVersion;
use onerom_metadata::{DeviceMemoryView, MaybeKnown, ParseError, Pointer, SerializeError};

/// The fixture schema's generated types, parser and serializer.
///
/// The `use super::*` supplies the names the generated code needs, as
/// `src/lib.rs` does for the real one.  The generators emit a whole crate's
/// worth of surface and these tests reach a fraction of it, hence the blanket
/// `dead_code`.
#[allow(dead_code)]
mod fixture {
    use super::*;

    include!(concat!(env!("OUT_DIR"), "/gating_fixture_generated.rs"));
    include!(concat!(
        env!("OUT_DIR"),
        "/gating_fixture_serialize_generated.rs"
    ));
}

use fixture::{
    Generations, METADATA_BASE, METADATA_SIZE, OneromAlgConfig, OneromExtra, OneromHardwareInfo,
    OneromMetadataHeader, OneromMode, OneromPullConfig, OneromRomSlot, OneromTag, SerializeContext,
};

// ===========================================================================
// Composing and reading back
// ===========================================================================

/// Generation the fixture's gated fields arrived in, and the release that
/// brought it - `build/gating_fixture.toml` says both.
const GATED_GENERATION: u32 = 3;
const GATED_RELEASE: FirmwareVersion = FirmwareVersion::new(0, 8, 0, 0);

/// The generation before them, which carries none of them.
const OLDER_GENERATION: u32 = 2;

/// An address for the two pointer fields the parser stores without following.
const BLOB_ADDR: u32 = 0x1000_D000;
const HOOK_ADDR: u32 = 0x1000_E000;

/// Compose `header` into a metadata buffer.
///
/// The three steps `onerom_metadata::serialize` takes, spelled out because
/// that entry point is hand-written against the real schema.  The generation
/// comes from the header, as it does there.
fn compose(header: &OneromMetadataHeader) -> Result<alloc::vec::Vec<u8>, SerializeError> {
    let mut buf = alloc::vec![0u8; METADATA_SIZE];
    let generation = header.version;
    header.check_generation(generation)?;
    let mut ctx = SerializeContext::new(METADATA_BASE, generation, &mut buf);
    header.layout(&mut ctx)?;
    header.write(&mut ctx, METADATA_BASE);
    Ok(buf)
}

/// Read a composed buffer back, learning the generation from the bytes the
/// way a host does rather than being told it.
fn read_back(buf: &[u8]) -> Result<OneromMetadataHeader, ParseError> {
    let view = DeviceMemoryView::new(buf, METADATA_BASE);
    OneromMetadataHeader::parse(&view, METADATA_BASE, Generations::UNKNOWN)
}

/// A header carrying the values below, at `generation`.
///
/// Every gated field is at its declared default, so this composes at either
/// generation.  The tests that need a value the older generation cannot carry
/// change one field of what this returns.
fn header(generation: u32) -> OneromMetadataHeader {
    OneromMetadataHeader {
        version: generation,
        hw: OneromHardwareInfo {
            board_id: 4660,
            mode: MaybeKnown::Known(OneromMode::ModeFast),
            pin_count: 28,
            revision: 291,
            pins: [1, 2, 3, 4],
            matrix: [[9; 3]; 2],
        },
        slot_count: 0,
        turbo_boot: 1,
        slots: alloc::vec![OneromRomSlot {
            size: 8192,
            bank: 7,
            serve_mode: MaybeKnown::Known(OneromMode::ModeFast),
        }],
        build_name: None,
        extra: None,
        tags: alloc::vec![],
        tag_count: 0,
        blob: Pointer::Null,
        hook: Pointer::Null,
        alg: None,
        pulls: None,
        extra_count: 0,
        extras: alloc::vec![],
    }
}

/// The same header with every gated field holding something of its own,
/// which only the generation that brought them in can carry.
fn header_with_values() -> OneromMetadataHeader {
    OneromMetadataHeader {
        turbo_boot: 0,
        build_name: Some(alloc::string::String::from("fixture")),
        extra: Some(OneromExtra { value: 0xABCD }),
        tags: alloc::vec![OneromTag { id: 11 }, OneromTag { id: 22 }],
        tag_count: 2,
        blob: Pointer::Addr32(BLOB_ADDR),
        hook: Pointer::Addr32(HOOK_ADDR),
        alg: Some(OneromAlgConfig::AlgOne {
            clkdiv: 1000,
            pin: 5,
        }),
        pulls: Some(OneromPullConfig {
            params: alloc::vec![1, 2, 3],
        }),
        hw: OneromHardwareInfo {
            board_id: 0x0505,
            revision: 0x0808,
            pins: [10, 11, 12, 13],
            matrix: [[1, 2, 3], [4, 5, 6]],
            ..header(GATED_GENERATION).hw
        },
        slots: alloc::vec![OneromRomSlot {
            size: 8192,
            bank: 2,
            serve_mode: MaybeKnown::Known(OneromMode::ModeSlow),
        }],
        extra_count: 1,
        extras: alloc::vec![OneromExtra { value: 0x1234 }],
        ..header(GATED_GENERATION)
    }
}

// ===========================================================================
// The round trip
// ===========================================================================

/// Metadata composed for older firmware leaves out what that firmware cannot
/// read, and a reader of it gets the declared default in place of each, with
/// everything else as written.
#[test]
fn older_metadata_reads_back_as_the_declared_defaults() {
    // Values the older generation has nowhere to put.  They are written into
    // the Rust object and must not reach the bytes.
    let value = OneromMetadataHeader {
        version: OLDER_GENERATION,
        ..header(OLDER_GENERATION)
    };

    let buf = compose(&value).expect("every gated field is at its default");
    let read = read_back(&buf).expect("the composed buffer should parse");

    assert_eq!(read.version, OLDER_GENERATION);
    // A number comes back as the number the schema declares.
    assert_eq!(read.turbo_boot, 1);
    assert_eq!(read.hw.board_id, 4660);
    assert_eq!(read.hw.revision, 291);
    assert_eq!(read.slots[0].bank, 7);
    assert_eq!(
        read.slots[0].serve_mode,
        MaybeKnown::Known(OneromMode::ModeFast)
    );
    // An array comes back as the elements it declares, whether those were
    // stated one by one or as a fill.
    assert_eq!(read.hw.pins, [1, 2, 3, 4]);
    assert_eq!(read.hw.matrix, [[9; 3]; 2]);
    // A pointer comes back as nothing there, in whichever way its own type
    // says that.
    assert_eq!(read.build_name, None);
    assert_eq!(read.extra, None);
    assert!(read.tags.is_empty());
    assert_eq!(read.tag_count, 0);
    assert_eq!(read.blob, Pointer::Null);
    assert_eq!(read.hook, Pointer::Null);
    assert_eq!(read.alg, None);
    assert_eq!(read.pulls, None);
    assert!(read.extras.is_empty());
    assert_eq!(read.extra_count, 0);
    // Everything else comes back as it went in.
    assert_eq!(read.hw.mode, MaybeKnown::Known(OneromMode::ModeFast));
    assert_eq!(read.hw.pin_count, 28);
    assert_eq!(read.slots[0].size, 8192);
    assert_eq!(read.slot_count, 1);
}

/// At the generation the fields arrived in, the same header round trips with
/// the values it was given rather than the defaults.
#[test]
fn metadata_at_the_fields_own_generation_carries_their_values() {
    let value = header_with_values();

    let buf = compose(&value).expect("this generation carries every field");
    let read = read_back(&buf).expect("the composed buffer should parse");

    assert_eq!(read.version, GATED_GENERATION);
    assert_eq!(read.turbo_boot, 0);
    assert_eq!(read.hw.board_id, 0x0505);
    assert_eq!(read.hw.revision, 0x0808);
    assert_eq!(read.hw.pins, [10, 11, 12, 13]);
    assert_eq!(read.hw.matrix, [[1, 2, 3], [4, 5, 6]]);
    assert_eq!(read.slots[0].bank, 2);
    assert_eq!(
        read.slots[0].serve_mode,
        MaybeKnown::Known(OneromMode::ModeSlow)
    );
    assert_eq!(read.build_name.as_deref(), Some("fixture"));
    assert_eq!(read.extra, Some(OneromExtra { value: 0xABCD }));
    assert_eq!(
        read.tags,
        alloc::vec![OneromTag { id: 11 }, OneromTag { id: 22 }]
    );
    assert_eq!(read.tag_count, 2);
    assert_eq!(read.blob, Pointer::Addr32(BLOB_ADDR));
    assert_eq!(read.hook, Pointer::Addr32(HOOK_ADDR));
    assert_eq!(
        read.alg,
        Some(OneromAlgConfig::AlgOne {
            clkdiv: 1000,
            pin: 5,
        })
    );
    assert_eq!(
        read.pulls,
        Some(OneromPullConfig {
            params: alloc::vec![1, 2, 3],
        })
    );
    assert_eq!(read.extras, alloc::vec![OneromExtra { value: 0x1234 }]);
    assert_eq!(read.extra_count, 1);
}

/// A field the composed generation predates is not written at all, so its
/// bytes read back as the 0xFF a device's unwritten metadata holds.  Writing
/// the default there instead would look like a value somebody chose.
#[test]
fn a_field_left_out_keeps_the_unwritten_byte() {
    let buf = compose(&header(OLDER_GENERATION)).expect("every gated field is at its default");

    // turbo_boot sits at offset 9 of the header, which is at METADATA_BASE,
    // and build_name - a pointer - at 16.
    assert_eq!(buf[9], 0xFF);
    assert_eq!(buf[16..20], [0xFF; 4]);
    // slot_count, the byte before turbo_boot, is written - so this is the gate
    // at work rather than a buffer nothing reached.
    assert_eq!(buf[8], 1);
}

/// An array's bytes sit in the structure whatever the generation, so what a
/// skipped one leaves behind is the buffer's own fill rather than the default
/// a reader is handed.
#[test]
fn a_skipped_array_keeps_the_unwritten_bytes() {
    let buf = compose(&header(OLDER_GENERATION)).expect("every gated field is at its default");
    let read = read_back(&buf).expect("the composed buffer should parse");

    // The hardware structure is laid out after the header, and pins sits 6
    // bytes into it.  Finding it through the pointer keeps this test off the
    // serializer's allocation order.
    let hw_addr = u32::from_le_bytes(buf[4..8].try_into().expect("four bytes")) as usize;
    let at = hw_addr - METADATA_BASE as usize + 6;
    assert_eq!(buf[at..at + 4], [0xFF; 4]);
    assert_eq!(read.hw.pins, [1, 2, 3, 4]);
}

/// The offsets of everything after a skipped field are unmoved: a field's
/// place in the layout is fixed whether or not this generation writes it.
#[test]
fn a_skipped_field_does_not_move_what_follows_it() {
    let older = compose(&header(OLDER_GENERATION)).expect("composes at the older generation");
    let newer = compose(&header(GATED_GENERATION)).expect("composes at the newer generation");

    // The slot array pointer follows turbo_boot in the header, and the two
    // buffers were laid out the same way, so it sits at the same offset and
    // points at the same address.
    assert_eq!(older[12..16], newer[12..16]);
}

// ===========================================================================
// Refusals
// ===========================================================================

/// Every gated field of the header, one at a time, holding something the
/// composed generation cannot express.
#[test]
fn a_header_field_the_generation_cannot_carry_is_refused() {
    let old = |f: fn(&mut OneromMetadataHeader)| {
        let mut value = header(OLDER_GENERATION);
        f(&mut value);
        compose(&value)
    };
    let too_new = |field| {
        Err(SerializeError::FieldTooNew {
            field,
            minimum: GATED_RELEASE,
        })
    };

    assert_eq!(
        old(|v| v.turbo_boot = 0),
        too_new("onerom_metadata_header_t.turbo_boot")
    );
    assert_eq!(
        old(|v| v.build_name = Some(alloc::string::String::from("fixture"))),
        too_new("onerom_metadata_header_t.build_name")
    );
    assert_eq!(
        old(|v| v.extra = Some(OneromExtra { value: 1 })),
        too_new("onerom_metadata_header_t.extra")
    );
    assert_eq!(
        old(|v| v.tags = alloc::vec![OneromTag { id: 11 }]),
        too_new("onerom_metadata_header_t.tags")
    );
    assert_eq!(
        old(|v| v.tag_count = 1),
        too_new("onerom_metadata_header_t.tag_count")
    );
    assert_eq!(
        old(|v| v.blob = Pointer::Addr32(BLOB_ADDR)),
        too_new("onerom_metadata_header_t.blob")
    );
    assert_eq!(
        old(|v| v.hook = Pointer::Addr32(HOOK_ADDR)),
        too_new("onerom_metadata_header_t.hook")
    );
    assert_eq!(
        old(|v| v.alg = Some(OneromAlgConfig::AlgOne {
            clkdiv: 1000,
            pin: 5,
        })),
        too_new("onerom_metadata_header_t.alg")
    );
    assert_eq!(
        old(|v| v.pulls = Some(OneromPullConfig {
            params: alloc::vec![1],
        })),
        too_new("onerom_metadata_header_t.pulls")
    );
    assert_eq!(
        old(|v| v.extras = alloc::vec![OneromExtra { value: 1 }]),
        too_new("onerom_metadata_header_t.extras")
    );
    assert_eq!(
        old(|v| v.extra_count = 1),
        too_new("onerom_metadata_header_t.extra_count")
    );
}

/// The same, on a structure the header points at, where the arrays are.
#[test]
fn a_field_of_a_pointed_at_structure_is_refused() {
    let old = |f: fn(&mut OneromHardwareInfo)| {
        let mut value = header(OLDER_GENERATION);
        f(&mut value.hw);
        compose(&value)
    };
    let too_new = |field| {
        Err(SerializeError::FieldTooNew {
            field,
            minimum: GATED_RELEASE,
        })
    };

    assert_eq!(
        old(|v| v.board_id = 0x0505),
        too_new("onerom_hardware_info_t.board_id")
    );
    assert_eq!(
        old(|v| v.revision = 0x0808),
        too_new("onerom_hardware_info_t.revision")
    );
    assert_eq!(
        old(|v| v.pins = [10, 11, 12, 13]),
        too_new("onerom_hardware_info_t.pins")
    );
    assert_eq!(
        old(|v| v.matrix = [[1, 2, 3], [4, 5, 6]]),
        too_new("onerom_hardware_info_t.matrix")
    );
}

/// And on an element of the header's ROM slot array, which the walk reaches
/// through the array rather than through a pointer field.
#[test]
fn a_field_of_an_array_element_is_refused() {
    let mut value = header(OLDER_GENERATION);
    value.slots[0].bank = 2;

    assert_eq!(
        compose(&value),
        Err(SerializeError::FieldTooNew {
            field: "onerom_rom_slot_t.bank",
            minimum: GATED_RELEASE,
        })
    );

    let mut value = header(OLDER_GENERATION);
    value.slots[0].serve_mode = MaybeKnown::Known(OneromMode::ModeSlow);

    assert_eq!(
        compose(&value),
        Err(SerializeError::FieldTooNew {
            field: "onerom_rom_slot_t.serve_mode",
            minimum: GATED_RELEASE,
        })
    );
}

/// A value equal to the declared default asks for nothing the older
/// generation cannot give, so it is accepted and simply left out.
#[test]
fn a_gated_field_at_its_default_is_accepted() {
    assert!(compose(&header(OLDER_GENERATION)).is_ok());
}

#[test]
fn nothing_is_refused_at_the_generation_that_carries_the_fields() {
    assert!(compose(&header_with_values()).is_ok());
}
