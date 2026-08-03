// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The specification's "Example — C64 Kernal Bootloader", run end to end.
//!
//! The worked example is nine steps, of which the first two are the host
//! copying itself into RAM and have no device side.  The rest are here, in
//! order, and each is a step the 6502 reference host has a routine for:
//!
//! | Example step | Here | Reference host |
//! |---|---|---|
//! | — | reset the device first | `rbcp_reset` |
//! | 3 | ENTER_CMD_RESP | `rbcp_cmd_enter_cmd_resp` |
//! | 4 | poll token then progress | `rbcp_issue_cmd` |
//! | — | check the protocol version | `rbcp_check_protocol_version` |
//! | 5, 6 | GET_FLASH_SLOT_INFO_ALL, read the menu | `rbcp_cmd_get_flash_slot_info_all` |
//! | 7 | LOAD_SLOT then SWITCH_AND_EXIT | `rbcp_cmd_load_slot`, `rbcp_cmd_switch_and_exit` |
//! | 9 | jump through the reset vector of the newly loaded ROM | — |
//!
//! with SLOT_POKE between the load and the switch, which is the pattern
//! SLOT_POKE itself prescribes: "LOAD_SLOT the target image into an inactive
//! RAM slot, issue SLOT_POKE commands to patch any vectors in that inactive
//! slot, then issue SWITCH_AND_EXIT to make it active".
//!
//! # What makes this an integration scenario rather than a longer conformance one
//!
//! Every command here has its own conformance scenario, and this asserts none
//! of them again.  What it asserts is the *end state a bootloader lands in*,
//! checked the way step 9 checks it — by reading the ROM.  A device that obeyed
//! every individual rule but left the host serving the wrong slot, or serving
//! the right slot without the patch, or still writing into a back-channel the
//! host has stopped reading, passes the conformance suite and fails here.
//!
//! So the verdict is three bus reads after the switch, and nothing else:
//!
//! - the patched vector is what the bus serves, which is one assertion about
//!   two things — the switch happened, and the patch went with it;
//! - the device takes a fresh command-mode session, which is the fence: it has
//!   demonstrably processed a command *after* the exit;
//! - the progress byte still holds a value this scenario chose, rather than
//!   either of the two the device writes while it is maintaining a
//!   back-channel.
//!
//! # Distinguishable values
//!
//! Three bytes live at the vector address over the course of the flow: the one
//! the served slot is marked with, the one LOAD_SLOT copies in, and the patch.
//! They are chosen to differ from each other, so the final read discriminates
//! rather than merely matching — a switch that did not happen serves the first,
//! a patch that did not land serves the second.
//!
//! # A second RAM slot
//!
//! The pattern is defined in terms of a slot that is not being served, so a
//! device with one RAM slot cannot run it.  Whether it has one turns on the
//! size of the ROM table region rather than on the part: a region of 512 KB
//! leaves room for a single slot, which a 27C400 reaches on either 40-pin
//! board and a 27C200 reaches on fire-40-a but not on fire-40-b.
//! It skips there rather than degenerating into a patch of the active slot,
//! which is the thing the specification warns against.

use crate::driver::{Bus, Hdr, Session, control, group, modify, read};
use crate::{Ctx, Outcome};

/// GET_FLASH_SLOT_INFO_ALL's preamble: "total_count, whole_count,
/// partial_flag, Reserved".
const PREAMBLE_SIZE: u32 = 4;

/// One flash slot record: "1 byte rom_type, 31 bytes name".
const RECORD_SIZE: u32 = 32;

/// The RBCP version this flow is written against, and the rule
/// `rbcp_check_protocol_version` applies to it: "A host implementation written
/// against version 0.Y.z is guaranteed to interoperate correctly with any
/// device implementing version 0.Y.w where w >= z."
const SPEC_MAJOR: u8 = 0;
const SPEC_MINOR: u8 = 1;
const SPEC_PATCH: u8 = 1;

/// How many bytes of the target slot are armed before LOAD_SLOT, to catch a
/// copy that did not happen.  The vector's two, plus two more so the check does
/// not rest on the image happening not to carry the armed byte at one address.
const LOAD_PROBE_LEN: u8 = 4;

/// One entry of the menu the bootloader builds from GET_FLASH_SLOT_INFO_ALL.
struct MenuEntry {
    slot: u8,
    rom_type: u8,
    name: String,
}

