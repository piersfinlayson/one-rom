// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Serial port 2's data, which the USB plugin moves between the port and log
//! channels 2 and 3.
//!
//! - Channel 2 goes to the host only while DTR is asserted.
//! - Data from the host is written to channel 3 up to its free space and the
//!   rest waits.
//! - Without channel 3 data from the host is discarded.
//!
//! The shim models one serial port, so each scenario keeps serial port 1 out of
//! the way. Serial port 1 holds its banner back for its settle time, which no
//! scenario here advances past, and its input is turned off by claiming
//! channel 1 before the plugin starts.

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_plugin_tester::run::Outcome;

use crate::device::Device;
use crate::suites::descriptors::{request_serial_port_2, serial_port_2_requested};
use crate::{Ctx, Scenario};

const SYSTEM: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_SYSTEM;
const USER: ffi::ora_plugin_type_t = ffi::ora_plugin_type_t_ORA_PLUGIN_TYPE_USER;

const TO_HOST: u32 = 2;
const FROM_HOST: u32 = 3;
const INPUT_CHANNEL_1: u32 = 1;

/// The user plugin's channel 3 size in the arrangements that have one.  Larger
/// than a packet, so a pass is limited by the packet and not the room.
const CHANNEL_3_SIZE: u16 = 0x100;

const DATA: &[u8] = b"serial port 2 data";

/// Passes to run for a few packets to move.
const PASSES: u32 = 8;

/// Serial port 2 is present, with serial port 1's input turned off and `DATA`
/// written to channel 2 by the user plugin.
fn arrange(emu: &Emulator, channel_3_size: u16) {
    request_serial_port_2(emu, 0, channel_3_size);
    emu.set_calling_plugin(USER);
    emu.log_open_write(INPUT_CHANNEL_1, c"other");
    emu.log_open_write(TO_HOST, c"user");
    emu.log_write(TO_HOST, DATA);
    emu.log_open_read(FROM_HOST);
    emu.set_calling_plugin(SYSTEM);
}

fn arrange_with_channel_3(emu: &Emulator) {
    arrange(emu, CHANNEL_3_SIZE);
}

fn arrange_without_channel_3(emu: &Emulator) {
    arrange(emu, 0);
}

fn pending(emu: &Emulator, channel: u32) -> Result<u32, String> {
    let (r, _, _, pending) = emu.log_query(channel);
    if r != OraResult::Ok {
        return Err(format!("query of channel {channel} failed: {r:?}"));
    }
    Ok(pending)
}

fn channel_3_contents(emu: &Emulator) -> Result<Vec<u8>, String> {
    emu.set_calling_plugin(USER);
    let (r, got) = emu.log_read(FROM_HOST, 256);
    emu.set_calling_plugin(SYSTEM);
    if r != OraResult::Ok {
        return Err(format!("read of channel 3 failed: {r:?}"));
    }
    Ok(got)
}

fn channel_2_waits_for_dtr(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = serial_port_2_requested()? {
        return Ok(skip);
    }

    let out = dev.collect_cdc(PASSES)?;
    let held = pending(dev.emulator(), TO_HOST)?;
    if !out.is_empty() || held != DATA.len() as u32 {
        return Err(format!(
            "without DTR the port received {out:?} and channel 2 holds {held} bytes, \
             want nothing sent and {} held",
            DATA.len()
        ));
    }
    Ok(Outcome::Pass)
}

fn channel_2_reaches_the_host(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = serial_port_2_requested()? {
        return Ok(skip);
    }

    // No room on the endpoint holds channel 2 back.
    dev.set_tx_capacity(0);
    dev.set_dtr(true);
    dev.step_n(PASSES)?;
    let held = pending(dev.emulator(), TO_HOST)?;
    if held != DATA.len() as u32 {
        return Err(format!(
            "with no room on the endpoint channel 2 holds {held} bytes, want {}",
            DATA.len()
        ));
    }

    dev.set_tx_capacity(u32::MAX);
    let out = dev.collect_cdc(PASSES)?;
    if out.as_bytes() != DATA {
        return Err(format!(
            "the port received {out:?}, not {:?}",
            String::from_utf8_lossy(DATA)
        ));
    }
    Ok(Outcome::Pass)
}

fn host_data_reaches_channel_3(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = serial_port_2_requested()? {
        return Ok(skip);
    }

    dev.push_cdc(b"to the user plugin");
    dev.step_n(PASSES)?;
    let got = channel_3_contents(dev.emulator())?;
    if got != b"to the user plugin" || dev.cdc_waiting() != 0 {
        return Err(format!(
            "channel 3 holds {:?} with {} bytes waiting on the endpoint",
            String::from_utf8_lossy(&got),
            dev.cdc_waiting()
        ));
    }
    Ok(Outcome::Pass)
}

/// The ring holds one byte back, so channel 3 takes one byte less than its
/// size.
fn host_data_waits_while_channel_3_is_full(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    if let Some(skip) = serial_port_2_requested()? {
        return Ok(skip);
    }

    let sent = 300u32;
    let room = u32::from(CHANNEL_3_SIZE) - 1;
    dev.push_cdc(&vec![b'x'; sent as usize]);
    dev.step_n(PASSES)?;
    let held = pending(dev.emulator(), FROM_HOST)?;
    if held != room || dev.cdc_waiting() != sent - room {
        return Err(format!(
            "channel 3 holds {held} bytes with {} waiting on the endpoint, want {room} and {}",
            dev.cdc_waiting(),
            sent - room
        ));
    }
    Ok(Outcome::Pass)
}

fn host_data_is_discarded_without_channel_3(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    if let Some(skip) = serial_port_2_requested()? {
        return Ok(skip);
    }

    dev.push_cdc(b"nowhere to go");
    dev.step_n(PASSES)?;
    if dev.cdc_waiting() != 0 {
        return Err(format!(
            "{} bytes waiting on the endpoint, want all discarded",
            dev.cdc_waiting()
        ));
    }
    Ok(Outcome::Pass)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "serial_port_2.channel_2_waits_for_dtr",
        about: "channel 2 isn't sent until DTR is asserted",
        run: channel_2_waits_for_dtr,
        before_start: Some(arrange_with_channel_3),
    },
    Scenario {
        name: "serial_port_2.channel_2_reaches_the_host",
        about: "channel 2 is sent once DTR is asserted and the endpoint has room",
        run: channel_2_reaches_the_host,
        before_start: Some(arrange_with_channel_3),
    },
    Scenario {
        name: "serial_port_2.host_data_reaches_channel_3",
        about: "data from the host is written to channel 3",
        run: host_data_reaches_channel_3,
        before_start: Some(arrange_with_channel_3),
    },
    Scenario {
        name: "serial_port_2.host_data_waits_while_channel_3_is_full",
        about: "data from the host that doesn't fit in channel 3 waits on the endpoint",
        run: host_data_waits_while_channel_3_is_full,
        before_start: Some(arrange_with_channel_3),
    },
    Scenario {
        name: "serial_port_2.host_data_is_discarded_without_channel_3",
        about: "without channel 3 data from the host is discarded",
        run: host_data_is_discarded_without_channel_3,
        before_start: Some(arrange_without_channel_3),
    },
];
