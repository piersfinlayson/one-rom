// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The USB controller DPRAM reservation and the memory pool at plugin launch.
//!
//! - Only one plugin can reserve USB controller DPRAM. If both do, the user
//!   plugin doesn't start.
//! - The pool starts at the reservation rounded up to 4 bytes.
//! - A system plugin requiring firmware before 0.8.1 leaves the pool empty. A
//!   user plugin requiring firmware before 0.8.1 doesn't.

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_metadata::{
    ALLOC_REQUEST_MIN_SIZE, USB_CONTROLLER_DPRAM_MAX, USB_DPRAM_ACCESS_CYCLES, USB_DPRAM_BASE_ADDR,
    onerom_firmware_flag_t,
};

use crate::flash_range::PLUGINS;
use crate::report::TestReport;

const LOG_IN_DPRAM: u8 = onerom_firmware_flag_t::FIRMWARE_FLAG_LOG_IN_UP1;

pub fn run(report: &mut TestReport) {
    let addrs = PLUGINS.map(|p| Emulator::rom_slot_flash_addr(p.index));
    Emulator::set_sel_image(0);
    let emulator = Emulator::boot();
    Emulator::install_plugin_slots();
    let flags = emulator.firmware_flags();
    emulator.set_firmware_flags(flags & !LOG_IN_DPRAM);

    report.add_check("usb dpram: one reservation", check_one_reservation());
    report.add_check("usb dpram: pool start", check_pool_start(&emulator));
    report.add_check("usb dpram: older plugins", check_older_plugins(&emulator));

    emulator.set_firmware_flags(flags);
    Emulator::restore_rom_slots();
    for (p, addr) in PLUGINS.iter().zip(addrs) {
        Emulator::set_rom_slot_flash_addr(p.index, addr);
    }
}

/// Both plugins require firmware 0.8.1 and reserve `sizes`.
fn place(sizes: [u16; 2]) {
    for (p, size) in PLUGINS.iter().zip(sizes) {
        p.place(true, 0, 0);
        Emulator::set_plugin_min_fw_version(p.index, 0, 8, 1);
        Emulator::set_plugin_usb_controller_dpram_size(p.index, size);
    }
}

/// The address a 4-byte allocation returns, or the result where it failed.
fn first_alloc(emu: &Emulator) -> Result<u32, OraResult> {
    let req = ffi::ora_alloc_request_t {
        size: ALLOC_REQUEST_MIN_SIZE,
        reserved: [0],
        max_access_cycles: USB_DPRAM_ACCESS_CYCLES,
        accept: ffi::ORA_MEM_ACCEPT_ALIGNED_ONLY
            | ffi::ORA_MEM_ACCEPT_NO_EXEC
            | ffi::ORA_MEM_ACCEPT_SLOWER,
        bytes: 4,
    };
    let (r, addr) = emu.alloc_mem(&req);
    if r == OraResult::Ok { Ok(addr) } else { Err(r) }
}

fn check_one_reservation() -> Result<(), String> {
    for (label, sizes, user_starts) in [
        ("neither plugin", [0, 0], true),
        ("the system plugin", [0x340, 0], true),
        ("the user plugin", [0, 0x340], true),
        ("both plugins", [0x340, 0x340], false),
    ] {
        place(sizes);
        let found = (
            Emulator::system_plugin_to_launch(),
            Emulator::user_plugin_to_start(),
            Emulator::user_plugin_to_launch(true),
        );
        if found != (true, user_starts, user_starts) {
            return Err(format!(
                "{label} reserving: system plugin launch {}, user plugin start {}, user \
                 plugin launch {}",
                found.0, found.1, found.2
            ));
        }
    }
    Ok(())
}

fn check_pool_start(emu: &Emulator) -> Result<(), String> {
    let max = USB_CONTROLLER_DPRAM_MAX as u16;
    for (sizes, start) in [
        ([0, 0], 0),
        ([0x340, 0], 0x340),
        ([0, 0x101], 0x104),
        ([max, 0], u32::from(max)),
    ] {
        place(sizes);
        emu.launch_usb_dpram(0);
        let found = first_alloc(emu);
        if found != Ok(USB_DPRAM_BASE_ADDR + start) {
            return Err(format!(
                "reservations {sizes:x?}: the first allocation is {found:x?}, not at \
                 offset {start:#x}"
            ));
        }
    }
    Ok(())
}

fn check_older_plugins(emu: &Emulator) -> Result<(), String> {
    for (p, minor, expected) in [
        (&PLUGINS[0], 8, Err(OraResult::InsufficientFreeMem)),
        (&PLUGINS[0], 7, Err(OraResult::InsufficientFreeMem)),
        (&PLUGINS[1], 8, Ok(USB_DPRAM_BASE_ADDR)),
    ] {
        place([0, 0]);
        Emulator::set_plugin_min_fw_version(p.index, 0, minor, 0);
        emu.launch_usb_dpram(0);
        let found = first_alloc(emu);
        if found != expected {
            return Err(format!(
                "a {} plugin requiring firmware 0.{minor}.0: the first allocation is \
                 {found:x?}, not {expected:x?}",
                p.label
            ));
        }
    }
    Ok(())
}
