// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Group 0x00 — Control".
//!
//! Two of this group's commands exit command-response mode *silently*:
//! EXIT_CMD_RESP_SILENT "exits command-response mode without updating the
//! response header", and SWITCH_AND_EXIT "switches to the specified slot and
//! exits command-response mode without updating the response header".
//!
//! Both are asserted the same way, and the same way as RBCP_RESET in
//! [`super::reset`]: a NOP first, so the last command field holds a value the
//! scenario chose rather than whatever the image happened to contain; then the
//! exit; then a verified command-mode poke as a fence, proving the device has
//! processed the exit and is taking sessions again; and only then the header
//! read.  Without the fence the read would prove nothing, since a header not
//! yet written and a header never written look identical.
//!
//! # ENTER_CMD_RESP's two kinds of refusal
//!
//! The rest of the group turns on ENTER_CMD_RESP, whose entry the
//! specification refuses in two distinct ways, and the distinction is the
//! point: four malformed arguments make "the device silently discard the
//! command", while an oversized back-channel and a re-entry both make it
//! "return failure".  A host tells them apart by the token: a discarded
//! command never increments it, a failed one does and then reports failure in
//! the response field.
//!
//! Discard is an absence, so [`expect_entry_discarded`] does not merely look
//! for one.  It arms the byte the token *would* land on with a value of its
//! own choosing, written by a verified command-mode poke; sends the malformed
//! entry; fences with a second verified poke, so the device has demonstrably
//! processed a command after the stimulus; and only then reads the byte, where
//! anything but the armed value means the device wrote there and an accepted
//! entry's increment says by how much.  It finishes by entering properly,
//! which proves the refusal was about the argument rather than a wedged device
//! or a frame the device never saw.
//!
//! Exit is asserted through re-entry throughout, for the reason [`super::reset`]
//! gives: ENTER_CMD_RESP is defined to fail while the device is already in
//! command-response mode, so an entry that *succeeds* is positive proof the
//! device had left.

use crate::driver::{Bus, CmdFailure, Hdr, Session, control, group, read, slot_peek_args};
use crate::{Ctx, Outcome};

/// EXIT_CMD_RESP_SILENT must leave the response header untouched.
pub fn exit_silent_writes_no_response_header(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;
    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::CONTROL)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)?;

    // Send only.  The specification gives the host nothing to poll for.
    bus.send_cmd(
        s.command_page,
        group::CONTROL,
        control::EXIT_CMD_RESP_SILENT,
        &[],
    )?;

    fence_in_command_mode(bus, ctx)?;

    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::CONTROL)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)
        .map_err(|e| format!("{e} — EXIT_CMD_RESP_SILENT updated the response header"))?;

    Ok(Outcome::Pass)
}

/// SWITCH_AND_EXIT must leave the response header untouched.
///
/// The slot switched to is the one already active.  That is deliberate: the
/// back-channel lives in the slot that was active when the session began, so
/// switching to a *different* slot would leave the header read below pointing
/// at unrelated memory and assert nothing.  Switching to the active slot
/// exercises the same code path while keeping the header observable.
pub fn switch_and_exit_writes_no_response_header(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;
    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::CONTROL)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)?;

    bus.send_cmd(
        s.command_page,
        group::CONTROL,
        control::SWITCH_AND_EXIT,
        &[ctx.active_ram_slot],
    )?;

    fence_in_command_mode(bus, ctx)?;

    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::CONTROL)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)
        .map_err(|e| format!("{e} — SWITCH_AND_EXIT updated the response header"))?;

    Ok(Outcome::Pass)
}

/// A back-channel start address that is not 4-byte aligned is discarded.
///
/// "A2/A3/A4 specify the start address of the back-channel region within the
/// active RAM slot; this address must be 4-byte aligned — if it is not, the
/// device silently discards the command."
pub fn enter_discards_unaligned_back_channel(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let bad = Session {
        // One byte past an aligned start: the smallest violation there is, so
        // a device masking the low bits off rather than rejecting them is
        // caught as surely as one ignoring the requirement entirely.
        bch_start: ctx.bch_start() + 1,
        ..ctx.session()
    };
    expect_entry_discarded(
        bus,
        ctx,
        &bad,
        "a back-channel start that is not 4-byte aligned",
    )?;
    Ok(Outcome::Pass)
}

