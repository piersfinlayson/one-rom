// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `ORA_ID_GET_PEER_DATA`.
//!
//! Each plugin retrieves one of the other plugin's peer data blocks by format.
//! The call returns `ORA_RESULT_NOT_FOUND` when the firmware isn't starting
//! another plugin or none of its valid blocks has that format.  The plugin
//! slots are installed for the test and the metadata's own slots restored
//! afterwards.

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_metadata::{
    FLASH_CS0_BASE_ADDR, SYSTEM_PLUGIN_OFFSET, SYSTEM_PLUGIN_SIZE, USER_PLUGIN_OFFSET,
    USER_PLUGIN_SIZE,
};

const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USER: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER;

const REGIONS: [u32; 2] = [
    FLASH_CS0_BASE_ADDR + SYSTEM_PLUGIN_OFFSET,
    FLASH_CS0_BASE_ADDR + USER_PLUGIN_OFFSET,
];
const SIZES: [u32; 2] = [SYSTEM_PLUGIN_SIZE as u32, USER_PLUGIN_SIZE as u32];

/// Offset of each plugin's blocks in its region, past the header.
const BLOCK_OFFSET: u32 = 0x100;

/// Format words for the tests.
const FORMAT_A: u32 = 0x3153_5953;
const FORMAT_B: u32 = 0x3152_5355;
const FORMAT_C: u32 = 0x3143_5355;

/// Different in every word and in size so a swapped or partial copy fails.
const PEER: [&[u32]; 2] = [&[FORMAT_A, 32, 1, 2, 3, 4, 5, 6], &[FORMAT_B, 16, 1, 7]];
const PEER_FORMATS: [u32; 2] = [FORMAT_A, FORMAT_B];

/// Write a valid header for plugin `index` in its region, with `words` written
/// at `addr` and the header pointing at `addr`.
fn place(index: u8, addr: u32, words: &[u32]) {
    let region = REGIONS[usize::from(index)];
    Emulator::set_plugin_header(index, region + SIZES[usize::from(index)] - 1, 0, 0);
    Emulator::set_flash_words(addr, words);
    Emulator::set_plugin_peer_data(index, addr);
    Emulator::set_rom_slot_flash_addr(index, region);
}

/// Place `blocks` back to back for plugin `index`, followed by a 0 word.
fn place_run(index: u8, blocks: &[&[u32]]) {
    let mut words: Vec<u32> = blocks.concat();
    words.push(0);
    place(index, block_addr(index), &words);
}

fn block_addr(index: u8) -> u32 {
    REGIONS[usize::from(index)] + BLOCK_OFFSET
}

/// Retrieve the block with `format` as `caller` and check it is `want`, or
/// isn't found for `None`.
fn check(
    emu: &Emulator,
    what: &str,
    caller: ffi::ora_plugin_type_t,
    format: u32,
    want: Option<&[u32]>,
) -> Result<(), String> {
    emu.set_calling_plugin(caller);
    let (result, got) = emu.get_peer_data(format, 0);
    let ok = match want {
        Some(want) => result == OraResult::Ok && got == want,
        None => result == OraResult::NotFound && got.is_empty(),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("{what}: got {result:?} {got:x?}, want {want:x?}"))
    }
}

fn check_one_block(emu: &Emulator) -> Result<(), String> {
    place_run(0, &[PEER[0]]);
    place_run(1, &[PEER[1]]);
    check(emu, "system plugin", SYSTEM, PEER_FORMATS[1], Some(PEER[1]))?;
    check(emu, "user plugin", USER, PEER_FORMATS[0], Some(PEER[0]))?;
    check(emu, "another format", SYSTEM, FORMAT_C, None)?;
    check(
        emu,
        "the caller's own format",
        SYSTEM,
        PEER_FORMATS[0],
        None,
    )?;

    // Each invalid block is the user plugin's, read by the system plugin.
    let user = REGIONS[1];
    let end = user + SIZES[1];
    let invalid: [(&str, u32, &[u32]); 7] = [
        ("no block", 0, &[]),
        ("an unaligned block", block_addr(1) + 2, PEER[1]),
        (
            "a block in the other plugin's region",
            block_addr(0) + 0x40,
            PEER[1],
        ),
        (
            "a block smaller than the minimum",
            block_addr(1),
            &[FORMAT_B, 8, 1],
        ),
        (
            "a block whose size isn't a multiple of 4",
            block_addr(1),
            &[FORMAT_B, 14, 1, 7],
        ),
        (
            "a block running past the region",
            end - 12,
            &[FORMAT_B, 16, 1],
        ),
        ("a block starting past the region", end, &[]),
    ];
    for (what, addr, block) in invalid {
        Emulator::clear_flash();
        place(1, addr, block);
        check(emu, what, SYSTEM, FORMAT_B, None)?;
    }

    // The smallest block is valid at the region's end, where the 0 word doesn't
    // fit.
    Emulator::clear_flash();
    let smallest: &[u32] = &[FORMAT_B, ffi::ORA_PEER_DATA_MIN_SIZE, 1];
    place(1, end - ffi::ORA_PEER_DATA_MIN_SIZE, smallest);
    check(
        emu,
        "the smallest block at the region's end",
        SYSTEM,
        FORMAT_B,
        Some(smallest),
    )
}

