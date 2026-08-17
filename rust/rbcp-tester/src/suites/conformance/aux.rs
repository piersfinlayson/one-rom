// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Group 0x05 — Auxiliary I/O".
//!
//! Six commands over device pins that are not part of the ROM interface: three
//! that describe what the device exposes, and three that drive a pin — one
//! plain, one that exits the session, one that exits and switches slot as it
//! goes.
//!
//! # Auxiliary I/O is optional, and absence is a legitimate answer
//!
//! "A device that exposes no auxiliary pins reports a group count of zero from
//! GET_AUX_CAPABILITY.  All other commands in this group return failure on such
//! a device."  So a zero count is conformant, and every scenario past
//! [`get_aux_capability`] skips on it.  That one scenario never skips — a
//! device with no pins still has to answer the capability command — and it is
//! also where the "all other commands fail" rule is asserted, since that is the
//! only place a zero count is a thing to test rather than a thing to skip on.
//!
//! # What a pin does is read back through the firmware's test build
//!
//! There is no silicon here, so `ora_gpio_set`'s register writes are compiled
//! out.  What it does instead is record what it drove into the test build's pad
//! model, and `ora_gpio_query` reads that back — so a scenario can assert that a
//! pin was driven low, driven high or released, and the pin-state scenarios
//! below do.
//!
//! What that does not prove is the register writes themselves.  The pad model is
//! a second implementation of the same intent rather than a check on the shipped
//! one, so "the firmware set the register" remains a hardware test.
//!
//! The scenarios ask rather than assume: [`pin_state_is_observable`] drives a pin
//! both ways and looks for the reported state to differ, and stands down with
//! that as the reason where it does not.  That keeps them honest against a build
//! whose reporting is not live, which is what this suite faced before the pad
//! model existed.
//!
//! One part of a pin's condition is visible beside the protocol: what One ROM is
//! using the GPIO for, which `ora_gpio_query` answers for real.  That is what
//! [`one_rom_withholds_pins_it_is_serving_with`] holds the drivable flag to.
//!
//! Everything else is asserted throughout: which commands succeed and which are
//! refused, that a hold does not complete before its time, that the terminal
//! variants write no response header and do leave the session, and that a slot
//! switch happens.  Those are most of the group's contract, and all of the part
//! a host depends on to stay in step with the device.
//!
//! # What this suite does not verify
//!
//! Kept here because this is where it stays accurate.
//!
//! The first list is reachable today and simply not written.  The pad model puts
//! every one of these within a scenario's grasp, so each is work rather than a
//! limitation:
//!
//! 1. SET_AUX with a non-zero hold "holds that state for the requested
//!    duration" — that the state is held rather than merely set.  Only that the
//!    command does not complete early is asserted.
//! 2. The same command "then applies `after`" — that `after` reaches the pin.
//! 3. "A value of zero holds the state until a subsequent SET_AUX changes it" —
//!    the latch.  Only that such a command completes untimed is asserted.
//! 4. SET_AUX_AND_EXIT "as SET_AUX" — the pin half.  The no-header, hold and
//!    exit halves are asserted.
//! 5. SET_AUX_SWITCH_EXIT "sets the specified pin ... in the order given by
//!    flags" — the pin half, and with it the ordering, which is only visible
//!    through the pin.
//! 6. "Under set-first ordering the device does not apply `after` until the
//!    slot switch has completed.  The effective hold is therefore the greater
//!    of the requested hold and the time the switch takes."
//! 7. "If received, neither the pin is set nor the slot switched" (slot 0xAA),
//!    and the same clause for a reserved flags bit, and for a device exposing
//!    no pins — the pin half of each.  The slot half and the exit are asserted
//!    in all three.
//!
//! The rest are out of reach for other reasons, and a pad model does not help:
//!
//! 8. "Only a device reset restores a pin to its power-on state."  There is no
//!    device reset in this harness.
//! 9. "Driving it through one group is indistinguishable from driving it
//!    through another."
//! 10. "Where a pin appears in several groups, every group reports the same
//!     properties for it."  Lining a group's pins up with another group's would
//!     mean rebuilding the device's own group-to-pin mapping in the tester.
//! 11. Group type 0x00 (None).  This device never reports it, so it is checked
//!     only as a value the protocol allows.
//! 12. A `pin_count` of zero standing for 256 pins.  This device never reports
//!     it.  The helpers here decode it, but nothing exercises the decoding
//!     against a device.
//! 13. The rule making 0xAA an invalid *group* distinguishable from an absent
//!     one.  A device would have to expose 171 groups for the two to differ,
//!     and this one exposes three.
//! 14. A device declaring *fewer* argument bytes than a command carries.  The
//!     leftover byte sits in front of the next knock and the knock detector
//!     slides past it, so no host can see it — see
//!     [`argument_counts_are_consumed_exactly`].
//! 15. `GET_AUX_PIN_INFO` reporting a non-zero `level` or `driven` with flags
//!     bit 1 clear.  [`pin_info_reports_flags`] asserts the rule for every pin
//!     of every group, so this is verified as far as the device can be driven
//!     into it — no argument reaches a state where this device reports bit 1
//!     clear at all.
//!
//! # Holds are timed against a clock the scenario owns
//!
//! A hold is the device busy-waiting on its millisecond counter.  Under
//! emulation that counter moves only when a scenario moves it — see
//! [`Bus::set_clock_us`] — so [`set_aux_holds_before_completing`] can require
//! the command to be *incomplete* at one clock value and complete at the next,
//! which is the whole of what the specification says a hold is.

use onerom_fw_emulator::ffi;

use crate::driver::{Bus, CmdFailure, HDR_SIZE, Hdr, Session, aux, control, group, read};
use crate::{Ctx, Outcome};

/// Response data section every command in the group needs, in bytes.
const REQUIRED_DATA_SIZE: u32 = 8;

/// Auxiliary pin states ("Auxiliary Pin States").
const DRIVE_LOW: u8 = 0x00;
const DRIVE_HIGH: u8 = 0x01;
const RELEASE: u8 = 0x02;

/// The lowest state value the specification leaves undefined.
const STATE_UNDEFINED: u8 = 0x03;

/// GET_AUX_PIN_INFO flags: bit 0 "the host may drive this pin with SET_AUX",
/// bit 1 "level and driven below are meaningful".
const PIN_DRIVABLE: u8 = 0x01;
const PIN_LEVEL_VALID: u8 = 0x02;

/// The GPIO group type, whose pins are the device's own GPIOs "numbered as the
/// device's documentation numbers them".
const GROUP_TYPE_GPIO: u8 = 0x01;

/// Bit 0 of SET_AUX_SWITCH_EXIT's flags: activate the slot before setting the
/// pin.  Every other bit is reserved.
const SLOT_FIRST: u8 = 0x01;

/// Where the scenarios put the device's millisecond counter before a hold.
///
/// A whole number of milliseconds, so that a hold of n units ends exactly n×10
/// milliseconds later and the assertion either side of that instant is exact.
/// Not zero: a device comparing against an absolute counter rather than against
/// the moment the command arrived would pass from zero and nowhere else.
const CLOCK_BASE_US: u64 = 4_000_000;

/// Reads of the progress byte a scenario makes before concluding that a command
/// which should still be holding is not about to complete.
///
/// Each read hands the device a turn, so this is turns given rather than time
/// passed — the device has been let round its hold loop this many times with
/// the clock standing still.
const HOLD_POLLS: u32 = 64;

/// The same, for a terminal command, which has no progress byte and is watched
/// through a command-mode poke instead.
///
/// Smaller, because a holding device drains nothing: every read taken while it
/// spins sits in the capture ring, and the poke that has to survive until the
/// hold ends is already in there.  Twenty leaves the frame well clear of the
/// ring, whose size the device chooses and whose overflow would lose the poke
/// and fail the positive half for the wrong reason.
const HELD_POLLS: u32 = 20;

/// API identifiers withheld from the plugin for the scenarios that need them.
///
/// The three entries here are the degradation paths a plugin takes on firmware
/// older than a call it uses.  Nothing else can reach them: the emulator
/// implements the whole API, so every lookup succeeds.  Consulted by
/// [`crate::suites::withheld_api`] before the plugin starts, which is when a
/// plugin resolves its pointers.
pub static WITHHELD_API: &[(&str, &[u32])] = &[
    (
        "conformance.aux.no_uptime_offers_no_timed_holds",
        &[ffi::api_id_t_ORA_ID_GET_PLUGIN_UPTIME_MS],
    ),
    (
        "conformance.aux.groups_stay_consistent_without_indexed_metadata",
        &[ffi::api_id_t_ORA_ID_GET_METADATA_UINT_AT],
    ),
    (
        "conformance.aux.no_groups_without_the_gpio_calls",
        &[
            ffi::api_id_t_ORA_ID_GPIO_SET,
            ffi::api_id_t_ORA_ID_GPIO_QUERY,
        ],
    ),
];