/// A command page beyond the ROM being served is discarded.
///
/// "If the command page is out of range for the ROM type currently being
/// served, the device silently discards the command."  Such a page could never
/// be signalled on — no address the host can drive would reach it — so a device
/// accepting it would enter a mode in which it could receive nothing.
pub fn enter_discards_out_of_range_command_page(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    // The command page lives in observed address space, which on a ROM whose
    // low address line(s) the device does not observe is narrower than the
    // byte-addressed slot.  This is the first page whose bytes lie past its
    // end.
    let observed_span = ctx.ram_slot_size >> ctx.unobserved;
    let Ok(page) = u16::try_from(observed_span >> 8) else {
        return Ok(Outcome::Skip(format!(
            "an observed span of {observed_span} bytes puts the first out-of-range command \
             page beyond the 16 bits ENTER_CMD_RESP has for it"
        )));
    };

    let bad = Session {
        command_page: page,
        ..ctx.session()
    };
    expect_entry_discarded(
        bus,
        ctx,
        &bad,
        &format!("command page 0x{page:04X}, past the end of the {observed_span}-byte ROM"),
    )?;
    Ok(Outcome::Pass)
}

/// A complete value of 0xAA is discarded.
///
/// "Neither A7 nor A8 may be 0xAA — if either is, the device silently discards
/// the command."  0xAA is the reset value, barred from every final argument so
/// that a reset started mid-command stays detectable; a complete value of 0xAA
/// would make the progress field indistinguishable from one.
pub fn enter_discards_complete_of_aa(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let bad = Session {
        complete: 0xAA,
        ..ctx.session()
    };
    expect_entry_discarded(bus, ctx, &bad, "a complete value of 0xAA")?;
    Ok(Outcome::Pass)
}

/// A status-OK value of 0xAA is discarded.
///
/// The other half of "Neither A7 nor A8 may be 0xAA".  A8 is the frame's *final*
/// argument, so this is also the case the blanket rule on final arguments
/// covers: "If a device received a command with the final argument set to 0xAA,
/// it rejects the command."
pub fn enter_discards_status_ok_of_aa(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let bad = Session {
        status_ok: 0xAA,
        ..ctx.session()
    };
    expect_entry_discarded(bus, ctx, &bad, "a status-OK value of 0xAA")?;
    Ok(Outcome::Pass)
}

/// A back-channel region running past the end of the RAM slot fails.
///
/// "A5/A6 specify the size of the back-channel region in bytes; if the
/// requested size exceeds the available space in the RAM slot, the device
/// returns failure."  Returning failure, not discarding: the specification uses
/// both phrases for ENTER_CMD_RESP and the difference is what the host can see,
/// so this scenario requires the token to increment and the response field to
/// say failed — the start address is in range, so there is room for the header
/// the failure is reported in.
///
/// The region overruns by four bytes rather than wildly, so a bound that is off
/// by one is caught rather than merely a bound that is missing.
pub fn enter_fails_when_back_channel_exceeds_slot(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let want = ctx.ram_slot_size - ctx.bch_start() + 4;
    let Ok(bch_size) = u16::try_from(want) else {
        return Ok(Outcome::Skip(format!(
            "a region overrunning this device's {}-byte RAM slot needs a size of {want}, \
             more than the 16 bits ENTER_CMD_RESP has for it",
            ctx.ram_slot_size
        )));
    };

    let over = Session {
        bch_size,
        ..ctx.session()
    };
    match bus.enter_cmd_resp(&over) {
        Err(CmdFailure::Failed) => Ok(Outcome::Pass),
        Ok(()) => Err(format!(
            "the device accepted a {bch_size}-byte back-channel at 0x{:06X}, which runs four \
             bytes past the end of its {}-byte RAM slot",
            over.bch_start, ctx.ram_slot_size
        )),
        Err(e) => Err(format!(
            "ENTER_CMD_RESP with an oversized back-channel: {e} — the specification requires \
             the device to return failure here, which the host sees as the token incrementing \
             and the response field holding the failed value, and not as the silent discard it \
             specifies separately for a misaligned start, an out-of-range command page and a \
             complete or status-OK value of 0xAA"
        )),
    }
}

