// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Command Processing Sequence" and "Response Header".
//!
//! On receipt of a command the device must, in order: set progress = pending,
//! increment the token, update last command, process the command, set the
//! response field, and finally set progress = complete.

use crate::driver::{
    Bus, CmdFailure, Hdr, RegionWrite, Session, aux, control, group, led, modify, nv, pipes, read,
};
use crate::{Ctx, Outcome};

/// The values written to one response header field, in order.
fn written_to(writes: &[RegionWrite], field: Hdr) -> Vec<u8> {
    writes
        .iter()
        .filter(|w| w.field == Some(field))
        .map(|w| w.val)
        .collect()
}

/// Index of the first write to a field, or None where it was never written.
fn first_write_to(writes: &[RegionWrite], field: Hdr) -> Option<usize> {
    writes.iter().position(|w| w.field == Some(field))
}

/// Index of the last write to a field, or None where it was never written.
fn last_write_to(writes: &[RegionWrite], field: Hdr) -> Option<usize> {
    writes.iter().rposition(|w| w.field == Some(field))
}

/// Render the header writes for a failure message, in the order made.
fn render(writes: &[RegionWrite]) -> String {
    let shown: Vec<String> = writes
        .iter()
        .filter(|w| w.field.is_some())
        .map(|w| w.to_string())
        .collect();
    if shown.is_empty() {
        "nothing".to_string()
    } else {
        shown.join(", ")
    }
}

/// The processing sequence, asserted over what the device wrote.
///
/// "On receipt of a command the device performs the following steps in order:
/// 1. Set progress = pending. 2. Increment token. 3. Update last command.
/// 4. Process command. 5. Set response. 6. Set progress = complete."
///
/// Read off [`Bus::region_writes`], not the bus.  The device runs a whole
/// command between two of the driver's turns, so a scenario polling the region
/// sees the state the command left and never the states it passed through.
///
/// `want_response` is what step 5 must write, which is status-OK for a command
/// that succeeded and failed for one the device refused.  The sequence is the
/// same either way — it is the steps a command *received* goes through, not the
/// steps a command that worked goes through.
fn check_sequence(
    writes: &[RegionWrite],
    s: &Session,
    want_token: u8,
    want_response: u8,
    group_byte: u8,
    cmd_byte: u8,
) -> Result<(), String> {
    // Step 2.  Any other value satisfies a host watching for the token to
    // change, and so tells it the command arrived when it has not.
    let token = written_to(writes, Hdr::TokenLsb);
    if token.is_empty() {
        return Err(format!(
            "the token LSB was never written — wrote {}",
            render(writes)
        ));
    }
    if let Some(bad) = token.iter().find(|&&v| v != want_token) {
        return Err(format!(
            "the token LSB was written 0x{bad:02X}, and only the previous value plus one, \
             0x{want_token:02X}, may appear there — wrote {}",
            render(writes)
        ));
    }

    // Steps 1 and 6.  Progress is a boolean field, so only those two values.
    let progress = written_to(writes, Hdr::Progress);
    if progress != vec![s.pending(), s.complete] {
        let shown: Vec<String> = progress.iter().map(|v| format!("0x{v:02X}")).collect();
        return Err(format!(
            "progress was written [{}], and the sequence writes it pending (0x{:02X}) then \
             complete (0x{:02X}) — wrote {}",
            shown.join(", "),
            s.pending(),
            s.complete,
            render(writes)
        ));
    }

    // Step 5.  A boolean field on the same terms, and one outcome per command.
    let response = written_to(writes, Hdr::Response);
    if let Some(bad) = response.iter().find(|&&v| v != want_response) {
        return Err(format!(
            "the response field was written 0x{bad:02X}, and this command {}, so only \
             0x{want_response:02X} may appear there — wrote {}",
            if want_response == s.status_ok {
                "succeeded"
            } else {
                "was refused"
            },
            render(writes)
        ));
    }
    if response.is_empty() {
        return Err(format!(
            "the response field was never written — wrote {}",
            render(writes)
        ));
    }

    // Step 3.
    let last_group = written_to(writes, Hdr::LastCmdGroup);
    let last_cmd = written_to(writes, Hdr::LastCmdCmd);
    if last_group.is_empty() || last_cmd.is_empty() {
        return Err(format!(
            "last command was not written, and step 3 updates it on receipt of every \
             command — wrote {}",
            render(writes)
        ));
    }
    if last_group.iter().any(|&v| v != group_byte) || last_cmd.iter().any(|&v| v != cmd_byte) {
        return Err(format!(
            "last command was written something other than the command being processed, \
             0x{group_byte:02X}/0x{cmd_byte:02X} — wrote {}",
            render(writes)
        ));
    }

    // "The device sets progress = pending before incrementing the token,
    // ensuring no false-complete condition is possible during the transition
    // into command-response mode."
    let pending_at = first_write_to(writes, Hdr::Progress).expect("progress was written");
    let token_at = first_write_to(writes, Hdr::TokenLsb).expect("the token was written");
    if pending_at > token_at {
        return Err(format!(
            "the token was written before progress = pending, so a host that sees the token \
             move may read a progress field this command has not touched — wrote {}",
            render(writes)
        ));
    }

    let response_at = last_write_to(writes, Hdr::Response).expect("the response was written");
    let complete_at = last_write_to(writes, Hdr::Progress).expect("progress was written");
    if response_at > complete_at {
        return Err(format!(
            "progress = complete was written before the response field, so a host polling \
             progress reads a response this command has not written — wrote {}",
            render(writes)
        ));
    }

    // Steps 2, 3 and 5 in that order.  The half that bites is last command
    // before the response: the response is the field a host reads once
    // progress says complete, and anything written after it is a field the
    // host may already have read at its previous value.
    for field in [Hdr::LastCmdGroup, Hdr::LastCmdCmd] {
        let at = first_write_to(writes, field).expect("last command was written");
        if at < token_at {
            return Err(format!(
                "{field} was written before the token, and step 3 follows step 2 — wrote {}",
                render(writes)
            ));
        }
        let at = last_write_to(writes, field).expect("last command was written");
        if at > response_at {
            return Err(format!(
                "{field} was written after the response field, so a host that saw progress \
                 reach complete and read it got the previous command's — wrote {}",
                render(writes)
            ));
        }
    }

    Ok(())
}

