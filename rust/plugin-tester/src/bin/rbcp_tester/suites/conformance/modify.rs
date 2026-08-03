// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Group 0x02 — Modify".
//!
//! Four commands that change device state: SLOT_POKE writes one byte into a RAM
//! slot, SWITCH_SLOT activates one, LOAD_SLOT copies a flash image into one
//! "without activating the slot", and SLOT_POKE_ALL_BYTE fills one.  Each names
//! a slot in its final argument, and each "is invalid and rejected" when that
//! argument is 0xAA — the value reserved so that a reset started mid-command is
//! always detectable.  The group is "valid in both command and command-response
//! modes", so SLOT_POKE and SWITCH_SLOT are exercised in each.
//!
//! # How a change of state is observed
//!
//! Three of these commands act on a slot that is *not* being served, so their
//! effect cannot be read as ROM data.  The host's way in is SLOT_PEEK, which
//! "read\[s\] one or more bytes from the specified RAM slot" into the
//! back-channel: a device-side read of the target slot, answered over the bus.
//! Every such scenario therefore arranges three distinguishable values —
//!
//! - **the served slot's**, put there by a verified SLOT_POKE, so a peek that
//!   answered from the served slot instead of the one it named is caught;
//! - **the target slot's**, put there by a SLOT_POKE naming that slot, so a
//!   command that did nothing at all is caught;
//! - **the flash image's own**, read over the bus before anything is written
//!   over it, which is what LOAD_SLOT must produce —
//!
//! and no assertion rests on a byte simply not having changed.
//!
//! # A second RAM slot
//!
//! Most of this group is about a slot other than the one being served, and the
//! device may have only one: the RAM slot count falls out of the ROM table
//! size, and a 512 KB region leaves room for one.  Those scenarios skip there,
//! naming what the device lacks, rather than degenerating into a poke at the
//! active slot that would assert something weaker under the same name.
//!
//! # Addresses below the command page
//!
//! A read is how the host signals, so any read below the command page's first
//! address *is* a command byte.  The device's own reads are not, which makes
//! SLOT_PEEK the only way to inspect the bottom of a slot — and the fill
//! scenario needs it for exactly that.

use crate::driver::{Bus, Hdr, Session, control, group, modify, slot_poke_args};
use crate::{Ctx, Outcome};

/// Why a scenario about a second slot cannot run on this device.
fn needs_second_slot(ctx: &Ctx, what: &str) -> Outcome {
    Outcome::Skip(format!(
        "the device has one RAM slot (a {} served from slot {}), and {what} needs a slot other \
         than the one being served",
        ctx.chip_type.name(),
        ctx.active_ram_slot
    ))
}

/// SLOT_POKE into the active slot, and require the device to serve it.
///
/// Proves the whole path at that moment, and leaves a value this module chose
/// in the slot being served — the value a peek at another slot must be told
/// apart from.
fn poke_active_verified(
    bus: &mut Bus,
    s: &Session,
    ctx: &Ctx,
    addr: u32,
    value: u8,
) -> Result<(), String> {
    bus.poke_slot_verified(s, ctx.active_ram_slot, addr, value)
}

/// Peek one byte of a slot and require it to hold `want`.
fn expect_peek(
    bus: &mut Bus,
    s: &Session,
    slot: u8,
    addr: u32,
    want: u8,
    why: &str,
) -> Result<(), String> {
    let got = bus.peek_slot(s, slot, addr, 1)?[0];
    if got != want {
        return Err(format!(
            "slot {slot} holds 0x{got:02X} at 0x{addr:06X}, expected 0x{want:02X} — {why}"
        ));
    }
    Ok(())
}

/// What the device itself holds at `addr` in each of two slots.
///
/// Diagnostic only, appended to a failure about which slot is being served: it
/// separates "the device wrote the wrong thing" from "the device wrote the
/// right thing somewhere the host cannot read it".
fn slot_view(bus: &Bus, ctx: &Ctx, target: u8, addr: u32) -> String {
    format!(
        "the device's own view of 0x{addr:06X} is 0x{:02X} in slot {} and 0x{:02X} in slot \
         {target}",
        bus.api_slot_bytes(ctx.active_ram_slot, addr, 1)[0],
        ctx.active_ram_slot,
        bus.api_slot_bytes(target, addr, 1)[0],
    )
}