/// ENTER_CMD_RESP while already in command-response mode fails.
///
/// "Not supported when in command-response mode — the device returns failure."
/// Here the device does have a back-channel to say so in, so the whole
/// processing sequence must run and end with the response field failed —
/// which is what [`Bus::expect_rejected`] requires.  No knock: the device is
/// mid-session, and a knock's bytes would be read as command bytes.
pub fn enter_fails_when_already_in_command_response_mode(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("first ENTER_CMD_RESP: {e}"))?;

    // The same arguments the device has just accepted, so the only thing that
    // can make the device refuse them is that it is already in the mode.
    bus.expect_rejected(&s, group::CONTROL, control::ENTER_CMD_RESP, &s.enter_args())
        .map_err(|e| format!("{e} — ENTER_CMD_RESP is not supported in command-response mode"))?;

    Ok(Outcome::Pass)
}

/// EXIT_CMD_RESP_ACK runs the whole processing sequence before it exits.
///
/// "The device completes the full command processing sequence, including
/// setting progress = complete, before exiting command-response mode. The host
/// should poll progress for complete as normal."  So the host's ordinary
/// polling sequence must work on it — token, progress, response — and step 3 of
/// the processing sequence, "update last command", must have happened too.  A
/// NOP first, so the last command field holds a value this scenario chose.
///
/// "Once complete is observed, the device has exited command-response mode":
/// the entry at the end is what proves the exit, since ENTER_CMD_RESP fails
/// while the device is still in that mode.
pub fn exit_ack_completes_processing_sequence(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)?;

    // Success here *is* the assertion on the token, progress and response
    // fields: issue_cmd is the specification's host polling sequence.
    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| {
            format!(
                "EXIT_CMD_RESP_ACK: {e} — the specification requires the full command \
                 processing sequence, including progress = complete, before the device exits"
            )
        })?;

    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::CONTROL)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::EXIT_CMD_RESP_ACK)
        .map_err(|e| format!("{e} — EXIT_CMD_RESP_ACK did not update the last command field"))?;

    bus.enter_cmd_resp(&s).map_err(|e| {
        format!(
            "ENTER_CMD_RESP after EXIT_CMD_RESP_ACK: {e} — once complete is observed the \
             device has exited command-response mode"
        )
    })?;

    Ok(Outcome::Pass)
}

/// After EXIT_CMD_RESP_ACK the back-channel region is no longer maintained.
///
/// "Once complete is observed, the device has exited command-response mode and
/// the back-channel region is no longer maintained."
///
/// Armed, stimulated and discriminated rather than watched: the progress byte
/// is loaded with a value that is neither the complete nor the pending value,
/// and the stimulus is a knocked, verified poke — a command the device
/// demonstrably received and acted on, so it doubles as the fence.  A device
/// still maintaining the region would have driven progress pending then
/// complete on receiving it, and both of those are values it writes routinely.
pub fn exit_ack_stops_maintaining_back_channel(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    // Arm.  Derived from what is already there so it cannot be satisfied by the
    // existing contents, then moved off the two values a maintained region
    // would write, so the discrimination below has three distinguishable cases.
    let progress = s.bch_start + Hdr::Progress.offset();
    let mut armed = bus.read(progress)? ^ 0xFF;
    if armed == s.complete || armed == s.pending() {
        armed ^= 0x0F;
    }
    bus.poke_verified(ctx, progress, armed)
        .map_err(|e| format!("arming the progress byte after the exit: {e}"))?;

    // Stimulate, and fence: the device served this poke back, so it has
    // processed a command receipt since the arming and is not merely late.
    let scratch = ctx.scratch_addr();
    let value = bus.read(scratch)? ^ 0xFF;
    bus.poke_verified(ctx, scratch, value)
        .map_err(|e| format!("the device took no command-mode session after the exit: {e}"))?;

    // Discriminate.
    let got = bus.read(progress)?;
    if got == s.complete || got == s.pending() {
        return Err(format!(
            "the progress byte at 0x{progress:06X} holds 0x{got:02X}, the {} value, rather \
             than the armed 0x{armed:02X} — the device is still maintaining the back-channel \
             region after EXIT_CMD_RESP_ACK",
            if got == s.complete {
                "complete"
            } else {
                "pending"
            }
        ));
    }
    if got != armed {
        return Err(format!(
            "the progress byte at 0x{progress:06X} holds 0x{got:02X}, which is neither the \
             armed 0x{armed:02X} nor either of the values a maintained back-channel would \
             write — something else reached it"
        ));
    }

    Ok(Outcome::Pass)
}

