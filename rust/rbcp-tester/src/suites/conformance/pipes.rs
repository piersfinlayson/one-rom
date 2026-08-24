// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Group 0x04 — Pipes".
//!
//! Three commands over a device's pipes: one reporting how many there are, one
//! describing a single pipe, and one transferring bytes to it.  Only the
//! host-to-device direction exists in RBCP 0.1.2.
//!
//! # Pipes are optional, and absence is a legitimate answer
//!
//! "A device that exposes no pipes reports a count of zero from
//! GET_PIPE_CAPABILITY.  All other commands in this group return failure on
//! such a device."  So a zero count is conformant, and every scenario past
//! [`get_pipe_capability`] skips on it rather than failing.  A device with no
//! pipes still has to answer the capability command, which is why that one
//! scenario never skips.
//!
//! # How a write is observed
//!
//! The far end of a pipe is outside the protocol.  PIPE_WRITE reports only
//! whether the bytes were accepted, and no RBCP command reads them back, so
//! "the bytes arrived" cannot be asserted over the bus at all.
//!
//! [`Bus::drain_pipe`] therefore stands where the device's own reader stands,
//! and every write scenario drains immediately before the write it is about, so
//! that what comes back afterwards is attributable to that write and not to
//! something the device logged earlier.  The protocol-level assertion is still
//! the response field — the drain says whether a status-OK meant anything.
//!
//! # Asserting `free`
//!
//! `free` saturates at 0xFF, so on a pipe with any real capacity it reports the
//! saturation and nothing else — a scenario reading it on an empty pipe would
//! be asserting against a constant, and would pass on a device that returned
//! 0xFF unconditionally.  [`free_reports_the_room_left`] therefore fills the
//! pipe first, and makes every assertion in the range where the byte carries
//! information, which is also the only range where a host has any use for it.
//!
//! The specification refuses to guarantee the value: "A PIPE_WRITE of no more
//! than this many bytes is not guaranteed to succeed: the value may be stale by
//! the time the host acts on it."  That bounds what can be required in the
//! permissive direction — no scenario requires a write of `free` bytes to
//! succeed.  It does not bound the restrictive one: `free` is what the device
//! "is able to accept", so a write of more than that must fail, and nothing
//! else touches this pipe to make the value stale.

use onerom_fw_emulator::ffi;

use crate::driver::{Bus, CmdFailure, HDR_SIZE, Session, control, group, pipes};
use crate::{Ctx, Outcome};

/// Payload bytes PIPE_WRITE carries at most, and so the largest valid count.
const MAX_PAYLOAD: u8 = 4;

/// API identifiers withheld from the plugin for the scenarios that need them.
///
/// A pipe is one of the firmware's log channels, and the calls that reach them
/// arrived in firmware later than this plugin's minimum — so the plugin degrades
/// where they are absent, and the emulator, which implements the whole API,
/// cannot reach that path unless they are taken away.  Consulted by
/// [`crate::suites::withheld_api`] before the plugin starts, which is when a
/// plugin resolves its pointers.
pub static WITHHELD_API: &[(&str, &[u32])] = &[(
    "conformance.pipes.no_pipes_without_the_log_calls",
    &[
        ffi::api_id_t_ORA_ID_LOG_OPEN_WRITE,
        ffi::api_id_t_ORA_ID_LOG_WRITE,
        ffi::api_id_t_ORA_ID_LOG_QUERY,
    ],
)];

/// Response data section both query commands need, in bytes.
const REQUIRED_DATA_SIZE: u32 = 8;

/// Ask the device how many pipes it has.
fn pipe_count(bus: &mut Bus, s: &Session) -> Result<u8, String> {
    bus.issue_cmd(s, group::PIPES, pipes::GET_PIPE_CAPABILITY, &[])
        .map_err(|e| format!("GET_PIPE_CAPABILITY: {e}"))?;
    Ok(bus.read_data(s, 0, 1)?[0])
}

