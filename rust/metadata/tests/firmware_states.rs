// tests/firmware_states.rs
//
// Tests for how a host reads runtime info's firmware states and flags.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_config::fw::FirmwareVersion;
use onerom_config::mcu::{RP235X_BASE_FLASH, RP235X_BASE_SRAM};
use onerom_metadata::{
    DeviceMemoryView, Generations, ONEROM_FAMILY_MAGIC, ONEROM_INFO_BUILD_DATE_OFFSET,
    ONEROM_INFO_MAGIC_OFFSET, ONEROM_INFO_MAJOR_VERSION_OFFSET, ONEROM_INFO_MINOR_VERSION_OFFSET,
    ONEROM_INFO_PATCH_VERSION_OFFSET, ONEROM_INFO_RUNTIME_OFFSET, ONEROM_INFO_SIZE,
    ONEROM_INFO_VERSION, ONEROM_INFO_VERSION_OFFSET, ONEROM_RUNTIME_INFO_FIRMWARE_FLAGS_OFFSET,
    ONEROM_RUNTIME_INFO_FIRMWARE_STATES_OFFSET, ONEROM_RUNTIME_INFO_SIZE, OneromFirmwareState,
    OneromInfo, OneromRuntimeInfo, RUNTIME_INFO_MAGIC, onerom_firmware_flag_t as Flag,
    onerom_firmware_state_t as State,
};

const ROM_LOADED: u32 = State::FIRMWARE_STATE_ROM_LOADED;
const PLUGINS_STARTED: u32 = State::FIRMWARE_STATE_PLUGINS_STARTED;
const STARTUP_DONE: u32 = State::FIRMWARE_STATE_STARTUP_DONE;
const ALL: u32 = ROM_LOADED | PLUGINS_STARTED | STARTUP_DONE;

/// The release the states arrived in.
const FIRST: FirmwareVersion = FirmwareVersion::new(0, 8, 0, 0);

/// The last release without them.
const BEFORE: FirmwareVersion = FirmwareVersion::new(0, 7, 3, 0);

// ===========================================================================
// Reading the bits
// ===========================================================================

#[test]
fn firmware_with_the_states_reads_each_one() {
    let states = OneromFirmwareState::from_raw(ROM_LOADED | STARTUP_DONE, Some(FIRST));
    assert_eq!(states.rom_loaded, Some(true));
    assert_eq!(states.plugins_started, Some(false));
    assert_eq!(states.startup_done, Some(true));
    assert_eq!(states.unknown_bits, 0);
}

/// A bit set by older firmware is kept as unknown.
#[test]
fn firmware_older_than_the_states_reads_none() {
    let states = OneromFirmwareState::from_raw(ALL, Some(BEFORE));
    assert_eq!(states.rom_loaded, None);
    assert_eq!(states.plugins_started, None);
    assert_eq!(states.startup_done, None);
    assert_eq!(states.unknown_bits, ALL);
}

/// An unread release predates every member, as an unread generation predates
/// every gated field.
#[test]
fn an_unknown_release_reads_none() {
    let states = OneromFirmwareState::from_raw(ALL, None);
    assert_eq!(states.rom_loaded, None);
    assert_eq!(states.unknown_bits, ALL);
}

#[test]
fn a_bit_no_member_covers_is_kept() {
    let newer = 1u32 << 31;
    let states = OneromFirmwareState::from_raw(ROM_LOADED | newer, Some(FIRST));
    assert_eq!(states.rom_loaded, Some(true));
    assert_eq!(states.unknown_bits, newer);
}

#[test]
fn the_json_lists_each_member_by_name() {
    let states = OneromFirmwareState::from_raw(ROM_LOADED | PLUGINS_STARTED, Some(FIRST));
    assert_eq!(
        serde_json::to_value(states).expect("the states should serialize"),
        serde_json::json!({
            "rom_loaded": true,
            "plugins_started": true,
            "startup_done": false,
            "unknown_bits": 0,
        })
    );
}

// ===========================================================================
// Through the parser
// ===========================================================================

const RUNTIME_ADDR: u32 = RP235X_BASE_SRAM;

/// The runtime structure's generation follows its magic.
const RUNTIME_VERSION_OFFSET: usize = 4;

fn parsed_states(release: (u16, u16, u16), generation: u32, states: u32) -> OneromFirmwareState {
    parsed_runtime(release, generation, states, 0).firmware_states
}

