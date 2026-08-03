// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Command Framing" and its "Command Mode Constraint".
//!
//! A frame is `[GROUP] [CMD] [A0..An]` with no length field: both ends know
//! the argument count from the GROUP+CMD pair.  The consequence, which this
//! module is really about, is what happens when a host stops short.  The
//! device goes on consuming address reads as arguments of the command already
//! in progress until that count is satisfied, and only then can a knock
//! re-establish framing.
//!
//! The specification bounds the damage: "a host recovering from desync in
//! command mode need transmit at most 10 additional address reads before a
//! knock can re-establish framing", the maximum argument count being 9.
//!
//! All three scenarios observe through SLOT_POKE, which is valid in command
//! mode — see [`crate::driver::Probe`] for why they are framed as
//! discriminations between two values rather than as "nothing changed".

use crate::driver::{Bus, group, modify};
use crate::{Ctx, Outcome};

/// A command's argument bytes are consumed exactly, no more and no less.
///
/// Two properly framed pokes are sent back to back with nothing between them.
/// The second is framed correctly only if the device stopped consuming the
/// first command's arguments at exactly five — one byte out in either
/// direction and its knock is misread, so the second poke never lands.  Both
/// writes are then required, which is what makes this a positive test of the
/// argument count rather than of either poke individually.
pub fn arguments_are_consumed(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let first_addr = ctx.scratch_addr();
    let second_addr = ctx.probe_addr();

    let first_val = bus.read(first_addr)? ^ 0xFF;
    let second_val = bus.read(second_addr)? ^ 0xFF;

    bus.knock(ctx.command_page())?;
    bus.send_poke(ctx, first_addr, first_val)?;
    bus.knock(ctx.command_page())?;
    bus.send_poke(ctx, second_addr, second_val)?;

    bus.await_byte(second_addr, second_val).map_err(|e| {
        format!(
            "second poke: {e} — the device did not resume framing after \
                              exactly five argument bytes"
        )
    })?;
    bus.expect_byte(first_addr, first_val)
        .map_err(|e| format!("first poke: {e}"))?;

    Ok(Outcome::Pass)
}

/// A knock arriving mid-argument-collection must not open a session.
///
/// This is the Command Mode Constraint stated as a requirement: while the
/// device is still owed argument bytes it cannot see a knock, because those
/// bytes *are* the arguments.  Here the knock's first three reads are swallowed
/// to complete an interrupted SLOT_POKE, so what follows is not a session and
/// the poke aimed at the probe must not take effect.
pub fn knock_not_seen_during_argument_collection(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let page = ctx.command_page();
    let probe = bus.arm_probe(ctx)?;

    // Start a SLOT_POKE and stop two arguments in, leaving three outstanding.
    // Aimed at the scratch byte with value zero, so that whatever eventually
    // completes it is harmless wherever it lands.
    let args = bus.poke_args(ctx, ctx.scratch_addr(), 0x00);
    bus.knock(page)?;
    bus.send_cmd(page, group::MODIFY, modify::SLOT_POKE, &args[..2])?;

    // The stimulus: a full knock and a poke at the probe.  The knock's first
    // three bytes satisfy the outstanding arguments instead.
    bus.knock(page)?;
    bus.send_probe_poke(ctx, &probe)?;

    bus.expect_stimulus_ignored(ctx, &probe)?;
    Ok(Outcome::Pass)
}

/// Ten reads and a knock restore framing after a desync.
///
/// The specification's recovery guarantee: whatever state a host has left the
/// device in, at most ten further address reads are needed before a knock is
/// seen again.  Asserted by requiring the poke after that knock to land.
///
/// The interrupted command is a SLOT_POKE of zero, and the filler reads are
/// all on command value zero, so the three bytes that complete it write 0x00
/// to slot address 0 — deterministic, and clear of every address these
/// scenarios read.
pub fn desync_recovers_within_ten_reads(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let page = ctx.command_page();
    let addr = ctx.probe_addr();
    let value = bus.read(addr)? ^ 0xFF;

    let args = bus.poke_args(ctx, 0x0000, 0x00);
    bus.knock(page)?;
    bus.send_cmd(page, group::MODIFY, modify::SLOT_POKE, &args[..2])?;

    // The specification's bound, sent verbatim: ten reads, then a knock.
    for _ in 0..10 {
        bus.send_byte(page, 0x00)?;
    }

    bus.poke_verified(ctx, addr, value).map_err(|e| {
        format!(
            "{e} — a knock did not re-establish framing within the ten reads the \
                 specification allows"
        )
    })?;

    Ok(Outcome::Pass)
}
