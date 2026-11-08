// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Log channels 2 and 3, which the firmware creates at plugin launch from the
//! plugin headers' `channel_2_size` and `channel_3_size`.
//!
//! - A size above its maximum, or a size set by a plugin requiring firmware
//!   before 0.8.1, fails the plugin check.
//! - A system plugin requiring firmware before 0.8.1 leaves the pool empty, so
//!   the firmware doesn't start a user plugin that sets a size.
//! - Each channel is the larger of the two plugins' sizes and is allocated from
//!   the pool ahead of any other allocation.
//! - A channel with a size of 0 doesn't exist.

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_metadata::{
    ALLOC_REQUEST_MIN_SIZE, LOG_CHANNEL_2_MAX_SIZE, LOG_CHANNEL_3_MAX_SIZE,
    USB_DPRAM_ACCESS_CYCLES, USB_DPRAM_BASE_ADDR, onerom_firmware_flag_t,
};

use crate::flash_range::PLUGINS;
use crate::report::TestReport;

const LOG_IN_DPRAM: u8 = onerom_firmware_flag_t::FIRMWARE_FLAG_LOG_IN_UP1;
const CH2: u32 = 2;
const CH3: u32 = 3;
const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USER: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER;

pub fn run(report: &mut TestReport) {
    let addrs = PLUGINS.map(|p| Emulator::rom_slot_flash_addr(p.index));
    Emulator::set_sel_image(0);
    let emulator = Emulator::boot();
    Emulator::install_plugin_slots();
    let flags = emulator.firmware_flags();
    emulator.set_firmware_flags(flags & !LOG_IN_DPRAM);

    report.add_check("log channels: start rules", check_start_rules());
    report.add_check("log channels: created", check_created(&emulator));
    report.add_check("log channels: absent", check_absent(&emulator));

    emulator.set_calling_plugin(SYSTEM);
    emulator.set_firmware_flags(flags);
    Emulator::restore_rom_slots();
    for (p, addr) in PLUGINS.iter().zip(addrs) {
        Emulator::set_rom_slot_flash_addr(p.index, addr);
    }
}

/// Both plugin headers require firmware 0.8.1 and have the channel 2 and 3
/// sizes in `sizes`.
fn place(sizes: [(u16, u16); 2]) {
    for (p, (ch2, ch3)) in PLUGINS.iter().zip(sizes) {
        p.place(true, 0, 0);
        Emulator::set_plugin_min_fw_version(p.index, 0, 8, 1);
        Emulator::set_plugin_channel_sizes(p.index, ch2, ch3);
    }
}

fn check_start_rules() -> Result<(), String> {
    let ch2_max = LOG_CHANNEL_2_MAX_SIZE;
    let ch3_max = LOG_CHANNEL_3_MAX_SIZE;
    for (label, sizes, starts) in [
        (
            "system at both maximums",
            [(ch2_max, ch3_max), (0, 0)],
            (true, true),
        ),
        (
            "user at both maximums",
            [(0, 0), (ch2_max, ch3_max)],
            (true, true),
        ),
        (
            "system above channel 2's maximum",
            [(ch2_max + 1, 0), (0, 0)],
            (false, false),
        ),
        (
            "system above channel 3's maximum",
            [(0, ch3_max + 1), (0, 0)],
            (false, false),
        ),
        (
            "user above channel 2's maximum",
            [(0, 0), (ch2_max + 1, 0)],
            (true, false),
        ),
        (
            "user above channel 3's maximum",
            [(0, 0), (0, ch3_max + 1)],
            (true, false),
        ),
    ] {
        place(sizes);
        let found = (
            Emulator::system_plugin_to_launch(),
            Emulator::user_plugin_to_start(),
        );
        if found != starts {
            return Err(format!(
                "{label}: system plugin launch {}, user plugin start {}",
                found.0, found.1
            ));
        }
    }

    const V0_8_0: (u16, u16, u16) = (0, 8, 0);
    const V0_8_1: (u16, u16, u16) = (0, 8, 1);
    for (label, system_fw, user_sizes, user_fw, starts) in [
        (
            "user setting a size and requiring 0.8.0",
            V0_8_1,
            (0x10, 0),
            V0_8_0,
            false,
        ),
        (
            "system requiring 0.8.0, user setting a size",
            V0_8_0,
            (0, 0x10),
            V0_8_1,
            false,
        ),
        (
            "system requiring 0.8.0, user without sizes",
            V0_8_0,
            (0, 0),
            V0_8_1,
            true,
        ),
    ] {
        place([(0, 0), user_sizes]);
        for (p, (major, minor, patch)) in PLUGINS.iter().zip([system_fw, user_fw]) {
            Emulator::set_plugin_min_fw_version(p.index, major, minor, patch);
        }
        let found = (
            Emulator::user_plugin_to_start(),
            Emulator::user_plugin_to_launch(true),
        );
        if found != (starts, starts) {
            return Err(format!(
                "{label}: user plugin start {}, user plugin launch {}",
                found.0, found.1
            ));
        }
    }
    Ok(())
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

/// `writer` sends to `reader` over `channel`.
fn round_trip(
    emu: &Emulator,
    channel: u32,
    writer: ffi::ora_plugin_type_t,
    reader: ffi::ora_plugin_type_t,
) -> Result<(), String> {
    let data = b"serial port 2";
    emu.set_calling_plugin(writer);
    let opened = emu.log_open_write(channel, c"writer");
    let wrote = emu.log_write(channel, data);
    emu.set_calling_plugin(reader);
    let read_opened = emu.log_open_read(channel);
    let (read, bytes) = emu.log_read(channel, 64);
    let read_closed = emu.log_close_read(channel);
    emu.set_calling_plugin(writer);
    let closed = emu.log_close_write(channel);

    let results = [opened, wrote, read_opened, read, read_closed, closed];
    if results.iter().any(|&r| r != OraResult::Ok) || bytes != data {
        return Err(format!(
            "channel {channel}: open, write, open read, read, close read and close {results:?}, \
             read {bytes:x?}"
        ));
    }
    Ok(())
}

fn check_created(emu: &Emulator) -> Result<(), String> {
    place([(0x40, 0), (0x100, 0x22)]);
    emu.launch_usb_dpram(0);

    for (channel, want) in [(CH2, 0x100), (CH3, 0x22)] {
        let (r, size, _, _) = emu.log_query(channel);
        if (r, size) != (OraResult::Ok, want) {
            return Err(format!(
                "channel {channel}: query {r:?} with size {size:#x}, want size {want:#x}"
            ));
        }
    }

    // Channel 3's 0x22 bytes round up to 0x24.
    let found = first_alloc(emu);
    if found != Ok(USB_DPRAM_BASE_ADDR + 0x124) {
        return Err(format!(
            "the first allocation is {found:x?}, not after the channels at offset 0x124"
        ));
    }

    round_trip(emu, CH2, USER, SYSTEM)?;
    round_trip(emu, CH3, SYSTEM, USER)
}

fn check_absent(emu: &Emulator) -> Result<(), String> {
    place([(0, 0), (0, 0)]);
    emu.launch_usb_dpram(0);
    for channel in [CH2, CH3] {
        let (r, ..) = emu.log_query(channel);
        if r != OraResult::NotSupported {
            return Err(format!("channel {channel} with size 0: query {r:?}"));
        }
    }
    Ok(())
}
