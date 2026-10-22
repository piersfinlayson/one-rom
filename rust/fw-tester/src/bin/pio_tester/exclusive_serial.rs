// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The firmware's handling of the plugins' exclusive serial IN bits.
//!
//! - A plugin requesting exclusive serial IN starts. The user plugin doesn't
//!   start without the system plugin.
//! - Every started plugin has `ORA_ENTRY_FLAG_EXCLUSIVE_SERIAL_IN` if one of
//!   them requests exclusive serial IN.
//! - At plugin launch in exclusive serial IN mode the firmware resets the USB
//!   controller, moves One ROM's log to USB DPRAM with as much of channel 0's
//!   contents as fits, then sets `FIRMWARE_FLAG_LOG_IN_UP1`.
//! - After the move One ROM's log reaches only USB DPRAM and channel 0 holds
//!   only what its writer writes.
//! - Without exclusive serial IN the launch leaves the log, the USB controller
//!   and `FIRMWARE_FLAG_LOG_IN_UP1` alone.
//! - A plugin's claim doesn't move the log.

use onerom_fw_emulator::{Emulator, OraResult, UsbReset, UsbResetStep, ffi};
use onerom_metadata::{PROPERTY1_EXCLUSIVE_SERIAL_IN, onerom_firmware_flag_t};

use crate::flash_range::{PLUGINS, check_plugin_flash_slots};
use crate::report::TestReport;

const IN: u8 = PROPERTY1_EXCLUSIVE_SERIAL_IN;
const FLAG_IN: u8 = ffi::ORA_ENTRY_FLAG_EXCLUSIVE_SERIAL_IN as u8;
const LOG_SIZE: u32 = ffi::USB_DPRAM_LOG_SIZE;
const MOVED_FLAG: u8 = onerom_firmware_flag_t::FIRMWARE_FLAG_LOG_IN_UP1;

const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USER: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER;

/// `ONEROM_RTT_UP_BUFFER_SIZE` in `firmware/src/rtt.c`.
const CHANNEL_0_SIZE: u32 = 3072;

pub fn run(report: &mut TestReport) {
    let addrs = PLUGINS.map(|p| Emulator::rom_slot_flash_addr(p.index));
    Emulator::set_sel_image(0);
    let emulator = Emulator::boot();
    Emulator::install_plugin_slots();

    report.add_check("exclusive serial: launch check", check_launch());
    report.add_check("exclusive serial: entry flags", check_entry_flags());
    report.add_check("exclusive serial: log move", check_log_move(&emulator));
    report.add_check(
        "exclusive serial: launches that don't move the log",
        check_no_move(&emulator),
    );
    report.add_check(
        "exclusive serial: boot log truncated to fit",
        check_truncation(&emulator),
    );
    report.add_check(
        "exclusive serial: launch decisions",
        check_launch_decisions(&emulator),
    );
    report.add_check("exclusive serial: entry args", check_entry_args());
    place(0, 0);
    report.add_check(
        "exclusive serial: flash slot calls on the plugin slots",
        check_plugin_flash_slots(&emulator, 2),
    );

    emulator.reset_log_claims();
    emulator.unmove_log();
    Emulator::restore_rom_slots();
    for (p, addr) in PLUGINS.iter().zip(addrs) {
        Emulator::set_rom_slot_flash_addr(p.index, addr);
    }
}

fn place(system: u8, user: u8) {
    PLUGINS[0].place(true, 0, system);
    PLUGINS[1].place(true, 0, user);
}

/// Each case's system and user properties1 and the entry flags.
const CASES: [(&str, u8, u8, u8); 4] = [
    ("serial bits unset", 0, 0, 0),
    ("user IN", 0, IN, FLAG_IN),
    ("system IN", IN, 0, FLAG_IN),
    ("both IN", IN, IN, FLAG_IN),
];

fn starts(started: bool) -> &'static str {
    if started { "starts" } else { "doesn't start" }
}

