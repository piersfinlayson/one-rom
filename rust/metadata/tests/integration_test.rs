// tests/integration_test.rs
//
// Round-trip and structural tests for the onerom_metadata crate.
//
// The serializer and parser are treated as black boxes.  Tests 1–8 validate
// the combined round-trip (construct → serialize → parse → compare).  Tests
// 9–13 inspect raw buffer bytes to verify behavioural contracts that parse
// alone cannot confirm.  Tests 14–15 exercise the error paths.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_config::chip::ChipType;
use onerom_config::mcu::{RP235X_BASE_FLASH, RP235X_BASE_SRAM, RP235X_END_SRAM};
use onerom_metadata::{
    BitModes, CURRENT_METADATA_VERSION, DeviceMemoryView, FireVreg, GPIO_NONE, OneromAlgAddrConfig,
    OneromAlgConfig, OneromAlgCsConfig, OneromAlgDataConfig, OneromAlgDmaConfig,
    OneromAlgOverrideConfig, OneromAlgPullConfig, OneromFirmwareConfig, OneromFirmwareOverrides,
    OneromHardwareInfo, OneromMetadataHeader, OneromRomInfo, OneromRomPinMap, OneromRomSlot,
    Pointer, RomSlotType, Rp235xVariant, generate_host_metadata_c,
};
use onerom_metadata::{METADATA_BASE, METADATA_SIZE, SerializeError, serialize};

// ===========================================================================
// Buffer helpers
// ===========================================================================

fn read_u32_le(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap())
}

/// Translate a flash-address pointer to a slice offset relative to `base`.
fn ptr_to_off(ptr: u32, base: u32) -> usize {
    (ptr - base) as usize
}

// ===========================================================================
// Construction helpers
// ===========================================================================

fn default_pin_map() -> Option<OneromRomPinMap> {
    Some(OneromRomPinMap {
        addr: [GPIO_NONE; 24],
        data: [GPIO_NONE; 16],
    })
}

fn make_rom_info(rom_type: &str) -> OneromRomInfo {
    let chip =
        ChipType::try_from_str(rom_type).unwrap_or_else(|| panic!("unknown chip type: {rom_type}"));
    OneromRomInfo {
        rom_type: rom_type.into(),
        filename: None,
        pin_map: default_pin_map(),
        chip_size: chip.size_bytes() as u32,
        rbcp_rom_type: chip.rbcp_chip_type(),
    }
}

fn default_alg() -> Option<OneromAlgConfig> {
    Some(OneromAlgConfig {
        alg_cs: OneromAlgCsConfig::AlgCs0 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            base_cs_pin: 0,
            num_cs_pins: 1,
            base_data_pin: 8,
            num_data_pins: 8,
            cs_active_delay: 0,
            cs_inactive_delay: 0,
            serve_cs_low_0: 1,
            byte_pin: GPIO_NONE,
            first_rom_cs_base: 0,
            first_rom_num_cs_pins: 1,
        },
        alg_addr: OneromAlgAddrConfig::AlgAddr0 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            num_delay_cycles: 2,
            base_addr_pin: 0,
            num_addr_pins: 13,
            num_rom_table_bits: 13,
        },
        alg_data: OneromAlgDataConfig::AlgData0 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            base_data_pin: 8,
            word_size: 8,
        },
        alg_dma: OneromAlgDmaConfig::AlgDma0 {
            bit_mode: BitModes::BitMode8,
            continuous: 1,
        },
        gpio_pull_config: None,
        gpio_override_config: None,
    })
}

fn make_slot(roms: Vec<OneromRomInfo>) -> OneromRomSlot {
    OneromRomSlot {
        data: Pointer::Null,
        size: 8192,
        roms,
        rom_count: 0,
        slot_type: RomSlotType::RomSlotTypeSingleRom,
        alg: default_alg(),
        firmware_overrides: None,
    }
}