/// SLOT_POKE writes the byte it is given, in both modes, a byte at a time.
///
/// "Writes a single byte into the specified RAM slot at the specified address",
/// and the group is "valid in both command and command-response modes" — so the
/// same write is made once in each, and read back over the bus as ordinary ROM
/// data.
///
/// The two addresses are adjacent, so on a word-organised ROM they are the two
/// halves of one word.  The second poke must leave the first byte's neighbour
/// alone: a device writing a whole word would pass every scenario that only
/// ever pokes isolated bytes.  That read is fenced by the poke immediately
/// before it, which the host has already watched complete.
pub fn slot_poke_in_both_modes(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let even = ctx.scratch_addr();
    let odd = even + 1;

    // Command mode: knock, poke, and the device serves it back.
    let first = bus.read(even)? ^ 0xFF;
    bus.poke_verified(ctx, even, first)
        .map_err(|e| format!("SLOT_POKE in command mode: {e}"))?;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let neighbour = bus.read(odd)? ^ 0xFF;
    poke_active_verified(bus, &s, ctx, odd, neighbour)?;

    let second = first ^ 0x5A;
    poke_active_verified(bus, &s, ctx, even, second)?;

    // The processing sequence records what was just run.
    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::MODIFY)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, modify::SLOT_POKE)?;

    bus.expect_byte(odd, neighbour).map_err(|e| {
        format!(
            "{e} — poking 0x{even:06X} disturbed its neighbour at 0x{odd:06X}, and SLOT_POKE \
             writes a single byte"
        )
    })?;

    Ok(Outcome::Pass)
}

/// SLOT_POKE must reject a slot argument of 0xAA.
///
/// "An A4 value of 0xAA is invalid and rejected."  Every other argument is
/// valid, so the rejection can only be on account of the 0xAA; and rejection is
/// not silence — the device runs the processing sequence and reports failure.
pub fn slot_poke_rejects_slot_aa(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let addr = ctx.scratch_addr();

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let value = bus.read(addr)? ^ 0xFF;
    bus.expect_rejected(
        &s,
        group::MODIFY,
        modify::SLOT_POKE,
        &slot_poke_args(addr, value, 0xAA),
    )?;

    Ok(Outcome::Pass)
}

/// Patching a vector in an inactive slot, the way the specification recommends.
///
/// "The safe pattern is: LOAD_SLOT the target image into an inactive RAM slot,
/// issue SLOT_POKE commands to patch any vectors in that inactive slot, then
/// issue SWITCH_AND_EXIT to make it active.  The vector bytes are consistent at
/// the instant the slot becomes active."
///
/// What makes that pattern safe is everything before the switch, and that is
/// what this asserts: both halves of a two-byte vector are patched in the slot
/// that is not being served, and one peek reads them back together — so they
/// are already consistent when the switch comes.  Meanwhile the served slot
/// keeps values of its own at those addresses, which is what "the target slot
/// need not be active" has to mean for a host: a patch aimed at an inactive
/// slot must not appear in the one the machine is running from.  The vector
/// straddles a word boundary, as a real one does.
pub fn slot_poke_patches_inactive_slot(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let Some(target) = ctx.inactive_ram_slot() else {
        return Ok(needs_second_slot(ctx, "patching an inactive slot"));
    };
    let lo_addr = ctx.scratch_addr();
    let hi_addr = lo_addr + 1;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // The image's own bytes, read before anything is written over them.
    let flash = [bus.read(lo_addr)?, bus.read(hi_addr)?];

    // Give the served slot values of its own, so a patch that reached it — or a
    // peek that answered from it — is visible.
    let in_active = [flash[0] ^ 0x5A, flash[1] ^ 0x5A];
    poke_active_verified(bus, &s, ctx, lo_addr, in_active[0])?;
    poke_active_verified(bus, &s, ctx, hi_addr, in_active[1])?;

    bus.issue_cmd(&s, group::MODIFY, modify::LOAD_SLOT, &[target, 0])
        .map_err(|e| format!("LOAD_SLOT of flash slot 0 into RAM slot {target}: {e}"))?;

    let patch = [flash[0] ^ 0xFF, flash[1] ^ 0xFF];
    bus.poke_slot(&s, target, lo_addr, patch[0])?;
    bus.poke_slot(&s, target, hi_addr, patch[1])?;

    // One peek, both bytes: they are consistent together, which is the point of
    // patching before the slot is activated.
    let got = bus.peek_slot(&s, target, lo_addr, 2)?;
    if got != patch {
        return Err(format!(
            "slot {target} holds {got:02X?} at 0x{lo_addr:06X}, expected the patched \
             {patch:02X?} — the image's own bytes are {flash:02X?} and the served slot holds \
             {in_active:02X?}"
        ));
    }

    // And the served slot is untouched by any of it — fenced by the peek above,
    // a command the device completed after both patching pokes.
    bus.expect_byte(lo_addr, in_active[0])?;
    bus.expect_byte(hi_addr, in_active[1]).map_err(|e| {
        format!("{e} — a patch aimed at inactive slot {target} reached the slot being served")
    })?;

    Ok(Outcome::Pass)
}

