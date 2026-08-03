// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Session Initiation — The Knock".
//!
//! Every session begins with a knock: a series of *contiguous* ROM address
//! reads whose low-order address bits match a pattern agreed in advance
//! between the device and every host targeting it.  The knock precedes every
//! session, "including re-entry after exiting command-response mode".
//!
//! # Shape of these scenarios
//!
//! Each asserts that some malformed framing does *not* open a session, and
//! none of them does so by checking that nothing changed — see [`crate::driver::Probe`] for
//! why that would be worthless.  Instead all four follow the same four steps:
//!
//! 1. **Arm** — a properly knocked SLOT_POKE, verified over the bus.  Proves
//!    the whole path works at that moment, and leaves a value the test chose.
//! 2. **Stimulate** — the malformed framing under test, followed by a poke
//!    aimed at the same byte with a different value.
//! 3. **Fence** — another properly knocked, verified poke, to a different
//!    address.  Proves the device has processed a command since the stimulus.
//! 4. **Discriminate** — the probe must hold the armed value, not the
//!    stimulus value.  Both are writes this device makes routinely.
//!
//! Steps 1, 3 and 4 are identical throughout and live in the driver, so each
//! scenario below is just its own step 2.

use crate::driver::{Bus, KNOCK, control, group};
use crate::{Ctx, Outcome};

/// A command with no knock in front of it must not open a session.
pub fn required_before_command(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let probe = bus.arm_probe(ctx)?;

    // The stimulus: a well-formed command frame, entirely unannounced.
    bus.send_probe_poke(ctx, &probe)?;

    bus.expect_stimulus_ignored(ctx, &probe)?;
    Ok(Outcome::Pass)
}

/// A truncated knock must not open a session.
///
/// The knock's length is what makes accidental activation negligible, so a
/// device that accepted a prefix of it would be accepting a far weaker pattern
/// than the one agreed.
pub fn partial_does_not_open(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let probe = bus.arm_probe(ctx)?;

    bus.partial_knock(ctx.command_page(), KNOCK.len() - 1)?;
    bus.send_probe_poke(ctx, &probe)?;

    bus.expect_stimulus_ignored(ctx, &probe)?;
    Ok(Outcome::Pass)
}

/// A knock interrupted by an unrelated read must not open a session.
///
/// All six bytes are sent, in order — only their contiguity is broken.  A
/// device matching the pattern loosely, rather than across consecutive
/// captures, would open a session here.
pub fn must_be_contiguous(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let page = ctx.command_page();
    let probe = bus.arm_probe(ctx)?;

    // The interrupting read is sent on the command page, like the knock
    // itself, so what breaks the sequence is its position in the pattern and
    // nothing else about it.
    let split = KNOCK.len() / 2;
    bus.partial_knock(page, split)?;
    bus.send_byte(page, 0x00)?;
    for b in &KNOCK[split..] {
        bus.send_byte(page, *b)?;
    }
    bus.send_probe_poke(ctx, &probe)?;

    bus.expect_stimulus_ignored(ctx, &probe)?;
    Ok(Outcome::Pass)
}

/// Re-entry after exiting command-response mode needs its own knock.
///
/// "The knock precedes every session, including re-entry after exiting
/// command-response mode."  A device that carried its framing across the exit
/// would act on the unannounced command below.
pub fn required_after_exit(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // EXIT_CMD_RESP_ACK completes the full processing sequence before exiting,
    // so the host observes completion as it would for any other command.
    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    // Arm only now.  Arming before the session above would prove the path
    // worked before the exit, which is not the claim being tested.
    let probe = bus.arm_probe(ctx)?;

    bus.send_probe_poke(ctx, &probe)?;

    bus.expect_stimulus_ignored(ctx, &probe)?;
    Ok(Outcome::Pass)
}