/// Construct the minimal valid header exactly as specified in the test brief.
fn minimal_header() -> OneromMetadataHeader {
    let pin_map = OneromRomPinMap {
        addr: [GPIO_NONE; 24],
        data: [GPIO_NONE; 16],
    };
    let chip_2364 = ChipType::Chip2364;
    let rom_info = OneromRomInfo {
        rom_type: "2364".into(),
        filename: None,
        pin_map: Some(pin_map),
        chip_size: chip_2364.size_bytes() as u32,
        rbcp_rom_type: chip_2364.rbcp_chip_type(),
    };
    let alg = OneromAlgConfig {
        alg_cs: OneromAlgCsConfig::AlgCs0 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            base_cs_pin: 0,
            num_cs_pins: 1,
            base_data_pin: 8,
            num_data_pins: 8,
            cs_active_delay: 0,
            cs_inactive_delay: 0,
            serve_cs_low_0: 1,
            byte_pin: GPIO_NONE,
            first_rom_cs_base: 0,
            first_rom_num_cs_pins: 1,
        },
        alg_addr: OneromAlgAddrConfig::AlgAddr0 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            num_delay_cycles: 2,
            base_addr_pin: 0,
            num_addr_pins: 13,
            num_rom_table_bits: 13,
        },
        alg_data: OneromAlgDataConfig::AlgData0 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            base_data_pin: 8,
            word_size: 8,
        },
        alg_dma: OneromAlgDmaConfig::AlgDma0 {
            bit_mode: BitModes::BitMode8,
            continuous: 1,
        },
        gpio_pull_config: None,
        gpio_override_config: None,
    };
    let slot = OneromRomSlot {
        data: Pointer::Null,
        size: 8192,
        roms: vec![rom_info],
        rom_count: 0,
        slot_type: RomSlotType::RomSlotTypeSingleRom,
        alg: Some(alg),
        firmware_overrides: None,
    };
    let hw = OneromHardwareInfo {
        hw_rev: "1.0".into(),
        rp235x: Rp235xVariant::Rp235xb,
        num_phys_pins: 28,
        usb_capable: 1,
        gpio_vbus: GPIO_NONE,
        gpio_ext_flash_cs: GPIO_NONE,
        gpio_status: 25,
        gpio_neopixel: GPIO_NONE,
        gpio_swdio: GPIO_NONE,
        gpio_swclk: GPIO_NONE,
        gpio_sel: [GPIO_NONE; 7],
        sel_jumper_pull: 0,
        gpio_from_phys_pin: [[GPIO_NONE; 2]; 40],
        gpio_x1: [GPIO_NONE; 2],
        gpio_x2: [GPIO_NONE; 2],
    };
    let fw = OneromFirmwareConfig {
        name: None,
        serial_number: None,
    };
    OneromMetadataHeader {
        magic: *b"ONEROM_METADATA\0",
        version: CURRENT_METADATA_VERSION,
        hw,
        fw,
        rom_slot_count: 0,
        boot_logging: 0,
        swd_enabled: 0,
        turbo_boot: 0,
        rom_slots: vec![slot],
    }
}

/// One `Vec<u8>` of small dummy ROM image bytes per slot.
///
/// `generate_host_metadata_c` needs one entry per ROM slot.  For structural
/// and compile tests we just need valid (non-zero-length) byte arrays; the
/// actual content doesn't matter.
fn dummy_rom_data(n_slots: usize) -> Vec<Vec<u8>> {
    (0..n_slots).map(|_| vec![0xAAu8; 8]).collect()
}

// ===========================================================================
// Serialize / round-trip helpers
// ===========================================================================

/// Serialize `header` into a freshly allocated METADATA_SIZE buffer.
fn do_serialize(header: &OneromMetadataHeader) -> Vec<u8> {
    let mut buf = vec![0u8; METADATA_SIZE];
    serialize(header, METADATA_BASE, &mut buf).expect("serialize failed");
    buf
}

/// Full round-trip: serialize then parse, returning the reconstructed header.
fn round_trip(header: &OneromMetadataHeader) -> OneromMetadataHeader {
    let buf = do_serialize(header);
    let view = DeviceMemoryView::new(&buf, METADATA_BASE);
    OneromMetadataHeader::parse(&view, METADATA_BASE).expect("parse failed")
}