/// SWITCH_SLOT activates the slot, and the back-channel goes with it.
///
/// "Activates the specified RAM slot", and the back-channel region is "a
/// structured region within the active RAM slot" — so once the switch is done,
/// the next command's header writes must land in the new slot, which is where
/// the host is now reading.
///
/// The switch itself is observed through the served image rather than through
/// the header: its token increment happens before the slot changes and its
/// completion after, so the two halves of its own processing sequence are in
/// different slots and neither alone is what is being asserted here.  The new
/// slot's header is armed with values the device would not write, so the NOP
/// that follows can only match by having been written there.
pub fn switch_slot_moves_the_back_channel(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let Some(target) = ctx.inactive_ram_slot() else {
        return Ok(needs_second_slot(ctx, "switching slots"));
    };
    let marker_addr = ctx.scratch_addr();

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // One address, a different value in each slot: which one is served says
    // which slot is active, and the device wrote both.
    let original = bus.read(marker_addr)?;
    let in_active = original ^ 0xFF;
    let in_target = original ^ 0x5A;
    poke_active_verified(bus, &s, ctx, marker_addr, in_active)?;
    bus.poke_slot(&s, target, marker_addr, in_target)?;

    // Arm the target slot's header.  0x77 is the CMD of no command in any
    // group, and the token is armed far from the value the device must write.
    const ARMED_LAST_CMD: u8 = 0x77;
    bus.poke_slot(
        &s,
        target,
        s.bch_start + Hdr::LastCmdGroup.offset(),
        ARMED_LAST_CMD,
    )?;
    bus.poke_slot(
        &s,
        target,
        s.bch_start + Hdr::LastCmdCmd.offset(),
        ARMED_LAST_CMD,
    )?;
    let token = bus.read_hdr(&s, Hdr::TokenLsb)?;
    bus.poke_slot(
        &s,
        target,
        s.bch_start + Hdr::TokenLsb.offset(),
        token.wrapping_add(0x40),
    )?;

    // Three commands follow that token read: the arming poke above, the switch,
    // and the NOP.  The token is "incremented by exactly 1 by the device on
    // receipt of every command", so the NOP's write must be token + 3.
    let want_token = token.wrapping_add(3);

    // Sent rather than polled: the specification gives the host nothing to poll
    // for while the back-channel is moving between slots.
    bus.send_cmd(
        s.command_page,
        group::MODIFY,
        modify::SWITCH_SLOT,
        &[target],
    )?;
    bus.await_byte(marker_addr, in_target).map_err(|e| {
        format!(
            "{e} — SWITCH_SLOT must activate slot {target}, whose 0x{marker_addr:06X} holds \
             0x{in_target:02X} against the served slot's 0x{in_active:02X}; {}",
            slot_view(bus, ctx, target, marker_addr)
        )
    })?;

    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP after SWITCH_SLOT: {e}"))?;

    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::CONTROL)
        .map_err(|e| {
            format!(
                "{e} — 0x{ARMED_LAST_CMD:02X} is what this scenario armed slot {target}'s header \
                 with, so the NOP's back-channel write did not follow the active slot"
            )
        })?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)?;
    bus.expect_hdr(&s, Hdr::TokenLsb, want_token).map_err(|e| {
        format!("{e} — three commands were issued after the token read 0x{token:02X}")
    })?;
    bus.expect_hdr(&s, Hdr::Progress, s.complete)?;
    bus.expect_hdr(&s, Hdr::Response, s.status_ok)?;

    Ok(Outcome::Pass)
}

