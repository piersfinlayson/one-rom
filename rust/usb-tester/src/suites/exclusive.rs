// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Exclusive serial IN mode, where the serial port sends only channel 0.
//!
//! Without the entry flag the plugin behaves as the log and input suites
//! assert.

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_metadata::{PROPERTY1_EXCLUSIVE_SERIAL_IN, onerom_firmware_flag_t};
use onerom_plugin_tester::run::Outcome;

use crate::device::{self, Device};
use crate::suites::input;
use crate::suites::log::BANNER_TITLE;
use crate::{Ctx, Scenario};

const FLAG_IN: u8 = ffi::ORA_ENTRY_FLAG_EXCLUSIVE_SERIAL_IN as u8;

const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USER: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER;

const CHANNEL: u32 = 0;

/// `ONEROM_RTT_UP_BUFFER_SIZE` in `firmware/src/rtt.c`.
const CHANNEL_0_SIZE: u32 = 3072;

const BOOT_LOG: &[u8] = b"boot log line\r\n";

const LATER_LOG: &[u8] = b"later log line\r\n";

/// `LOG_DRAIN_SETTLE_MS` in `usb_log.c`.
const SETTLE_MS: u64 = 250;

/// Passes to run, enough for a whole banner.
const PASSES: u32 = 40;

fn arrange(emu: &Emulator, user: u8, flags: u8) {
    if device::install_plugin_slots(emu, user) {
        device::set_entry_flags(flags);
    }

    // The log's contents outlive a boot here.
    while !emu.fw_log_read(CHANNEL_0_SIZE).is_empty() {}
    emu.fw_log_write(BOOT_LOG);
    emu.launch_log_move(flags);
}

fn arrange_exclusive_in(emu: &Emulator) {
    arrange(emu, PROPERTY1_EXCLUSIVE_SERIAL_IN, FLAG_IN);
}

fn arrange_no_request(emu: &Emulator) {
    arrange(emu, 0, 0);
}

/// `None` if the firmware launches the plugins with `flags`, or a skip if the
/// config has fewer than two ROM slots.
fn arranged(flags: u8) -> Result<Option<Outcome>, String> {
    if !device::plugin_slots_installed() {
        return Ok(Some(Outcome::Skip(
            "the config has fewer than two ROM slots for the plugin headers".to_string(),
        )));
    }
    if !Emulator::user_plugin_to_start() {
        return Err("the firmware doesn't start the installed user plugin".to_string());
    }
    let found = Emulator::plugin_entry_flags();
    if found != flags {
        return Err(format!(
            "the firmware's entry flags are {found:#04x}, not {flags:#04x}"
        ));
    }
    Ok(None)
}

fn user_plugin_write(emu: &Emulator, bytes: &[u8]) -> Result<(), String> {
    emu.set_calling_plugin(USER);
    let claimed = emu.log_open_write(CHANNEL, c"user");
    let wrote = emu.log_write(CHANNEL, bytes);
    emu.set_calling_plugin(SYSTEM);

    if claimed != OraResult::Ok && claimed != OraResult::LogChannelInUse {
        return Err(format!(
            "the user plugin's claim of channel 0 returned {claimed:?}"
        ));
    }
    if wrote != OraResult::Ok {
        return Err(format!(
            "the user plugin's write to channel 0 returned {wrote:?}"
        ));
    }
    Ok(())
}

fn channel_0_pending(emu: &Emulator) -> Result<u32, String> {
    let (r, _, _, pending) = emu.log_query(CHANNEL);
    if r != OraResult::Ok {
        return Err(format!("query of channel 0 failed: {r:?}"));
    }
    Ok(pending)
}

fn attach_and_settle(dev: &mut Device) -> Result<(), String> {
    dev.set_dtr(true);
    dev.step()?;
    dev.advance_ms(SETTLE_MS);
    Ok(())
}

// ---------------------------------------------------------------------------

