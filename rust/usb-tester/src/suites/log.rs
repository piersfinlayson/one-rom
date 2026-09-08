// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Forwarding the device's log to the CDC serial port.
//!
//! The plugin claims the log channel at startup and, while a terminal has the
//! port open, copies what the firmware and the other plugin write into the CDC
//! IN endpoint, opening each session with a banner naming the device.
//!
//! Almost everything interesting here is a response to something the endpoint
//! or the terminal did rather than to what the device logged, which is why the
//! shim models the endpoint's room rather than accepting whatever it is given.
//! A scenario arms a state, does one thing, and asserts against a nearby wrong
//! answer: a banner that arrives before the settle window, a line lost when the
//! endpoint filled, a session that inherited the last one's bytes.

use onerom_fw_emulator::{Emulator, OraResult};

use crate::device::Device;
use crate::{Ctx, Scenario};
use onerom_plugin_tester::run::Outcome;

/// The log channel the plugin forwards.
const CHANNEL: u32 = 0;

/// What the plugin waits out before writing to a terminal that has just opened
/// the port — LOG_DRAIN_SETTLE_MS in usb_log.c.
const SETTLE_MS: u64 = 250;

/// The banner's opening line, and the rule that closes it.
const BANNER_TITLE: &str = "----- One ROM USB log -----";
const BANNER_RULE: &str = "---------------------------";

/// Passes to run when the plugin has a whole banner to write and a full packet
/// of room.  Generous: the banner spans several passes even then, and a pass
/// costs nothing.
const PASSES: u32 = 40;

/// Attach a terminal and let the settle window elapse, leaving the plugin free
/// to write.
///
/// The window is armed on the first pass after DTR rises, so the clock is moved
/// only after that pass has run — moving it first would leave the deadline in
/// the future by the whole window and prove nothing about the wait.
fn attach_and_settle(dev: &mut Device) -> Result<(), String> {
    dev.set_dtr(true);
    dev.step()?;
    dev.advance_ms(SETTLE_MS);
    Ok(())
}

/// The whole banner, as one string, after a terminal attaches.
fn banner_after_attach(dev: &mut Device) -> Result<String, String> {
    attach_and_settle(dev)?;
    dev.collect_cdc(PASSES)
}

// ---------------------------------------------------------------------------

/// Nothing reaches the port until a terminal opens it.
///
/// A device with USB but no terminal must still let a debug probe read the log,
/// so the channel fills and nothing is forwarded.  The discriminating half is
/// the second: the same device, the same log, one terminal — and now it speaks.
fn nothing_before_a_terminal_attaches(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let quiet = dev.collect_cdc(PASSES)?;
    if !quiet.is_empty() {
        return Err(format!(
            "the port carried {} bytes with no terminal attached: {quiet:?}",
            quiet.len()
        ));
    }

    let banner = banner_after_attach(dev)?;
    if banner.is_empty() {
        return Err("the port stayed silent after a terminal attached".to_string());
    }

    Ok(Outcome::Pass)
}

/// A session opens with a titled rule and closes with a plain one.
///
/// The closing rule is what says the port has finished introducing itself, so a
/// banner that opened and never closed would leave a reader waiting.
fn the_banner_opens_and_closes(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let banner = banner_after_attach(dev)?;

    let Some(title_at) = banner.find(BANNER_TITLE) else {
        return Err(format!("no opening rule in {banner:?}"));
    };
    // Searched past the title, because the two are the same width and the title
    // line would otherwise match the closing rule's own prefix.
    let Some(rule_at) = banner[title_at + BANNER_TITLE.len()..].find(BANNER_RULE) else {
        return Err(format!(
            "no closing rule after the opening one in {banner:?}"
        ));
    };
    if rule_at == 0 {
        return Err(format!(
            "the banner closed immediately after opening, with nothing between: {banner:?}"
        ));
    }

    Ok(Outcome::Pass)
}

/// The banner says which device the reader has attached to.
///
/// A reader who gets an unlabelled stream cannot tell a quiet device from the
/// wrong port, which is the whole reason the banner is written at all.
fn the_banner_names_the_device(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let banner = banner_after_attach(dev)?;

    if !banner.contains("One ROM") {
        return Err(format!("the banner does not name the product: {banner:?}"));
    }

    // The serial the shim's chip ID produces.  A device reads this out of OTP,
    // and the plugin narrows picoboot's UTF-16 to ASCII, so asserting the
    // digits asserts that whole path rather than only that something appeared.
    if !banner.contains("0123456789ABCDEF") {
        return Err(format!(
            "the banner does not carry the chip ID as the serial: {banner:?}"
        ));
    }

    Ok(Outcome::Pass)
}