/// A NOP in command-response mode must leave the response header exactly as
/// the specification's processing sequence describes.
///
/// NOP is the right command for this: the specification says it exists so the
/// host can "verify the device is alive and processing commands", so what is
/// under test here is the header machinery itself and nothing else.
pub fn nop(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // The token continues from whatever ENTER_CMD_RESP left it at — the
    // device must never reset it — so snapshot rather than assume a value.
    let token_before = bus.read_hdr(&s, Hdr::TokenLsb)?;

    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;

    // Step 2: incremented by exactly one, LSB first.
    bus.expect_hdr(&s, Hdr::TokenLsb, token_before.wrapping_add(1))
        .map_err(|e| format!("{e} — the increment must be exactly 1 per command"))?;

    // Step 3: last command records the GROUP and CMD just processed.
    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::CONTROL)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)?;

    // Step 5: NOP cannot fail, so the response field must hold status-OK.
    // issue_cmd already required this to return Ok; reading it back names the
    // field rather than the whole command when it disagrees.
    bus.expect_hdr(&s, Hdr::Response, s.status_ok)?;

    // Response Header: "Reserved — must be set to zero by the device."
    bus.expect_hdr(&s, Hdr::Reserved0, 0)?;
    bus.expect_hdr(&s, Hdr::Reserved1, 0)?;

    Ok(Outcome::Pass)
}

