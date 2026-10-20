// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! ROM slot selection by the plugins' header overrides.
//!
//! - With `OVERRIDE1_BOOT_SLOT` the byte at the header's `boot_slot_addr`
//!   selects the slot if it is a ROM slot number, counting from 0 at the first
//!   slot that isn't a plugin.
//! - The byte is read only if its address is within that plugin's flash region.
//! - The system plugin's boot slot is used ahead of the user plugin's.
//! - Without a boot slot from either plugin `OVERRIDE1_IGNORE_IMAGE_SELECT` in
//!   either header boots the first ROM slot.
//! - The image select jumpers select the slot when the overrides don't.
//! - The overrides apply only from the header of a plugin the firmware starts.
//! - A boot serves the slot the overrides select.

use onerom_config::hw::Board;
use onerom_fw_emulator::{Emulator, ffi};
use onerom_metadata::{
    FLASH_CS1_BASE_ADDR, OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE, OTP_BOOT_FLAGS0_ROW,
    OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT, OTP_FLASH_DEVINFO_ROW, OVERRIDE1_BOOT_SLOT,
    OVERRIDE1_IGNORE_IMAGE_SELECT, OneromFlashSize,
};

use crate::flash_range::{PLUGINS, check_plugin_flash_slots};
use crate::report::TestReport;

const BOOT_SLOT: u8 = OVERRIDE1_BOOT_SLOT;
const IGNORE: u8 = OVERRIDE1_IGNORE_IMAGE_SELECT;

const NUM_PLUGINS: u8 = 2;

/// A plugin's overrides1 and the value of its boot slot byte.
type Overrides = (u8, u8);

const NONE: Overrides = (0, 0xFF);

/// `sel_values` is the number of image select values the board's jumpers can
/// set.
pub fn run(board: Board, num_slots: usize, sel_values: usize, report: &mut TestReport) {
    let addrs = PLUGINS.map(|p| Emulator::rom_slot_flash_addr(p.index));
    let rom_slots = (num_slots - NUM_PLUGINS as usize) as u8;
    Emulator::overlay_plugin_slots();

    report.add_check("boot slot: stored byte", check_stored_byte(rom_slots));
    report.add_check("boot slot: address", check_address());
    if rom_slots > 1 {
        report.add_check("boot slot: system plugin first", check_priority(rom_slots));
    }
    report.add_check("boot slot: plugins not started", check_not_started());
    // The overrides can be told apart from the jumpers only if the jumpers can
    // select a slot other than slot 0.
    if rom_slots > 1 && sel_values > 1 {
        report.add_check(
            "boot slot: boots the chosen slot",
            check_boot(board, rom_slots),
        );
    }
    report.add_check(
        "boot slot: flash slot calls on the plugin slots",
        check_flash_slots(board, num_slots as u8),
    );

    Emulator::clear_flash();
    Emulator::clear_otp();
    Emulator::restore_rom_slots();
    for (p, addr) in PLUGINS.iter().zip(addrs) {
        Emulator::set_rom_slot_flash_addr(p.index, addr);
    }
}

fn nv_addr(index: u8) -> u32 {
    let p = &PLUGINS[usize::from(index)];
    p.region + p.size - 0x1000
}

fn place_one(index: u8, (overrides1, value): Overrides, addr: u32) {
    PLUGINS[usize::from(index)].place(true, overrides1, 0);
    Emulator::set_plugin_boot_slot_addr(index, addr);
    Emulator::set_flash_byte(addr, value);
}

fn place(system: Overrides, user: Overrides) {
    Emulator::clear_flash();
    place_one(0, system, nv_addr(0));
    place_one(1, user, nv_addr(1));
}

fn place_only(index: u8, overrides: Overrides) {
    if index == 0 {
        place(overrides, NONE);
    } else {
        place(NONE, overrides);
    }
}

fn describe(slot: Option<u8>) -> String {
    match slot {
        Some(slot) => format!("slot {slot}"),
        None => "the jumpers".to_string(),
    }
}

fn check_stored_byte(rom_slots: u8) -> Result<(), String> {
    let last = rom_slots - 1;
    let cases = [
        ("no overrides and a stored slot", 0, last, None),
        ("BOOT_SLOT and slot 0", BOOT_SLOT, 0, Some(0)),
        ("BOOT_SLOT and the last slot", BOOT_SLOT, last, Some(last)),
        (
            "BOOT_SLOT and one past the last slot",
            BOOT_SLOT,
            rom_slots,
            None,
        ),
        ("BOOT_SLOT and 0xFF", BOOT_SLOT, 0xFF, None),
        ("IGNORE_IMAGE_SELECT alone", IGNORE, last, Some(0)),
        (
            "both and the last slot",
            BOOT_SLOT | IGNORE,
            last,
            Some(last),
        ),
        (
            "both and one past the last slot",
            BOOT_SLOT | IGNORE,
            rom_slots,
            Some(0),
        ),
        ("both and 0xFF", BOOT_SLOT | IGNORE, 0xFF, Some(0)),
    ];
    for p in &PLUGINS {
        for (label, overrides1, value, expected) in cases {
            place_only(p.index, (overrides1, value));
            let found = Emulator::plugin_boot_slot(NUM_PLUGINS);
            if found != expected {
                return Err(format!(
                    "the {} plugin with {label}: {} chose, not {}",
                    p.label,
                    describe(found),
                    describe(expected)
                ));
            }
        }
    }
    Ok(())
}