/// Set the derived count fields to their correct values (Vec lengths).
///
/// The serializer ignores `rom_slot_count` and `rom_count` on input and
/// always writes the Vec lengths into the binary.  The parser then reads
/// those written values back, so after a round-trip those fields reflect
/// the actual counts rather than whatever the caller originally set.
/// Calling this before comparing the original against the round-trip result
/// brings the two into alignment without masking any other mismatch.
fn normalize_counts(h: &mut OneromMetadataHeader) {
    h.rom_slot_count = h.rom_slots.len() as u8;
    for slot in &mut h.rom_slots {
        slot.rom_count = slot.roms.len() as u8;
    }
}

/// Serialize `original`, parse it back, and assert equality — accounting for
/// the derived count fields that the parser always repopulates from the binary.
fn assert_round_trips(original: &OneromMetadataHeader) {
    let mut expected = original.clone();
    normalize_counts(&mut expected);
    assert_eq!(expected, round_trip(original));
}

// ===========================================================================
// Round-trip tests (1–8)
// ===========================================================================

/// 1. Smoke test: minimal valid header serializes and parses to an equal value.
#[test]
fn round_trip_minimal() {
    let original = minimal_header();
    assert_round_trips(&original);
}

/// 2. Optional string fields (name, serial_number, filename) and
///    OneromFirmwareOverrides all present.
#[test]
fn round_trip_optional_fields() {
    let fw = OneromFirmwareConfig {
        name: Some("MyUnit".into()),
        serial_number: Some("SN-0042".into()),
    };
    let chip_2364 = ChipType::Chip2364;
    let rom = OneromRomInfo {
        rom_type: "2364".into(),
        filename: Some("basic.rom".into()),
        pin_map: default_pin_map(),
        chip_size: chip_2364.size_bytes() as u32,
        rbcp_rom_type: chip_2364.rbcp_chip_type(),
    };
    let fw_overrides = OneromFirmwareOverrides {
        // Byte 0 bit 2 = fire-freq override present.
        override_present: [0x04, 0, 0, 0, 0, 0, 0, 0],
        ice_freq: 0,
        fire_freq: 0,
        fire_vreg: FireVreg::FireVregNone,
        override_value: [0; 8],
    };
    let slot = OneromRomSlot {
        firmware_overrides: Some(fw_overrides),
        ..make_slot(vec![rom])
    };
    let original = OneromMetadataHeader {
        fw,
        rom_slots: vec![slot],
        ..minimal_header()
    };
    assert_round_trips(&original);
}

/// 3. Three slots with distinct sizes and slot types.
#[test]
fn round_trip_multiple_slots() {
    let slot_a = make_slot(vec![make_rom_info("2364")]);
    let slot_b = OneromRomSlot {
        size: 16384,
        slot_type: RomSlotType::RomSlotTypeMultiRom,
        ..make_slot(vec![make_rom_info("27128")])
    };
    let slot_c = OneromRomSlot {
        size: 32768,
        slot_type: RomSlotType::RomSlotTypeBankedRom,
        ..make_slot(vec![make_rom_info("27256")])
    };
    let original = OneromMetadataHeader {
        rom_slots: vec![slot_a, slot_b, slot_c],
        ..minimal_header()
    };
    assert_round_trips(&original);
}

/// 4. A single slot carrying two ROM images.
#[test]
fn round_trip_multiple_roms_per_slot() {
    let roms = vec![make_rom_info("2364"), make_rom_info("27128")];
    let original = OneromMetadataHeader {
        rom_slots: vec![make_slot(roms)],
        ..minimal_header()
    };
    assert_round_trips(&original);
}

/// 5a. CS algorithm variant 1 (non-contiguous single-gap).
#[test]
fn round_trip_alg_cs1() {
    let alg = OneromAlgConfig {
        alg_cs: OneromAlgCsConfig::AlgCs1 {
            clkdiv_int: 2,
            clkdiv_frac: 0,
            gpio_base: 0,
            base_cs_pin: 0,
            num_cs_pins: 2,
            base_data_pin: 8,
            num_data_pins: 8,
            cs_active_delay: 1,
            cs_inactive_delay: 1,
            cs_ignore_index: 1,
        },
        ..default_alg().unwrap()
    };
    let original = OneromMetadataHeader {
        rom_slots: vec![OneromRomSlot {
            alg: Some(alg),
            ..make_slot(vec![make_rom_info("2364")])
        }],
        ..minimal_header()
    };
    assert_round_trips(&original);
}