/// Nothing is written until the settle window has passed.
///
/// A host opening the port is not necessarily reading it yet, and the bytes
/// written in between are lost.  Nothing on the device can tell a reader from
/// an open file descriptor, so the only defence is to wait — and a wait that
/// did not actually hold would lose the boot log to every terminal.
fn the_settle_window_holds_output_back(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    dev.set_dtr(true);
    dev.step()?;

    // One millisecond short of the deadline: as close as the window can be
    // approached without reaching it, so this fails if the comparison is off by
    // one in the direction that would let output through early.
    dev.advance_ms(SETTLE_MS - 1);
    let early = dev.collect_cdc(PASSES)?;
    if !early.is_empty() {
        return Err(format!(
            "{} bytes were written {}ms into a {SETTLE_MS}ms settle window: {early:?}",
            early.len(),
            SETTLE_MS - 1
        ));
    }

    dev.advance_ms(1);
    let after = dev.collect_cdc(PASSES)?;
    if !after.contains(BANNER_TITLE) {
        return Err(format!(
            "the banner did not follow the settle window: {after:?}"
        ));
    }

    Ok(Outcome::Pass)
}

/// A banner the endpoint could not take in one go arrives whole anyway.
///
/// The banner is bigger than the endpoint even at full size, so it is resumed
/// from a cursor and every line generated again from there.  Squeezed to a byte
/// at a time, a cursor that advanced by what was generated rather than by what
/// was taken would duplicate or drop, and the result would no longer match.
fn the_banner_resumes_through_a_full_endpoint(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let whole = banner_after_attach(dev)?;

    // A second session, the same device, one byte of room per pass.  Enough
    // passes for the banner to come out a byte at a time and then some.
    dev.set_dtr(false);
    dev.set_tx_capacity(1);
    dev.set_dtr(true);
    dev.step()?;
    dev.advance_ms(SETTLE_MS);
    let squeezed = dev.collect_cdc(whole.len() as u32 * 2)?;

    // Compared over the banner alone: what follows it is forwarded log, and how
    // much of that has arrived depends on how many passes each side ran.
    let end = whole
        .find(BANNER_RULE)
        .map(|at| at + BANNER_RULE.len())
        .ok_or_else(|| format!("no closing rule in the unsqueezed banner: {whole:?}"))?;
    let want = &whole[..end];

    if squeezed.len() < want.len() || &squeezed[..want.len()] != want {
        return Err(format!(
            "a banner written a byte at a time differs from the same banner written whole\n\
             whole:    {want:?}\n\
             squeezed: {:?}",
            &squeezed[..squeezed.len().min(want.len())]
        ));
    }

    Ok(Outcome::Pass)
}

/// A terminal that closes and reopens the port gets the banner again.
///
/// tinyusb reports the transition rather than the level, so a close and reopen
/// between two passes is still both — and the second terminal has no idea what
/// the first was told.
fn a_new_session_starts_the_banner_again(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let first = banner_after_attach(dev)?;
    if !first.contains(BANNER_TITLE) {
        return Err(format!("no banner for the first terminal: {first:?}"));
    }

    dev.set_dtr(false);
    dev.step()?;
    let _ = dev.take_cdc_text();

    dev.set_dtr(true);
    dev.step()?;
    dev.advance_ms(SETTLE_MS);
    let second = dev.collect_cdc(PASSES)?;

    if !second.contains(BANNER_TITLE) {
        return Err(format!(
            "the second terminal was not given a banner of its own: {second:?}"
        ));
    }

    Ok(Outcome::Pass)
}

/// A bus that cannot carry data pauses the forwarding and resumes it.
///
/// A host suspending drops the connection while the terminal stays open, so the
/// backlog is left where it is rather than written into a bus that will not
/// take it.
fn a_suspended_bus_carries_nothing(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    dev.set_connected(false);
    attach_and_settle(dev)?;
    let suspended = dev.collect_cdc(PASSES)?;
    if !suspended.is_empty() {
        return Err(format!(
            "{} bytes crossed a suspended bus: {suspended:?}",
            suspended.len()
        ));
    }

    dev.set_connected(true);
    let resumed = dev.collect_cdc(PASSES)?;
    if !resumed.contains(BANNER_TITLE) {
        return Err(format!(
            "the banner did not resume when the bus came back: {resumed:?}"
        ));
    }

    Ok(Outcome::Pass)
}