/// Load an image into an inactive slot, patch its vector, switch to it, and
/// verify the end state by reading the ROM.
///
/// The whole of the specification's worked example, and the point of the suite:
/// see the module documentation for what is asserted and why the verdict is
/// three bus reads rather than a back-channel poll.
pub fn kernal_bootloader(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let Some(target) = ctx.inactive_ram_slot() else {
        return Ok(Outcome::Skip(format!(
            "the device has one RAM slot (a {} served from slot {}), and the specification's \
             patch-then-switch pattern needs a slot other than the one being served",
            ctx.chip_type.name(),
            ctx.active_ram_slot
        )));
    };

    // The two bytes of the vector the bootloader patches.  Adjacent, as a real
    // one is, so on a word-organised ROM they are the two halves of one word.
    let vec_lo = ctx.scratch_addr();
    let vec_hi = vec_lo + 1;

    let s = ctx.session();

    // The bootloader cannot know what state the device is in — it may have been
    // left mid-command by a previous run — so it resets before anything else.
    bus.reset(s.command_page)?;

    // Example step 3, and step 4: enter_cmd_resp is the specification's own
    // bootstrap, token snapshot and all.
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    check_protocol_version(bus, &s)?;

    // Which ROM type is being served?  A bootloader must know, because the
    // image it loads has to be one this socket can serve.
    let served_type = ram_slot_info(bus, &s, ctx)?;

    // Example steps 5 and 6: read the catalogue and build the menu.
    let menu = flash_slot_menu(bus, &s)?;
    let chosen = select_slot(&menu, served_type)?;

    // Mark the served slot at the vector, so a switch that did not happen is
    // caught: the bus would go on serving this value.
    let served = [bus.read(vec_lo)?, bus.read(vec_hi)?];
    let marked = [served[0] ^ 0x5A, served[1] ^ 0x5A];
    for (addr, value) in [(vec_lo, marked[0]), (vec_hi, marked[1])] {
        bus.poke_slot_verified(&s, ctx.active_ram_slot, addr, value)?;
    }

    // Fill the target slot across the vector and its neighbours with a value of
    // this scenario's choosing, so that a LOAD_SLOT which copied nothing is
    // caught: it would leave every one of them holding it.  Four bytes rather
    // than the vector's two so that the check does not rest on the image
    // happening not to carry that byte at one address.
    let armed_before_load = served[0] ^ 0x33;
    for i in 0..LOAD_PROBE_LEN {
        bus.poke_slot(&s, target, vec_lo + u32::from(i), armed_before_load)?;
    }

    // Example step 7, first half.
    bus.issue_cmd(&s, group::MODIFY, modify::LOAD_SLOT, &[target, chosen.slot])
        .map_err(|e| {
            format!(
                "LOAD_SLOT of flash slot {} (\"{}\") into RAM slot {target}: {e}",
                chosen.slot, chosen.name
            )
        })?;

    // What the copy produced, which the patch must be told apart from.
    let window = bus.peek_slot(&s, target, vec_lo, LOAD_PROBE_LEN)?;
    if window.iter().all(|&b| b == armed_before_load) {
        return Err(format!(
            "after LOAD_SLOT of flash slot {} (\"{}\") into RAM slot {target}, all \
             {LOAD_PROBE_LEN} bytes from 0x{vec_lo:06X} still hold the 0x{armed_before_load:02X} \
             they were armed with — the copy did not happen, and everything below would then be \
             asserting only what this scenario itself wrote",
            chosen.slot, chosen.name
        ));
    }
    let loaded = [window[0], window[1]];

    let patch = [
        distinct_from(loaded[0], marked[0]),
        distinct_from(loaded[1], marked[1]),
    ];
    for (addr, value) in [(vec_lo, patch[0]), (vec_hi, patch[1])] {
        bus.poke_slot(&s, target, addr, value)?;
    }

    // Arm the target slot's progress byte with a value the device would never
    // write, so that after the exit the read below discriminates rather than
    // merely finding the byte unchanged.  Peeked back, because an arming write
    // this scenario has not seen land proves nothing.
    let progress_addr = s.bch_start + Hdr::Progress.offset();
    let armed = arm_progress(bus, &s, target, progress_addr)?;

    // Example step 7, second half.  Sent, not polled: "The host must not poll
    // the back-channel region after issuing this command — the device begins
    // serving the new slot immediately and the previous back-channel region is
    // invalidated."
    bus.send_cmd(
        s.command_page,
        group::CONTROL,
        control::SWITCH_AND_EXIT,
        &[target],
    )?;

    // Example step 9, and the verdict: the bootloader jumps through the reset
    // vector of the newly loaded ROM, so it reads it from the ROM.
    bus.await_byte(vec_lo, patch[0]).map_err(|e| {
        format!(
            "{e} — after SWITCH_AND_EXIT to RAM slot {target} the bus must serve the patched \
             vector 0x{:02X}{:02X}, against the 0x{:02X}{:02X} marking the slot that was active \
             and the 0x{:02X}{:02X} LOAD_SLOT copied in from flash slot {}",
            patch[1], patch[0], marked[1], marked[0], loaded[1], loaded[0], chosen.slot
        )
    })?;
    bus.expect_byte(vec_hi, patch[1]).map_err(|e| {
        format!(
            "{e} — both halves of the vector were patched before the switch, so both are \
             consistent at the instant the slot becomes active"
        )
    })?;

    // The fence, aimed at the slot that is now being served: the device serving
    // this poke back proves it has taken a command-mode session since the exit,
    // and so is not merely running late.
    bus.fence_slot(ctx, target)
        .map_err(|e| format!("the device took no command-mode session after the exit: {e}"))?;

    let got = bus.read(progress_addr)?;
    if got == s.complete || got == s.pending() {
        return Err(format!(
            "the progress byte at 0x{progress_addr:06X} holds 0x{got:02X}, the {} value, rather \
             than the armed 0x{armed:02X} — SWITCH_AND_EXIT exits command-response mode, so the \
             device must no longer be maintaining a back-channel in the slot it switched to",
            if got == s.complete {
                "complete"
            } else {
                "pending"
            }
        ));
    }
    if got != armed {
        return Err(format!(
            "the progress byte at 0x{progress_addr:06X} holds 0x{got:02X}, which is neither the \
             armed 0x{armed:02X} nor either of the values a maintained back-channel would write \
             — something else reached it"
        ));
    }

    // Positive control.  ENTER_CMD_RESP "is not supported when in
    // command-response mode", so an entry that succeeds is proof the device had
    // left it, rather than merely having gone quiet.  Last, because entering
    // writes the header the read above depends on.
    bus.enter_cmd_resp(&s).map_err(|e| {
        format!(
            "ENTER_CMD_RESP after SWITCH_AND_EXIT: {e} — a bootloader that wanted the device \
             back must be able to knock and re-enter, and an entry that fails here would mean \
             the device never left command-response mode"
        )
    })?;

    Ok(Outcome::Pass)
}