fn parsed_runtime(
    release: (u16, u16, u16),
    generation: u32,
    states: u32,
    flags: u8,
) -> OneromRuntimeInfo {
    const BUILD_DATE: &[u8] = b"2026-10-03\0";
    let put_u16 = |b: &mut [u8], off: usize, v: u16| {
        b[off..off + 2].copy_from_slice(&v.to_le_bytes());
    };
    let put_u32 = |b: &mut [u8], off: usize, v: u32| {
        b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    };

    let mut info = vec![0u8; ONEROM_INFO_SIZE + BUILD_DATE.len()];
    info[ONEROM_INFO_MAGIC_OFFSET..ONEROM_INFO_MAGIC_OFFSET + 4]
        .copy_from_slice(ONEROM_FAMILY_MAGIC.as_bytes());
    put_u16(&mut info, ONEROM_INFO_MAJOR_VERSION_OFFSET, release.0);
    put_u16(&mut info, ONEROM_INFO_MINOR_VERSION_OFFSET, release.1);
    put_u16(&mut info, ONEROM_INFO_PATCH_VERSION_OFFSET, release.2);
    put_u32(
        &mut info,
        ONEROM_INFO_BUILD_DATE_OFFSET,
        RP235X_BASE_FLASH + ONEROM_INFO_SIZE as u32,
    );
    put_u32(&mut info, ONEROM_INFO_VERSION_OFFSET, ONEROM_INFO_VERSION);
    put_u32(&mut info, ONEROM_INFO_RUNTIME_OFFSET, RUNTIME_ADDR);
    info[ONEROM_INFO_SIZE..].copy_from_slice(BUILD_DATE);

    let mut runtime = vec![0u8; ONEROM_RUNTIME_INFO_SIZE];
    runtime[..4].copy_from_slice(RUNTIME_INFO_MAGIC.as_bytes());
    put_u32(&mut runtime, RUNTIME_VERSION_OFFSET, generation);
    put_u32(
        &mut runtime,
        ONEROM_RUNTIME_INFO_FIRMWARE_STATES_OFFSET,
        states,
    );
    runtime[ONEROM_RUNTIME_INFO_FIRMWARE_FLAGS_OFFSET] = flags;

    let mut view = DeviceMemoryView::new(&info, RP235X_BASE_FLASH);
    view.add_region(&runtime, RUNTIME_ADDR);
    let info = OneromInfo::parse(&view, RP235X_BASE_FLASH, Generations::UNKNOWN)
        .expect("the info header should parse");
    info.runtime.expect("the runtime structure should parse")
}

#[test]
fn the_parser_reads_the_states_against_the_release_info_records() {
    let states = parsed_states((0, 8, 0), 3, ALL);
    assert_eq!(states.rom_loaded, Some(true));
    assert_eq!(states.plugins_started, Some(true));
    assert_eq!(states.startup_done, Some(true));

    let states = parsed_states((0, 7, 3), 3, ALL);
    assert_eq!(states.rom_loaded, None);
    assert_eq!(states.unknown_bits, ALL);
}

/// The field's bytes aren't read from a runtime structure older than the
/// field.
#[test]
fn an_older_runtime_structure_reads_no_states() {
    let states = parsed_states((0, 7, 3), 2, ALL);
    assert_eq!(states.rom_loaded, None);
    assert_eq!(states.unknown_bits, 0);
}

// ===========================================================================
// Firmware flags
// ===========================================================================

const STANDBY: u8 = Flag::FIRMWARE_FLAG_STANDBY;

#[test]
fn the_parser_reads_the_flags_against_the_release_info_records() {
    let flags = parsed_runtime((0, 8, 0), 3, 0, STANDBY).firmware_flags;
    assert_eq!(flags.standby, Some(true));
    assert_eq!(flags.unknown_bits, 0);

    let flags = parsed_runtime((0, 8, 0), 3, 0, 0).firmware_flags;
    assert_eq!(flags.standby, Some(false));

    let flags = parsed_runtime((0, 7, 3), 3, 0, STANDBY).firmware_flags;
    assert_eq!(flags.standby, None);
    assert_eq!(flags.unknown_bits, STANDBY);
}

/// The field's bytes aren't read from a runtime structure older than the
/// field.
#[test]
fn an_older_runtime_structure_reads_no_flags() {
    let flags = parsed_runtime((0, 7, 3), 2, 0, STANDBY).firmware_flags;
    assert_eq!(flags.standby, None);
    assert_eq!(flags.unknown_bits, 0);
}
