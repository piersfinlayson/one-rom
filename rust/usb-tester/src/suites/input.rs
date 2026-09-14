// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Terminal input reaching the device.
//!
//! The plugin claims channel 1 for writing at startup.  Each pass it moves what
//! the terminal sent into the channel, up to the room available.  The rest
//! stays on the endpoint, which holds the host off.  Nothing is dropped.  Input
//! does not wait on the log going the other way.

use onerom_fw_emulator::{Emulator, OraResult};

use crate::device::Device;
use crate::{Ctx, Scenario};
use onerom_plugin_tester::run::Outcome;

/// The channel terminal input goes to.
const CHANNEL: u32 = 1;

/// The log channel, which input must leave alone.
const LOG_CHANNEL: u32 = 0;

/// Passes to run for a few packets to move.  A pass moves at most one packet.
const PASSES: u32 = 8;

/// Claim channel 1 for reading and drain it.
fn drain(emu: &Emulator) -> Result<Vec<u8>, String> {
    let claimed = emu.log_open_read(CHANNEL);
    if claimed != OraResult::Ok && claimed != OraResult::LogChannelInUse {
        return Err(format!(
            "could not claim channel 1 for reading: {claimed:?}"
        ));
    }
    let mut out = Vec::new();
    loop {
        let (r, got) = emu.log_read(CHANNEL, 256);
        if r != OraResult::Ok {
            return Err(format!("read of channel 1 failed: {r:?}"));
        }
        if got.is_empty() {
            return Ok(out);
        }
        out.extend_from_slice(&got);
    }
}

/// How many bytes channel `ch` holds.
fn pending(emu: &Emulator, ch: u32) -> Result<u32, String> {
    let (r, _, _, pending) = emu.log_query(ch);
    if r != OraResult::Ok {
        return Err(format!("query of channel {ch} failed: {r:?}"));
    }
    Ok(pending)
}

/// Terminal input lands on channel 1, in order, and nowhere else.
///
/// The log channel is checked too.  Bytes that went there instead would still
/// count as received.
fn input_reaches_channel_1(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let log_before = pending(dev.emulator(), LOG_CHANNEL)?;

    dev.push_cdc(b"hello, device");
    dev.step_n(PASSES)?;

    let got = drain(dev.emulator())?;
    if got != b"hello, device" {
        return Err(format!(
            "channel 1 holds {got:?}, want the bytes the terminal sent"
        ));
    }
    if dev.cdc_waiting() != 0 {
        return Err(format!("{} bytes left on the endpoint", dev.cdc_waiting()));
    }

    let log_after = pending(dev.emulator(), LOG_CHANNEL)?;
    if log_after != log_before {
        return Err(format!(
            "input moved the log channel from {log_before} to {log_after} bytes"
        ));
    }

    Ok(Outcome::Pass)
}

/// Input beyond what channel 1 holds waits on the endpoint, and moves once the
/// channel is drained.
///
/// A full channel holds one byte less than its size.  The final check is that
/// every byte sent arrives, in order, across the stall.
fn input_is_held_when_channel_1_is_full(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let (r, size, _, _) = dev.emulator().log_query(CHANNEL);
    if r != OraResult::Ok {
        return Err(format!("query of channel 1 failed: {r:?}"));
    }
    let sent: Vec<u8> = (0..(size + 300)).map(|i| (i % 251) as u8).collect();

    dev.push_cdc(&sent);
    dev.step_n(64)?;

    let held = pending(dev.emulator(), CHANNEL)?;
    if held != size - 1 {
        return Err(format!(
            "channel 1 holds {held} bytes, want it full at {}",
            size - 1
        ));
    }
    let waiting = dev.cdc_waiting();
    if waiting != sent.len() as u32 - held {
        return Err(format!(
            "{waiting} bytes waiting on the endpoint, want {}",
            sent.len() as u32 - held
        ));
    }

    // Draining the channel lets the rest through.
    let mut got = drain(dev.emulator())?;
    dev.step_n(64)?;
    got.extend(drain(dev.emulator())?);
    if got != sent {
        return Err(format!(
            "received {} bytes, want all {} in order",
            got.len(),
            sent.len()
        ));
    }
    if dev.cdc_waiting() != 0 {
        return Err(format!(
            "{} bytes still waiting after the drain",
            dev.cdc_waiting()
        ));
    }

    Ok(Outcome::Pass)
}

/// Input moves whether or not the port is open.
///
/// The log direction waits for DTR and a settle window.  Input does not, as
/// the channels are independent.
fn input_does_not_wait_for_the_terminal(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    dev.set_dtr(false);
    dev.push_cdc(b"early");
    dev.step_n(PASSES)?;

    let got = drain(dev.emulator())?;
    if got != b"early" {
        return Err(format!(
            "channel 1 holds {got:?} with the port closed, want the bytes sent"
        ));
    }

    Ok(Outcome::Pass)
}

/// Claim channel 1 for writing, so the plugin cannot.
fn claim_input_writer(emu: &Emulator) {
    emu.log_open_write(CHANNEL, c"other");
}

/// If the plugin cannot claim channel 1, input stays on the endpoint.
///
/// Firmware before 0.7.3 takes the same path, as the claim fails because the
/// channel does not exist.  Either way the plugin runs and forwards its log.
fn input_stays_on_the_port_without_the_channel(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    dev.push_cdc(b"nowhere to go");
    dev.step_n(PASSES)?;

    if dev.cdc_waiting() != 13 {
        return Err(format!(
            "{} bytes waiting on the endpoint, want all 13 left there",
            dev.cdc_waiting()
        ));
    }
    let held = pending(dev.emulator(), CHANNEL)?;
    if held != 0 {
        return Err(format!(
            "{held} bytes reached a channel the plugin does not hold"
        ));
    }

    Ok(Outcome::Pass)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "input.reaches_channel_1",
        about: "what a terminal sends lands on channel 1, in order, and not on the log",
        run: input_reaches_channel_1,
        before_start: None,
    },
    Scenario {
        name: "input.is_held_when_channel_1_is_full",
        about: "a full channel holds the host off, and every byte arrives once it drains",
        run: input_is_held_when_channel_1_is_full,
        before_start: None,
    },
    Scenario {
        name: "input.does_not_wait_for_the_terminal",
        about: "input moves with the port closed",
        run: input_does_not_wait_for_the_terminal,
        before_start: None,
    },
    Scenario {
        name: "input.stays_on_the_port_without_the_channel",
        about: "a plugin that could not claim channel 1 leaves input on the endpoint",
        run: input_stays_on_the_port_without_the_channel,
        before_start: Some(claim_input_writer),
    },
];