/// The device must not initialise the token on entering command-response mode.
///
/// "The device must not initialise the token on entering command-response
/// mode.  Instead the device increments whatever value is already present."
/// So the token is seeded here to a value of this scenario's choosing, using
/// command-mode pokes, and the entry must carry on from it.
///
/// The writes made during the entry are the second half of the requirement.  A
/// host watches that byte for the change that says its command arrived, so a
/// device passing through any other value on the way — by clearing the header
/// before rewriting it, say — tells the host so while the command is still
/// running.  Every value written there must be the seed plus one.
///
/// Asserted over [`Bus::region_writes`].  Polling from the bus cannot show
/// this, the device running the whole command between two of the driver's
/// turns, so a scenario that sampled the token would pass whatever it did.
pub fn token_continues_across_entry(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    const SEED_LSB: u8 = 0x40;
    const SEED_MSB: u8 = 0x00;

    bus.poke_verified(ctx, s.bch_start + Hdr::TokenLsb.offset(), SEED_LSB)
        .map_err(|e| format!("seeding the token LSB: {e}"))?;
    bus.poke_verified(ctx, s.bch_start + Hdr::TokenMsb.offset(), SEED_MSB)
        .map_err(|e| format!("seeding the token MSB: {e}"))?;

    // After the seeding, so the log holds the entry and nothing else.
    bus.reset_write_log();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let want = SEED_LSB.wrapping_add(1);
    let writes = bus.region_writes(&s)?;
    let seen = written_to(&writes, Hdr::TokenLsb);
    if let Some(bad) = seen.iter().find(|&&v| v != want) {
        return Err(format!(
            "the token LSB was written 0x{bad:02X} during entry, and only the seeded \
             0x{SEED_LSB:02X} plus one may appear there — wrote {}",
            render(&writes)
        ));
    }

    bus.expect_hdr(&s, Hdr::TokenLsb, want)?;
    bus.expect_hdr(&s, Hdr::TokenMsb, SEED_MSB)?;

    Ok(Outcome::Pass)
}

/// Entry runs the command processing sequence, in the order it defines.
///
/// ENTER_CMD_RESP builds the region it then reports through, so it is the
/// command where setup can land among the steps.
pub fn entry_writes_the_header_in_order(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    const SEED_LSB: u8 = 0x40;
    const SEED_MSB: u8 = 0x00;

    bus.poke_verified(ctx, s.bch_start + Hdr::TokenLsb.offset(), SEED_LSB)
        .map_err(|e| format!("seeding the token LSB: {e}"))?;
    bus.poke_verified(ctx, s.bch_start + Hdr::TokenMsb.offset(), SEED_MSB)
        .map_err(|e| format!("seeding the token MSB: {e}"))?;

    bus.reset_write_log();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let writes = bus.region_writes(&s)?;
    check_sequence(
        &writes,
        &s,
        SEED_LSB.wrapping_add(1),
        s.status_ok,
        group::CONTROL,
        control::ENTER_CMD_RESP,
    )?;

    Ok(Outcome::Pass)
}