fn check_runs(emu: &Emulator) -> Result<(), String> {
    let a: &[u32] = &[FORMAT_A, 16, 1, 11];
    let b: &[u32] = &[FORMAT_B, 20, 2, 12, 13];
    let c: &[u32] = &[FORMAT_C, 12, 1];

    Emulator::clear_flash();
    place_run(1, &[a, b, c]);
    check(emu, "the first block", SYSTEM, FORMAT_A, Some(a))?;
    check(emu, "the second block", SYSTEM, FORMAT_B, Some(b))?;
    check(emu, "the last block", SYSTEM, FORMAT_C, Some(c))?;

    // Two blocks with the same format: the first is returned.
    Emulator::clear_flash();
    let b2: &[u32] = &[FORMAT_B, 12, 1];
    place_run(1, &[b, b2]);
    check(
        emu,
        "the first of two with a format",
        SYSTEM,
        FORMAT_B,
        Some(b),
    )?;

    // A block after the 0 word isn't found.
    Emulator::clear_flash();
    place(1, block_addr(1), &[a, &[0], b].concat());
    check(emu, "a block before the 0 word", SYSTEM, FORMAT_A, Some(a))?;
    check(emu, "a block after the 0 word", SYSTEM, FORMAT_B, None)?;

    // An invalid block stops the check.  The blocks before it are still found.
    Emulator::clear_flash();
    let bad: &[u32] = &[FORMAT_C, 14, 1, 0];
    place(1, block_addr(1), &[a, bad, b, &[0]].concat());
    check(
        emu,
        "a block before an invalid one",
        SYSTEM,
        FORMAT_A,
        Some(a),
    )?;
    check(emu, "the invalid block", SYSTEM, FORMAT_C, None)?;
    check(emu, "a block after an invalid one", SYSTEM, FORMAT_B, None)?;

    // A block running past the region stops the check.
    Emulator::clear_flash();
    let end = REGIONS[1] + SIZES[1];
    place(1, end - 16, &[a[0], 16, 1, 11]);
    check(
        emu,
        "a block ending at the region's end",
        SYSTEM,
        FORMAT_A,
        Some(a),
    )?;
    Emulator::clear_flash();
    place(
        1,
        end - 24,
        &[[FORMAT_B, 12, 1].as_slice(), &[FORMAT_A, 16, 1]].concat(),
    );
    check(
        emu,
        "a block before one running past the region",
        SYSTEM,
        FORMAT_B,
        Some(&[FORMAT_B, 12, 1]),
    )?;
    check(
        emu,
        "a block running past the region",
        SYSTEM,
        FORMAT_A,
        None,
    )
}

fn check_args(emu: &Emulator) -> Result<(), String> {
    Emulator::clear_flash();
    place_run(0, &[PEER[0]]);
    place_run(1, &[PEER[1]]);
    emu.set_calling_plugin(SYSTEM);
    for flags in [1u32, 1 << 31] {
        let (result, got) = emu.get_peer_data(FORMAT_B, flags);
        if result != OraResult::InvalidArg || !got.is_empty() {
            return Err(format!(
                "flags {flags:#x}: got {result:?} {got:x?}, want InvalidArg"
            ));
        }
    }
    let result = emu.get_peer_data_null_out(FORMAT_B);
    if result != OraResult::InvalidArg {
        return Err(format!("a NULL block_out: got {result:?}, want InvalidArg"));
    }
    Ok(())
}

fn check_not_started(emu: &Emulator) -> Result<(), String> {
    // The firmware doesn't start the user plugin when both reserve USB
    // controller DPRAM.
    Emulator::clear_flash();
    place_run(0, &[PEER[0]]);
    place_run(1, &[PEER[1]]);
    Emulator::set_plugin_usb_controller_dpram_size(0, 0x340);
    Emulator::set_plugin_usb_controller_dpram_size(1, 0x340);
    check(emu, "user plugin not started", SYSTEM, FORMAT_B, None)?;

    // Nor without a valid system plugin.
    place_run(1, &[PEER[1]]);
    Emulator::set_plugin_magic(0, 0);
    check(emu, "system plugin invalid", USER, FORMAT_A, None)?;
    check(
        emu,
        "system plugin invalid, so user plugin not started",
        SYSTEM,
        FORMAT_B,
        None,
    )
}

fn check_peer_data(emu: &Emulator) -> Result<(), String> {
    check_one_block(emu)?;
    check_runs(emu)?;
    check_args(emu)?;
    check_not_started(emu)
}

pub fn test_peer_data(emu: &Emulator) -> Result<(), String> {
    if emu.get_flash_slot_count(0) < 2 {
        println!("  peer_data: skipped, the config has fewer than two slots");
        return Ok(());
    }

    let addrs = [0, 1].map(Emulator::rom_slot_flash_addr);
    Emulator::install_plugin_slots();
    let result = check_peer_data(emu);
    Emulator::clear_flash();
    Emulator::restore_rom_slots();
    for (index, addr) in (0..).zip(addrs) {
        Emulator::set_rom_slot_flash_addr(index, addr);
    }
    emu.set_calling_plugin(SYSTEM);
    result
}