/// SWITCH_AND_EXIT with a slot of 0xAA still exits.
///
/// "An A0 value of 0xAA is invalid.  If received the slot is NOT switched, but
/// the exit DOES complete."  The exit is silent, so there is nothing to poll
/// for; the entry afterwards is the proof, since ENTER_CMD_RESP fails while the
/// device is still in command-response mode.
pub fn switch_and_exit_slot_aa_still_exits(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.send_cmd(
        s.command_page,
        group::CONTROL,
        control::SWITCH_AND_EXIT,
        &[0xAA],
    )?;

    bus.enter_cmd_resp(&s).map_err(|e| {
        format!(
            "ENTER_CMD_RESP after SWITCH_AND_EXIT with a slot of 0xAA: {e} — the invalid slot \
             must not stop the exit completing"
        )
    })?;

    Ok(Outcome::Pass)
}

/// SWITCH_AND_EXIT with a slot of 0xAA does not switch slot.
///
/// The other half of "the slot is NOT switched, but the exit DOES complete".
/// Every slot the device could wrongly land on — 0xAA masked, folded or
/// truncated into range — is marked with one value first, and the active slot
/// with another, both written by the device itself and both read back through
/// it.  The verdict is which of the two the bus serves afterwards.
///
/// The fence is deliberately slot-agnostic: it pokes one value into the fence
/// byte of *every* slot, so it lands on whichever slot the device ended up
/// serving.  A fence aimed at one slot would fail rather than discriminate on
/// the very device this scenario exists to catch, and a failing fence names the
/// wrong fault.
pub fn switch_and_exit_slot_aa_does_not_switch(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    if ctx.ram_slot_count < 2 {
        return Ok(Outcome::Skip(
            "the device has a single RAM slot, so there is no other slot a mishandled 0xAA \
             could switch to"
                .to_string(),
        ));
    }

    let s = ctx.session();
    // The probe and fence bytes are adjacent, so one SLOT_PEEK reads back both
    // of a slot's marks.
    let addr = ctx.probe_addr();
    let fence = ctx.fence_addr();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // Every slot the device says a host may name — read from the device rather
    // than taken from the firmware's own count, because a plugin may keep some
    // slots back and those are not ones a mishandled 0xAA could land the host
    // on.
    bus.issue_cmd(&s, group::READ, read::GET_RAM_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_RAM_SLOT_INFO_ALL: {e}"))?;
    let slots = bus.read_data(&s, 0, 1)?[0];

    // All derived from the image, so none can be satisfied by what is already
    // there, and they differ from each other whatever that is.
    let original = bus.read(addr)?;
    let active_value = original ^ 0xFF;
    let marker = original ^ 0x5A;
    let fence_before = bus.read(fence)? ^ 0xFF;

    // Mark every slot, and read each back with SLOT_PEEK: an inactive slot is
    // not on the bus, so that is the only way to know the marks landed and that
    // the discrimination below has two live values to choose between.
    for slot in 0..slots {
        let value = if slot == ctx.active_ram_slot {
            active_value
        } else {
            marker
        };
        for (at, byte) in [(addr, value), (fence, fence_before)] {
            bus.poke_slot(&s, slot, at, byte)
                .map_err(|e| format!("marking 0x{at:06X} of slot {slot}: {e}"))?;
        }
        bus.issue_cmd(
            &s,
            group::READ,
            read::SLOT_PEEK,
            &slot_peek_args(addr, 2, slot),
        )
        .map_err(|e| format!("SLOT_PEEK of slot {slot}: {e}"))?;
        bus.expect_data(
            &s,
            0,
            &[value, fence_before],
            &format!("SLOT_PEEK of marked slot {slot}"),
        )?;
    }
    bus.expect_byte(addr, active_value)?;

    bus.send_cmd(
        s.command_page,
        group::CONTROL,
        control::SWITCH_AND_EXIT,
        &[0xAA],
    )?;

    // Fence: one command-mode poke per slot, moving the fence byte off the
    // value every slot was just left holding.  The device serving it proves it
    // has processed a command since the stimulus, and says nothing about which
    // slot that is — which is the question the read below answers.
    let fence_after = !fence_before;
    for slot in 0..slots {
        bus.knock(ctx.command_page())?;
        bus.send_poke_slot(ctx.command_page(), slot, fence, fence_after)?;
    }
    bus.await_byte(fence, fence_after).map_err(|e| {
        format!("the device took no command-mode session after SWITCH_AND_EXIT: {e}")
    })?;

    let got = bus.read(addr)?;
    if got == marker {
        return Err(format!(
            "0x{addr:06X} serves 0x{got:02X}, the value marking every inactive slot — \
             SWITCH_AND_EXIT with a slot of 0xAA switched slot, which the specification \
             forbids"
        ));
    }
    if got != active_value {
        return Err(format!(
            "0x{addr:06X} serves 0x{got:02X}, neither the 0x{active_value:02X} marking the \
             slot that was active nor the 0x{marker:02X} marking the others — the device is \
             serving something else again"
        ));
    }

    Ok(Outcome::Pass)
}

/// A knocked, verified poke: proof the device has processed whatever preceded
/// it and is taking command-mode sessions again.
fn fence_in_command_mode(bus: &mut Bus, ctx: &Ctx) -> Result<(), String> {
    let addr = ctx.probe_addr();
    let value = bus.read(addr)? ^ 0xFF;
    bus.poke_verified(ctx, addr, value)
        .map_err(|e| format!("the device took no command-mode session after the exit: {e}"))
}

/// Require the device to discard a malformed ENTER_CMD_RESP silently.
///
/// The specification's own advice is that this is what a host concludes from
/// the token failing to increment, so the byte the token would have landed on
/// is *armed* first — with a verified command-mode poke, which proves the write
/// path works at that instant and leaves a value this module chose rather than
/// whatever the image held.  A fence follows the malformed entry, so the device
/// has demonstrably processed a command after it; only then is the byte read,
/// and the question is which of two values the device is plainly capable of
/// writing it holds.
///
/// The entry at the end is the positive control.  Without it a discard would be
/// indistinguishable from a device that was wedged, or that never saw the frame
/// at all — both of which also leave the token alone.
fn expect_entry_discarded(
    bus: &mut Bus,
    ctx: &Ctx,
    bad: &Session,
    what: &str,
) -> Result<(), String> {
    // Where the token would land had the device honoured this entry.
    let token = bad.bch_start + Hdr::TokenLsb.offset();

    let armed = bus.read(token)? ^ 0xFF;
    bus.poke_verified(ctx, token, armed)
        .map_err(|e| format!("arming the token byte at 0x{token:06X}: {e}"))?;

    bus.knock(ctx.command_page())?;
    bus.send_enter_cmd_resp(ctx.command_page(), bad)?;

    bus.fence(ctx)
        .map_err(|e| format!("after an ENTER_CMD_RESP with {what}: {e}"))?;

    // Any value but the armed one means the device wrote there, and the only
    // reason it would is having honoured the entry: an accepted one leaves the
    // incremented token, and a device that then went on processing commands in
    // the mode it should never have entered leaves a later one still.
    let acted = armed.wrapping_add(1);
    let got = bus.read(token)?;
    if got != armed {
        return Err(format!(
            "the token at 0x{token:06X} went 0x{armed:02X} → 0x{got:02X} (an accepted entry \
             leaves 0x{acted:02X}): the device acted on an ENTER_CMD_RESP with {what}, which \
             the specification requires it to discard silently"
        ));
    }

    // Positive control: the same command with that one argument sound is acted
    // on, so what the device declined was the argument.
    let good = ctx.session();
    bus.enter_cmd_resp(&good).map_err(|e| {
        format!(
            "ENTER_CMD_RESP with {what} corrected: {e} — the discard above cannot be \
             attributed to the argument if a sound entry is refused too"
        )
    })?;
    if good.bch_start == bad.bch_start {
        bus.expect_hdr(&good, Hdr::TokenLsb, acted).map_err(|e| {
            format!("{e} — an accepted entry increments the very byte the discard left alone")
        })?;
    }

    Ok(())
}

/// LOAD_AND_EXIT must leave the response header untouched.
///
/// Asserted as the two exits above are, and with the same care over the slot:
/// the reload names the slot being served, which is the case the command
/// exists for and the only one in which the header stays observable.
///
/// The reload here would put the header back on its own, so the last command
/// field is not what proves silence — the *image's* byte at that offset is
/// what a correct device leaves, and a header write after the reload would
/// replace it.  [`load_and_exit_restores_the_back_channel`] makes that the
/// whole assertion.  This scenario asserts the weaker, independent thing: the
/// device did exit, and did so without leaving a header behind.
pub fn load_and_exit_writes_no_response_header(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();

    // The image's own byte where the last command field sits, read before the
    // region becomes a back-channel.  Flash slot 0 is the image the device
    // booted serving, so this is what a reload of it must restore.
    let hdr_group = bus.read(s.bch_start + Hdr::LastCmdGroup.offset())?;
    let hdr_cmd = bus.read(s.bch_start + Hdr::LastCmdCmd.offset())?;

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)?;

    bus.send_cmd(
        s.command_page,
        group::CONTROL,
        control::LOAD_AND_EXIT,
        &[ctx.active_ram_slot, 0],
    )?;

    fence_in_command_mode(bus, ctx)?;

    bus.expect_hdr(&s, Hdr::LastCmdGroup, hdr_group)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, hdr_cmd).map_err(|e| {
        format!(
            "{e} — LOAD_AND_EXIT reloaded the image, which puts the image's own byte back at \
             that offset, so anything else there is a response header the device wrote \
             afterwards"
        )
    })?;

    Ok(Outcome::Pass)
}