/// GET_PIPE_INFO's `free` byte for pipe 0.
fn read_free(bus: &mut Bus, s: &Session) -> Result<u8, String> {
    bus.issue_cmd(s, group::PIPES, pipes::GET_PIPE_INFO, &[0])
        .map_err(|e| format!("GET_PIPE_INFO: {e}"))?;
    Ok(bus.read_data(s, 0, 3)?[2])
}

/// Write full payloads to pipe 0 until one is refused.
///
/// Returns the bytes the device accepted, or `None` where it never refused —
/// a pipe that drains by itself need never be full, and nothing requires it to
/// be.  A distinct byte per write, so the pipe's contents afterwards say
/// exactly which writes were taken.
fn fill_until_refused(bus: &mut Bus, s: &Session) -> Result<Option<Vec<u8>>, String> {
    let mut accepted: Vec<u8> = Vec::new();
    for i in 0..2048u32 {
        let mark = (i % 251) as u8;
        match bus.issue_cmd(
            s,
            group::PIPES,
            pipes::PIPE_WRITE,
            &[mark, mark, mark, mark, 0, MAX_PAYLOAD],
        ) {
            Ok(()) => accepted.extend_from_slice(&[mark; MAX_PAYLOAD as usize]),
            Err(CmdFailure::Failed) => return Ok(Some(accepted)),
            Err(e) => return Err(format!("PIPE_WRITE: {e}")),
        }
    }
    Ok(None)
}

/// Enter command-response mode and report the pipe count, or skip.
///
/// The shape every scenario past the capability one starts with: a device with
/// no pipes is conformant and has nothing further to assert.
fn session_with_a_pipe(bus: &mut Bus, ctx: &Ctx) -> Result<Result<Session, Outcome>, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    if pipe_count(bus, &s)? == 0 {
        return Ok(Err(Outcome::Skip(
            "the device exposes no pipes, which the specification permits".into(),
        )));
    }
    Ok(Ok(s))
}

/// GET_PIPE_CAPABILITY answers, and its reserved bytes are zero.
///
/// "Fails if the response data section is smaller than 8 bytes", and the
/// session these scenarios run in is larger than that, so the command must
/// succeed on every device — including one with no pipes, whose answer is a
/// count of zero.
pub fn get_pipe_capability(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.issue_cmd(&s, group::PIPES, pipes::GET_PIPE_CAPABILITY, &[])
        .map_err(|e| format!("GET_PIPE_CAPABILITY: {e}"))?;

    bus.expect_data(&s, 1, &[0x00; 7], "GET_PIPE_CAPABILITY reserved bytes 1-7")?;

    Ok(Outcome::Pass)
}

/// GET_PIPE_INFO describes a pipe the device has.
///
/// The type is Raw, the only value the specification defines, or an
/// implementation-specific value from 0x80 upwards.  "At least one of bits 0
/// and 1 is always set", so a pipe the device exposes carries OUT, IN or both.
/// Bit 3 is "meaningful only where bit 2 is set, and must be set to zero by the
/// device where bit 2 is clear".  `waiting` reads "zero where the pipe does not
/// support IN", and the far end follows the same range rules as the type.
pub fn get_pipe_info(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = match session_with_a_pipe(bus, ctx)? {
        Ok(s) => s,
        Err(skip) => return Ok(skip),
    };

    bus.issue_cmd(&s, group::PIPES, pipes::GET_PIPE_INFO, &[0])
        .map_err(|e| format!("GET_PIPE_INFO: {e}"))?;

    let info = bus.read_data(&s, 0, 8)?;

    if info[0] != 0x00 && info[0] < 0x80 {
        return Err(format!(
            "pipe 0 reports type 0x{:02X}; the specification defines only 0x00 (Raw), reserves \
             0x01-0x7F, and leaves 0x80-0xFE to the implementation",
            info[0]
        ));
    }
    if info[1] & 0x03 == 0 {
        return Err(format!(
            "pipe 0 reports flags 0x{:02X}, with both direction bits clear — the device exposes \
             the pipe, and at least one of bits 0 and 1 is always set",
            info[1]
        ));
    }
    if info[1] & 0x04 == 0 && info[1] & 0x08 != 0 {
        return Err(format!(
            "pipe 0 reports flags 0x{:02X}, with the attached bit set while the bit saying the \
             device reports attachment is clear — bit 3 must be zero where bit 2 is",
            info[1]
        ));
    }
    if info[1] & 0xF0 != 0 {
        return Err(format!(
            "pipe 0 reports flags 0x{:02X}, with a bit set above bit 3 — bits 4-7 are reserved \
             and must be set to zero by the device",
            info[1]
        ));
    }

    if info[1] & 0x02 == 0 && info[3] != 0 {
        return Err(format!(
            "pipe 0 carries no IN direction but reports {} bytes waiting — waiting reads zero \
             where the pipe does not support IN",
            info[3]
        ));
    }

    if info[4] > 0x03 && info[4] < 0x80 {
        return Err(format!(
            "pipe 0 reports far end 0x{:02X}; the specification defines 0x00-0x03, reserves \
             0x04-0x7F, and leaves 0x80-0xFE to the implementation",
            info[4]
        ));
    }

    bus.expect_data(&s, 5, &[0x00; 3], "GET_PIPE_INFO reserved bytes 5-7")?;

    Ok(Outcome::Pass)
}

