// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Command Processing Sequence" and "Response Header".
//!
//! On receipt of a command the device must, in order: set progress = pending,
//! increment the token, update last command, process the command, set the
//! response field, and finally set progress = complete.

use crate::driver::{Bus, Hdr, RegionWrite, Session, control, group};
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
fn check_sequence(
    writes: &[RegionWrite],
    s: &Session,
    want_token: u8,
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

    // Step 5.  A boolean field on the same terms, and this command succeeded.
    let response = written_to(writes, Hdr::Response);
    if let Some(bad) = response.iter().find(|&&v| v != s.status_ok) {
        return Err(format!(
            "the response field was written 0x{bad:02X}, and this command succeeded, so only \
             status-OK (0x{:02X}) may appear there — wrote {}",
            s.status_ok,
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