/// LOAD_AND_EXIT puts back every byte the back-channel displaced.
///
/// This is the command's whole purpose, so the assertion is the region itself,
/// not a field of it: the bytes the image holds across the region are read
/// before entry, the session then writes a header and a response over them,
/// and after the reload every one of them must be back.
///
/// Only the header's eight bytes are compared.  The device writes response
/// data beyond them only for commands that return some, and this scenario
/// issues none — so eight bytes is the whole of what it dirtied, and a
/// mismatch anywhere in them is the failure this command exists to prevent.
///
/// The NOP is what makes the assertion mean anything: without a command in the
/// session the header would hold the entry's own writes, and a device that
/// simply never wrote would pass.  The header is read back mid-session and
/// required to *differ* first, so the comparison at the end is against damage
/// known to have been done.
pub fn load_and_exit_restores_the_back_channel(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();

    let mut original = Vec::new();
    for i in 0..crate::driver::HDR_SIZE {
        original.push(bus.read(s.bch_start + i)?);
    }

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;

    // The damage, established rather than assumed.
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)?;
    bus.expect_hdr(&s, Hdr::Progress, s.complete)?;

    bus.send_cmd(
        s.command_page,
        group::CONTROL,
        control::LOAD_AND_EXIT,
        &[ctx.active_ram_slot, 0],
    )?;

    fence_in_command_mode(bus, ctx)?;

    let mut restored = Vec::new();
    for i in 0..crate::driver::HDR_SIZE {
        restored.push(bus.read(s.bch_start + i)?);
    }
    if restored != original {
        let i = (0..original.len())
            .find(|&i| restored[i] != original[i])
            .unwrap_or(0);
        return Err(format!(
            "after LOAD_AND_EXIT of flash slot 0 into the served slot, byte {i} of the \
             back-channel region is 0x{:02X}, expected the image's own 0x{:02X} — the region \
             reads {restored:02X?} against the {original:02X?} it held before the session",
            restored[i], original[i]
        ));
    }

    Ok(Outcome::Pass)
}

