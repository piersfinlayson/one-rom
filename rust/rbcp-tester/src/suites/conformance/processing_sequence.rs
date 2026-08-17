// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Command Processing Sequence" and "Response Header".
//!
//! On receipt of a command the device must, in order: set progress = pending,
//! increment the token, update last command, process the command, set the
//! response field, and finally set progress = complete.

use crate::driver::{Bus, Hdr, control, group};
use crate::{Ctx, Outcome};

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
/// The samples taken while waiting are the second half of the requirement.  A
/// host snapshots the token before issuing ENTER_CMD_RESP and watches for it
/// to change; if the device passes through any *other* value on the way — by
/// clearing the header before rewriting it, say — a host can see that change
/// and conclude the command completed while it is still in progress.  So every
/// observed value must be either the seed or the seed plus one.
pub fn token_continues_across_entry(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    const SEED_LSB: u8 = 0x40;
    const SEED_MSB: u8 = 0x00;

    bus.poke_verified(ctx, s.bch_start + Hdr::TokenLsb.offset(), SEED_LSB)
        .map_err(|e| format!("seeding the token LSB: {e}"))?;
    bus.poke_verified(ctx, s.bch_start + Hdr::TokenMsb.offset(), SEED_MSB)
        .map_err(|e| format!("seeding the token MSB: {e}"))?;

    let seen = bus.enter_cmd_resp_sampling_token(&s)?;

    let want = SEED_LSB.wrapping_add(1);
    if let Some(bad) = seen.iter().find(|&&v| v != SEED_LSB && v != want) {
        return Err(format!(
            "the token LSB was observed as 0x{bad:02X} during entry, which is neither the \
             seeded 0x{SEED_LSB:02X} nor 0x{want:02X}; a host watching for the token to \
             change would take that as the command having completed"
        ));
    }

    bus.expect_hdr(&s, Hdr::TokenLsb, want)?;
    bus.expect_hdr(&s, Hdr::TokenMsb, SEED_MSB)?;

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