fn the_log_moved_at_launch(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = arranged(FLAG_IN)? {
        return Ok(skip);
    }
    let emu = dev.emulator();

    let up0 = emu.rtt_up(0).ok_or("up[0] has no buffer")?;
    if up0.is_usb_dpram_log || up0.size != CHANNEL_0_SIZE {
        return Err(format!("up[0] isn't channel 0: {up0:?}"));
    }
    let up1 = emu.rtt_up(1).ok_or("up[1] has no buffer")?;
    if !emu.log_moved() || !up1.is_usb_dpram_log {
        return Err(format!(
            "the log didn't move to USB DPRAM in up[1]: {up1:?}"
        ));
    }
    let flags = emu.firmware_flags();
    if flags & onerom_firmware_flag_t::FIRMWARE_FLAG_LOG_IN_UP1 == 0 {
        return Err(format!(
            "the firmware flags are {flags:#04x}, without FIRMWARE_FLAG_LOG_IN_UP1"
        ));
    }

    let moved = emu.fw_log_read(up1.size);
    if moved != BOOT_LOG {
        return Err(format!(
            "the USB DPRAM log holds {:?}, not the boot log",
            String::from_utf8_lossy(&moved)
        ));
    }
    let pending = channel_0_pending(emu)?;
    if pending != 0 {
        return Err(format!("channel 0 holds {pending} bytes after the move"));
    }

    Ok(Outcome::Pass)
}

fn tinyusb_skips_its_reset(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = arranged(FLAG_IN)? {
        return Ok(skip);
    }
    check_tinyusb_resets(dev, 0)
}

/// The plugin starts with entry flags 0, as firmware before 0.8.1 starts it.
fn tinyusb_resets_without_the_mode(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    check_tinyusb_resets(dev, 1)
}

fn check_tinyusb_resets(dev: &Device, resets: u32) -> Result<Outcome, String> {
    let found = dev.tinyusb_inits_and_resets();
    if found != (1, resets) {
        return Err(format!(
            "tinyusb's initialisations and USB controller resets were {found:?}, not \
             (1, {resets})"
        ));
    }
    Ok(Outcome::Pass)
}

fn no_banner_when_a_terminal_attaches(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = arranged(FLAG_IN)? {
        return Ok(skip);
    }

    attach_and_settle(dev)?;
    let out = dev.collect_cdc(PASSES)?;
    if !out.is_empty() {
        return Err(format!(
            "the port received {} bytes with nothing in channel 0: {out:?}",
            out.len()
        ));
    }

    Ok(Outcome::Pass)
}

fn the_log_stays_off_the_port(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = arranged(FLAG_IN)? {
        return Ok(skip);
    }

    if dev.emulator().fw_log_write(LATER_LOG) != LATER_LOG.len() as u32 {
        return Err("the later log line was dropped".to_string());
    }
    attach_and_settle(dev)?;
    let out = dev.collect_cdc(PASSES)?;
    if !out.is_empty() {
        return Err(format!(
            "One ROM's log reached the port: {} bytes, {out:?}",
            out.len()
        ));
    }

    let log_size = dev.emulator().rtt_up(1).ok_or("up[1] has no buffer")?.size;
    let log = dev.emulator().fw_log_read(log_size);
    if log != [BOOT_LOG, LATER_LOG].concat() {
        return Err(format!(
            "the USB DPRAM log holds {:?}, not both log lines",
            String::from_utf8_lossy(&log)
        ));
    }

    Ok(Outcome::Pass)
}

fn output_reaches_the_port_without_dtr(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = arranged(FLAG_IN)? {
        return Ok(skip);
    }

    let reply = b"\x14\x09O";
    user_plugin_write(dev.emulator(), reply)?;
    dev.step()?;
    let out = dev.take_cdc();
    if out != reply {
        return Err(format!(
            "the port received {out:02x?}, not the user plugin's {reply:02x?}"
        ));
    }

    Ok(Outcome::Pass)
}

fn output_waits_for_the_bus(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = arranged(FLAG_IN)? {
        return Ok(skip);
    }

    let reply = b"reply";
    dev.set_connected(false);
    user_plugin_write(dev.emulator(), reply)?;
    let early = dev.collect_cdc(PASSES)?;
    if !early.is_empty() {
        return Err(format!(
            "{} bytes crossed a bus that isn't ready: {early:?}",
            early.len()
        ));
    }
    let held = channel_0_pending(dev.emulator())?;
    if held != reply.len() as u32 {
        return Err(format!(
            "channel 0 holds {held} bytes, not the {} written",
            reply.len()
        ));
    }

    dev.set_connected(true);
    dev.step()?;
    let out = dev.take_cdc();
    if out != reply {
        return Err(format!(
            "the port received {out:02x?} once the bus was ready, not {reply:02x?}"
        ));
    }

    Ok(Outcome::Pass)
}

