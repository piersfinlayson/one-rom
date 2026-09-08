// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: the RAM slot argument, wherever a command takes one.
//!
//! Seven commands across four groups name a RAM slot, and each is specified to
//! fail where "the RAM slot specified is invalid".  Two different things make it
//! invalid, and the per-group modules already cover one of them: a value of
//! 0xAA, which is barred from every final argument so that a reset started
//! mid-command stays detectable.
//!
//! This module is the other one — an index the device simply does not have.  A
//! device that checked only for 0xAA would pass every `*_rejects_slot_aa`
//! scenario in the suite and still read or write memory it was never given, so
//! the two belong to the same requirement and neither implies the other.
//!
//! # Which index is absent
//!
//! Slots are numbered contiguously from zero, so the count the device reports
//! through GET_RAM_SLOT_INFO_ALL is itself the first number that is not one of
//! them — the same reasoning [`super::pipes::commands_reject_an_absent_pipe`]
//! applies to pipe numbers.  Reading it off the device is what keeps this
//! board-independent: nothing here knows how much RAM a board has.
//!
//! The one exception is a device offering the full 170.  Slot 170 is 0xAA, which
//! every slot argument bars, so no host could name it and no device can offer
//! more — which is why 170 is the ceiling, and why 171 is a slot no conformant
//! device has.  A device claiming more than 170 fails here rather than skipping,
//! since it is advertising slots no host could ever reach.
//!
//! SET_AUX_SWITCH_EXIT is the seventh command and is not here.  It writes no
//! response header, so its refusal is not readable this way — it lives in
//! [`super::aux`], next to the 0xAA scenario it mirrors.
//!
//! Each command's own module establishes that it works with a slot the device
//! does have, so nothing here needs a positive control of its own.

use crate::driver::{Bus, Session, group, modify, nv, read, slot_peek_args, slot_poke_args};
use crate::{Ctx, Outcome};

/// Slots a device can offer at most: 0xAA is barred from every slot argument, so
/// slot 170 could never be named and nothing above it could be reached either.
pub const MAX_NAMEABLE_SLOTS: u8 = 0xAA;

/// The lowest RAM slot index the device does not have, over the bus.
///
/// The count is the first index past the last slot, except where the device
/// offers every slot it can, and then the first absent index is one further on —
/// 0xAA itself being barred by a rule of its own.
pub fn absent_slot(bus: &mut Bus, s: &Session) -> Result<u8, String> {
    bus.issue_cmd(s, group::READ, read::GET_RAM_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_RAM_SLOT_INFO_ALL: {e}"))?;
    let count = bus.read_data(s, 0, 1)?[0];
    if count > MAX_NAMEABLE_SLOTS {
        return Err(format!(
            "the device advertises {count} RAM slots; a slot argument of 0xAA is invalid, so \
             slot {MAX_NAMEABLE_SLOTS} can never be named and a device may offer at most that \
             many"
        ));
    }
    Ok(if count == MAX_NAMEABLE_SLOTS {
        MAX_NAMEABLE_SLOTS + 1
    } else {
        count
    })
}

/// Every command that names a RAM slot rejects one the device does not have.
///
/// The slot is the lowest index the device does not have, and every other
/// argument is sound, so the rejection can only be on account of the slot.
/// Rejection is not silence: [`Bus::expect_rejected`] requires the token to have
/// moved and the response field to say failed, which is what the specification's
/// "fails" means and what tells this apart from a device that lost the frame.
pub fn commands_reject_a_slot_the_device_lacks(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let absent = absent_slot(bus, &s)?;

    let addr = ctx.scratch_addr();
    let value = bus.read(addr)? ^ 0xFF;

    let peek = slot_peek_args(addr, 1, absent);
    let poke = slot_poke_args(addr, value, absent);
    let switch = [absent];
    let load = [absent, 0];
    let fill = [value, absent];
    let begin = [absent];

    for (what, grp, cmd, args) in [
        ("SLOT_PEEK", group::READ, read::SLOT_PEEK, &peek[..]),
        ("SLOT_POKE", group::MODIFY, modify::SLOT_POKE, &poke[..]),
        (
            "SWITCH_SLOT",
            group::MODIFY,
            modify::SWITCH_SLOT,
            &switch[..],
        ),
        ("LOAD_SLOT", group::MODIFY, modify::LOAD_SLOT, &load[..]),
        (
            "SLOT_POKE_ALL_BYTE",
            group::MODIFY,
            modify::SLOT_POKE_ALL_BYTE,
            &fill[..],
        ),
        (
            "NV_POKE_BEGIN",
            group::NV_STORAGE,
            nv::NV_POKE_BEGIN,
            &begin[..],
        ),
    ] {
        bus.expect_rejected(&s, grp, cmd, args).map_err(|e| {
            format!(
                "{what}: {e} — slot {absent} is not one this device has, and a host may not name \
                 it"
            )
        })?;
    }

    Ok(Outcome::Pass)
}