/// 5b. CS algorithm variant 2 (enable/address-qualified).
#[test]
fn round_trip_alg_cs2() {
    let alg = OneromAlgConfig {
        alg_cs: OneromAlgCsConfig::AlgCs2 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            base_cs_pin: 0,
            num_cs_pins: 1,
            base_data_pin: 8,
            num_data_pins: 8,
            cs_active_delay: 0,
            cs_inactive_delay: 0,
            base_qualifier_pin: 2,
            num_qualifier_pins: 2,
            qualifier_inactive_pattern: 0b11,
        },
        ..default_alg().unwrap()
    };
    let original = OneromMetadataHeader {
        rom_slots: vec![OneromRomSlot {
            alg: Some(alg),
            ..make_slot(vec![make_rom_info("2364")])
        }],
        ..minimal_header()
    };
    assert_round_trips(&original);
}

/// 6. Data algorithm variant 1 (byte-mode, with byte_pin and a_minus_1_pin).
#[test]
fn round_trip_alg_data1() {
    let alg = OneromAlgConfig {
        alg_data: OneromAlgDataConfig::AlgData1 {
            clkdiv_int: 1,
            clkdiv_frac: 0,
            gpio_base: 0,
            base_data_pin: 0,
            word_size: 16,
            byte_pin: 5,
            a_minus_1_pin: 6,
        },
        ..default_alg().unwrap()
    };
    let original = OneromMetadataHeader {
        rom_slots: vec![OneromRomSlot {
            alg: Some(alg),
            ..make_slot(vec![make_rom_info("2364")])
        }],
        ..minimal_header()
    };
    assert_round_trips(&original);
}

/// 7. Both simple FAMs (gpio_pull_config and gpio_override_config) present
///    with non-empty params.
#[test]
fn round_trip_simple_fams() {
    let alg = OneromAlgConfig {
        // MSB=1 → pull-up; lower 7 bits = GPIO number.
        gpio_pull_config: Some(OneromAlgPullConfig {
            params: vec![0x85, 0x86], // pull-up GPIO 5, pull-up GPIO 6
        }),
        // Top 2 bits = override mode (gpio_override_t); lower 6 = GPIO number.
        gpio_override_config: Some(OneromAlgOverrideConfig {
            params: vec![0x47], // mode 1 (invert), GPIO 7
        }),
        ..default_alg().unwrap()
    };
    let original = OneromMetadataHeader {
        rom_slots: vec![OneromRomSlot {
            alg: Some(alg),
            ..make_slot(vec![make_rom_info("2364")])
        }],
        ..minimal_header()
    };
    assert_round_trips(&original);
}

/// 8. Two slots whose rom_type strings are identical; the round-trip must
///    reconstruct equal values regardless of whether the serializer deduplicates.
#[test]
fn round_trip_string_reuse() {
    let original = OneromMetadataHeader {
        rom_slots: vec![
            make_slot(vec![make_rom_info("2364")]),
            make_slot(vec![make_rom_info("2364")]),
        ],
        ..minimal_header()
    };
    assert_round_trips(&original);
}

// ===========================================================================
// Structural byte checks (9–13)
// ===========================================================================