/// A pipe number the device does not expose is rejected, by both commands.
///
/// GET_PIPE_INFO "fails if the pipe is not one the device exposes", and
/// PIPE_WRITE likewise.  Pipe numbering is dense and 0-based, so the count is
/// itself the first number that is not one of them.
pub fn commands_reject_an_absent_pipe(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let absent = pipe_count(bus, &s)?;

    bus.expect_rejected(&s, group::PIPES, pipes::GET_PIPE_INFO, &[absent])?;
    bus.expect_rejected(
        &s,
        group::PIPES,
        pipes::PIPE_WRITE,
        &[b'x', 0, 0, 0, absent, 1],
    )?;

    Ok(Outcome::Pass)
}

/// GET_PIPE_INFO must reject a pipe number of 0xAA.
///
/// Its only argument is also its final one, so the blanket rule covers it: "If a
/// device received a command with the final argument set to 0xAA, it rejects the
/// command."  That is a rule about the value, not about the count — a device
/// that happened to expose 171 pipes would still have to refuse this one.
///
/// Where the device has a pipe, the same command naming pipe 0 is required to
/// succeed, so the refusal is on account of the 0xAA rather than of the command
/// being unavailable.  With no pipes there is no such control, and the rule that
/// every command but the capability one fails is asserted by
/// [`commands_reject_an_absent_pipe`] instead.
pub fn get_pipe_info_rejects_pipe_aa(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.expect_rejected(&s, group::PIPES, pipes::GET_PIPE_INFO, &[0xAA])?;

    if pipe_count(bus, &s)? > 0 {
        bus.issue_cmd(&s, group::PIPES, pipes::GET_PIPE_INFO, &[0])
            .map_err(|e| {
                format!(
                    "GET_PIPE_INFO for pipe 0: {e} — the refusal above cannot be attributed to \
                     the 0xAA if a pipe the device exposes is refused too"
                )
            })?;
    }

    Ok(Outcome::Pass)
}

/// Without the firmware's log calls the device exposes no pipes.
///
/// A pipe is one of the firmware's log channels, so a plugin running on firmware
/// with no log API has nothing to expose.  The protocol already provides for
/// that — a count of zero — so the group goes quiet rather than the plugin
/// refusing to run, and "all other commands in this group return failure on such
/// a device".
///
/// Reached by withholding the three log calls the plugin uses; see
/// [`WITHHELD_API`].  The count being zero is the sentinel that the withholding
/// took effect, since the refusals below are what a device with no pipes does
/// anyway.
pub fn no_pipes_without_the_log_calls(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let count = pipe_count(bus, &s)?;
    if count != 0 {
        return Err(format!(
            "the device reports {count} pipe(s) with the firmware's log calls withheld; a pipe \
             is a log channel, and without those calls there is none to reach"
        ));
    }

    bus.expect_rejected(&s, group::PIPES, pipes::GET_PIPE_INFO, &[0])?;
    bus.expect_rejected(&s, group::PIPES, pipes::PIPE_WRITE, &[b'x', 0, 0, 0, 0, 1])?;

    Ok(Outcome::Pass)
}

