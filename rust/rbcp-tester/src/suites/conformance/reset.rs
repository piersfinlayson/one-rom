// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Communication Initiation — Resetting the Device" and
//! "Group 0xAA — Reset".
//!
//! RBCP_RESET is deliberately unlike every other command.  Its GROUP and CMD
//! bytes are both 0xAA and identical to each other, so a device expecting
//! either receives the reset value; 0xAA is barred from every command's final
//! argument so a reset started mid-command is still detectable; and it never
//! produces a response, in either mode.
//!
//! Proving an exit happened is the awkward part, because "no response" is an
//! absence.  Two things make it positive instead.  ENTER_CMD_RESP is defined
//! to *fail* when the device is already in command-response mode, so an entry
//! that succeeds is proof the device had left.  And the response header's last
//! command field holds a value the test put there, so a reset that wrongly
//! wrote a response would replace it with 0xAA/0xAA.

use crate::driver::{Bus, Hdr, control, group, reset};
use crate::{Ctx, Outcome};

/// A reset in command-response mode writes nothing to the response header.
///
/// A NOP first, so the last command field holds a value this scenario chose
/// rather than whatever the image happened to contain.  The verified poke
/// afterwards is the fence: it proves the device processed the reset and is
/// back in command mode, so the header read that follows is not merely early.
pub fn exits_without_writing_a_response(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;
    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::CONTROL)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)?;

    bus.send_reset(s.command_page)?;

    // Fence: a knocked, verified poke proves the device has processed the
    // reset and is taking command-mode sessions again.
    let addr = ctx.probe_addr();
    let value = bus.read(addr)? ^ 0xFF;
    bus.poke_verified(ctx, addr, value)
        .map_err(|e| format!("after RBCP_RESET the device took no command-mode session: {e}"))?;

    // The last command field must still name the NOP.  A device that ran the
    // reset through the normal processing sequence would have written 0xAA.
    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::CONTROL)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)?;

    Ok(Outcome::Pass)
}

/// After a reset, command-response mode can be entered again.
///
/// ENTER_CMD_RESP "is not supported when in command-response mode — the device
/// returns failure", so an entry that succeeds is positive proof the reset
/// took the device out of that mode.
pub fn exit_allows_re_entry(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("first ENTER_CMD_RESP: {e}"))?;

    bus.send_reset(s.command_page)?;

    bus.enter_cmd_resp(&s).map_err(|e| {
        format!(
            "second ENTER_CMD_RESP: {e} — the device did not leave command-response \
                 mode on RBCP_RESET, or did not survive it"
        )
    })?;

    Ok(Outcome::Pass)
}

/// A reset off the command page must be filtered out and have no effect.
///
/// "For this reset to work it is crucial that the reset is issued using the
/// command page — if the device was in command-response mode, it is filtering
/// command bytes using the command page, and will ignore any command bytes
/// that do not match that page."  The NOP afterwards is the assertion: it can
/// only be processed by a device still in command-response mode.
pub fn off_page_reset_is_filtered(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.send_reset(ctx.other_page())?;

    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| {
            format!(
                "NOP after an off-page RBCP_RESET: {e} — the device acted on command bytes \
                 that did not match the configured command page"
            )
        })?;

    Ok(Outcome::Pass)
}

/// The specification's recommended reset sequence recovers a desynced device.
///
/// Five resets to flush any argument collection in progress, one for a now-idle
/// device, then a knock and one more in case it was in command mode.  The
/// device is deliberately left mid-argument first, which is the state the
/// sequence exists to recover from.
pub fn recommended_sequence_recovers_from_desync(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let page = ctx.command_page();
    let addr = ctx.probe_addr();
    let value = bus.read(addr)? ^ 0xFF;

    // Leave a SLOT_POKE two arguments in, three outstanding.
    let args = bus.poke_args(ctx, 0x0000, 0x00);
    bus.knock(page)?;
    bus.send_cmd(
        page,
        group::MODIFY,
        crate::driver::modify::SLOT_POKE,
        &args[..2],
    )?;

    bus.reset(page)?;

    bus.poke_verified(ctx, addr, value).map_err(|e| {
        format!(
            "{e} — the recommended reset sequence did not return the device to a state \
                 in which a knocked command is acted on"
        )
    })?;

    Ok(Outcome::Pass)
}

/// The reset command's two bytes are identical, as the specification requires.
///
/// Not a device behaviour but a constraint on the protocol constants, checked
/// here because everything above depends on it: the sequence works precisely
/// because a device expecting either a GROUP or a CMD receives the same value.
pub fn group_and_command_bytes_match(_bus: &mut Bus, _ctx: &Ctx) -> Result<Outcome, String> {
    if group::RESET != reset::RBCP_RESET {
        return Err(format!(
            "RBCP_RESET group is 0x{:02X} and command is 0x{:02X}; the specification requires \
             them to be identical so that a device expecting either receives the reset value",
            group::RESET,
            reset::RBCP_RESET
        ));
    }
    Ok(Outcome::Pass)
}