// Header layout (offsets into buf, which starts at METADATA_BASE):
//   0..16  magic [u8; 16]
//  16..20  version u32
//  20..24  hw ptr
//  24..28  fw ptr
//  28      rom_slot_count u8
//  29      boot_logging u8
//  30      swd_enabled u8
//  31      turbo_boot u8
//  32..36  rom_slots ptr
//  36..256 reserved (220 bytes, must be 0xFF)
//
// OneromRomSlot layout (32 bytes, offsets relative to slot start):
//   0..4   data (opaque_ptr, u32)
//   4..8   size u32
//   8..12  roms ptr (struct_ptr_array_ptr)
//  12      rom_count u8
//  13      slot_type u8
//  14..16  reserved1 (2 bytes)
//  16..20  alg ptr
//  20..24  firmware_overrides ptr
//  24..32  reserved2 (8 bytes)
//
// OneromAlgConfig layout (32 bytes, offsets relative to alg start):
//   0..4   alg_cs ptr
//   4..8   alg_addr ptr
//   8..12  alg_data ptr
//  12..16  alg_dma ptr
//  16..20  gpio_pull_config ptr
//  20..24  gpio_override_config ptr
//  24..32  reserved (8 bytes)

/// 9. Both derived count fields (rom_slot_count in the header and rom_count in
///    the slot) are written from the corresponding Vec length, regardless of the
///    field values set on the Rust structs.
#[test]
fn byte_check_count_fields_derived_from_vec() {
    let slot = OneromRomSlot {
        rom_count: 99, // wrong; should be overridden to roms.len() = 1
        ..make_slot(vec![make_rom_info("2364")])
    };
    let header = OneromMetadataHeader {
        rom_slot_count: 42, // wrong; should be overridden to rom_slots.len() = 1
        rom_slots: vec![slot],
        ..minimal_header()
    };
    let buf = do_serialize(&header);

    // rom_slot_count at header offset 28.
    assert_eq!(
        buf[28], 1,
        "rom_slot_count should equal rom_slots.len() (1), not 42",
    );

    // rom_count at slot offset 12.
    let slots_ptr = read_u32_le(&buf, 32);
    let slots_off = ptr_to_off(slots_ptr, METADATA_BASE);
    assert_eq!(
        buf[slots_off + 12],
        1,
        "rom_count should equal roms.len() (1), not 99",
    );
}

/// 10. opaque_ptr fields are copied verbatim into the buffer; the serializer
///     must not interpret or redirect them.
#[test]
fn byte_check_opaque_ptr_verbatim() {
    let slot = OneromRomSlot {
        data: Pointer::Addr32(0xDEAD_BEEF),
        ..make_slot(vec![make_rom_info("2364")])
    };
    let header = OneromMetadataHeader {
        rom_slots: vec![slot],
        ..minimal_header()
    };
    let buf = do_serialize(&header);

    // Follow the rom_slots pointer from the header.
    let slots_ptr = read_u32_le(&buf, 32);
    let slots_off = ptr_to_off(slots_ptr, METADATA_BASE);

    // data is the first u32 (offset 0) within the 32-byte slot.
    let written = read_u32_le(&buf, slots_off);
    assert_eq!(
        written, 0xDEAD_BEEF,
        "opaque_ptr data field must be written verbatim"
    );
}

/// 11. Two slots with equal OneromAlgConfig (by value) share a single copy in
///     the buffer — both slots' alg pointer fields hold the same address.
#[test]
fn byte_check_object_dedup() {
    let alg = default_alg();
    let slot_a = OneromRomSlot {
        alg: alg.clone(),
        ..make_slot(vec![make_rom_info("2364")])
    };
    let slot_b = OneromRomSlot {
        alg,
        ..make_slot(vec![make_rom_info("27128")])
    };
    let header = OneromMetadataHeader {
        rom_slots: vec![slot_a, slot_b],
        ..minimal_header()
    };
    let buf = do_serialize(&header);

    // Locate the contiguous slots array.
    let slots_ptr = read_u32_le(&buf, 32);
    let slots_off = ptr_to_off(slots_ptr, METADATA_BASE);

    // alg pointer at offset 16 within each 32-byte slot.
    let alg_ptr_a = read_u32_le(&buf, slots_off + 16);
    let alg_ptr_b = read_u32_le(&buf, slots_off + 32 + 16);

    assert_eq!(
        alg_ptr_a, alg_ptr_b,
        "identical alg configs must share one object: \
         slot_a.alg → 0x{alg_ptr_a:08X}, slot_b.alg → 0x{alg_ptr_b:08X}",
    );
}