/// PIPE_WRITE transfers its payload, and the bytes reach the pipe.
///
/// Every count from 1 to 4 is exercised, because count is what says how much of
/// the fixed four-argument payload is meaningful — a device that ignored it and
/// took all four would pass a single-count scenario and corrupt every stream.
/// The payload includes 0xAA, which is valid in these arguments precisely
/// because none of them is the last one.
pub fn pipe_write_transfers_its_payload(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = match session_with_a_pipe(bus, ctx)? {
        Ok(s) => s,
        Err(skip) => return Ok(skip),
    };

    let payload: [u8; 4] = [0xAA, 0x00, 0xFF, b'Z'];

    for count in 1..=MAX_PAYLOAD {
        let _ = bus.drain_pipe(0);

        bus.issue_cmd(
            &s,
            group::PIPES,
            pipes::PIPE_WRITE,
            &[payload[0], payload[1], payload[2], payload[3], 0, count],
        )
        .map_err(|e| format!("PIPE_WRITE with count {count}: {e}"))?;

        let got = bus.drain_pipe(0);
        let want = &payload[..usize::from(count)];
        if got != want {
            return Err(format!(
                "PIPE_WRITE with count {count} put {got:02X?} on the pipe, expected {want:02X?} \
                 — the device transfers \"count bytes, taken from A0 onwards\""
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// A count outside 1 to 4 is rejected, and nothing is transferred.
///
/// "A5 must be in the range 0x01 to 0x04 — any other value is invalid and the
/// device rejects the command."  0xAA is in that set and needs no separate
/// rule, which is the reason count is the final argument: a payload byte there
/// could not have carried the value.
pub fn pipe_write_rejects_a_bad_count(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = match session_with_a_pipe(bus, ctx)? {
        Ok(s) => s,
        Err(skip) => return Ok(skip),
    };

    for count in [0x00, MAX_PAYLOAD + 1, 0xAA, 0xFF] {
        let _ = bus.drain_pipe(0);

        bus.expect_rejected(
            &s,
            group::PIPES,
            pipes::PIPE_WRITE,
            &[b'a', b'b', b'c', b'd', 0, count],
        )?;

        let got = bus.drain_pipe(0);
        if !got.is_empty() {
            return Err(format!(
                "PIPE_WRITE with count 0x{count:02X} was rejected but still put {got:02X?} on \
                 the pipe"
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// A refused write transfers nothing at all.
///
/// "Either all count bytes are transferred or none are: the device returns
/// failure if it cannot accept them all, and in that case transfers nothing."
/// The pipe is filled until a write is refused, and what the pipe then holds
/// must be a whole number of the writes that succeeded — with no tail from the
/// one that did not.
///
/// Skipped where nothing ever refuses.  A device whose pipe drains by itself
/// need never be full, and the specification does not require it to be.
pub fn pipe_write_is_all_or_nothing(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = match session_with_a_pipe(bus, ctx)? {
        Ok(s) => s,
        Err(skip) => return Ok(skip),
    };

    let _ = bus.drain_pipe(0);

    let Some(accepted) = fill_until_refused(bus, &s)? else {
        return Ok(Outcome::Skip(
            "the pipe accepted every write, so there is no refusal to inspect".into(),
        ));
    };

    let got = bus.drain_pipe(0);
    if got != accepted {
        return Err(format!(
            "after a refused PIPE_WRITE the pipe holds {} bytes, expected the {} bytes of the \
             writes that succeeded — a refused write must transfer nothing",
            got.len(),
            accepted.len()
        ));
    }

    Ok(Outcome::Pass)
}

/// GET_PIPE_INFO's `free` reports the room actually left, unsaturated.
///
/// A pipe that is not nearly full says only 0xFF, which is the saturation and
/// not a measurement — a scenario that read `free` on an empty pipe would be
/// reading a constant.  So this fills the pipe first, and every assertion is
/// made in the range where the byte carries information, which is also the only
/// range in which a host has any use for it.
///
/// The last write before a refusal leaves less than a full payload of room, so
/// `free` must then be below [`MAX_PAYLOAD`] and the byte must be a real count
/// rather than the saturated value.  Draining must put it back.
pub fn free_reports_the_room_left(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = match session_with_a_pipe(bus, ctx)? {
        Ok(s) => s,
        Err(skip) => return Ok(skip),
    };

    let _ = bus.drain_pipe(0);

    if fill_until_refused(bus, &s)?.is_none() {
        return Ok(Outcome::Skip(
            "the pipe accepted every write, so it never reports less than 0xFF free".into(),
        ));
    }

    let free = read_free(bus, &s)?;
    if free >= MAX_PAYLOAD {
        return Err(format!(
            "a {MAX_PAYLOAD}-byte PIPE_WRITE was refused, but free reports {free} — the device \
             refused a write it had room for, or free does not report the room left"
        ));
    }

    // The boundary, in the direction the specification does bind: free is what
    // the device "is able to accept", so a write of more than that must fail.
    // Nothing else touches this pipe, so the value cannot have gone stale.
    if free < MAX_PAYLOAD {
        let over = free + 1;
        bus.expect_rejected(
            &s,
            group::PIPES,
            pipes::PIPE_WRITE,
            &[b'o', b'v', b'e', b'r', 0, over],
        )
        .map_err(|e| format!("a {over}-byte write with {free} free: {e}"))?;
    }

    let drained = bus.drain_pipe(0);
    if drained.is_empty() {
        return Err("the pipe reported itself full but drained empty".into());
    }

    let recovered = read_free(bus, &s)?;
    if recovered <= free {
        return Err(format!(
            "free was {free} with the pipe full and {recovered} after draining {} bytes — \
             draining must give the room back",
            drained.len()
        ));
    }

    Ok(Outcome::Pass)
}

/// The group is command-response mode only, and refusing costs the host nothing.
///
/// The bytes must not reach the pipe, which the drain says.  The SLOT_POKE
/// afterwards is a liveness check - the device must still be taking commands.
///
/// Consumption is deliberately not asserted, and cannot be from here.  A
/// knocked command afterwards cannot see a missing discard, because the knock
/// is matched by the firmware's address monitor as a sliding window and
/// leftover argument bytes slide past it.  Command-response mode is where the
/// token makes consumption observable.
pub fn not_valid_in_command_mode(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let addr = ctx.scratch_addr();
    let armed = bus.read(addr)? ^ 0xFF;

    let _ = bus.drain_pipe(0);

    bus.knock(ctx.command_page())?;
    bus.send_cmd(
        ctx.command_page(),
        group::PIPES,
        pipes::PIPE_WRITE,
        &[b'x', b'y', b'z', b'!', 0, MAX_PAYLOAD],
    )?;

    bus.knock(ctx.command_page())?;
    bus.send_poke(ctx, addr, armed)?;

    bus.await_byte(addr, armed).map_err(|e| {
        format!(
            "{e} — the SLOT_POKE after a command-mode PIPE_WRITE never landed, so the device \
             stopped answering after a command-mode PIPE_WRITE"
        )
    })?;

    let got = bus.drain_pipe(0);
    if !got.is_empty() {
        return Err(format!(
            "a PIPE_WRITE issued in command mode put {got:02X?} on the pipe — the Pipes group \
             is valid in command-response mode only"
        ));
    }

    Ok(Outcome::Pass)
}

/// Both query commands fail where the data section cannot hold their answer.
///
/// Each "fails if the response data section is smaller than 8 bytes", and both
/// answers are exactly that long, so there is no partial answer to give.  The
/// device must refuse rather than write past the region the host gave it.
///
/// Asserted on a device with no pipes too: GET_PIPE_CAPABILITY has an answer
/// either way, and a count of zero still needs eight bytes to report.
pub fn query_commands_need_room_for_their_answer(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let short = (REQUIRED_DATA_SIZE - 4) as u16;
    let s = bus.enter_sized(ctx, HDR_SIZE as u16 + short)?;

    bus.expect_rejected(&s, group::PIPES, pipes::GET_PIPE_CAPABILITY, &[])
        .map_err(|e| format!("{e} — the data section is {short} bytes and the answer is 8"))?;
    bus.expect_rejected(&s, group::PIPES, pipes::GET_PIPE_INFO, &[0])
        .map_err(|e| format!("{e} — the data section is {short} bytes and the answer is 8"))?;

    Ok(Outcome::Pass)
}

/// The query commands are refused in command mode, and cost the host nothing.
///
/// The companion to [`not_valid_in_command_mode`], which covers PIPE_WRITE.
/// These two are the ones with an answer to write, so the assertion is that
/// they did not write it: the first byte of where the data section used to be
/// is armed with a value the device would have to overwrite to have acted, and
/// must still hold it afterwards.  "Once complete is observed, the device has
/// exited command-response mode and the back-channel region is no longer
/// maintained."
///
/// The SLOT_POKE at the end is a liveness check.  It carries no framing
/// verdict: a knocked command cannot see a missing discard, because the knock
/// is matched by the firmware's address monitor as a sliding window and
/// leftover argument bytes slide past it.
pub fn query_commands_not_valid_in_command_mode(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    let dst = s.bch_start + HDR_SIZE;
    let armed = bus.read(dst)? ^ 0xFF;
    bus.poke_verified(ctx, dst, armed)
        .map_err(|e| format!("arming the response data section: {e}"))?;

    bus.knock(ctx.command_page())?;
    bus.send_cmd(
        ctx.command_page(),
        group::PIPES,
        pipes::GET_PIPE_CAPABILITY,
        &[],
    )?;

    bus.knock(ctx.command_page())?;
    bus.send_cmd(ctx.command_page(), group::PIPES, pipes::GET_PIPE_INFO, &[0])?;

    let scratch = ctx.scratch_addr();
    let framed = bus.read(scratch)? ^ 0xFF;
    bus.knock(ctx.command_page())?;
    bus.send_poke(ctx, scratch, framed)?;
    bus.await_byte(scratch, framed).map_err(|e| {
        format!(
            "{e} — the SLOT_POKE after a command-mode GET_PIPE_INFO never landed, so the device \
             stopped answering after the command-mode queries"
        )
    })?;

    let got = bus.read(dst)?;
    if got != armed {
        return Err(format!(
            "the device answered a Pipes query in command mode: 0x{dst:06X} serves 0x{got:02X} \
             rather than the armed 0x{armed:02X} — the group is valid in command-response mode \
             only, and after EXIT_CMD_RESP_ACK the back-channel is no longer maintained"
        ));
    }

    Ok(Outcome::Pass)
}

/// 0xAA is carried in every payload position, not just the first.
///
/// "All 256 values are valid in A0 to A3."  0xAA is the one value with a
/// meaning of its own elsewhere in the protocol — it is the reset GROUP and
/// CMD, and is rejected in every final argument — so it is the value a device
/// is most likely to act on rather than carry.  Which position it sits in is
/// exactly what that rule turns on, so each is exercised, and a full-payload
/// write is used so all four are on the wire every time.
pub fn pipe_write_carries_aa_in_every_position(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = match session_with_a_pipe(bus, ctx)? {
        Ok(s) => s,
        Err(skip) => return Ok(skip),
    };

    for position in 0..MAX_PAYLOAD as usize {
        let mut payload = [b'.'; MAX_PAYLOAD as usize];
        payload[position] = 0xAA;

        let _ = bus.drain_pipe(0);

        bus.issue_cmd(
            &s,
            group::PIPES,
            pipes::PIPE_WRITE,
            &[
                payload[0],
                payload[1],
                payload[2],
                payload[3],
                0,
                MAX_PAYLOAD,
            ],
        )
        .map_err(|e| format!("PIPE_WRITE with 0xAA in A{position}: {e}"))?;

        let got = bus.drain_pipe(0);
        if got != payload {
            return Err(format!(
                "PIPE_WRITE with 0xAA in A{position} put {got:02X?} on the pipe, expected \
                 {payload:02X?} — every value is valid in A0 to A3"
            ));
        }
    }

    Ok(Outcome::Pass)
}
