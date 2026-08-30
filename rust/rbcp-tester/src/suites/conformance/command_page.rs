// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Command Page".
//!
//! "During command-response mode, the device filters incoming address reads
//! using the command page configured in ENTER_CMD_RESP.  Only reads whose upper
//! address bits (above A7) match the command page value are treated as command
//! bytes."
//!
//! This is what lets a host read its own back-channel.  Those reads are on the
//! page the region lives on, and the host makes hundreds of them per command —
//! so a device that decoded them would be taking its own answers as commands.
//! The knock this scenario sends off the page lands inside the back-channel
//! region for that very reason: on this tester's layout the six knock bytes are
//! six ordinary reads of the region, which is the case the filter exists for.
//!
//! [`super::reset::off_page_reset_is_filtered`] covers the same filter for
//! RBCP_RESET, which needs no knock in front of it and so says nothing about
//! whether a knock is filtered.
//!
//! # Every other session in this tester runs at page zero
//!
//! [`crate::Ctx::command_page`] is zero, and every scenario outside this module
//! enters with it.  A device that ignored ENTER_CMD_RESP's A0/A1 altogether and
//! filtered on page zero would pass all of them, which is why
//! [`the_configured_page_is_the_one_filtered_on`] enters at a page of its own
//! and builds its own back-channel to do it.
//!
//! # What this suite does not verify
//!
//! 1. The filter outside command-response mode.  "Outside command-response
//!    mode, the command page has no effect", so there is no requirement to
//!    hold a device to.
//! 2. Pages beyond the two these scenarios name.  A device is held to filtering
//!    one wrong page in each direction rather than all of them.
//! 3. How wide the field is, exactly.  The configured page below is the highest
//!    the ROM allows, so it differs from zero in the top bit of the field as
//!    well as the bottom, but a device reading one bit more or fewer than the
//!    specification names is not distinguished from a conformant one by two
//!    pages alone.

use crate::driver::{Bus, HDR_SIZE, Hdr, Session, control, group};
use crate::{Ctx, Outcome};

/// A knock and a command on another page must not be a command.
///
/// Asserted by arithmetic on the token rather than by its standing still: an
/// on-page NOP follows, and that NOP moves the token by exactly one.  A device
/// that acted on the off-page frame leaves it two on, and one that let those
/// bytes into its framing never completes the NOP at all — so the same read
/// distinguishes the two ways of getting this wrong from the right answer.
pub fn off_page_knock_and_command_are_filtered(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let before = bus.read_hdr(&s, Hdr::TokenLsb)?;

    // A whole session's worth of signalling, on a page the device is not
    // listening to.
    bus.knock(ctx.other_page())?;
    bus.send_cmd(ctx.other_page(), group::CONTROL, control::NOP, &[])?;

    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| {
            format!(
                "NOP after an off-page knock and NOP: {e} — the device let command bytes that \
                 did not match the configured command page into its framing"
            )
        })?;

    let want = before.wrapping_add(1);
    let got = bus.read_hdr(&s, Hdr::TokenLsb)?;
    if got != want {
        return Err(format!(
            "the token went 0x{before:02X} to 0x{got:02X} over an off-page knock and NOP \
             followed by one on-page NOP, and only the on-page NOP may move it, to \
             0x{want:02X} — the device acted on command bytes outside its command page"
        ));
    }

    Ok(Outcome::Pass)
}

/// The page the device filters on is the one the host configured.
///
/// "A0/A1 specify the command page: during command-response mode the device
/// treats only address reads whose upper address bits match this value as
/// command bytes."  A device that took no notice of those two argument bytes
/// and filtered on page zero would satisfy every other scenario in this tester,
/// all of which enter with [`crate::Ctx::command_page`] — so this one enters
/// somewhere else and holds the device to it both ways round: its own page is
/// where commands are acted on, and page zero is where they are ignored.
///
/// The highest page the ROM allows, rather than page one, so that the two pages
/// in play differ in the top bit of the field as well as the bottom.
///
/// The back-channel goes at the bottom of the slot, below the command page,
/// which is the opposite of this tester's usual layout and is what the choice
/// of page forces.  Reads of it are then reads below the command page, which is
/// what the device must not treat as signalling.
pub fn the_configured_page_is_the_one_filtered_on(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    // The page has to be one the *host* can drive, so it is bounded by the ROM
    // being served and not by the RAM slot holding it.  A banked configuration
    // puts several images in one slot, and a host reaching only the served
    // one's address lines cannot signal on a page above them.  The slot bounds
    // it too, where the ROM type is the larger of the two.
    let addressable = ctx.ram_slot_size.min(ctx.chip_type.size_bytes() as u32);
    let pages = (addressable >> ctx.unobserved) >> 8;
    if pages < 2 {
        return Ok(Outcome::Skip(format!(
            "the {} bytes of this {} the host can address are {pages} page(s) of observed \
             address space, and a page other than zero needs two",
            addressable,
            ctx.chip_type.name()
        )));
    }
    let command_page = (pages - 1) as u16;

    // Everything below the command page's first byte is the room the region
    // has.  On the smallest ROM here that is a page of it, which is ample for
    // a header.
    let room = (u32::from(command_page) << 8) << ctx.unobserved;
    let bch_size = room.min(u32::from(ctx.bch_size())) as u16;
    if u32::from(bch_size) < HDR_SIZE {
        return Ok(Outcome::Skip(format!(
            "page {command_page} starts at byte {room} of the slot, which leaves no room \
             below it for the {HDR_SIZE}-byte response header"
        )));
    }

    let s = Session {
        command_page,
        bch_start: 0,
        bch_size,
        ..ctx.session()
    };

    bus.enter_cmd_resp(&s).map_err(|e| {
        format!(
            "ENTER_CMD_RESP naming command page {command_page}: {e} — the device is \
                 required to honour a page the ROM has room for"
        )
    })?;

    let before = bus.read_hdr(&s, Hdr::TokenLsb)?;

    // A whole session's worth of signalling on page zero, which this device was
    // not told to listen to and every other scenario here uses.
    bus.knock(ctx.command_page())?;
    bus.send_cmd(ctx.command_page(), group::CONTROL, control::NOP, &[])?;

    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| {
            format!(
                "NOP on command page {command_page}: {e} — that is the page the device was \
                 told to take commands on"
            )
        })?;

    let want = before.wrapping_add(1);
    let got = bus.read_hdr(&s, Hdr::TokenLsb)?;
    if got != want {
        return Err(format!(
            "the token went 0x{before:02X} to 0x{got:02X} over a knock and NOP on page 0 \
             followed by one NOP on the configured page {command_page}, and only the second \
             may move it, to 0x{want:02X} — the device is filtering on page 0 rather than on \
             the page ENTER_CMD_RESP named"
        ));
    }

    Ok(Outcome::Pass)
}