/// 12. The 220-byte reserved region in OneromMetadataHeader (offsets 36–255)
///     is all 0xFF after serialization.
#[test]
fn byte_check_header_padding_is_ff() {
    let buf = do_serialize(&minimal_header());
    #[allow(clippy::needless_range_loop)]
    for offset in 36..256 {
        assert_eq!(
            buf[offset], 0xFF,
            "header reserved byte at offset {offset} should be 0xFF, got 0x{:02X}",
            buf[offset],
        );
    }
}

/// 13. Every non-null pointer stored in the buffer is 4-byte aligned.
///     Covers header pointers, slot pointers, and alg sub-pointers.
#[test]
fn byte_check_pointer_alignment() {
    let buf = do_serialize(&minimal_header());

    // Returns true if ptr should be checked for alignment (non-null, non-sentinel).
    let is_valid_ptr = |ptr: u32| ptr != 0 && ptr != 0xFFFF_FFFF;

    let check_aligned = |label: &str, ptr: u32| {
        if is_valid_ptr(ptr) {
            assert_eq!(
                ptr % 4,
                0,
                "{label}: flash pointer 0x{ptr:08X} is not 4-byte aligned",
            );
        }
    };

    // Header-level pointers.
    let hw_ptr = read_u32_le(&buf, 20);
    let fw_ptr = read_u32_le(&buf, 24);
    let slots_ptr = read_u32_le(&buf, 32);
    check_aligned("hw", hw_ptr);
    check_aligned("fw", fw_ptr);
    check_aligned("rom_slots", slots_ptr);

    // Slot-level pointers (first slot only).
    let slots_off = ptr_to_off(slots_ptr, METADATA_BASE);
    let roms_ptr = read_u32_le(&buf, slots_off + 8);
    let alg_ptr = read_u32_le(&buf, slots_off + 16);
    let fw_ovr_ptr = read_u32_le(&buf, slots_off + 20);
    check_aligned("slot.roms", roms_ptr);
    check_aligned("slot.alg", alg_ptr);
    check_aligned("slot.firmware_overrides", fw_ovr_ptr);

    // AlgConfig sub-pointers.
    let alg_off = ptr_to_off(alg_ptr, METADATA_BASE);
    check_aligned("alg.alg_cs", read_u32_le(&buf, alg_off));
    check_aligned("alg.alg_addr", read_u32_le(&buf, alg_off + 4));
    check_aligned("alg.alg_data", read_u32_le(&buf, alg_off + 8));
    check_aligned("alg.alg_dma", read_u32_le(&buf, alg_off + 12));
    check_aligned("alg.gpio_pull_config", read_u32_le(&buf, alg_off + 16));
    check_aligned("alg.gpio_override_config", read_u32_le(&buf, alg_off + 20));
}

// ===========================================================================
// Error path tests (14–15)
// ===========================================================================

/// 14. A buffer that is too small for even the root header returns Overflow.
#[test]
fn error_overflow() {
    let header = minimal_header();
    // 64 bytes is far smaller than the 256-byte root struct alone.
    let mut buf = vec![0u8; 64];
    assert_eq!(
        serialize(&header, METADATA_BASE, &mut buf),
        Err(SerializeError::Overflow),
    );
}

/// 15. A rom_slots Vec with 256 entries (one above the u8 maximum of 255)
///     returns CountOverflow naming the "rom_slot_count" field.
#[test]
fn error_count_overflow() {
    let mut header = minimal_header();
    header.rom_slots = (0..256)
        .map(|_| make_slot(vec![make_rom_info("2364")]))
        .collect();
    let mut buf = vec![0u8; METADATA_SIZE];
    assert_eq!(
        serialize(&header, METADATA_BASE, &mut buf),
        Err(SerializeError::CountOverflow {
            field: "rom_slot_count"
        }),
    );
}

// ---------------------------------------------------------------------------
// Host C generation tests (16–18) — append at the end of the file
// ---------------------------------------------------------------------------

/// 16. Smoke: `generate_host_metadata_c` returns a non-empty string without
///     panicking for the minimal valid header.
#[test]
fn host_c_gen_smoke() {
    let header = minimal_header();
    // minimal_header() has exactly one ROM slot.
    let c_src = generate_host_metadata_c(&header, dummy_rom_data(1));
    assert!(!c_src.is_empty(), "generated C source should not be empty");
}