/// EXIT_CMD_RESP_RESTORE writes the host's bytes and nothing of its own.
///
/// The bytes handed back are the ones the host read before entering, so a
/// correct device leaves the region exactly as the image had it — the same
/// end state as [`load_and_exit_restores_the_back_channel`], reached without
/// flash.
///
/// A header write after the restore is what this would catch: the count covers
/// all eight bytes, so every field the device could write is one the host has
/// just supplied a value for, and any difference is the device's own.
pub fn exit_restore_writes_the_hosts_bytes(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    let mut original = Vec::new();
    for i in 0..crate::driver::HDR_SIZE {
        original.push(bus.read(s.bch_start + i)?);
    }

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)?;

    let mut args = original.clone();
    args.push(crate::driver::HDR_SIZE as u8);
    bus.send_cmd(
        s.command_page,
        group::CONTROL,
        control::EXIT_CMD_RESP_RESTORE,
        &args,
    )?;

    fence_in_command_mode(bus, ctx)?;

    let mut restored = Vec::new();
    for i in 0..crate::driver::HDR_SIZE {
        restored.push(bus.read(s.bch_start + i)?);
    }
    if restored != original {
        let i = (0..original.len())
            .find(|&i| restored[i] != original[i])
            .unwrap_or(0);
        return Err(format!(
            "after EXIT_CMD_RESP_RESTORE of the region's original eight bytes, byte {i} is \
             0x{:02X} and the host supplied 0x{:02X} — the region reads {restored:02X?} against \
             the {original:02X?} handed back",
            restored[i], original[i]
        ));
    }

    Ok(Outcome::Pass)
}