/// SWITCH_SLOT works in command mode, as does a poke at an inactive slot.
///
/// The Modify group is "valid in both command and command-response modes", and
/// in command mode there is no back-channel — "the host must assume that any
/// well-formed command was received and is being processed".  So the exchange
/// is unconfirmed until the device serves the new slot: the target slot's byte
/// is written blind, by a SLOT_POKE naming a slot that is not being served, and
/// only the switch makes it readable.  Until then the address serves the active
/// slot's value, and the device wrote both.
pub fn switch_slot_in_command_mode(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let Some(target) = ctx.inactive_ram_slot() else {
        return Ok(needs_second_slot(ctx, "switching slots"));
    };
    let marker_addr = ctx.scratch_addr();
    let page = ctx.command_page();

    let original = bus.read(marker_addr)?;
    let in_active = original ^ 0xFF;
    let in_target = original ^ 0x5A;

    bus.poke_verified(ctx, marker_addr, in_active)
        .map_err(|e| format!("marking the served slot: {e}"))?;

    bus.knock(page)?;
    bus.send_poke_slot(page, target, marker_addr, in_target)?;

    bus.knock(page)?;
    bus.send_cmd(page, group::MODIFY, modify::SWITCH_SLOT, &[target])?;

    bus.await_byte(marker_addr, in_target).map_err(|e| {
        format!(
            "{e} — in command mode, SLOT_POKE into inactive slot {target} followed by \
             SWITCH_SLOT must leave 0x{marker_addr:06X} serving 0x{in_target:02X} rather than \
             the served slot's 0x{in_active:02X}; {}",
            slot_view(bus, ctx, target, marker_addr)
        )
    })?;

    Ok(Outcome::Pass)
}

/// SWITCH_SLOT must reject a slot argument of 0xAA.
///
/// "An A0 value of 0xAA is invalid and rejected."
pub fn switch_slot_rejects_slot_aa(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.expect_rejected(&s, group::MODIFY, modify::SWITCH_SLOT, &[0xAA])?;

    Ok(Outcome::Pass)
}

/// LOAD_SLOT copies a flash image into a RAM slot and does not activate it.
///
/// "Copies the specified ROM image from the slot on the ROM into the specified
/// RAM slot.  Does not activate the slot."  Both halves are asserted, each
/// against a value the device itself wrote:
///
/// - **copied** — the target slot is given values of its own at the test
///   addresses before the copy, so the image's own bytes can only be peeked out
///   of it because LOAD_SLOT put them there;
/// - **not activated** — the served slot is given a third set of values, and
///   after the copy — fenced by the peek, a command the device completed after
///   it — the bus must still serve those rather than the image's.
///
/// Flash slot 0 is the image the device booted serving, so what the copy must
/// produce is exactly what the bus served before anything was written over it.
pub fn load_slot_copies_without_activating(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    /// Enough bytes that a copy is being asserted rather than a byte, starting
    /// on an odd address so the range straddles a word.
    const LEN: u32 = 4;

    let Some(target) = ctx.inactive_ram_slot() else {
        return Ok(needs_second_slot(
            ctx,
            "loading a slot that is not being served",
        ));
    };
    let base = ctx.scratch_addr() + 1;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let mut in_flash = Vec::new();
    for i in 0..LEN {
        in_flash.push(bus.read(base + i)?);
    }
    for (i, &v) in in_flash.iter().enumerate() {
        poke_active_verified(bus, &s, ctx, base + i as u32, v ^ 0xFF)?;
        bus.poke_slot(&s, target, base + i as u32, v ^ 0x5A)?;
    }

    // A peek is only evidence about the copy if it reads the slot it names, so
    // require that while the two slots still differ everywhere.
    expect_peek(
        bus,
        &s,
        target,
        base,
        in_flash[0] ^ 0x5A,
        &format!(
            "that is what slot {target} was poked with, against 0x{:02X} in the served slot — a \
             peek must read the slot it names",
            in_flash[0] ^ 0xFF
        ),
    )?;

    bus.issue_cmd(&s, group::MODIFY, modify::LOAD_SLOT, &[target, 0])
        .map_err(|e| format!("LOAD_SLOT of flash slot 0 into RAM slot {target}: {e}"))?;

    let got = bus.peek_slot(&s, target, base, LEN as u8)?;
    if got != in_flash {
        return Err(format!(
            "after LOAD_SLOT of flash slot 0 into RAM slot {target}, that slot holds {got:02X?} \
             at 0x{base:06X}, expected the image's own {in_flash:02X?} — it was poked with \
             {:02X?} beforehand, so anything else is not a copy of the image",
            in_flash.iter().map(|v| v ^ 0x5A).collect::<Vec<_>>()
        ));
    }

    for (i, &v) in in_flash.iter().enumerate() {
        let addr = base + i as u32;
        bus.expect_byte(addr, v ^ 0xFF).map_err(|e| {
            format!(
                "{e} — LOAD_SLOT does not activate the slot it loads, so 0x{addr:06X} must still \
                 serve the value poked into slot {}, not the image's 0x{v:02X}",
                ctx.active_ram_slot
            )
        })?;
    }

    Ok(Outcome::Pass)
}