fn check_address() -> Result<(), String> {
    for p in &PLUGINS {
        let region = p.region;
        let end = region + p.size;
        let cases = [
            ("the region's first byte", region, Some(0)),
            ("the region's last byte", end - 1, Some(0)),
            ("the byte past the region", end, None),
            ("the byte before the region", region - 1, None),
            ("chip select 1", FLASH_CS1_BASE_ADDR, None),
            ("zero", 0, None),
        ];
        for (label, addr, expected) in cases {
            place_only(p.index, NONE);
            place_one(p.index, (BOOT_SLOT, 0), addr);
            let found = Emulator::plugin_boot_slot(NUM_PLUGINS);
            if found != expected {
                return Err(format!(
                    "the {} plugin's stored slot 0 at {label}, {addr:#010x}: {} chose",
                    p.label,
                    describe(found)
                ));
            }
        }
    }

    // A 128KB first chip holds the system plugin region and not the user
    // plugin region so the firmware doesn't start the user plugin.
    Emulator::clear_otp();
    Emulator::set_otp_raw(
        OTP_BOOT_FLAGS0_ROW,
        &[OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE; 3],
    );
    let devinfo = (OneromFlashSize::FlashSize128kb as u16) << OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT;
    Emulator::set_otp_ecc(OTP_FLASH_DEVINFO_ROW, &[devinfo]);
    let mut result = Ok(());
    for (p, expected) in PLUGINS.iter().zip([Some(0), None]) {
        place_only(p.index, (BOOT_SLOT, 0));
        let found = Emulator::plugin_boot_slot(NUM_PLUGINS);
        if found != expected {
            result = Err(format!(
                "the {} plugin's stored slot 0 on a 128KB first chip: {} chose",
                p.label,
                describe(found)
            ));
            break;
        }
    }
    Emulator::clear_otp();
    result
}

fn check_priority(rom_slots: u8) -> Result<(), String> {
    let last = rom_slots - 1;
    let cases = [
        ("both BOOT_SLOT", (BOOT_SLOT, 0), (BOOT_SLOT, last), Some(0)),
        (
            "system BOOT_SLOT past the last slot",
            (BOOT_SLOT, rom_slots),
            (BOOT_SLOT, last),
            Some(last),
        ),
        (
            "system IGNORE_IMAGE_SELECT and user BOOT_SLOT",
            (IGNORE, 0xFF),
            (BOOT_SLOT, last),
            Some(last),
        ),
        (
            "system BOOT_SLOT and user IGNORE_IMAGE_SELECT",
            (BOOT_SLOT, last),
            (IGNORE, 0xFF),
            Some(last),
        ),
        (
            "both IGNORE_IMAGE_SELECT",
            (IGNORE, 0xFF),
            (IGNORE, 0xFF),
            Some(0),
        ),
    ];
    for (label, system, user, expected) in cases {
        place(system, user);
        let found = Emulator::plugin_boot_slot(NUM_PLUGINS);
        if found != expected {
            return Err(format!(
                "{label}: {} chose, not {}",
                describe(found),
                describe(expected)
            ));
        }
    }
    Ok(())
}

fn check_not_started() -> Result<(), String> {
    let both = (BOOT_SLOT | IGNORE, 0);

    // Neither plugin starts when the system plugin is invalid.
    place(both, both);
    PLUGINS[0].place(false, both.0, 0);
    Emulator::set_plugin_boot_slot_addr(0, nv_addr(0));
    if let Some(slot) = Emulator::plugin_boot_slot(NUM_PLUGINS) {
        return Err(format!("an invalid system plugin chose slot {slot}"));
    }

    place(NONE, both);
    Emulator::set_plugin_header(1, PLUGINS[1].region + PLUGINS[1].size, both.0, 0);
    Emulator::set_plugin_boot_slot_addr(1, nv_addr(1));
    if let Some(slot) = Emulator::plugin_boot_slot(NUM_PLUGINS) {
        return Err(format!("an invalid user plugin chose slot {slot}"));
    }
    Ok(())
}

fn check_flash_slots(board: Board, num_slots: u8) -> Result<(), String> {
    place(NONE, NONE);
    Emulator::set_rp_variant(board.rp_variant());
    Emulator::set_sel_image(0);
    let emulator = Emulator::boot();
    check_plugin_flash_slots(&emulator, num_slots)
}

fn check_boot(board: Board, rom_slots: u8) -> Result<(), String> {
    let sel = 1;
    let by_jumpers = 1;
    let mut cases = vec![
        ("no overrides and slot 0 stored", (0, 0), (0, 0), by_jumpers),
        ("user BOOT_SLOT and slot 0", NONE, (BOOT_SLOT, 0), 0),
        ("system BOOT_SLOT and slot 0", (BOOT_SLOT, 0), NONE, 0),
        (
            "user BOOT_SLOT and 0xFF",
            NONE,
            (BOOT_SLOT, 0xFF),
            by_jumpers,
        ),
        ("user IGNORE_IMAGE_SELECT alone", NONE, (IGNORE, 0xFF), 0),
    ];
    if rom_slots > 2 {
        cases.push(("user BOOT_SLOT and slot 2", NONE, (BOOT_SLOT, 2), 2));
    }
    for (label, system, user, expected) in cases {
        place(system, user);
        Emulator::set_rp_variant(board.rp_variant());
        Emulator::set_sel_image(sel);
        let emulator = Emulator::boot();

        if emulator.limp_mode() || !emulator.pios_enabled() {
            return Err(format!(
                "{label}: limp mode {} after boot",
                emulator.limp_mode_pattern()
            ));
        }
        let (r, index) =
            emulator.get_metadata_uint(ffi::ora_metadata_key_t_ORA_METADATA_KEY_ROM_SLOT_INDEX);
        let want = u32::from(expected + NUM_PLUGINS);
        if !r.is_ok() || index != Some(want) {
            return Err(format!(
                "{label} with the jumpers at {sel}: ROM slot index {index:?} ({r:?}), not \
                 {want}"
            ));
        }
    }
    Ok(())
}
