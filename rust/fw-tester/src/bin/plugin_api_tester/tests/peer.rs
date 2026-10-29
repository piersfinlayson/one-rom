// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `ORA_ID_GET_PEER_DATA`.
//!
//! Each plugin retrieves the other plugin's `peer_data`, and NULL when the
//! firmware isn't starting another plugin. The plugin slots are installed for
//! the test and the metadata's own slots restored afterwards.

use onerom_fw_emulator::{Emulator, ffi};
use onerom_metadata::{
    FLASH_CS0_BASE_ADDR, SYSTEM_PLUGIN_OFFSET, SYSTEM_PLUGIN_SIZE, USER_PLUGIN_OFFSET,
};

const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USER: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER;

const REGIONS: [u32; 2] = [
    FLASH_CS0_BASE_ADDR + SYSTEM_PLUGIN_OFFSET,
    FLASH_CS0_BASE_ADDR + USER_PLUGIN_OFFSET,
];

/// Different in every word, so a swapped or partial copy fails.
const PEER: [[u32; 8]; 2] = [
    [0x3153_5953, 1, 2, 3, 4, 5, 6, 7],
    [0x3152_5355, 8, 9, 10, 11, 12, 13, 14],
];

/// Write a valid header for plugin `index` with `peer`, in its region.
fn place(index: u8, peer: [u32; 8]) {
    let region = REGIONS[usize::from(index)];
    Emulator::set_plugin_header(index, region + SYSTEM_PLUGIN_SIZE as u32 - 1, 0, 0);
    Emulator::set_plugin_peer_data(index, peer);
    Emulator::set_rom_slot_flash_addr(index, region);
}

fn check(
    emu: &Emulator,
    what: &str,
    caller: ffi::ora_plugin_type_t,
    want: Option<[u32; 8]>,
) -> Result<(), String> {
    emu.set_calling_plugin(caller);
    let got = emu.get_peer_data();
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: got {got:x?}, want {want:x?}"))
    }
}

fn check_peer_data(emu: &Emulator) -> Result<(), String> {
    place(0, PEER[0]);
    place(1, PEER[1]);
    check(emu, "system plugin", SYSTEM, Some(PEER[1]))?;
    check(emu, "user plugin", USER, Some(PEER[0]))?;

    place(1, [0; 8]);
    check(emu, "user plugin without peer data", SYSTEM, Some([0; 8]))?;

    // The firmware doesn't start the user plugin when both reserve USB
    // controller DPRAM.
    place(1, PEER[1]);
    Emulator::set_plugin_usb_controller_dpram_size(0, 0x340);
    Emulator::set_plugin_usb_controller_dpram_size(1, 0x340);
    check(emu, "user plugin not started", SYSTEM, None)?;

    // Nor without a valid system plugin.
    place(1, PEER[1]);
    Emulator::set_plugin_magic(0, 0);
    check(emu, "system plugin invalid", USER, None)?;
    check(
        emu,
        "system plugin invalid, so user plugin not started",
        SYSTEM,
        None,
    )
}

pub fn test_peer_data(emu: &Emulator) -> Result<(), String> {
    if emu.get_flash_slot_count(0) < 2 {
        println!("  peer_data: skipped, the config has fewer than two slots");
        return Ok(());
    }

    let addrs = [0, 1].map(Emulator::rom_slot_flash_addr);
    Emulator::install_plugin_slots();
    let result = check_peer_data(emu);
    Emulator::restore_rom_slots();
    for (index, addr) in (0..).zip(addrs) {
        Emulator::set_rom_slot_flash_addr(index, addr);
    }
    emu.set_calling_plugin(SYSTEM);
    result
}