fn check_launch() -> Result<(), String> {
    for (label, system, user, _) in CASES {
        place(system, user);
        let found = (
            Emulator::system_plugin_to_launch(),
            Emulator::user_plugin_to_start(),
        );
        if found != (true, true) {
            return Err(format!(
                "{label}: the firmware {} the system plugin and {} the user plugin",
                starts(found.0),
                starts(found.1)
            ));
        }
    }

    place(0, IN);
    PLUGINS[0].place(false, 0, 0);
    if Emulator::user_plugin_to_start() || Emulator::plugin_entry_flags() != 0 {
        return Err(
            "serial IN with an invalid system plugin: the user plugin starts or the entry \
             flags aren't zero"
                .to_string(),
        );
    }
    Ok(())
}

fn check_entry_flags() -> Result<(), String> {
    for (label, system, user, flags) in CASES {
        place(system, user);
        let found = Emulator::plugin_entry_flags();
        if found != flags {
            return Err(format!(
                "{label}: entry flags {found:#04x}, not {flags:#04x}"
            ));
        }
    }

    place(IN, 0);
    PLUGINS[1].place(false, 0, 0);
    let found = Emulator::plugin_entry_flags();
    if found != FLAG_IN {
        return Err(format!(
            "system IN without a user plugin: entry flags {found:#04x}, not {FLAG_IN:#04x}"
        ));
    }
    Ok(())
}

fn check_unmoved(emu: &Emulator, when: &str) -> Result<(), String> {
    let up0 = emu
        .rtt_up(0)
        .ok_or(format!("{when}: up[0] has no buffer"))?;
    if emu.log_moved() || up0.is_usb_dpram_log || up0.size != CHANNEL_0_SIZE {
        return Err(format!("{when}: the log moved ({up0:?})"));
    }
    if emu.rtt_up(1).is_some() {
        return Err(format!("{when}: up[1] has a buffer"));
    }
    if emu.firmware_flags() & MOVED_FLAG != 0 {
        return Err(format!("{when}: FIRMWARE_FLAG_LOG_IN_UP1 is set"));
    }
    Ok(())
}

fn reset(emu: &Emulator) {
    emu.reset_log_claims();
    unsafe { ffi::onerom_test_reset() };
    emu.set_firmware_flags(emu.firmware_flags() & !MOVED_FLAG);
}

/// Also empties channel 0 while it shares a ring with the log.
fn drain_log(emu: &Emulator) {
    while !emu.fw_log_read(CHANNEL_0_SIZE).is_empty() {}
}