fn output_is_held_when_the_endpoint_is_full(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    if let Some(skip) = arranged(FLAG_IN)? {
        return Ok(skip);
    }

    const ROOM: u32 = 16;
    let sent: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    dev.set_tx_capacity(ROOM);
    user_plugin_write(dev.emulator(), &sent)?;

    dev.step_n(PASSES)?;
    let held = channel_0_pending(dev.emulator())?;
    if held != sent.len() as u32 - ROOM {
        return Err(format!(
            "channel 0 holds {held} bytes with a full endpoint, not {}",
            sent.len() as u32 - ROOM
        ));
    }

    let mut got = dev.take_cdc();
    for _ in 0..(sent.len() as u32 / ROOM + 2) {
        dev.step()?;
        got.extend(dev.take_cdc());
    }
    if got != sent {
        return Err(format!(
            "received {} bytes, not all {} in order",
            got.len(),
            sent.len()
        ));
    }

    Ok(Outcome::Pass)
}

fn input_reaches_channel_1(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = arranged(FLAG_IN)? {
        return Ok(skip);
    }
    input::input_reaches_channel_1(dev, ctx)
}

fn input_is_held_when_channel_1_is_full(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = arranged(FLAG_IN)? {
        return Ok(skip);
    }
    input::input_is_held_when_channel_1_is_full(dev, ctx)
}

fn the_log_reaches_the_port_without_a_request(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    if let Some(skip) = arranged(0)? {
        return Ok(skip);
    }

    if dev.emulator().log_moved() {
        return Err("the log moved without exclusive serial IN".to_string());
    }
    attach_and_settle(dev)?;
    let out = dev.collect_cdc(PASSES)?;
    let boot_log = String::from_utf8_lossy(BOOT_LOG);
    let banner_at = out.find(BANNER_TITLE);
    let log_at = out.find(boot_log.as_ref());
    match (banner_at, log_at) {
        (Some(banner_at), Some(log_at)) if banner_at < log_at => Ok(Outcome::Pass),
        _ => Err(format!(
            "the port didn't receive the banner then the boot log: {out:?}"
        )),
    }
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "exclusive.the_log_moved_at_launch",
        about: "the boot log moved to USB DPRAM at launch survives tinyusb's initialisation",
        run: the_log_moved_at_launch,
        before_start: Some(arrange_exclusive_in),
    },
    Scenario {
        name: "exclusive.tinyusb_skips_its_reset",
        about: "tinyusb's initialisation doesn't reset the USB controller",
        run: tinyusb_skips_its_reset,
        before_start: Some(arrange_exclusive_in),
    },
    Scenario {
        name: "exclusive.tinyusb_resets_without_the_mode",
        about: "without the mode tinyusb's initialisation resets the USB controller",
        run: tinyusb_resets_without_the_mode,
        before_start: None,
    },
    Scenario {
        name: "exclusive.no_banner_when_a_terminal_attaches",
        about: "a terminal attaching doesn't get a banner",
        run: no_banner_when_a_terminal_attaches,
        before_start: Some(arrange_exclusive_in),
    },
    Scenario {
        name: "exclusive.the_log_stays_off_the_port",
        about: "One ROM's log reaches the USB DPRAM log and not the port",
        run: the_log_stays_off_the_port,
        before_start: Some(arrange_exclusive_in),
    },
    Scenario {
        name: "exclusive.output_reaches_the_port_without_dtr",
        about: "the user plugin's output reaches the port without DTR or a settle window",
        run: output_reaches_the_port_without_dtr,
        before_start: Some(arrange_exclusive_in),
    },
    Scenario {
        name: "exclusive.output_waits_for_the_bus",
        about: "the user plugin's output stays in channel 0 until the bus is ready",
        run: output_waits_for_the_bus,
        before_start: Some(arrange_exclusive_in),
    },
    Scenario {
        name: "exclusive.output_is_held_when_the_endpoint_is_full",
        about: "a full endpoint holds output in channel 0, and every byte arrives in order",
        run: output_is_held_when_the_endpoint_is_full,
        before_start: Some(arrange_exclusive_in),
    },
    Scenario {
        name: "exclusive.input_reaches_channel_1",
        about: "what a terminal sends lands on channel 1, in order, and not on channel 0",
        run: input_reaches_channel_1,
        before_start: Some(arrange_exclusive_in),
    },
    Scenario {
        name: "exclusive.input_is_held_when_channel_1_is_full",
        about: "a full channel 1 holds the host off, and every byte arrives once it drains",
        run: input_is_held_when_channel_1_is_full,
        before_start: Some(arrange_exclusive_in),
    },
    Scenario {
        name: "exclusive.the_log_reaches_the_port_without_a_request",
        about: "with neither plugin requesting the mode the port receives the banner then the boot log",
        run: the_log_reaches_the_port_without_a_request,
        before_start: Some(arrange_no_request),
    },
];