/// What GET_AUX_CAPABILITY reports.
#[derive(Clone, Copy)]
struct Capability {
    groups: u8,
    max_hold: u8,
}

/// A pin the device exposes, named the way the protocol names one.
#[derive(Clone, Copy)]
struct Pin {
    group: u8,
    pin: u8,
}

/// SET_AUX's five arguments: "A0=state, A1=after, A2=hold, A3=pin, A4=group".
///
/// Shared with SET_AUX_AND_EXIT, which takes them unchanged.
fn set_args(state: u8, after: u8, hold: u8, pin: &Pin) -> [u8; 5] {
    [state, after, hold, pin.pin, pin.group]
}

/// SET_AUX_SWITCH_EXIT's seven: the same, with flags between hold and pin, and
/// the slot last.
fn switch_args(state: u8, hold: u8, flags: u8, pin: &Pin, slot: u8) -> [u8; 7] {
    [state, RELEASE, hold, flags, pin.pin, pin.group, slot]
}

fn capability(bus: &mut Bus, s: &Session) -> Result<Capability, String> {
    bus.issue_cmd(s, group::AUX, aux::GET_AUX_CAPABILITY, &[])
        .map_err(|e| format!("GET_AUX_CAPABILITY: {e}"))?;
    let d = bus.read_data(s, 0, 2)?;
    Ok(Capability {
        groups: d[0],
        max_hold: d[1],
    })
}

/// GET_AUX_GROUP_INFO's type and pin count for one group.
///
/// A pin count of zero means 256 pins, so it is returned as the count it
/// stands for and no caller has to remember the encoding.
fn group_info(bus: &mut Bus, s: &Session, grp: u8) -> Result<(u8, u32), String> {
    bus.issue_cmd(s, group::AUX, aux::GET_AUX_GROUP_INFO, &[grp])
        .map_err(|e| format!("GET_AUX_GROUP_INFO for group {grp}: {e}"))?;
    let d = bus.read_data(s, 0, 2)?;
    let pins = if d[1] == 0 { 256 } else { u32::from(d[1]) };
    Ok((d[0], pins))
}

/// GET_AUX_PIN_INFO's whole eight-byte answer for one pin.
fn pin_info(bus: &mut Bus, s: &Session, grp: u8, pin: u8) -> Result<Vec<u8>, String> {
    bus.issue_cmd(s, group::AUX, aux::GET_AUX_PIN_INFO, &[pin, grp])
        .map_err(|e| format!("GET_AUX_PIN_INFO for pin {pin} of group {grp}: {e}"))?;
    bus.read_data(s, 0, 8)
}

/// Walk the device's groups and pins until one matching `want` turns up.
///
/// `want` sees each pin's flags byte.  Taking the target pin off the device
/// this way is what keeps the suite board-independent: nothing here knows a
/// GPIO number, and a board whose free pins are somewhere else needs no edit.
fn find_pin(
    bus: &mut Bus,
    s: &Session,
    groups: u8,
    want: impl Fn(u8) -> bool,
) -> Result<Option<Pin>, String> {
    for grp in 0..groups {
        let (_, pins) = group_info(bus, s, grp)?;
        for pin in 0..pins.min(256) {
            let pin = pin as u8;
            if want(pin_info(bus, s, grp, pin)?[0]) {
                return Ok(Some(Pin { group: grp, pin }));
            }
        }
    }
    Ok(None)
}

/// Enter command-response mode and find a pin the host may drive, or skip.
///
/// The shape every driving scenario starts with.  Two ways out with nothing to
/// assert: a device with no auxiliary pins at all, and one whose pins are all
/// spoken for — both conformant, and neither leaves a SET_AUX to make.
fn session_with_a_drivable_pin(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Result<(Session, Pin), Outcome>, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    if cap.groups == 0 {
        return Ok(Err(Outcome::Skip(
            "the device exposes no auxiliary pins, which the specification permits".into(),
        )));
    }

    match find_pin(bus, &s, cap.groups, |flags| flags & PIN_DRIVABLE != 0)? {
        Some(pin) => Ok(Ok((s, pin))),
        None => Ok(Err(Outcome::Skip(
            "the device exposes no pin it will let the host drive, so there is no SET_AUX \
             it can be asked to honour"
                .into(),
        ))),
    }
}

/// Whether the device's report of a pin moves when the pin is made to move.
///
/// Drives the pin low and then high and compares the level and driven bytes.  A
/// device whose reporting is live answers differently.  One whose GPIO writes
/// reach nothing — which is the case against the firmware emulator, where
/// `ora_gpio_set` only logs — answers the same both times, and a scenario about
/// what a pin *is* has nothing to work with.
fn pin_state_is_observable(bus: &mut Bus, s: &Session, pin: &Pin) -> Result<bool, String> {
    let mut seen = Vec::new();
    for state in [DRIVE_LOW, DRIVE_HIGH] {
        bus.issue_cmd(
            s,
            group::AUX,
            aux::SET_AUX,
            &set_args(state, RELEASE, 0, pin),
        )
        .map_err(|e| format!("SET_AUX with state {state}: {e}"))?;
        seen.push(pin_info(bus, s, pin.group, pin.pin)?[1..3].to_vec());
    }
    Ok(seen[0] != seen[1])
}

/// The reason a pin-state scenario gives for standing down.
fn pin_state_not_observable(what: &str) -> Outcome {
    Outcome::Skip(format!(
        "the device reports the same level and driven bytes after being told to drive the pin \
         low and then high, so what the pin is doing cannot be read back over the protocol and \
         {what} cannot be asserted"
    ))
}

/// Put the whole of the zero-pin rule to a device reporting no groups.
///
/// "All other commands in this group return failure on such a device, except
/// SET_AUX_AND_EXIT and SET_AUX_SWITCH_EXIT, which have no response header to
/// report it in.  Neither sets a pin nor switches a slot, but the exit DOES
/// complete."
///
/// The three answering commands are asserted through the response field.  The
/// two terminal ones have no failure a host can read, so what is asserted is
/// what the specification says instead: the slot does not move, which the slot
/// marks say, and the session ends, which the entry afterwards says —
/// ENTER_CMD_RESP being defined to fail while the device is still in
/// command-response mode.
///
/// That the pin was not set is not asserted here — see this module's header,
/// where it is the pin half of item 7.
fn expect_zero_pin_rule(bus: &mut Bus, ctx: &Ctx, s: &Session) -> Result<(), String> {
    bus.expect_rejected(s, group::AUX, aux::GET_AUX_GROUP_INFO, &[0])?;
    bus.expect_rejected(s, group::AUX, aux::GET_AUX_PIN_INFO, &[0, 0])?;
    bus.expect_rejected(s, group::AUX, aux::SET_AUX, &[RELEASE, RELEASE, 0, 0, 0])?;

    let slots = host_slots(bus, s)?;
    let from = active_slot(bus, s)?;
    let marks = mark_slots(bus, s, slots, ctx.probe_addr())?;
    // A slot the device would visibly move to, where it has one.  With a single
    // slot the switch half cannot discriminate and only the exit is asserted.
    let target = if slots > 1 { (from + 1) % slots } else { from };

    for (cmd, args) in [
        (aux::SET_AUX_AND_EXIT, &[RELEASE, RELEASE, 0, 0, 0][..]),
        (
            aux::SET_AUX_SWITCH_EXIT,
            &[RELEASE, RELEASE, 0, 0, 0, 0, target][..],
        ),
    ] {
        bus.send_cmd(s.command_page, group::AUX, cmd, args)?;
        fence_every_slot(bus, ctx, slots)?;

        let serving = served_slot(bus, &marks, ctx.probe_addr())?;
        if serving != from {
            return Err(format!(
                "0x05/0x{cmd:02X} moved the device from slot {from} to slot {serving} on a \
                 device reporting no auxiliary pin groups — neither the pin is set nor the \
                 slot switched"
            ));
        }

        bus.enter_cmd_resp(s).map_err(|e| {
            format!(
                "ENTER_CMD_RESP after 0x05/0x{cmd:02X} on a device reporting no auxiliary pin \
                 groups: {e} — the exit DOES complete"
            )
        })?;
    }

    Ok(())
}