fn check_log_move(emu: &Emulator) -> Result<(), String> {
    reset(emu);
    place(0, IN);
    drain_log(emu);

    let boot_log = b"boot log line\r\n";
    if emu.fw_log_write(boot_log) != boot_log.len() as u32 {
        return Err("the boot log write was dropped".to_string());
    }
    check_unmoved(emu, "before the launch")?;
    if !emu.usb_resets().is_empty() {
        return Err(format!(
            "the USB controller's reset changed before the launch: {:?}",
            emu.usb_resets()
        ));
    }

    if !emu.launch_log_move(Emulator::plugin_entry_flags()) {
        return Err("the launch didn't move the log".to_string());
    }

    let before_move = |step| UsbReset {
        step,
        log_moved: false,
    };
    let resets = emu.usb_resets();
    if resets != [UsbResetStep::Assert, UsbResetStep::Release].map(before_move) {
        return Err(format!(
            "the USB controller's reset at launch wasn't an assert then a release \
             before the move: {resets:?}"
        ));
    }
    if emu.firmware_flags() & MOVED_FLAG == 0 {
        return Err("FIRMWARE_FLAG_LOG_IN_UP1 isn't set".to_string());
    }

    emu.set_calling_plugin(SYSTEM);
    let claimed = emu.log_open_read(0);
    if claimed != OraResult::Ok {
        return Err(format!("the system plugin's claim returned {claimed:?}"));
    }

    let up0 = emu.rtt_up(0).ok_or("up[0] has no buffer after the move")?;
    let up1 = emu.rtt_up(1).ok_or("up[1] has no buffer after the move")?;
    if up0.is_usb_dpram_log || up0.size != CHANNEL_0_SIZE || up0.write_offset != up0.read_offset {
        return Err(format!("up[0] isn't an empty channel 0: {up0:?}"));
    }
    if !emu.log_moved()
        || !up1.is_usb_dpram_log
        || up1.size != LOG_SIZE
        || up1.read_offset != 0
        || up1.write_offset != boot_log.len() as u32
    {
        return Err(format!(
            "up[1] isn't the USB DPRAM log holding the boot log: {up1:?}"
        ));
    }

    let moved = emu.fw_log_read(LOG_SIZE);
    if moved != boot_log {
        return Err(format!(
            "the USB DPRAM log holds {:?}, not the boot log",
            String::from_utf8_lossy(&moved)
        ));
    }
    let (r, bytes) = emu.log_read(0, CHANNEL_0_SIZE);
    if r != OraResult::Ok || !bytes.is_empty() {
        return Err(format!(
            "channel 0 after the move: {r:?}, {} bytes",
            bytes.len()
        ));
    }

    let log_line = b"later log line\r\n";
    emu.fw_log_write(log_line);
    let (_, bytes) = emu.log_read(0, CHANNEL_0_SIZE);
    if !bytes.is_empty() {
        return Err("One ROM's log reached channel 0 after the move".to_string());
    }
    if emu.fw_log_read(LOG_SIZE) != log_line {
        return Err("One ROM's log didn't reach the USB DPRAM log".to_string());
    }

    emu.set_calling_plugin(USER);
    let reply = b"\x14\x09O";
    let wrote = (emu.log_open_write(0, c"ostrich"), emu.log_write(0, reply));
    if wrote != (OraResult::Ok, OraResult::Ok) {
        return Err(format!(
            "the user plugin's claim and write returned {wrote:?}"
        ));
    }
    if !emu.fw_log_read(LOG_SIZE).is_empty() {
        return Err("the claimant's write reached the USB DPRAM log".to_string());
    }
    emu.set_calling_plugin(SYSTEM);
    let (r, bytes) = emu.log_read(0, CHANNEL_0_SIZE);
    if r != OraResult::Ok || bytes != reply {
        return Err(format!("channel 0 holds {bytes:?} ({r:?}), not the reply"));
    }

    emu.fw_log_write(log_line);
    if emu.launch_log_move(FLAG_IN) {
        return Err("a second move was made".to_string());
    }
    if emu.fw_log_read(LOG_SIZE) != log_line {
        return Err("a second move disturbed the USB DPRAM log".to_string());
    }

    reset(emu);
    check_unmoved(emu, "after a reset")
}

fn check_no_move(emu: &Emulator) -> Result<(), String> {
    let no_bits = || place(0, 0);
    let user_outside_flash = || {
        place(0, IN);
        PLUGINS[1].place(false, 0, IN);
    };
    let cases: [(&str, &dyn Fn(), ffi::ora_plugin_type_t); 3] = [
        ("no serial bits", &no_bits, SYSTEM),
        ("no serial bits and the user plugin's claim", &no_bits, USER),
        (
            "a user plugin outside the flash requesting serial IN",
            &user_outside_flash,
            SYSTEM,
        ),
    ];
    for (label, arrange, claimant) in cases {
        reset(emu);
        arrange();
        let flags = Emulator::plugin_entry_flags();
        if flags != 0 {
            return Err(format!("{label}: entry flags {flags:#04x}, not 0x00"));
        }
        if emu.launch_log_move(flags) {
            return Err(format!("{label}: the launch moved the log"));
        }
        if !emu.usb_resets().is_empty() {
            return Err(format!(
                "{label}: the launch changed the USB controller's reset: {:?}",
                emu.usb_resets()
            ));
        }
        emu.set_calling_plugin(claimant);
        let r = emu.log_open_read(0);
        if r != OraResult::Ok {
            return Err(format!("{label}: the claim of channel 0 returned {r:?}"));
        }
        check_unmoved(emu, label)?;
    }

    reset(emu);
    place(0, IN);
    emu.set_calling_plugin(SYSTEM);
    let r = emu.log_open_read(0);
    if r != OraResult::Ok {
        return Err(format!("the claim before the launch returned {r:?}"));
    }
    check_unmoved(emu, "a claim in exclusive serial IN mode before the launch")?;
    reset(emu);
    Ok(())
}