/// EXIT_CMD_RESP_RESTORE writes only as many bytes as the count asks for.
///
/// "Writes count bytes, taken from A0 onwards", and the arguments beyond it
/// "are ignored by the device, but are still transmitted, as the argument
/// count is fixed".  A device writing all eight regardless would leave the
/// last four holding argument bytes the host never asked it to use.
pub fn exit_restore_writes_only_count_bytes(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    const COUNT: usize = 4;
    let s = ctx.session();

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;

    // What the session left in the bytes past the count, read from the device
    // rather than predicted, so the assertion is that they did not move.
    let mut untouched = Vec::new();
    for i in COUNT as u32..crate::driver::HDR_SIZE {
        untouched.push(bus.read(s.bch_start + i)?);
    }

    // A payload sharing no value with anything already there, so a stray write
    // is visible wherever it lands.
    let payload = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let mut args = payload.to_vec();
    args.push(COUNT as u8);
    bus.send_cmd(
        s.command_page,
        group::CONTROL,
        control::EXIT_CMD_RESP_RESTORE,
        &args,
    )?;

    fence_in_command_mode(bus, ctx)?;

    for i in 0..COUNT as u32 {
        let got = bus.read(s.bch_start + i)?;
        if got != payload[i as usize] {
            return Err(format!(
                "byte {i} of the region is 0x{got:02X}, expected the 0x{:02X} the host supplied \
                 within a count of {COUNT}",
                payload[i as usize]
            ));
        }
    }
    for (n, &want) in untouched.iter().enumerate() {
        let i = COUNT as u32 + n as u32;
        let got = bus.read(s.bch_start + i)?;
        if got != want {
            return Err(format!(
                "byte {i} of the region is 0x{got:02X} and held 0x{want:02X} before the \
                 command — it lies past the count of {COUNT}, so the device must not have \
                 written it"
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// A count outside 1 to 8 writes nothing, and the exit still completes.
///
/// "A8 must be in the range 0x01 to 0x08.  If it is not, no bytes are written,
/// but the exit DOES complete."  Both halves are asserted for each of the two
/// bounds: the region is required to still hold what the session put there,
/// and the device is required to have left command-response mode — which a
/// fresh entry succeeding is positive proof of, ENTER_CMD_RESP being defined
/// to fail while the device is already in that mode.
pub fn exit_restore_rejects_count_out_of_range(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    for count in [0u8, 9u8] {
        let s = ctx.session();
        bus.enter_cmd_resp(&s)
            .map_err(|e| format!("ENTER_CMD_RESP before a count of {count}: {e}"))?;
        bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
            .map_err(|e| format!("NOP: {e}"))?;

        let mut before = Vec::new();
        for i in 0..crate::driver::HDR_SIZE {
            before.push(bus.read(s.bch_start + i)?);
        }

        let payload = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let mut args = payload.to_vec();
        args.push(count);
        bus.send_cmd(
            s.command_page,
            group::CONTROL,
            control::EXIT_CMD_RESP_RESTORE,
            &args,
        )?;

        fence_in_command_mode(bus, ctx)
            .map_err(|e| format!("{e} — a count of {count} must still complete the exit"))?;

        for i in 0..crate::driver::HDR_SIZE {
            let got = bus.read(s.bch_start + i)?;
            if got != before[i as usize] {
                return Err(format!(
                    "with a count of {count}, byte {i} of the region changed from \
                     0x{:02X} to 0x{got:02X} — the specification requires no bytes to be written",
                    before[i as usize]
                ));
            }
        }
    }

    Ok(Outcome::Pass)
}

/// EXIT_CMD_RESP_RESTORE is not valid in command mode.
///
/// "Not supported in command mode, where there is no back-channel region to
/// write to — the device consumes the arguments and discards the command."
/// Both halves are asserted.
///
/// *Discarded*: the bytes the region occupied are armed with values of this
/// scenario's own, and the command asks for eight different ones at the same
/// place.  A device acting on it in command mode would write them there, using
/// a region offset that stopped meaning anything when the session ended.
///
/// *Arguments consumed*: a NOP follows, in the same command-mode session, and
/// must be acted on.  Had the nine argument bytes been left on the wire the
/// device would read the NOP's two bytes as arguments and the session would be
/// desynchronised, which the fence would then catch.
pub fn restore_not_valid_in_command_mode(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();

    // A session, then a clean exit: the device has been in command-response
    // mode and knows a region offset, which is the state that makes acting on
    // this command in command mode possible at all.
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    let payload = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let mut armed = Vec::new();
    for (i, &p) in payload.iter().enumerate() {
        let a = p ^ 0xFF;
        bus.poke_verified(ctx, s.bch_start + i as u32, a)
            .map_err(|e| format!("arming byte {i} of the region: {e}"))?;
        armed.push(a);
    }

    bus.knock(ctx.command_page())?;
    let mut args = payload.to_vec();
    args.push(crate::driver::HDR_SIZE as u8);
    bus.send_cmd(
        ctx.command_page(),
        group::CONTROL,
        control::EXIT_CMD_RESP_RESTORE,
        &args,
    )?;

    // Still the same command-mode session: if the nine arguments were consumed
    // this is a fresh frame, and the fence proves the device acted on it.
    bus.send_cmd(ctx.command_page(), group::CONTROL, control::NOP, &[])?;
    bus.fence(ctx).map_err(|e| {
        format!(
            "{e} — the device took no further command-mode session, which is what an \
             EXIT_CMD_RESP_RESTORE whose nine argument bytes were left on the wire would cause"
        )
    })?;

    for (i, &want) in armed.iter().enumerate() {
        let got = bus.read(s.bch_start + i as u32)?;
        if got != want {
            return Err(format!(
                "byte {i} of the former back-channel region serves 0x{got:02X}, expected the \
                 armed 0x{want:02X}{} — the device is not in command-response mode, so there \
                 is no region for this command to write to",
                if got == payload[i] {
                    ", and that is the byte the command carried"
                } else {
                    ""
                }
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// LOAD_AND_EXIT with a slot argument of 0xAA loads nothing and exits anyway.
///
/// "A0 or A1 values of 0xAA are invalid.  If received the slot is NOT loaded,
/// but the exit DOES complete."  Both arguments are tried, and each is
/// asserted twice over: the served image must still hold the byte this
/// scenario poked, proving no reload happened, and a command-mode session must
/// be available, proving the exit did.
///
/// The poked byte is what makes the first half visible.  A reload of flash
/// slot 0 would put the image's own byte back there, so the poke is the only
/// thing distinguishing a device that obeyed from one that loaded anyway.
pub fn load_and_exit_slot_aa_loads_nothing_but_exits(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    for args in [[0xAAu8, 0u8], [ctx.active_ram_slot, 0xAAu8]] {
        let s = ctx.session();
        bus.enter_cmd_resp(&s)
            .map_err(|e| format!("ENTER_CMD_RESP before arguments {args:02X?}: {e}"))?;

        let addr = ctx.scratch_addr();
        let in_image = bus.read(addr)?;
        let poked = in_image ^ 0xFF;
        bus.poke_slot_verified(&s, ctx.active_ram_slot, addr, poked)
            .map_err(|e| format!("poking the served slot: {e}"))?;

        bus.send_cmd(
            s.command_page,
            group::CONTROL,
            control::LOAD_AND_EXIT,
            &args,
        )?;

        fence_in_command_mode(bus, ctx).map_err(|e| {
            format!("{e} — LOAD_AND_EXIT with arguments {args:02X?} must still complete the exit")
        })?;

        let got = bus.read(addr)?;
        if got != poked {
            return Err(format!(
                "after LOAD_AND_EXIT with arguments {args:02X?}, 0x{addr:06X} serves \
                 0x{got:02X} rather than the 0x{poked:02X} poked into the served slot{} — a \
                 0xAA argument means the slot is not loaded",
                if got == in_image {
                    ", which is the image's own byte"
                } else {
                    ""
                }
            ));
        }
    }

    Ok(Outcome::Pass)
}