/// 17. Structural: key patterns expected in the generated C for the minimal
///     header are all present.
#[test]
fn host_c_gen_structural() {
    let header = minimal_header();
    let c_src = generate_host_metadata_c(&header, dummy_rom_data(1));

    // Root symbol must be defined (not forward-declared — globals.c owns that).
    assert!(
        c_src.contains("_metadata_start"),
        "missing `_metadata_start` definition"
    );

    // Forward declarations section.
    assert!(
        c_src.contains("extern const"),
        "missing `extern const` forward declarations"
    );

    // String field values from minimal_header.
    assert!(
        c_src.contains("\"2364\""),
        "missing rom_type string literal \"2364\""
    );
    assert!(
        c_src.contains("\"1.0\""),
        "missing hw_rev string literal \"1.0\""
    );

    // NULL for None pointer fields (name, serial_number, filename all absent).
    assert!(
        c_src.contains("= NULL"),
        "missing `= NULL` for null pointer fields"
    );

    // FAM designated-initialiser pragma wrapper must be present.
    assert!(
        c_src.contains("#pragma GCC diagnostic push"),
        "missing `#pragma GCC diagnostic push` for FAM structs"
    );
    assert!(
        c_src.contains("-Wpedantic"),
        "missing `-Wpedantic` in FAM pragma"
    );

    // Enum C constant for RomSlotType::RomSlotTypeSingleRom.
    // NOTE: this is the C-side constant name from the schema TOML `name`
    // field; adjust if the actual name differs.
    assert!(
        c_src.contains("ROM_SLOT_TYPE_SINGLE_ROM"),
        "missing `ROM_SLOT_TYPE_SINGLE_ROM` enum constant"
    );
}