/// What the device logged reaches the terminal, after the banner.
///
/// The banner going out whole before any log byte is what lets a reader tell
/// the two apart, and forwarding nothing at all would leave a device that looks
/// identical to one whose log is empty.
fn the_log_follows_the_banner(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    // Written through the API a second plugin would use, so what the drain
    // forwards has arrived the way a plugin's own log line does.  The channel
    // is claimed for writing first, as any writer must.
    let marker = "usb-tester log marker";
    let emu = dev.emulator();
    let claim = emu.log_open_write(CHANNEL, c"usb-tester");
    if claim != OraResult::Ok {
        return Err(format!(
            "could not claim the log channel for writing: {claim:?}"
        ));
    }
    let wrote = emu.log_write(CHANNEL, marker.as_bytes());
    if wrote != OraResult::Ok {
        return Err(format!("could not write to the log channel: {wrote:?}"));
    }

    // A packet of room lets the whole banner out in one pass, which is the one
    // condition under which the ordering cannot be got wrong.  Squeezed, the
    // banner spans passes, and a device that wrote log bytes while it still had
    // banner to write would interleave the two.
    dev.set_tx_capacity(8);
    let out = banner_after_attach(dev)?;

    let Some(rule_at) = out.rfind(BANNER_RULE) else {
        return Err(format!("no banner to follow: {out:?}"));
    };
    let Some(marker_at) = out.find(marker) else {
        return Err(format!("the log did not reach the terminal: {out:?}"));
    };
    if marker_at < rule_at {
        return Err(format!(
            "log content appeared before the banner finished: {out:?}"
        ));
    }

    Ok(Outcome::Pass)
}

/// Claim the log channel's reader, so the plugin cannot.
fn claim_log_reader(emu: &Emulator) {
    emu.log_open_read(CHANNEL);
}

/// A device whose log another plugin is already reading says so.
///
/// Only one reader is allowed, and this plugin loses gracefully — but a
/// terminal that then received nothing would have no way to tell that from a
/// device with nothing to say.  The note must not claim the firmware is too
/// old, which is the one thing it is not.
fn another_reader_is_named_in_the_banner(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let banner = banner_after_attach(dev)?;

    if !banner.contains(BANNER_TITLE) {
        return Err(format!(
            "no banner on a device whose log is already being read: {banner:?}"
        ));
    }
    if !banner.contains("another plugin is already reading the log") {
        return Err(format!(
            "the banner does not say why nothing will be forwarded: {banner:?}"
        ));
    }

    Ok(Outcome::Pass)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "log.nothing_before_a_terminal_attaches",
        about: "a device with no terminal forwards nothing, and one with a terminal speaks",
        run: nothing_before_a_terminal_attaches,
        before_start: None,
    },
    Scenario {
        name: "log.the_banner_opens_and_closes",
        about: "the banner is bracketed by its rules, with the device between them",
        run: the_banner_opens_and_closes,
        before_start: None,
    },
    Scenario {
        name: "log.the_banner_names_the_device",
        about: "the banner carries the product and the serial derived from the chip ID",
        run: the_banner_names_the_device,
        before_start: None,
    },
    Scenario {
        name: "log.the_settle_window_holds_output_back",
        about: "nothing is written until the settle window has elapsed",
        run: the_settle_window_holds_output_back,
        before_start: None,
    },
    Scenario {
        name: "log.the_banner_resumes_through_a_full_endpoint",
        about: "a banner written a byte at a time matches one written whole",
        run: the_banner_resumes_through_a_full_endpoint,
        before_start: None,
    },
    Scenario {
        name: "log.a_new_session_starts_the_banner_again",
        about: "a terminal that reopens the port is given its own banner",
        run: a_new_session_starts_the_banner_again,
        before_start: None,
    },
    Scenario {
        name: "log.a_suspended_bus_carries_nothing",
        about: "a suspended bus pauses the forwarding and resumes it",
        run: a_suspended_bus_carries_nothing,
        before_start: None,
    },
    Scenario {
        name: "log.the_log_follows_the_banner",
        about: "what the device logged reaches the terminal, after the banner",
        run: the_log_follows_the_banner,
        before_start: None,
    },
    Scenario {
        name: "log.another_reader_is_named_in_the_banner",
        about: "a device whose log another plugin holds says so in its banner",
        run: another_reader_is_named_in_the_banner,
        before_start: Some(claim_log_reader),
    },
];