/// LOAD_SLOT must reject either slot argument being 0xAA.
///
/// "A0 or A1 values of 0xAA are invalid and rejected."  Each is tried with the
/// other valid, so each rejection stands on its own argument.
pub fn load_slot_rejects_slot_aa(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.expect_rejected(&s, group::MODIFY, modify::LOAD_SLOT, &[0xAA, 0])
        .map_err(|e| format!("{e} — A0, the RAM slot, was 0xAA"))?;
    bus.expect_rejected(
        &s,
        group::MODIFY,
        modify::LOAD_SLOT,
        &[ctx.active_ram_slot, 0xAA],
    )
    .map_err(|e| format!("{e} — A1, the flash slot, was 0xAA"))?;

    Ok(Outcome::Pass)
}

/// SLOT_POKE_ALL_BYTE fills the whole slot, and does not activate it.
///
/// "Fills the specified RAM slot with the specified byte.  Does not activate
/// the slot."  Four addresses spread across the image — its first byte, its
/// last, and a pair straddling a word in between — are each given a value that
/// is not the fill byte first, so each can only peek back as the fill byte by
/// having been filled.  A device that wrote one byte, or one page, leaves at
/// least one of them holding what was armed there.
///
/// The bottom of the image is reachable no other way: a *host* read below the
/// command page is a command byte, so the device's own read is what makes
/// address 0 assertable at all.
pub fn slot_poke_all_byte_fills_the_slot(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let Some(target) = ctx.inactive_ram_slot() else {
        return Ok(needs_second_slot(
            ctx,
            "filling a slot that is not being served",
        ));
    };
    let image_size = ctx.chip_type.size_bytes() as u32;
    let probes = [
        0,
        ctx.scratch_addr(),
        ctx.scratch_addr() + 1,
        image_size - 1,
    ];

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // The fill byte, and the value the target slot is armed with, both derived
    // from the image so that neither is already in place.
    let fill = bus.read(probes[1])? ^ 0x5A;
    let armed = fill ^ 0x0F;

    for &addr in &probes {
        bus.poke_slot(&s, target, addr, armed)?;
        // The served slot is marked too, wherever the host can read it, so a
        // fill that reached the wrong slot is visible.
        if addr > probes[0] {
            poke_active_verified(bus, &s, ctx, addr, fill ^ 0xFF)?;
        }
    }

    bus.issue_cmd(
        &s,
        group::MODIFY,
        modify::SLOT_POKE_ALL_BYTE,
        &[fill, target],
    )
    .map_err(|e| format!("SLOT_POKE_ALL_BYTE of 0x{fill:02X} into RAM slot {target}: {e}"))?;

    for &addr in &probes {
        expect_peek(
            bus,
            &s,
            target,
            addr,
            fill,
            &format!(
                "SLOT_POKE_ALL_BYTE fills the slot, and 0x{armed:02X} is what this address was \
                 armed with; every address of the {image_size}-byte image must hold 0x{fill:02X}"
            ),
        )?;
    }

    // Fenced by the peeks above: the fill went to the slot it named, and the
    // slot being served is still the one being served.
    for &addr in &probes[1..] {
        bus.expect_byte(addr, fill ^ 0xFF).map_err(|e| {
            format!(
                "{e} — SLOT_POKE_ALL_BYTE names slot {target} and does not activate it, so the \
                 served slot must keep the value poked into it"
            )
        })?;
    }

    Ok(Outcome::Pass)
}

/// SLOT_POKE_ALL_BYTE must reject a slot argument of 0xAA.
///
/// "An A1 value of 0xAA is invalid and rejected."
pub fn slot_poke_all_byte_rejects_slot_aa(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.expect_rejected(&s, group::MODIFY, modify::SLOT_POKE_ALL_BYTE, &[0x5A, 0xAA])?;

    Ok(Outcome::Pass)
}