/// 18. Compile: the generated C compiles cleanly under
///     `gcc -std=c99 -Wall -Wextra -Wpedantic` (Linux/macOS only).
///
/// Skips gracefully if `gcc` is not in PATH or if `onerom_metadata.h` has
/// not yet been written by the build script.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn host_c_gen_compiles() {
    use std::process::Command;

    // Locate onerom_metadata.h.  The build script writes it to
    // firmware/generated/ relative to the project root, which is two levels
    // above CARGO_MANIFEST_DIR (rust/metadata → rust → root).
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR should be two levels below the project root");
    let include_dir = project_root.join("firmware").join("generated");
    let firmware_include_dir = project_root.join("firmware").join("include");

    if !include_dir.join("onerom_metadata.h").exists() {
        eprintln!(
            "host_c_gen_compiles: skipping — onerom_metadata.h not found at {}",
            include_dir.display()
        );
        return;
    }

    let header = minimal_header();
    let c_src = generate_host_metadata_c(&header, dummy_rom_data(1));

    // Write generated C to a uniquely-named temp file.
    let pid = std::process::id();
    let tmp = std::env::temp_dir();
    let c_path = tmp.join(format!("onerom_host_gen_{pid}.c"));
    let o_path = tmp.join(format!("onerom_host_gen_{pid}.o"));

    std::fs::write(&c_path, &c_src).expect("failed to write temp C source file");

    let result = Command::new("gcc")
        .args([
            "-std=c99",
            "-Wall",
            "-Wextra",
            "-Wpedantic",
            "-c",
            "-I",
            &include_dir.to_string_lossy(),
            "-I",
            &firmware_include_dir.to_string_lossy(),
            "-D",
            "TEST_BUILD",
            "-o",
            &o_path.to_string_lossy(),
            &c_path.to_string_lossy(),
        ])
        .output();

    // Clean up temp files regardless of outcome.
    let _ = std::fs::remove_file(&c_path);
    let _ = std::fs::remove_file(&o_path);

    match result {
        Err(e) => {
            // gcc not in PATH — skip rather than fail.
            eprintln!("host_c_gen_compiles: skipping — gcc not available: {e}");
        }
        Ok(output) => {
            assert!(
                output.status.success(),
                "generated C failed to compile:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Pointer unit tests (19–25)
// ---------------------------------------------------------------------------
//
// Add to the imports at the top of integration_test.rs:
//
//   use onerom_metadata::{
//       Pointer, RP235X_BASE_FLASH, RP235X_BASE_SRAM, RP235X_END_SRAM,
//       ...existing imports...
//   };

/// 19. Pointer::new maps both null sentinels (0 and 0xFFFF_FFFF) to Pointer::Null
///     and any other value to Pointer::Addr32.
#[test]
fn pointer_new_sentinels() {
    assert_eq!(Pointer::new(0), Pointer::Null);
    assert_eq!(Pointer::new(0xFFFF_FFFF), Pointer::Null);
    assert_eq!(Pointer::new(0x1000_8000), Pointer::Addr32(0x1000_8000));
    assert_eq!(Pointer::new(0x0000_0001), Pointer::Addr32(0x0000_0001));
}

/// 20. is_null() returns true only for Pointer::Null.
#[test]
fn pointer_is_null() {
    assert!(Pointer::Null.is_null());
    assert!(!Pointer::Addr32(0x1000_0000).is_null());
}

/// 21. addr() returns None for Null, Some(a) for Addr32.
#[test]
fn pointer_addr() {
    assert_eq!(Pointer::Null.addr(), None);
    assert_eq!(Pointer::Addr32(0xABCD_1234).addr(), Some(0xABCD_1234));
}

/// 22. raw() returns 0 for Null and the stored address for Addr32.
///     This is what the serializer uses when writing opaque_ptr fields.
#[test]
fn pointer_raw() {
    assert_eq!(Pointer::Null.raw(), 0);
    assert_eq!(Pointer::Addr32(0xDEAD_BEEF).raw(), 0xDEAD_BEEF);
}

/// 23. is_flash() is true for addresses within the RP235x XIP flash region
///     [RP235X_BASE_FLASH, RP235X_END_FLASH) and false otherwise.
#[test]
fn pointer_is_flash() {
    // Base address — first flash byte.
    assert!(Pointer::Addr32(RP235X_BASE_FLASH).is_flash());
    // Mid-flash address.
    assert!(Pointer::Addr32(0x1000_C000).is_flash());
    // One byte before the exclusive end.
    assert!(Pointer::Addr32(0x1FFF_FFFF).is_flash());
    // Exactly at the exclusive end — not flash.
    assert!(!Pointer::Addr32(0x2000_0000).is_flash());
    // SRAM address — not flash.
    assert!(!Pointer::Addr32(RP235X_BASE_SRAM).is_flash());
    // Null — not flash.
    assert!(!Pointer::Null.is_flash());
}

/// 24. is_sram() is true for addresses within the RP235x SRAM region
///     [RP235X_BASE_SRAM, RP235X_END_SRAM] and false otherwise.
#[test]
fn pointer_is_sram() {
    // Base address — first SRAM byte.
    assert!(Pointer::Addr32(RP235X_BASE_SRAM).is_sram());
    // Inclusive end — last SRAM byte.
    assert!(Pointer::Addr32(RP235X_END_SRAM).is_sram());
    // One byte past the inclusive end — not SRAM.
    assert!(!Pointer::Addr32(RP235X_END_SRAM + 1).is_sram());
    // Flash address — not SRAM.
    assert!(!Pointer::Addr32(RP235X_BASE_FLASH).is_sram());
    // Null — not SRAM.
    assert!(!Pointer::Null.is_sram());
}

/// 25. Pointer::Addr32 survives a full serialize → parse round-trip unchanged.
///     Verifies that raw() and Pointer::new() compose correctly end-to-end.
#[test]
fn round_trip_opaque_ptr_addr32() {
    // Use a realistic flash address for the ROM data pointer.
    let slot = OneromRomSlot {
        data: Pointer::Addr32(0x1001_0000),
        ..make_slot(vec![make_rom_info("2364")])
    };
    let original = OneromMetadataHeader {
        rom_slots: vec![slot],
        ..minimal_header()
    };
    assert_round_trips(&original);
}
