// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Device robustness: a host that outpaces the device's capture ring.
//!
//! Not a specification requirement — RBCP says nothing about how a device
//! captures the bus — but a real failure mode.  The ring is finite, the device
//! drains it only when it runs, and a host under no obligation to wait can
//! wrap it several times over.  On hardware that is a race and hard to
//! provoke; here the driver simply withholds the device's turns, which makes
//! it exact.
//!
//! What is asserted is *recovery*, not loss.  "Entries were lost" is an
//! absence and cannot be established: a device that had somehow kept up would
//! look identical to one that dropped everything.  What matters, and what can
//! be shown positively, is that a device which has been overrun still takes
//! the next properly framed command.

use crate::driver::Bus;
use crate::{Ctx, Outcome};

/// How far past the ring's capacity the host runs ahead.
///
/// The plugin's ring holds 64 entries, so this wraps it several times with the
/// device frozen — the write pointer laps the read pointer repeatedly, which
/// is the state the device has to resynchronise from.
const OVERRUN_READS: u32 = 200;

/// A host that overruns the capture ring does not wedge the device.
///
/// The reads are issued through [`Bus::read_without_resuming`], so the device
/// gets no turn at all while they happen and cannot drain a single entry.  It
/// then has to recover by resynchronising to wherever the ring has got to,
/// which is exactly what its knock detection does on hardware.
pub fn overflow_then_recovers(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let page = ctx.command_page();
    let addr = ctx.probe_addr();
    let value = bus.read(addr)? ^ 0xFF;

    // Prove the device is live and framing before overrunning it, so a failure
    // afterwards can only be the overrun.
    let scratch = ctx.scratch_addr();
    let scratch_value = bus.read(scratch)? ^ 0xFF;
    bus.poke_verified(ctx, scratch, scratch_value)
        .map_err(|e| format!("before the overrun: {e}"))?;

    // Flood the ring with the device stopped.  The address is fixed and on the
    // command page, so the entries are well-formed captures the device simply
    // never gets to see — no value of it can spell a knock.
    let flood = bus.command_addr(page, 0x00);
    for _ in 0..OVERRUN_READS {
        bus.read_without_resuming(flood);
    }

    bus.poke_verified(ctx, addr, value).map_err(|e| {
        format!(
            "{e} — after being overrun by {OVERRUN_READS} reads the device did not act on \
                 a properly framed command"
        )
    })?;

    Ok(Outcome::Pass)
}