/// The version check `rbcp_check_protocol_version` performs before trusting the
/// device: major exact, and — because the major here is 0 — minor exact with
/// patch at least what the host was written against.
fn check_protocol_version(bus: &mut Bus, s: &Session) -> Result<(), String> {
    bus.issue_cmd(s, group::READ, read::GET_PROTOCOL_VERSION, &[])
        .map_err(|e| format!("GET_PROTOCOL_VERSION: {e}"))?;

    let v = bus.read_data(s, 0, 3)?;
    if v[0] != SPEC_MAJOR || v[1] != SPEC_MINOR || v[2] < SPEC_PATCH {
        return Err(format!(
            "the device reports RBCP {}.{}.{}; this flow is written against \
             {SPEC_MAJOR}.{SPEC_MINOR}.{SPEC_PATCH}, and a host \"should query the device \
             version using GET_PROTOCOL_VERSION and reject a device whose version falls outside \
             the bounds it was written for\"",
            v[0], v[1], v[2]
        ));
    }
    Ok(())
}

/// GET_RAM_SLOT_INFO_ALL, returning the ROM type currently being served.
///
/// The counts are checked against what the device reported through its own
/// plugin API at start-up, which is where [`Ctx`] gets them — two different
/// paths through the device agreeing, rather than a response agreeing with
/// itself.
fn ram_slot_info(bus: &mut Bus, s: &Session, ctx: &Ctx) -> Result<u8, String> {
    bus.issue_cmd(s, group::READ, read::GET_RAM_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_RAM_SLOT_INFO_ALL: {e}"))?;

    let info = bus.read_data(s, 0, 4)?;
    if info[1] != ctx.active_ram_slot {
        return Err(format!(
            "GET_RAM_SLOT_INFO_ALL reports slot {} active; the device's own plugin API reports \
             slot {} active",
            info[1], ctx.active_ram_slot
        ));
    }
    if info[0] == 0 || info[1] >= info[0] {
        return Err(format!(
            "GET_RAM_SLOT_INFO_ALL reports {} RAM slot(s) with slot {} active — the active slot \
             has to be one of the slots the device says it has",
            info[0], info[1]
        ));
    }
    Ok(info[2])
}

/// GET_FLASH_SLOT_INFO_ALL, read as a bootloader builds its menu: the preamble
/// first, then the whole records it says are present.
///
/// A truncated record is deliberately not listed.  The specification allows one
/// — "If partial_flag is 0x01, a truncated record follows" — but a menu entry
/// whose ROM type is known and whose name may be cut short is not one a
/// bootloader can offer, and this session's back-channel is large enough that
/// the case does not arise.
fn flash_slot_menu(bus: &mut Bus, s: &Session) -> Result<Vec<MenuEntry>, String> {
    bus.issue_cmd(s, group::READ, read::GET_FLASH_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_FLASH_SLOT_INFO_ALL: {e}"))?;

    let preamble = bus.read_data(s, 0, PREAMBLE_SIZE)?;
    let (total, whole, partial) = (preamble[0], preamble[1], preamble[2]);
    if preamble[3] != 0 {
        return Err(format!(
            "the GET_FLASH_SLOT_INFO_ALL preamble's reserved byte is 0x{:02X}, and it \"must be \
             zero\"",
            preamble[3]
        ));
    }
    if u32::from(whole) * RECORD_SIZE + PREAMBLE_SIZE > s.data_size() {
        return Err(format!(
            "the device reports {whole} whole record(s), which do not fit in the {}-byte data \
             section",
            s.data_size()
        ));
    }

    let mut menu = Vec::new();
    for slot in 0..whole {
        let record = bus.read_data(
            s,
            PREAMBLE_SIZE + u32::from(slot) * RECORD_SIZE,
            RECORD_SIZE,
        )?;
        let name = record[1..]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        menu.push(MenuEntry {
            slot,
            rom_type: record[0],
            name,
        });
    }
    if menu.is_empty() {
        return Err(format!(
            "GET_FLASH_SLOT_INFO_ALL returned no whole records (total_count {total}, \
             whole_count {whole}, partial_flag 0x{partial:02X}), so there is nothing a \
             bootloader could offer"
        ));
    }
    Ok(menu)
}

/// The menu entry a bootloader would choose: the last image whose ROM type is
/// the one the socket is being served as.
///
/// A real menu offers only images this socket can serve, and the last matching
/// one rather than the first so that the choice is made from the response data
/// rather than being index 0 by default wherever a device has more than one
/// image of a type.
fn select_slot(menu: &[MenuEntry], served_type: u8) -> Result<&MenuEntry, String> {
    menu.iter()
        .rfind(|e| e.rom_type == served_type)
        .ok_or_else(|| {
            format!(
                "no flash slot holds a ROM of the type being served (0x{served_type:02X}); the \
                 menu is {}",
                menu.iter()
                    .map(|e| format!("{}: 0x{:02X} \"{}\"", e.slot, e.rom_type, e.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// A value that is neither of two others.
///
/// The patch has to differ from what LOAD_SLOT copied in, or a patch that never
/// landed would read the same, and from what the served slot was marked with,
/// or a switch that never happened would.  Both bases are bytes of a random
/// image, so the first candidate almost always serves; the rest are there so
/// the flow does not depend on that.
fn distinct_from(a: u8, b: u8) -> u8 {
    [a ^ 0xFF, a ^ 0x3C, a ^ 0x11]
        .into_iter()
        .find(|&v| v != a && v != b)
        .expect("three values that differ pairwise cannot all collide with two bytes")
}

/// Arm the target slot's progress byte with a value the device would not write,
/// and verify it landed.
///
/// Neither the complete value nor its inverse, so a device still maintaining
/// the back-channel after the exit cannot leave the byte looking armed; and
/// read back with SLOT_PEEK, because the slot is not being served yet and an
/// arming write that never landed would make the final read vacuous.
fn arm_progress(bus: &mut Bus, s: &Session, slot: u8, addr: u32) -> Result<u8, String> {
    let mut armed = bus.peek_slot(s, slot, addr, 1)?[0] ^ 0xFF;
    if armed == s.complete || armed == s.pending() {
        armed ^= 0x0F;
    }
    bus.poke_slot(s, slot, addr, armed)?;

    let got = bus.peek_slot(s, slot, addr, 1)?[0];
    if got != armed {
        return Err(format!(
            "arming the progress byte of slot {slot}: 0x{addr:06X} holds 0x{got:02X}, not the \
             0x{armed:02X} it was poked with"
        ));
    }
    Ok(armed)
}