fn check_truncation(emu: &Emulator) -> Result<(), String> {
    reset(emu);
    place(0, IN);
    drain_log(emu);

    // Channel 0 is larger than the USB DPRAM log.
    let boot_log: Vec<u8> = (0..(CHANNEL_0_SIZE - 1)).map(|i| (i % 251) as u8).collect();
    if emu.fw_log_write(&boot_log) != boot_log.len() as u32 {
        return Err("the boot log write was dropped".to_string());
    }
    if !emu.launch_log_move(FLAG_IN) {
        return Err("the launch didn't move the log".to_string());
    }
    emu.set_calling_plugin(SYSTEM);
    let r = emu.log_open_read(0);
    if r != OraResult::Ok {
        return Err(format!("the system plugin's claim returned {r:?}"));
    }

    let fits = LOG_SIZE as usize - 1;
    let moved = emu.fw_log_read(LOG_SIZE);
    if moved != boot_log[..fits] {
        return Err(format!(
            "{} bytes of a {} byte boot log moved, not the first {fits}",
            moved.len(),
            boot_log.len()
        ));
    }
    let (_, rest) = emu.log_read(0, CHANNEL_0_SIZE);
    reset(emu);
    if !rest.is_empty() {
        return Err(format!("{} boot log bytes stayed in channel 0", rest.len()));
    }
    Ok(())
}

fn check_launch_decisions(emu: &Emulator) -> Result<(), String> {
    reset(emu);

    let cases = [
        ("serial bits unset", 0, 0, true, true),
        ("no system plugin started", 0, 0, false, false),
        ("user IN", 0, IN, true, true),
        ("system IN", IN, 0, true, true),
    ];
    for (label, system, user, system_launched, want) in cases {
        place(system, user);
        let found = Emulator::user_plugin_to_launch(system_launched);
        if found != want {
            return Err(format!("{label}: launch {} the user plugin", starts(found)));
        }
    }

    place(0, 0);
    Emulator::set_plugin_header(1, PLUGINS[1].region + PLUGINS[1].size, 0, 0);
    if Emulator::user_plugin_to_launch(true) {
        return Err("launch starts a user plugin with its entry point past its region".to_string());
    }

    place(0, 0);
    if !Emulator::system_plugin_to_launch() {
        return Err("launch doesn't start a valid system plugin".to_string());
    }
    PLUGINS[0].place(false, 0, 0);
    if Emulator::system_plugin_to_launch() {
        return Err("launch starts a system plugin outside the flash".to_string());
    }
    Ok(())
}

fn check_entry_args() -> Result<(), String> {
    for (label, plugin_type, core) in [
        ("system", SYSTEM, ffi::ora_core_t_ORA_CORE_1),
        ("user", USER, ffi::ora_core_t_ORA_CORE_0),
    ] {
        let ram = |a: &ffi::ora_entry_args_t| {
            (
                a.static_ram_base,
                a.static_ram_size,
                a.stack_top,
                a.stack_size,
            )
        };
        let base = ram(&Emulator::plugin_entry_args(plugin_type, 0));
        for flags in [0, FLAG_IN, 0xFF] {
            let args = Emulator::plugin_entry_args(plugin_type, flags);
            let want = flags & FLAG_IN;
            if args.core != core || args.flags != want || ram(&args) != base {
                return Err(format!(
                    "the {label} plugin's args with entry flags {flags:#04x}: core {}, \
                     flags {:#04x}, RAM {:?} against {base:?}",
                    args.core,
                    args.flags,
                    ram(&args)
                ));
            }
        }
    }

    let system = Emulator::plugin_entry_args(SYSTEM, 0);
    let user = Emulator::plugin_entry_args(USER, 0);
    if system.static_ram_base == user.static_ram_base || system.stack_top == user.stack_top {
        return Err("the system and user plugins share RAM".to_string());
    }
    Ok(())
}