/// A command within the session runs the same sequence, in the same order.
///
/// Separate from the entry scenario because the entry has region setup to do
/// and this has none, so one passing says nothing about the other.
pub fn nop_writes_the_header_in_order(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    let token_before = bus.read_hdr(&s, Hdr::TokenLsb)?;

    bus.reset_write_log();
    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;

    let writes = bus.region_writes(&s)?;
    check_sequence(
        &writes,
        &s,
        token_before.wrapping_add(1),
        s.status_ok,
        group::CONTROL,
        control::NOP,
    )?;

    // No step of the sequence names the reserved pair, so nothing writes it
    // once the entry has.
    for field in [Hdr::Reserved0, Hdr::Reserved1] {
        if first_write_to(&writes, field).is_some() {
            return Err(format!(
                "{field} was written by a command within the session, and only the entry \
                 writes it — wrote {}",
                render(&writes)
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// The reserved bytes are zeroed inside the entry, between the response field
/// and complete.
///
/// "The device sets the reserved bytes to zero while processing
/// ENTER_CMD_RESP, after it has set the response field and before it sets
/// progress to complete."
///
/// The point is fixed rather than left to the device so that a later version
/// giving those bytes a meaning need not move them.  A value the host reads
/// once progress says complete is then one this command wrote.
pub fn entry_zeroes_the_reserved_bytes(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    // Seeded away from zero, so a device that never writes them cannot be
    // mistaken for one that wrote zero.
    bus.poke_verified(ctx, s.bch_start + Hdr::Reserved0.offset(), 0x5Au8)
        .map_err(|e| format!("seeding reserved byte 0: {e}"))?;
    bus.poke_verified(ctx, s.bch_start + Hdr::Reserved1.offset(), 0xA5u8)
        .map_err(|e| format!("seeding reserved byte 1: {e}"))?;

    bus.reset_write_log();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let writes = bus.region_writes(&s)?;
    let Some(response_at) = last_write_to(&writes, Hdr::Response) else {
        return Err(format!(
            "the response field was never written — wrote {}",
            render(&writes)
        ));
    };
    let Some(complete_at) = last_write_to(&writes, Hdr::Progress) else {
        return Err(format!(
            "progress was never written — wrote {}",
            render(&writes)
        ));
    };

    for field in [Hdr::Reserved0, Hdr::Reserved1] {
        let seen = written_to(&writes, field);
        if seen != vec![0u8] {
            let shown: Vec<String> = seen.iter().map(|v| format!("0x{v:02X}")).collect();
            return Err(format!(
                "{field} was written [{}], and the entry writes it zero exactly once — \
                 wrote {}",
                shown.join(", "),
                render(&writes)
            ));
        }
        let at = first_write_to(&writes, field).expect("the field was written");
        if at < response_at || at > complete_at {
            return Err(format!(
                "{field} was written outside the window the entry must write it in, \
                 between the response field and progress = complete — wrote {}",
                render(&writes)
            ));
        }
    }

    bus.expect_hdr(&s, Hdr::Reserved0, 0)?;
    bus.expect_hdr(&s, Hdr::Reserved1, 0)?;

    Ok(Outcome::Pass)
}

/// An ENTER_CMD_RESP whose region overruns the RAM slot leaves the reserved
/// bytes alone.
///
/// "Where ENTER_CMD_RESP is discarded or fails, the device does not write them,
/// having not entered command-response mode."  The negative half of
/// [`entry_zeroes_the_reserved_bytes`], and the half a device gets wrong by
/// zeroing the pair on the way into the command rather than on the way out of a
/// successful one.
///
/// One of the two ways the entry can *fail* —
/// [`refused_re_entry_leaves_the_reserved_bytes_alone`] is the other, and they
/// are separate scenarios because they are separate paths through the device
/// and this one cannot be expressed on every slot size.  The two ways the entry
/// can be *discarded* need nothing here: those write nothing at all, which
/// [`super::control::enter_discards_unaligned_back_channel`] and the rest of
/// that family require of the whole slot.
///
/// Asserted over the writes rather than by reading the bytes back.  A device
/// writing zero where the pair already holds zero is indistinguishable from one
/// that left it alone, and this is the requirement a later version giving those
/// bytes a meaning depends on.
pub fn oversized_entry_leaves_the_reserved_bytes_alone(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    // The failure is reported through a header at the start address the host
    // named, which is this region.
    let s = ctx.session();

    // Four bytes over, as in
    // `super::control::enter_fails_when_back_channel_exceeds_slot`.
    let want = ctx.ram_slot_size - ctx.bch_start() + 4;
    let Ok(bch_size) = u16::try_from(want) else {
        return Ok(Outcome::Skip(format!(
            "a region overrunning this device's {}-byte RAM slot needs a size of {want}, \
             more than the 16 bits ENTER_CMD_RESP has for it",
            ctx.ram_slot_size
        )));
    };
    let over = Session { bch_size, ..s };

    bus.reset_write_log();
    match bus.enter_cmd_resp(&over) {
        Err(CmdFailure::Failed) => (),
        Ok(()) => {
            return Err(
                "ENTER_CMD_RESP with a region four bytes past the end of the RAM slot was \
                 accepted, and the specification requires failure"
                    .to_string(),
            );
        }
        Err(e) => {
            return Err(format!(
                "ENTER_CMD_RESP with a region four bytes past the end of the RAM slot: {e}"
            ));
        }
    }

    no_reserved_writes(
        bus,
        &s,
        "an ENTER_CMD_RESP whose region overruns the RAM slot",
    )?;
    Ok(Outcome::Pass)
}

/// An ENTER_CMD_RESP refused from inside a session leaves the reserved bytes
/// alone.
///
/// "Not supported when in command-response mode — the device returns failure",
/// and a failed entry "does not write them".  The companion to
/// [`oversized_entry_leaves_the_reserved_bytes_alone`], and the arm where only
/// the write log can see the fault: the sound entry that makes this a re-entry
/// has already zeroed the pair, so a device zeroing it again changes nothing a
/// host could read.
pub fn refused_re_entry_leaves_the_reserved_bytes_alone(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.reset_write_log();
    bus.expect_rejected(&s, group::CONTROL, control::ENTER_CMD_RESP, &s.enter_args())?;

    no_reserved_writes(bus, &s, "an ENTER_CMD_RESP issued from inside a session")?;
    Ok(Outcome::Pass)
}

/// Require that neither reserved byte was written.
fn no_reserved_writes(bus: &mut Bus, s: &Session, what: &str) -> Result<(), String> {
    let writes = bus.region_writes(s)?;
    for field in [Hdr::Reserved0, Hdr::Reserved1] {
        if first_write_to(&writes, field).is_some() {
            return Err(format!(
                "{field} was written by {what}, and the device writes the reserved pair only \
                 on entering command-response mode — wrote {}",
                render(&writes)
            ));
        }
    }
    Ok(())
}

/// A command the device refuses runs the sequence too, in every group.
///
/// The steps are what the device does "on receipt of a command", not on receipt
/// of one it can carry out: a refusal still increments the token by one, still
/// names itself in last command, and still writes the response field before
/// progress reaches complete.  A host has no other way to learn that its
/// command was refused rather than lost.
///
/// Every group, because the risk is per-group: a device dispatches on the group
/// and each group has its own early returns, so one group skipping the header
/// on the way out says nothing about the next.  The command chosen from each is
/// refused whatever the device has — a slot, pipe, group or LED number of 0xAA
/// is invalid by the specification rather than by what this device exposes, and
/// ENTER_CMD_RESP is defined to fail inside a session.
///
/// [`super::framing::unknown_cmd_in_every_group_consumes_no_arguments`] walks
/// the same groups for a command the device has no definition for.  This walks
/// commands it knows and declines, which is the other arm and a different path
/// through the device.
pub fn refusal_writes_the_header_in_order(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let entry = s.enter_args();
    let refusals: [(u8, u8, &[u8]); 7] = [
        // "Not supported when in command-response mode — the device returns
        // failure."
        (group::CONTROL, control::ENTER_CMD_RESP, &entry),
        (group::READ, read::GET_FLASH_SLOT_INFO, &[0xAA]),
        (group::MODIFY, modify::SWITCH_SLOT, &[0xAA]),
        // "The location MSB must not exceed 0x7F; if it does, the device
        // rejects the command."
        (group::NV_STORAGE, nv::NV_PEEK, &[1, 0x00, 0x80]),
        (group::PIPES, pipes::GET_PIPE_INFO, &[0xAA]),
        (group::AUX, aux::GET_AUX_GROUP_INFO, &[0xAA]),
        (group::LED, led::GET_LED_INFO, &[0xAA]),
    ];

    for (grp, cmd, args) in refusals {
        let token_before = bus.read_hdr(&s, Hdr::TokenLsb)?;

        bus.reset_write_log();
        bus.expect_rejected(&s, grp, cmd, args)?;

        let writes = bus.region_writes(&s)?;
        check_sequence(
            &writes,
            &s,
            token_before.wrapping_add(1),
            s.failed(),
            grp,
            cmd,
        )
        .map_err(|e| format!("0x{grp:02X}/0x{cmd:02X} refused: {e}"))?;
    }

    Ok(Outcome::Pass)
}

/// The token wraps from 0xFFFF to 0x0000, carrying into the MSB.
///
/// "Incremented by exactly 1 by the device on receipt of every command.  The
/// LSB is incremented first; when it wraps from 0xFF to 0x00 the MSB is
/// incremented."  Seeded just below the boundary so that entry takes the LSB
/// to 0xFF and one further command carries it.
pub fn token_wraps(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    const SEED_LSB: u8 = 0xFE;
    const SEED_MSB: u8 = 0x00;

    bus.poke_verified(ctx, s.bch_start + Hdr::TokenLsb.offset(), SEED_LSB)
        .map_err(|e| format!("seeding the token LSB: {e}"))?;
    bus.poke_verified(ctx, s.bch_start + Hdr::TokenMsb.offset(), SEED_MSB)
        .map_err(|e| format!("seeding the token MSB: {e}"))?;

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.expect_hdr(&s, Hdr::TokenLsb, 0xFF)?;
    bus.expect_hdr(&s, Hdr::TokenMsb, 0x00)?;

    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;

    bus.expect_hdr(&s, Hdr::TokenLsb, 0x00)?;
    bus.expect_hdr(&s, Hdr::TokenMsb, 0x01)
        .map_err(|e| format!("{e} — the LSB wrapped without carrying into the MSB"))?;

    Ok(Outcome::Pass)
}