/// Read the progress byte `times`, and report whether it ever read complete.
///
/// Each read gives the device a turn, so a device holding is let round its loop
/// `times` over — this is the device being given every chance to finish, with
/// the clock standing still.
fn completed_within(bus: &mut Bus, s: &Session, times: u32) -> Result<bool, String> {
    for _ in 0..times {
        if bus.read_hdr(s, Hdr::Progress)? == s.complete {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Send a frame and require the device to have taken it, without waiting for it
/// to finish.
///
/// The token increments as step 2 of the processing sequence, before the
/// command is processed, so it separates "the device never saw this" from "the
/// device is still working on it" — which is exactly the distinction a hold
/// scenario turns on.
fn send_and_expect_received(
    bus: &mut Bus,
    s: &Session,
    grp: u8,
    cmd: u8,
    args: &[u8],
) -> Result<(), String> {
    let before = bus.read_hdr(s, Hdr::TokenLsb)?;
    bus.send_cmd(s.command_page, grp, cmd, args)?;
    for _ in 0..HOLD_POLLS {
        if bus.read_hdr(s, Hdr::TokenLsb)? != before {
            return Ok(());
        }
    }
    Err(format!(
        "the token never moved from 0x{before:02X} after 0x{grp:02X}/0x{cmd:02X} — the device \
         did not receive the command"
    ))
}

/// The number of RAM slots the device says a host may name.
fn host_slots(bus: &mut Bus, s: &Session) -> Result<u8, String> {
    bus.issue_cmd(s, group::READ, read::GET_RAM_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_RAM_SLOT_INFO_ALL: {e}"))?;
    Ok(bus.read_data(s, 0, 1)?[0])
}

/// Write a distinct byte at `addr` in every slot, and read each back.
///
/// Whatever the bus serves at that address afterwards then says which slot is
/// active, which is the only way to see a switch a terminal command performed
/// on its way out.  Every mark is derived from what the image already holds, so
/// none can be satisfied by what was there before, and SLOT_PEEK proves each
/// landed — an inactive slot is not on the bus, and a mark that never arrived
/// would leave the verdict below with nothing to choose between.
fn mark_slots(bus: &mut Bus, s: &Session, slots: u8, addr: u32) -> Result<Vec<u8>, String> {
    let original = bus.read(addr)?;
    let marks: Vec<u8> = (0..slots).map(|slot| original ^ (0x80 | slot)).collect();

    for (slot, &mark) in marks.iter().enumerate() {
        let slot = slot as u8;
        bus.poke_slot(s, slot, addr, mark)
            .map_err(|e| format!("marking 0x{addr:06X} of slot {slot}: {e}"))?;
        bus.issue_cmd(
            s,
            group::READ,
            read::SLOT_PEEK,
            &crate::driver::slot_peek_args(addr, 1, slot),
        )
        .map_err(|e| format!("SLOT_PEEK of slot {slot}: {e}"))?;
        bus.expect_data(s, 0, &[mark], &format!("SLOT_PEEK of marked slot {slot}"))?;
    }
    Ok(marks)
}

/// Poke the fence byte of every slot: proof the device has processed what came
/// before, and deliberately silent about which slot it is serving.
///
/// A fence aimed at one slot would fail rather than discriminate on exactly the
/// device a switch scenario exists to catch, and a failing fence names the
/// wrong fault.
fn fence_every_slot(bus: &mut Bus, ctx: &Ctx, slots: u8) -> Result<(), String> {
    let addr = ctx.fence_addr();
    let value = bus.read(addr)? ^ 0xFF;
    for slot in 0..slots {
        bus.knock(ctx.command_page())?;
        bus.send_poke_slot(ctx.command_page(), slot, addr, value)?;
    }
    bus.await_byte(addr, value)
        .map_err(|e| format!("the device took no command-mode session afterwards: {e}"))
}

/// Which slot's mark the bus is serving, from marks [`mark_slots`] left.
fn served_slot(bus: &mut Bus, marks: &[u8], addr: u32) -> Result<u8, String> {
    let got = bus.read(addr)?;
    marks
        .iter()
        .position(|&m| m == got)
        .map(|i| i as u8)
        .ok_or_else(|| {
            format!(
                "0x{addr:06X} serves 0x{got:02X}, which marks no slot — the device is serving \
                 something else again"
            )
        })
}

/// GET_AUX_CAPABILITY answers, and its reserved bytes are zero.
///
/// "Fails if the response data section is smaller than 8 bytes", and these
/// sessions are larger than that, so the command must succeed on every device
/// — including one with no auxiliary pins, whose answer is a count of zero.
/// Where the count *is* zero this is also where "all other commands in this
/// group return failure on such a device" is asserted, that being the only
/// place a zero count is a thing to test rather than a thing to skip on.
pub fn get_aux_capability(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    bus.expect_data(&s, 2, &[0x00; 6], "GET_AUX_CAPABILITY reserved bytes 2-7")?;

    if cap.groups == 0 {
        expect_zero_pin_rule(bus, ctx, &s)?;
    }

    Ok(Outcome::Pass)
}

/// Every group the device advertises describes itself.
///
/// Groups are "contiguous and start from zero", so a count of n means groups 0
/// to n-1 all answer.  Each reports a type the protocol defines or leaves to
/// the implementation — 0x02 to 0x7F are reserved and 0xFF is Invalid, so
/// neither may appear — and its reserved bytes are zero.
pub fn group_info_describes_every_group(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    if cap.groups == 0 {
        return Ok(Outcome::Skip(
            "the device exposes no auxiliary pins, which the specification permits".into(),
        ));
    }

    for grp in 0..cap.groups {
        let (kind, _) = group_info(bus, &s, grp)?;
        if (0x02..=0x7F).contains(&kind) || kind == 0xFF {
            return Err(format!(
                "group {grp} reports type 0x{kind:02X}; RBCP 0.1.2 defines 0x00 (None) and \
                 0x01 (GPIO), reserves 0x02-0x7F, leaves 0x80-0xFE to the implementation and \
                 calls 0xFF Invalid"
            ));
        }
        bus.expect_data(
            &s,
            2,
            &[0x00; 6],
            &format!("GET_AUX_GROUP_INFO reserved bytes 2-7 for group {grp}"),
        )?;
    }

    Ok(Outcome::Pass)
}

/// GET_AUX_PIN_INFO's flags, level and driven obey their own rules.
///
/// Bits 2 to 7 of the flags "are reserved, and must be set to zero by the
/// device".  Level is "0 or 1", and both level and driven "must be set to zero
/// by the device where bit 1 of flags is clear", which is what stops a host
/// reading a level the device never had.  Asserted for every pin the device
/// advertises, since the rule is about the answer rather than about any one
/// pin.
pub fn pin_info_reports_flags(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    if cap.groups == 0 {
        return Ok(Outcome::Skip(
            "the device exposes no auxiliary pins, which the specification permits".into(),
        ));
    }

    for grp in 0..cap.groups {
        let (_, pins) = group_info(bus, &s, grp)?;
        for pin in 0..pins.min(256) {
            let pin = pin as u8;
            let info = pin_info(bus, &s, grp, pin)?;
            let (flags, level, driven) = (info[0], info[1], info[2]);
            let at = format!("pin {pin} of group {grp}");

            if flags & 0xFC != 0 {
                return Err(format!(
                    "{at} reports flags 0x{flags:02X}, with a bit set above bit 1 — bits 2-7 \
                     are reserved and must be set to zero by the device"
                ));
            }
            if level > 1 {
                return Err(format!(
                    "{at} reports level 0x{level:02X} — the level present on a pin is 0 or 1"
                ));
            }
            if flags & PIN_LEVEL_VALID == 0 && (level != 0 || driven != 0) {
                return Err(format!(
                    "{at} reports level 0x{level:02X} and driven 0x{driven:02X} with bit 1 of \
                     flags clear — both must be set to zero where the device is saying they \
                     are not meaningful"
                ));
            }
            bus.expect_data(
                &s,
                3,
                &[0x00; 5],
                &format!("GET_AUX_PIN_INFO reserved bytes 3-7 for {at}"),
            )?;
        }
    }

    Ok(Outcome::Pass)
}

/// A group or a pin the device does not expose is refused, by all three
/// commands that name one.
///
/// Group and pin numbering is dense and 0-based, so the counts are themselves
/// the first values that are not one of them.  0xAA is refused as a group
/// number too — it is the final argument of GET_AUX_GROUP_INFO and the last of
/// the pair GET_AUX_PIN_INFO and SET_AUX name a pin by, which is the reason
/// the protocol addresses a pin group-last.
pub fn queries_reject_an_absent_group_or_pin(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    if cap.groups == 0 {
        return Ok(Outcome::Skip(
            "the device exposes no auxiliary pins, which the specification permits".into(),
        ));
    }

    let absent = cap.groups;
    bus.expect_rejected(&s, group::AUX, aux::GET_AUX_GROUP_INFO, &[absent])?;
    bus.expect_rejected(&s, group::AUX, aux::GET_AUX_PIN_INFO, &[0, absent])?;
    bus.expect_rejected(
        &s,
        group::AUX,
        aux::SET_AUX,
        &[RELEASE, RELEASE, 0, 0, absent],
    )?;

    for grp in [0xAAu8, absent] {
        bus.expect_rejected(&s, group::AUX, aux::GET_AUX_GROUP_INFO, &[grp])?;
        bus.expect_rejected(&s, group::AUX, aux::GET_AUX_PIN_INFO, &[0, grp])?;
    }

    // The pin past the end of each group, where the group has an end: a pin
    // count of zero means 256 pins, so there every pin number is one the device
    // exposes and there is nothing to refuse.
    for grp in 0..cap.groups {
        let (_, pins) = group_info(bus, &s, grp)?;
        if pins == 256 {
            continue;
        }
        let pin = pins as u8;
        bus.expect_rejected(&s, group::AUX, aux::GET_AUX_PIN_INFO, &[pin, grp])?;
        bus.expect_rejected(
            &s,
            group::AUX,
            aux::SET_AUX,
            &[RELEASE, RELEASE, 0, pin, grp],
        )?;
    }

    Ok(Outcome::Pass)
}

/// All three query commands fail where the data section cannot hold the answer.
///
/// Each "fails if the response data section is smaller than 8 bytes", and every
/// answer in the group is exactly that long, so there is no partial answer to
/// give.  The device must refuse rather than write past the region the host
/// gave it.
///
/// Asserted on a device with no auxiliary pins too: GET_AUX_CAPABILITY has an
/// answer either way, and a count of zero still needs eight bytes to report.
pub fn query_commands_need_room_for_their_answer(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let short = (REQUIRED_DATA_SIZE - 4) as u16;
    let s = bus.enter_sized(ctx, HDR_SIZE as u16 + short)?;

    for (cmd, args) in [
        (aux::GET_AUX_CAPABILITY, &[][..]),
        (aux::GET_AUX_GROUP_INFO, &[0][..]),
        (aux::GET_AUX_PIN_INFO, &[0, 0][..]),
    ] {
        bus.expect_rejected(&s, group::AUX, cmd, args)
            .map_err(|e| format!("{e} — the data section is {short} bytes and the answer is 8"))?;
    }

    Ok(Outcome::Pass)
}

/// All six commands are refused in command mode, and cost the host nothing.
///
/// "All commands in this group are valid in command-response mode only", and a
/// command refused for being in the wrong mode "is nonetheless framed like any
/// other: the device consumes its argument bytes before discarding it".
///
/// Three things are asserted at once, because in command mode none is
/// observable on its own.  The frame must be consumed, which the SLOT_POKE at
/// the end says: it is properly knocked, so it lands only if the device took
/// all twenty argument bytes of the six commands off the wire first, and a
/// device one byte out anywhere reads that poke's knock wrong and it never
/// arrives.  The queries must not answer, which the armed data section says.
/// And SET_AUX_SWITCH_EXIT must not switch, which the slot marks say.
pub fn not_valid_in_command_mode(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let slots = host_slots(bus, &s)?;
    let marks = mark_slots(bus, &s, slots, ctx.probe_addr())?;
    let target = (ctx.active_ram_slot + 1) % slots.max(1);

    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    let dst = s.bch_start + HDR_SIZE;
    let armed = bus.read(dst)? ^ 0xFF;
    bus.poke_verified(ctx, dst, armed)
        .map_err(|e| format!("arming the response data section: {e}"))?;

    let set = [RELEASE, RELEASE, 0, 0, 0];
    for (cmd, args) in [
        (aux::GET_AUX_CAPABILITY, &[][..]),
        (aux::GET_AUX_GROUP_INFO, &[0][..]),
        (aux::GET_AUX_PIN_INFO, &[0, 0][..]),
        (aux::SET_AUX, &set[..]),
        (aux::SET_AUX_AND_EXIT, &set[..]),
        (
            aux::SET_AUX_SWITCH_EXIT,
            &[RELEASE, RELEASE, 0, 0, 0, 0, target][..],
        ),
    ] {
        bus.knock(ctx.command_page())?;
        bus.send_cmd(ctx.command_page(), group::AUX, cmd, args)?;
    }

    // One knock, so a device left owing an argument byte — or one byte ahead —
    // reads this knock wrong and the poke never lands.  The fence below sends a
    // knock per slot and would recover from that, which is why the framing
    // verdict is taken here and separately.
    let scratch = ctx.scratch_addr();
    let framed = bus.read(scratch)? ^ 0xFF;
    bus.poke_verified(ctx, scratch, framed).map_err(|e| {
        format!(
            "{e} — the device did not consume the twenty argument bytes of the six Auxiliary \
             I/O commands sent in command mode"
        )
    })?;

    fence_every_slot(bus, ctx, slots)?;

    let got = bus.read(dst)?;
    if got != armed {
        return Err(format!(
            "the device answered an Auxiliary I/O query in command mode: 0x{dst:06X} serves \
             0x{got:02X} rather than the armed 0x{armed:02X} — the group is valid in \
             command-response mode only, and after EXIT_CMD_RESP_ACK the back-channel is no \
             longer maintained"
        ));
    }

    let serving = served_slot(bus, &marks, ctx.probe_addr())?;
    if serving != ctx.active_ram_slot {
        return Err(format!(
            "the device is serving slot {serving} rather than slot {} — a \
             SET_AUX_SWITCH_EXIT issued in command mode switched slot",
            ctx.active_ram_slot
        ));
    }

    Ok(Outcome::Pass)
}

/// Each command takes exactly the argument bytes it declares.
///
/// "The count is fixed per GROUP+CMD pair", and both ends work from that count
/// alone — there is no length field to fall back on, so a device one byte out
/// reads the host's next frame as the tail of this one and the session
/// desynchronises with nothing to say so.
///
/// Each command is sent on its own in command mode, followed by a properly
/// knocked SLOT_POKE that lands only if the device stopped taking bytes where
/// it should have.  A device that takes one byte too many swallows the first
/// character of that knock and the poke never arrives, so the failure names the
/// command that caused it rather than the sequence it was in.
///
/// Only over-consumption is caught, and only over-consumption is a defect a
/// host can suffer.  A device that declares *fewer* bytes than the command
/// carries leaves the host's last argument byte sitting in front of the next
/// knock, and the knock detector slides past it — so nothing downstream is
/// disturbed and there is nothing for a scenario to see.
pub fn argument_counts_are_consumed_exactly(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let set = [RELEASE, RELEASE, 0, 0, 0];
    let switch = [RELEASE, RELEASE, 0, 0, 0, 0, ctx.active_ram_slot];

    for (cmd, args, count) in [
        (aux::GET_AUX_CAPABILITY, &[][..], 0),
        (aux::GET_AUX_GROUP_INFO, &[0][..], 1),
        (aux::GET_AUX_PIN_INFO, &[0, 0][..], 2),
        (aux::SET_AUX, &set[..], 5),
        (aux::SET_AUX_AND_EXIT, &set[..], 5),
        (aux::SET_AUX_SWITCH_EXIT, &switch[..], 7),
    ] {
        let addr = ctx.scratch_addr();
        let value = bus.read(addr)? ^ 0xFF;

        bus.knock(ctx.command_page())?;
        bus.send_cmd(ctx.command_page(), group::AUX, cmd, args)?;

        bus.poke_verified(ctx, addr, value).map_err(|e| {
            format!(
                "{e} — the SLOT_POKE after 0x05/0x{cmd:02X} never landed, so the device did \
                 not take exactly the {count} argument bytes that command declares"
            )
        })?;
    }

    Ok(Outcome::Pass)
}

/// SET_AUX places a pin in each of the three defined states.
///
/// "Places the specified pin in the specified state", the states being drive
/// low, drive high and release to high impedance.  All three must be accepted
/// on a pin the device says the host may drive, and the device must go on
/// accepting them — a pin does not become undrivable by having been driven.
///
/// What the pin then does is asserted where the device will report it — see
/// [`pin_state_is_observable`] for why that is not everywhere.
pub fn set_aux_drives_and_releases_a_pin(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    let observable = pin_state_is_observable(bus, &s, &pin)?;

    for state in [DRIVE_LOW, DRIVE_HIGH, RELEASE] {
        bus.issue_cmd(
            &s,
            group::AUX,
            aux::SET_AUX,
            &set_args(state, RELEASE, 0, &pin),
        )
        .map_err(|e| {
            format!(
                "SET_AUX with state 0x{state:02X} on pin {} of group {}: {e}",
                pin.pin, pin.group
            )
        })?;

        let info = pin_info(bus, &s, pin.group, pin.pin)?;
        if observable && state != RELEASE {
            let want = if state == DRIVE_HIGH { 1 } else { 0 };
            if info[2] != 1 || info[1] != want {
                return Err(format!(
                    "after SET_AUX told the device to drive pin {} of group {} \
                     {}, GET_AUX_PIN_INFO reports level {} and driven {}",
                    pin.pin,
                    pin.group,
                    if want == 1 { "high" } else { "low" },
                    info[1],
                    info[2]
                ));
            }
        }
        if info[0] & PIN_DRIVABLE == 0 {
            return Err(format!(
                "pin {} of group {} stopped being drivable once it had been driven",
                pin.pin, pin.group
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// What GET_AUX_PIN_INFO says about a pin is what SET_AUX does with it.
///
/// Bit 0 of the flags is "the host may drive this pin with SET_AUX", and
/// SET_AUX "fails if the pin is not drivable" — one rule stated from both ends,
/// so a device whose reporting path and driving path disagree is one a host
/// cannot use.  Every pin the device advertises is put to both.
///
/// The state used is release to high impedance, so a pin the device does let
/// the host drive is left as the specification's own least invasive option
/// rather than pulled to a rail this scenario knows nothing about.
pub fn pin_info_and_set_aux_agree_on_drivability(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    if cap.groups == 0 {
        return Ok(Outcome::Skip(
            "the device exposes no auxiliary pins, which the specification permits".into(),
        ));
    }

    let mut seen_both = (false, false);
    for grp in 0..cap.groups {
        let (_, pins) = group_info(bus, &s, grp)?;
        for pin in 0..pins.min(256) {
            let pin = pin as u8;
            let drivable = pin_info(bus, &s, grp, pin)?[0] & PIN_DRIVABLE != 0;
            let args = [RELEASE, RELEASE, 0, pin, grp];
            let accepted = bus.issue_cmd(&s, group::AUX, aux::SET_AUX, &args);

            match (drivable, accepted) {
                (true, Ok(())) => seen_both.0 = true,
                (false, Err(CmdFailure::Failed)) => seen_both.1 = true,
                (true, Err(e)) => {
                    return Err(format!(
                        "GET_AUX_PIN_INFO reports pin {pin} of group {grp} as drivable, but \
                         SET_AUX on it: {e}"
                    ));
                }
                (false, Ok(())) => {
                    return Err(format!(
                        "SET_AUX drove pin {pin} of group {grp}, which GET_AUX_PIN_INFO \
                         reports the host may not drive"
                    ));
                }
                (false, Err(e)) => return Err(format!("SET_AUX on pin {pin} of group {grp}: {e}")),
            }
        }
    }

    if !seen_both.0 && !seen_both.1 {
        return Ok(Outcome::Skip(
            "the device advertised no pins to put the rule to".into(),
        ));
    }

    Ok(Outcome::Pass)
}

/// A non-zero hold keeps the command incomplete until the hold has elapsed.
///
/// "Where hold is non-zero the device holds that state for the requested
/// duration and then applies after.  The device times the hold, and does not
/// complete the command until it has elapsed."  Hold is in units of 10ms, so
/// the assertion is made on both sides of one millisecond: at the last
/// millisecond of the hold the command must still be pending, and at the first
/// after it must complete and report success.
///
/// The device is given [`HOLD_POLLS`] turns at each of those clock values, so
/// "still pending" is the device having had every chance to finish rather than
/// the scenario having read too soon.
pub fn set_aux_holds_before_completing(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    let cap = capability(bus, &s)?;
    if cap.max_hold == 0 {
        return Ok(Outcome::Skip(
            "the device reports a maximum hold of zero, which the specification defines as \
             supporting no timed holds"
                .into(),
        ));
    }

    // The shortest hold there is, so the scenario turns on the boundary rather
    // than on how long it is prepared to wait.
    let hold = 1u8;
    let hold_ms = u64::from(hold) * 10;

    bus.set_clock_us(CLOCK_BASE_US);
    send_and_expect_received(
        bus,
        &s,
        group::AUX,
        aux::SET_AUX,
        &set_args(DRIVE_LOW, RELEASE, hold, &pin),
    )?;

    if completed_within(bus, &s, HOLD_POLLS)? {
        return Err(format!(
            "SET_AUX with a hold of {hold} completed with the device's clock standing still — \
             the device must not complete the command until the {hold_ms}ms hold has elapsed"
        ));
    }

    bus.advance_clock_us((hold_ms - 1) * 1000);
    if completed_within(bus, &s, HOLD_POLLS)? {
        return Err(format!(
            "SET_AUX with a hold of {hold} completed {}ms in — the hold is {hold_ms}ms",
            hold_ms - 1
        ));
    }

    bus.advance_clock_us(1000);
    if !completed_within(bus, &s, HOLD_POLLS)? {
        return Err(format!(
            "SET_AUX with a hold of {hold} had not completed {hold_ms}ms in, which is the whole \
             of the hold it asked for"
        ));
    }
    bus.expect_hdr(&s, Hdr::Response, s.status_ok)?;

    Ok(Outcome::Pass)
}

/// A hold of zero completes at once, and leaves the pin for the next SET_AUX.
///
/// "A value of zero holds the state until a subsequent SET_AUX changes it", so
/// there is nothing for the device to time and no reason for the command to
/// wait.  Asserted with the device's clock standing still, which is what makes
/// it an assertion: the command completes with no time having passed at all,
/// where [`set_aux_holds_before_completing`] shows the same command with a hold
/// refusing to.  A second SET_AUX afterwards is the "until" half — a latch the
/// device would not take out of is one a host could never use twice.
pub fn set_aux_zero_hold_completes_at_once(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    bus.set_clock_us(CLOCK_BASE_US);

    for state in [DRIVE_LOW, DRIVE_HIGH] {
        send_and_expect_received(
            bus,
            &s,
            group::AUX,
            aux::SET_AUX,
            &set_args(state, RELEASE, 0, &pin),
        )?;
        if !completed_within(bus, &s, HOLD_POLLS)? {
            return Err(format!(
                "SET_AUX with a hold of zero and state 0x{state:02X} had not completed with the \
                 device's clock standing still — a hold of zero is not timed"
            ));
        }
        bus.expect_hdr(&s, Hdr::Response, s.status_ok)?;
    }

    Ok(Outcome::Pass)
}

/// SET_AUX refuses a state the protocol does not define.
///
/// "Fails if ... state is not a defined value."  0x00 to 0x02 are the whole of
/// the state table, so 0x03 is the first undefined value and 0xFF the last.
/// 0xAA is undefined too and needs no rule of its own, state not being a final
/// argument.
pub fn set_aux_rejects_an_undefined_state(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    for state in [STATE_UNDEFINED, 0xAA, 0xFF] {
        bus.expect_rejected(
            &s,
            group::AUX,
            aux::SET_AUX,
            &set_args(state, RELEASE, 0, &pin),
        )
        .map_err(|e| format!("{e} — state 0x{state:02X} is not a defined value"))?;
    }

    Ok(Outcome::Pass)
}

/// `after` is checked only where there is a hold to apply it after.
///
/// "Fails if ... hold is non-zero and after is not a defined value."  The
/// condition is the whole point: with no hold the device never applies `after`,
/// so an undefined value there is a byte it has no business looking at, and a
/// device that checked it anyway would refuse commands the specification
/// requires it to honour.  Both halves are asserted, since either alone would
/// pass on a device that ignored the argument entirely.
pub fn set_aux_validates_after_only_with_a_hold(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    bus.issue_cmd(
        &s,
        group::AUX,
        aux::SET_AUX,
        &set_args(RELEASE, STATE_UNDEFINED, 0, &pin),
    )
    .map_err(|e| {
        format!(
            "SET_AUX with an undefined after and a hold of zero: {e} — after is checked only \
             where hold is non-zero"
        )
    })?;

    let cap = capability(bus, &s)?;
    if cap.max_hold == 0 {
        return Ok(Outcome::Skip(
            "the device reports a maximum hold of zero, so there is no valid non-zero hold to \
             pair an undefined after with"
                .into(),
        ));
    }

    bus.expect_rejected(
        &s,
        group::AUX,
        aux::SET_AUX,
        &set_args(RELEASE, STATE_UNDEFINED, 1, &pin),
    )
    .map_err(|e| format!("{e} — after 0x{STATE_UNDEFINED:02X} is not a defined value"))?;

    Ok(Outcome::Pass)
}

/// A hold equal to the device's maximum is accepted, and timed.
///
/// "Fails if ... hold exceeds the maximum reported by GET_AUX_CAPABILITY."  The
/// boundary has two sides and they are separate scenarios, because the negative
/// one cannot be reached on a device reporting 0xFF while this one always can —
/// and it is the half that catches a device rejecting one value too many, which
/// is the same off-by-one and just as useless to a host.
///
/// The hold is timed as well as accepted, so a device that took the maximum by
/// treating it as no hold at all does not pass.
pub fn set_aux_accepts_a_hold_of_the_reported_maximum(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    let cap = capability(bus, &s)?;
    if cap.max_hold == 0 {
        return Ok(Outcome::Skip(
            "the device reports a maximum hold of zero, which is no hold to accept — the \
             boundary there is a hold of one, and no_uptime_offers_no_timed_holds asserts it"
                .into(),
        ));
    }

    let hold_ms = u64::from(cap.max_hold) * 10;
    bus.set_clock_us(CLOCK_BASE_US);
    send_and_expect_received(
        bus,
        &s,
        group::AUX,
        aux::SET_AUX,
        &set_args(RELEASE, RELEASE, cap.max_hold, &pin),
    )
    .map_err(|e| format!("{e} — a hold equal to the reported maximum does not exceed it"))?;

    bus.advance_clock_us((hold_ms - 1) * 1000);
    if completed_within(bus, &s, HOLD_POLLS)? {
        return Err(format!(
            "SET_AUX with a hold of {}, the maximum the device reports, completed {}ms in — \
             the hold is {hold_ms}ms",
            cap.max_hold,
            hold_ms - 1
        ));
    }

    bus.advance_clock_us(1000);
    if !completed_within(bus, &s, HOLD_POLLS)? {
        return Err(format!(
            "SET_AUX with a hold of {}, the maximum the device reports, had not completed once \
             that hold had elapsed",
            cap.max_hold
        ));
    }
    bus.expect_hdr(&s, Hdr::Response, s.status_ok)
        .map_err(|e| format!("{e} — a hold equal to the reported maximum does not exceed it"))?;

    Ok(Outcome::Pass)
}

/// A hold larger than the device's maximum is refused.
///
/// The other side of "fails if ... hold exceeds the maximum reported by
/// GET_AUX_CAPABILITY".  Skipped where the device reports 0xFF, since hold is a
/// single byte and no value can then exceed it — the acceptance half is in
/// [`set_aux_accepts_a_hold_of_the_reported_maximum`], which such a device does
/// run.
pub fn set_aux_rejects_a_hold_above_the_maximum(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    let cap = capability(bus, &s)?;
    if cap.max_hold == 0xFF {
        return Ok(Outcome::Skip(
            "the device accepts a hold of 0xFF, and hold is a single byte, so no value exceeds \
             the maximum"
                .into(),
        ));
    }

    bus.set_clock_us(CLOCK_BASE_US);

    bus.expect_rejected(
        &s,
        group::AUX,
        aux::SET_AUX,
        &set_args(RELEASE, RELEASE, cap.max_hold + 1, &pin),
    )
    .map_err(|e| {
        format!(
            "{e} — the device reports a maximum hold of {}, and {} exceeds it",
            cap.max_hold,
            cap.max_hold + 1
        )
    })?;

    Ok(Outcome::Pass)
}

/// A pin keeps its state across the end of a command-response session.
///
/// "A pin's state persists across the end of a command-response session and
/// across RBCP_RESET.  Only a device reset restores a pin to its power-on
/// state."  The pin is driven, the session is ended with EXIT_CMD_RESP_ACK, a
/// new one is opened, and the device must report the pin as it was left.
pub fn pin_state_survives_leaving_command_response_mode(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    if !pin_state_is_observable(bus, &s, &pin)? {
        return Ok(pin_state_not_observable("persistence across a session"));
    }

    bus.issue_cmd(
        &s,
        group::AUX,
        aux::SET_AUX,
        &set_args(DRIVE_HIGH, RELEASE, 0, &pin),
    )
    .map_err(|e| format!("SET_AUX: {e}"))?;
    let before = pin_info(bus, &s, pin.group, pin.pin)?;

    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP after the exit: {e}"))?;

    let after = pin_info(bus, &s, pin.group, pin.pin)?;
    if after[1..3] != before[1..3] {
        return Err(format!(
            "pin {} of group {} read level {} driven {} before the session ended and level {} \
             driven {} after — a pin's state persists across the end of a session",
            pin.pin, pin.group, before[1], before[2], after[1], after[2]
        ));
    }

    Ok(Outcome::Pass)
}

/// A pin keeps its state across RBCP_RESET.
///
/// The other half of "a pin's state persists across the end of a
/// command-response session and across RBCP_RESET".  RBCP_RESET puts the
/// device's protocol implementation back to a known state, and the
/// specification is explicit that a pin is not part of that state — "only a
/// device reset restores a pin to its power-on state".
pub fn pin_state_survives_rbcp_reset(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    if !pin_state_is_observable(bus, &s, &pin)? {
        return Ok(pin_state_not_observable("persistence across RBCP_RESET"));
    }

    bus.issue_cmd(
        &s,
        group::AUX,
        aux::SET_AUX,
        &set_args(DRIVE_HIGH, RELEASE, 0, &pin),
    )
    .map_err(|e| format!("SET_AUX: {e}"))?;
    let before = pin_info(bus, &s, pin.group, pin.pin)?;

    bus.reset(ctx.command_page())?;
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP after RBCP_RESET: {e}"))?;

    let after = pin_info(bus, &s, pin.group, pin.pin)?;
    if after[1..3] != before[1..3] {
        return Err(format!(
            "pin {} of group {} read level {} driven {} before RBCP_RESET and level {} driven \
             {} after — only a device reset restores a pin to its power-on state",
            pin.pin, pin.group, before[1], before[2], after[1], after[2]
        ));
    }

    Ok(Outcome::Pass)
}

/// SET_AUX_AND_EXIT writes no response header, and does leave the session.
///
/// "As SET_AUX, but exits command-response mode without updating the response
/// header."  A NOP first, so the last-command and token bytes hold values this
/// scenario chose rather than whatever the image contained.  Then the command,
/// sent and not polled, since the specification gives the host nothing to poll
/// for.  Then a command-mode fence, so the device has demonstrably processed it
/// before the header is read.  The entry afterwards is what proves the exit —
/// ENTER_CMD_RESP is defined to fail while the device is already in
/// command-response mode, so one that succeeds says the device had left.
pub fn set_aux_and_exit_writes_no_header_and_exits(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
        .map_err(|e| format!("NOP: {e}"))?;
    let token = bus.read_hdr(&s, Hdr::TokenLsb)?;

    bus.send_cmd(
        s.command_page,
        group::AUX,
        aux::SET_AUX_AND_EXIT,
        &set_args(DRIVE_HIGH, RELEASE, 0, &pin),
    )?;

    let fence = ctx.probe_addr();
    let value = bus.read(fence)? ^ 0xFF;
    bus.poke_verified(ctx, fence, value)
        .map_err(|e| format!("the device took no command-mode session after the exit: {e}"))?;

    bus.expect_hdr(&s, Hdr::LastCmdGroup, group::CONTROL)?;
    bus.expect_hdr(&s, Hdr::LastCmdCmd, control::NOP)
        .map_err(|e| format!("{e} — SET_AUX_AND_EXIT updated the response header"))?;
    bus.expect_hdr(&s, Hdr::TokenLsb, token)
        .map_err(|e| format!("{e} — SET_AUX_AND_EXIT updated the response header"))?;

    bus.enter_cmd_resp(&s).map_err(|e| {
        format!(
            "ENTER_CMD_RESP after SET_AUX_AND_EXIT: {e} — the command is terminal to the session"
        )
    })?;

    // Terminal is stated of the command, not of the command succeeding, and the
    // zero-pin rule says the same thing in the case it spells out: "the exit
    // DOES complete".  An undefined state is a SET_AUX the device must refuse,
    // so this is the same command failing, and it must still leave the session.
    bus.send_cmd(
        s.command_page,
        group::AUX,
        aux::SET_AUX_AND_EXIT,
        &set_args(STATE_UNDEFINED, RELEASE, 0, &pin),
    )?;
    bus.enter_cmd_resp(&s).map_err(|e| {
        format!(
            "ENTER_CMD_RESP after a SET_AUX_AND_EXIT the device had to refuse: {e} — the \
             command is terminal to the session whether or not it sets the pin"
        )
    })?;

    Ok(Outcome::Pass)
}

/// Both terminal commands hold before they exit.
///
/// SET_AUX_AND_EXIT is "as SET_AUX", and SET_AUX takes a hold the device "does
/// not complete the command until it has elapsed".  Neither terminal command
/// writes a response header, so there is no progress byte to watch — what a
/// host sees instead is that the device is not yet taking sessions, since it
/// has not exited.
///
/// So the discrimination is a command-mode poke: sent while the device's clock
/// stands still it must not land, and once the clock has moved past the hold a
/// fresh one must.  The first poke stays in the capture ring meanwhile, which
/// is why the second uses a different value — the device drains both once the
/// hold ends, and the later write is the one that has to be there.
///
/// SET_AUX_SWITCH_EXIT names the slot already active, so the marks the other
/// switch scenarios rely on are not disturbed and the only thing under test is
/// the timing.
pub fn terminal_commands_hold_before_exiting(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    let cap = capability(bus, &s)?;
    if cap.max_hold == 0 {
        return Ok(Outcome::Skip(
            "the device reports a maximum hold of zero, which the specification defines as \
             supporting no timed holds"
                .into(),
        ));
    }

    let hold = 1u8;
    let hold_ms = u64::from(hold) * 10;
    let addr = ctx.probe_addr();

    // The session is already open from session_with_a_drivable_pin, and each
    // command below closes it, so re-entry belongs between the two rather than
    // in front of both.
    let switch = switch_args(DRIVE_LOW, hold, 0x00, &pin, ctx.active_ram_slot);
    for (i, (cmd, args)) in [
        (
            aux::SET_AUX_AND_EXIT,
            &set_args(DRIVE_LOW, RELEASE, hold, &pin)[..],
        ),
        (aux::SET_AUX_SWITCH_EXIT, &switch[..]),
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 {
            bus.enter_cmd_resp(&s)
                .map_err(|e| format!("ENTER_CMD_RESP before 0x05/0x{cmd:02X}: {e}"))?;
        }

        let held = bus.read(addr)? ^ 0xFF;
        bus.set_clock_us(CLOCK_BASE_US);
        bus.send_cmd(s.command_page, group::AUX, cmd, args)?;

        // Queued behind the hold.  The device is spinning, so it drains none of
        // this until its clock moves.
        bus.knock(ctx.command_page())?;
        bus.send_poke(ctx, addr, held)?;
        for _ in 0..HELD_POLLS {
            if bus.read(addr)? == held {
                return Err(format!(
                    "0x05/0x{cmd:02X} with a hold of {hold} took a command-mode session with \
                     the device's clock standing still — it must not exit until the {hold_ms}ms \
                     hold has elapsed"
                ));
            }
        }

        // Let the hold end before the knock below starts.  Leaving
        // command-response mode resets the device's ring read index, so a knock
        // half in the ring when the exit happens loses its first bytes and the
        // poke that follows is read as something else.
        bus.advance_clock_us(hold_ms * 1000);
        for _ in 0..HELD_POLLS {
            bus.read(addr)?;
        }

        let after = held ^ 0x5A;
        bus.poke_verified(ctx, addr, after).map_err(|e| {
            format!("{e} — 0x05/0x{cmd:02X} had not exited once its {hold_ms}ms hold had elapsed")
        })?;
    }

    Ok(Outcome::Pass)
}

/// SET_AUX_SWITCH_EXIT activates the named slot and leaves the session, under
/// both orderings.
///
/// "Sets the specified pin and activates the specified RAM slot, in the order
/// given by flags, then exits command-response mode without updating the
/// response header."  Which of the two happens first is not observable from the
/// bus — a slot switch is instantaneous here, so there is no window in which to
/// catch the pin having moved first — so each ordering is asserted to do both
/// things rather than to do them in a particular order.
///
/// Every slot is marked first, so what the bus serves afterwards says which one
/// is active, and the fence is aimed at all of them so that a device on the
/// wrong slot is discriminated rather than merely failing to fence.
pub fn set_aux_switch_exit_switches_and_exits(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    if ctx.ram_slot_count < 2 {
        return Ok(Outcome::Skip(
            "the device has a single RAM slot, so there is no switch to observe".into(),
        ));
    }

    for flags in [0x00, SLOT_FIRST] {
        let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
            Ok(found) => found,
            Err(skip) => return Ok(skip),
        };

        let slots = host_slots(bus, &s)?;
        if slots < 2 {
            return Ok(Outcome::Skip(
                "the device offers the host a single RAM slot, so there is no switch to \
                 observe"
                    .into(),
            ));
        }
        let from = active_slot(bus, &s)?;
        let target = (from + 1) % slots;
        let marks = mark_slots(bus, &s, slots, ctx.probe_addr())?;

        bus.send_cmd(
            s.command_page,
            group::AUX,
            aux::SET_AUX_SWITCH_EXIT,
            &switch_args(DRIVE_HIGH, 0, flags, &pin, target),
        )?;

        fence_every_slot(bus, ctx, slots)?;

        let serving = served_slot(bus, &marks, ctx.probe_addr())?;
        if serving != target {
            return Err(format!(
                "SET_AUX_SWITCH_EXIT with flags 0x{flags:02X} named slot {target}, and the \
                 device is serving slot {serving}"
            ));
        }

        // The back-channel moved with the slot, so re-entry is asserted against
        // the slot now being served.  A successful entry is what says the
        // device had left command-response mode.
        bus.enter_cmd_resp(&s).map_err(|e| {
            format!(
                "ENTER_CMD_RESP after SET_AUX_SWITCH_EXIT with flags 0x{flags:02X}: {e} — the \
                 command is terminal to the session"
            )
        })?;
        bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
            .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;
    }

    Ok(Outcome::Pass)
}

/// SET_AUX_SWITCH_EXIT refuses a flags byte with a reserved bit set.
///
/// "Reserved.  Must be zero.  If any reserved bit is set, neither the pin is set
/// nor the slot switched, but the exit DOES complete."  The command writes no
/// response header, so both halves are read from elsewhere: the device must
/// still be serving the slot it was serving before, and the entry afterwards
/// must succeed, ENTER_CMD_RESP being defined to fail while the device is still
/// in command-response mode.  Each reserved bit is sent on its own, since a
/// device masking the byte rather than testing it can pass on one and fail on
/// another.
///
/// That the pin was not set is not asserted — see this module's header, where
/// it is the pin half of item 7.
pub fn set_aux_switch_exit_rejects_reserved_flags(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    if ctx.ram_slot_count < 2 {
        return Ok(Outcome::Skip(
            "the device has a single RAM slot, so a switch that must not happen cannot be \
             told from one that did not"
                .into(),
        ));
    }

    for bit in 1..8u8 {
        let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
            Ok(found) => found,
            Err(skip) => return Ok(skip),
        };

        let slots = host_slots(bus, &s)?;
        if slots < 2 {
            return Ok(Outcome::Skip(
                "the device offers the host a single RAM slot, so a switch that must not \
                 happen cannot be told from one that did not"
                    .into(),
            ));
        }
        let from = active_slot(bus, &s)?;
        let target = (from + 1) % slots;
        let marks = mark_slots(bus, &s, slots, ctx.probe_addr())?;

        bus.send_cmd(
            s.command_page,
            group::AUX,
            aux::SET_AUX_SWITCH_EXIT,
            &switch_args(DRIVE_HIGH, 0, 1 << bit, &pin, target),
        )?;

        fence_every_slot(bus, ctx, slots)?;

        let serving = served_slot(bus, &marks, ctx.probe_addr())?;
        if serving != from {
            return Err(format!(
                "SET_AUX_SWITCH_EXIT with reserved bit {bit} set switched from slot {from} to \
                 slot {serving} — with a reserved bit set, neither the pin is set nor the slot \
                 switched"
            ));
        }

        bus.enter_cmd_resp(&s).map_err(|e| {
            format!(
                "ENTER_CMD_RESP after SET_AUX_SWITCH_EXIT with reserved bit {bit} set: {e} — \
                 the exit DOES complete"
            )
        })?;
        bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
            .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;
    }

    Ok(Outcome::Pass)
}

/// SET_AUX_SWITCH_EXIT with a slot of 0xAA switches nothing and still exits.
///
/// "An A6 value of 0xAA is invalid.  If received, neither the pin is set nor
/// the slot switched, but the exit DOES complete."  Both halves are asserted:
/// the bus must still serve the slot that was active, and the entry afterwards
/// must succeed, ENTER_CMD_RESP being defined to fail while the device is
/// already in command-response mode.
///
/// That the pin was not set is not asserted — see this module's header, where
/// it is the pin half of item 7.
pub fn set_aux_switch_exit_slot_aa_neither_sets_nor_switches(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    let slots = host_slots(bus, &s)?;
    let from = active_slot(bus, &s)?;
    let marks = mark_slots(bus, &s, slots, ctx.probe_addr())?;

    bus.send_cmd(
        s.command_page,
        group::AUX,
        aux::SET_AUX_SWITCH_EXIT,
        &switch_args(DRIVE_HIGH, 0, 0x00, &pin, 0xAA),
    )?;

    fence_every_slot(bus, ctx, slots)?;

    let serving = served_slot(bus, &marks, ctx.probe_addr())?;
    if serving != from {
        return Err(format!(
            "SET_AUX_SWITCH_EXIT with a slot of 0xAA switched from slot {from} to slot \
             {serving} — an invalid slot is not switched to"
        ));
    }

    bus.enter_cmd_resp(&s).map_err(|e| {
        format!(
            "ENTER_CMD_RESP after SET_AUX_SWITCH_EXIT with a slot of 0xAA: {e} — the invalid \
             slot must not stop the exit completing"
        )
    })?;

    Ok(Outcome::Pass)
}

/// A device with no millisecond counter offers no timed holds, and enforces it.
///
/// "A value of zero indicates the device does not support timed holds, and
/// rejects any SET_AUX with a non-zero hold."  Reached by withholding
/// `ORA_ID_GET_PLUGIN_UPTIME_MS` from the plugin — see [`WITHHELD_API`] — which
/// is the firmware this plugin degrades for.  The hold of zero at the end is
/// the positive control: a device that refused every SET_AUX would satisfy the
/// rejection and be useless.
///
/// The maximum being zero is checked first, and that check is this plugin's
/// degradation rather than a specification requirement — a device is free to
/// report any maximum it likes.  It is here as the sentinel: it is the only
/// thing that tells the scenario the call really was withheld, since the
/// rejection below is vacuous on a device that never had a maximum to exceed.
pub fn no_uptime_offers_no_timed_holds(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, pin) = match session_with_a_drivable_pin(bus, ctx)? {
        Ok(found) => found,
        Err(skip) => return Ok(skip),
    };

    let cap = capability(bus, &s)?;
    if cap.max_hold != 0 {
        return Err(format!(
            "the device reports a maximum hold of {} with no millisecond counter available to \
             time one with",
            cap.max_hold
        ));
    }

    for hold in [1u8, 0x7F, 0xFF] {
        bus.expect_rejected(
            &s,
            group::AUX,
            aux::SET_AUX,
            &set_args(RELEASE, RELEASE, hold, &pin),
        )
        .map_err(|e| {
            format!("{e} — a maximum hold of zero rejects any SET_AUX with a non-zero hold")
        })?;
    }

    bus.issue_cmd(
        &s,
        group::AUX,
        aux::SET_AUX,
        &set_args(RELEASE, RELEASE, 0, &pin),
    )
    .map_err(|e| format!("SET_AUX with a hold of zero: {e} — only a timed hold is refused"))?;

    Ok(Outcome::Pass)
}

/// Fewer groups is still a dense, self-consistent set of groups.
///
/// Reached by withholding `ORA_ID_GET_METADATA_UINT_AT`, which the plugin needs
/// to build all but one of its groups — see [`WITHHELD_API`].  What the
/// specification requires does not change with the count: groups stay
/// "contiguous and start from zero", every one of them answers with a type the
/// protocol allows, and the first number past the end is refused.
///
/// The count and type are checked first, and that check is this plugin's
/// documented degradation rather than a specification requirement.  It is here
/// because it is the only thing that tells the scenario the call really was
/// withheld — the assertions below hold whether it was or not, so without it a
/// mis-keyed [`WITHHELD_API`] entry would pass silently and cover nothing.
pub fn groups_stay_consistent_without_indexed_metadata(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    if cap.groups != 1 {
        return Err(format!(
            "the device reports {} auxiliary pin groups without the indexed metadata getter — \
             only the GPIO group can be built without it, so either the call reached the \
             plugin after all or the plugin builds its other groups some other way",
            cap.groups
        ));
    }

    for grp in 0..cap.groups {
        let (kind, _) = group_info(bus, &s, grp)?;
        if kind != GROUP_TYPE_GPIO {
            return Err(format!(
                "the one group left reports type 0x{kind:02X} rather than GPIO"
            ));
        }
    }
    bus.expect_rejected(&s, group::AUX, aux::GET_AUX_GROUP_INFO, &[cap.groups])?;

    Ok(Outcome::Pass)
}

/// A device with no auxiliary pins reports zero groups and refuses the rest.
///
/// "A device that exposes no auxiliary pins reports a group count of zero from
/// GET_AUX_CAPABILITY.  All other commands in this group return failure on such
/// a device, except SET_AUX_AND_EXIT and SET_AUX_SWITCH_EXIT ... neither sets a
/// pin nor switches a slot, but the exit DOES complete."  Reached by
/// withholding the two GPIO calls the plugin drives pins with — see
/// [`WITHHELD_API`] — since a device with the calls has pins.  This is the only
/// place that rule is put to a device it applies to.
///
/// The count being zero is checked as an error rather than a skip, and that
/// check is this plugin's degradation rather than a specification requirement.
/// It is the sentinel: a skip would turn a mis-keyed [`WITHHELD_API`] entry
/// into silence, and this is the one scenario whose whole subject would then go
/// untested.
pub fn no_groups_without_the_gpio_calls(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    if cap.groups != 0 {
        return Err(format!(
            "the device reports {} auxiliary pin groups without the calls it drives pins with \
             — either those calls reached the plugin after all, in which case this scenario is \
             not exercising the rule it exists for, or the plugin exposes pins it cannot drive",
            cap.groups
        ));
    }

    expect_zero_pin_rule(bus, ctx, &s)?;

    Ok(Outcome::Pass)
}

/// One ROM does not offer the host a pin it is using to serve the ROM.
///
/// **This is One ROM's policy, not a protocol requirement.**  The specification
/// says only that flags bit 0 means "the host may drive this pin with SET_AUX",
/// and leaves the choice of which pins to offer entirely to the device — a
/// conformant device with a different policy would fail this scenario, and
/// would be right to.  What the protocol does require of the flag is asserted
/// by [`pin_info_and_set_aux_agree_on_drivability`], which is where a device
/// other than this one should be judged.
///
/// It earns its place here because it is the only check that reaches outside
/// the protocol: the flag is compared against what the firmware itself says the
/// GPIO is doing, so a group whose pins are mapped to the wrong GPIOs is caught.
/// A mapping error is invisible to the agreement scenario, which asks the same
/// device the same question twice and gets two answers that are wrong together.
///
/// Put to the group of type GPIO, whose pins are "the device's own
/// general-purpose I/O pins, numbered as the device's documentation numbers
/// them" — so pin n is GPIO n, and no board knowledge is needed to line the two
/// up.
pub fn one_rom_withholds_pins_it_is_serving_with(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    let mut gpio_group = None;
    for grp in 0..cap.groups {
        let (kind, pins) = group_info(bus, &s, grp)?;
        if kind == GROUP_TYPE_GPIO {
            gpio_group = Some((grp, pins));
            break;
        }
    }
    let Some((grp, pins)) = gpio_group else {
        return Ok(Outcome::Skip(
            "the device exposes no group of type GPIO, so no pin number here names a GPIO the \
             firmware can be asked about"
                .into(),
        ));
    };

    let (mut in_use, mut offered) = (0u32, 0u32);
    for pin in 0..pins.min(256) {
        let pin = pin as u8;
        let Some(busy) = bus.gpio_in_use(pin) else {
            continue;
        };
        let drivable = pin_info(bus, &s, grp, pin)?[0] & PIN_DRIVABLE != 0;
        if busy {
            in_use += 1;
            if drivable {
                return Err(format!(
                    "GET_AUX_PIN_INFO offers pin {pin} of group {grp} to the host, and the \
                     firmware reports GPIO {pin} as one One ROM is using — driving it would \
                     take it away from whatever is using it, which may be the image the host \
                     is running from"
                ));
            }
        } else if drivable {
            offered += 1;
        }
    }

    if in_use == 0 {
        return Ok(Outcome::Skip(
            "the firmware reports no GPIO in the group as in use, so there is no pin that must \
             be withheld"
                .into(),
        ));
    }
    if offered == 0 {
        return Ok(Outcome::Skip(format!(
            "the device withholds all {in_use} of the GPIOs it is using and offers none of the \
             others, so the flag has not been seen to discriminate"
        )));
    }

    Ok(Outcome::Pass)
}

/// The RAM slot the device is serving, as it reports it.
///
/// [`Ctx::active_ram_slot`] is the slot that was active when the scenario
/// began, and a scenario that has already switched once is past it.
fn active_slot(bus: &mut Bus, s: &Session) -> Result<u8, String> {
    bus.issue_cmd(s, group::READ, read::GET_RAM_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_RAM_SLOT_INFO_ALL: {e}"))?;
    Ok(bus.read_data(s, 1, 1)?[0])
}
