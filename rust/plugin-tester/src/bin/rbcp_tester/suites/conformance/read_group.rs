// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Group 0x01 — Read", and the response format section of each
//! of its eight commands.
//!
//! The Read group is where the device describes itself, so almost every
//! scenario here is a field-by-field reading of a "… Response Format" table:
//! the data section begins at offset 8 within the back-channel region, and each
//! command lays out named bytes from there.  Reserved bytes must be zero, name
//! fields are ASCII and null-terminated with their unused bytes zeroed, and the
//! commands that take a slot argument must reject 0xAA in it.
//!
//! # Where the expected values come from
//!
//! Most fields are checked against something other than the device's own word
//! for them, because a response that merely agrees with itself asserts nothing:
//!
//! - the flash slot count against `ctx.config`, from which the firmware under
//!   test is built — its chip sets are the populated, non-plugin flash slots;
//! - GET_FLASH_SLOT_INFO_ALL's records against GET_FLASH_SLOT_INFO for the same
//!   slot, which the specification requires, since the former "provides the
//!   entirety of the information exposed by GET_FLASH_SLOT_COUNT and
//!   GET_FLASH_SLOT_INFO in a single request response";
//! - the RAM slot counts against what the device reports through its own plugin
//!   API at start-up, in [`crate::Ctx`];
//! - SLOT_PEEK's data against what the device *serves* at those same addresses.
//!   Serving and peeking are separate paths through the device, so agreement
//!   between them is a real assertion, and the bytes peeked are ones this module
//!   put there with a verified SLOT_POKE first.
//!
//! # Back-channel sizes
//!
//! Several requirements are about there being too little room in the response
//! data section, so those scenarios enter command-response mode with a
//! back-channel of their own size rather than [`Ctx::session`]'s — see
//! [`enter_sized`].

use crate::driver::{
    Bus, DEFAULT_COMPLETE, DEFAULT_STATUS_OK, HDR_SIZE, Session, control, group, read,
    slot_peek_args,
};
use crate::{Ctx, Outcome};

/// The RBCP version this module was written against.
///
/// "A host implementation written against version 0.Y.z is guaranteed to
/// interoperate correctly with any device implementing version 0.Y.w where
/// w >= z."  Every assertion in this suite is made against that document, so a
/// device outside those bounds is not one these scenarios can judge.
const SPEC_MAJOR: u8 = 0;
const SPEC_MINOR: u8 = 1;
const SPEC_PATCH: u8 = 1;

/// Size of one flash slot record: 1 byte of ROM type and a 31-byte name.
const RECORD_SIZE: u32 = 32;

/// Size of the GET_FLASH_SLOT_INFO_ALL preamble.
const PREAMBLE_SIZE: u32 = 4;

/// The value the ROM Types table gives to "Invalid/ROM not being served".
const ROM_TYPE_INVALID: u8 = 0xFF;

/// The device's chunk size for SLOT_PEEK is not visible to a host, so peeks are
/// sized to straddle any plausible one: 40 bytes crosses a 32-byte boundary and
/// ends part way through the next chunk.
const PEEK_LEN: u32 = 40;

/// Enter command-response mode with a back-channel region of a chosen size.
///
/// [`Ctx::session`] is generous enough for every response format at once, which
/// is exactly wrong for the requirements about insufficient space.  The start
/// address stays where `Ctx` puts it — 4-byte aligned and clear of the command
/// page — so only the size differs.
fn enter_sized(bus: &mut Bus, ctx: &Ctx, bch_size: u16) -> Result<Session, String> {
    let s = Session {
        command_page: ctx.command_page(),
        bch_start: ctx.bch_start(),
        bch_size,
        complete: DEFAULT_COMPLETE,
        status_ok: DEFAULT_STATUS_OK,
    };
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP with a {bch_size}-byte back-channel: {e}"))?;
    Ok(s)
}

/// Read `len` bytes of the served image over the bus, as ordinary ROM data.
fn served_bytes(bus: &mut Bus, addr: u32, len: u32) -> Result<Vec<u8>, String> {
    (0..len).map(|i| bus.read(addr + i)).collect()
}

/// The first address of a range SLOT_PEEK can read without touching anything
/// this suite depends on.
///
/// Above the back-channel and above the probe, fence and scratch bytes, and
/// deliberately odd: the address the host asks the device to read is not the
/// address the answer is written to, so an even-only peek would never exercise
/// the device's mapping of an odd source byte.
fn peek_src(ctx: &Ctx) -> u32 {
    ctx.scratch_addr() + 0x21
}

/// Require that a peek of `len` bytes fits in the slot, or say why it cannot.
fn peek_fits(ctx: &Ctx, len: u32) -> Option<Outcome> {
    let src = peek_src(ctx);
    (src + len > ctx.ram_slot_size).then(|| {
        Outcome::Skip(format!(
            "a {len}-byte peek from 0x{src:06X} does not fit in this device's \
             {}-byte RAM slot",
            ctx.ram_slot_size
        ))
    })
}

/// Require a name field to be "ASCII.  Unused bytes are filled with 0x00.
/// Null-terminated."
///
/// Applies to the 31-byte slot name of a flash slot record and, with a
/// different length, to GET_DEVICE_TYPE and GET_DEVICE_VERSION.  `required`
/// distinguishes the fields the specification says a device must provide from
/// the slot name, where "a zero length name is a valid response".
fn check_ascii_field(field: &[u8], required: bool, what: &str) -> Result<(), String> {
    let Some(nul) = field.iter().position(|&b| b == 0) else {
        return Err(format!(
            "{what}: no 0x00 in the {}-byte field, which the specification requires to be \
             null-terminated — read {}",
            field.len(),
            hex(field)
        ));
    };
    if required && nul == 0 {
        return Err(format!(
            "{what}: the field is empty, and the specification requires a device to provide one"
        ));
    }
    if let Some((i, b)) = field
        .iter()
        .enumerate()
        .take(nul)
        .find(|&(_, &b)| !(0x20..=0x7E).contains(&b))
    {
        return Err(format!(
            "{what}: byte {i} is 0x{b:02X}, which is not ASCII text — read {}",
            hex(field)
        ));
    }
    if let Some((i, b)) = field.iter().enumerate().skip(nul).find(|&(_, &b)| b != 0) {
        return Err(format!(
            "{what}: byte {i} after the terminator is 0x{b:02X}; the specification requires \
             unused bytes to be filled with 0x00 — read {}",
            hex(field)
        ));
    }
    Ok(())
}

/// Require a flash slot record to be well formed: a ROM type that identifies a
/// ROM, and a conforming name field.
fn check_record(record: &[u8], what: &str) -> Result<(), String> {
    if record[0] == ROM_TYPE_INVALID {
        return Err(format!(
            "{what}: rom_type is 0x{ROM_TYPE_INVALID:02X}, which the ROM Types table defines \
             as invalid / ROM not being served, for a slot the device reports as populated"
        ));
    }
    check_ascii_field(&record[1..], false, &format!("{what} name"))
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fetch one flash slot's record with GET_FLASH_SLOT_INFO.
fn flash_slot_record(bus: &mut Bus, s: &Session, slot: u8) -> Result<Vec<u8>, String> {
    bus.issue_cmd(s, group::READ, read::GET_FLASH_SLOT_INFO, &[slot])
        .map_err(|e| format!("GET_FLASH_SLOT_INFO for slot {slot}: {e}"))?;
    bus.read_data(s, 0, RECORD_SIZE)
}

/// Ask the device how many flash slots it has.
fn flash_slot_count(bus: &mut Bus, s: &Session) -> Result<u8, String> {
    bus.issue_cmd(s, group::READ, read::GET_FLASH_SLOT_COUNT, &[])
        .map_err(|e| format!("GET_FLASH_SLOT_COUNT: {e}"))?;
    Ok(bus.read_data(s, 0, 1)?[0])
}

/// GET_FLASH_SLOT_COUNT writes the flash slot count to data section offset 0.
///
/// The count is checked against the configuration the firmware under test was
/// built from: each of its chip sets is one populated, non-plugin flash slot,
/// which is exactly what the field is defined to count.
pub fn get_flash_slot_count(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.issue_cmd(&s, group::READ, read::GET_FLASH_SLOT_COUNT, &[])
        .map_err(|e| format!("GET_FLASH_SLOT_COUNT: {e}"))?;

    let want = u8::try_from(ctx.config.chip_sets.len())
        .map_err(|_| "the configuration has more chip sets than a slot count can hold")?;
    bus.expect_data(&s, 0, &[want], "GET_FLASH_SLOT_COUNT response")
        .map_err(|e| format!("{e} — the device is built from {want} flash slot(s)"))?;

    Ok(Outcome::Pass)
}

/// GET_FLASH_SLOT_INFO returns a well-formed record for every valid slot index.
///
/// The count "can be used to determine valid slot indices for subsequent
/// GET_FLASH_SLOT_INFO commands", so every index below it must answer and the
/// first index at or above it must not.  Each record is then read against its
/// response format: rom_type at offset 0, a 31-byte name from offset 1.
pub fn get_flash_slot_info(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let count = flash_slot_count(bus, &s)?;
    for slot in 0..count {
        let record = flash_slot_record(bus, &s, slot)?;
        check_record(&record, &format!("GET_FLASH_SLOT_INFO slot {slot}"))?;
    }

    // One past the last slot is not a valid index, so the device must not
    // report success for it.
    bus.expect_rejected(&s, group::READ, read::GET_FLASH_SLOT_INFO, &[count])
        .map_err(|e| {
            format!(
                "{e} — slot {count} is outside the {count} slot(s) the device reports through \
                 GET_FLASH_SLOT_COUNT"
            )
        })?;

    Ok(Outcome::Pass)
}

/// GET_FLASH_SLOT_INFO must reject a slot argument of 0xAA.
///
/// "An A0 value of 0xAA is invalid and rejected."  Rejection is not silence:
/// the device runs the processing sequence and reports failure, which is how
/// 0xAA stays reserved for detecting a reset started mid-command.
pub fn get_flash_slot_info_rejects_slot_aa(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.expect_rejected(&s, group::READ, read::GET_FLASH_SLOT_INFO, &[0xAA])?;

    Ok(Outcome::Pass)
}

/// GET_FLASH_SLOT_INFO fails when the data section cannot hold a record.
///
/// "Only succeeds if there is sufficient space".  A record is 32 bytes, and
/// here the data section is 16, so the command cannot be satisfied under any
/// reading of how much space is sufficient.
pub fn get_flash_slot_info_needs_room_for_a_record(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = enter_sized(bus, ctx, (HDR_SIZE + 16) as u16)?;

    bus.expect_rejected(&s, group::READ, read::GET_FLASH_SLOT_INFO, &[0])
        .map_err(|e| {
            format!(
                "{e} — the data section is {} bytes and a flash slot record is {RECORD_SIZE}",
                s.data_size()
            )
        })?;

    Ok(Outcome::Pass)
}

/// GET_FLASH_SLOT_INFO_ALL's preamble and records, where everything fits.
///
/// The command "provides the entirety of the information exposed by
/// GET_FLASH_SLOT_COUNT and GET_FLASH_SLOT_INFO in a single request response",
/// so with room for every record the preamble must account for all of them —
/// `whole_count` equal to `total_count`, no partial — and each record must be
/// the one GET_FLASH_SLOT_INFO gives for the same slot, "in slot index order".
pub fn get_flash_slot_info_all(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let total = flash_slot_count(bus, &s)?;
    let needed = PREAMBLE_SIZE + u32::from(total) * RECORD_SIZE;
    if needed > s.data_size() {
        return Ok(Outcome::Skip(format!(
            "the device has {total} flash slots, needing {needed} bytes, more than this \
             session's {}-byte data section",
            s.data_size()
        )));
    }

    // Collect the individual records first: they are what the combined
    // response is required to reproduce.
    let mut records = Vec::new();
    for slot in 0..total {
        records.push(flash_slot_record(bus, &s, slot)?);
    }

    bus.issue_cmd(&s, group::READ, read::GET_FLASH_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_FLASH_SLOT_INFO_ALL: {e}"))?;

    bus.expect_data(
        &s,
        0,
        &[total, total, 0x00, 0x00],
        "GET_FLASH_SLOT_INFO_ALL preamble (total_count, whole_count, partial_flag, reserved)",
    )
    .map_err(|e| {
        format!(
            "{e} — the {}-byte data section has room for all {total} record(s), so the \
             response must carry every one of them",
            s.data_size()
        )
    })?;

    for (i, record) in records.iter().enumerate() {
        let offset = PREAMBLE_SIZE + i as u32 * RECORD_SIZE;
        bus.expect_data(
            &s,
            offset,
            record,
            &format!("GET_FLASH_SLOT_INFO_ALL record {i}"),
        )
        .map_err(|e| {
            format!(
                "{e} — records follow the preamble in slot index order, and record {i} must \
                     match GET_FLASH_SLOT_INFO for slot {i}"
            )
        })?;
    }

    Ok(Outcome::Pass)
}

/// GET_FLASH_SLOT_INFO_ALL's partial record, and the arithmetic the host uses
/// to find its length.
///
/// "Where partial_flag is 0x01, the number of bytes present for the partial
/// record is: data_section_size − 4 − (whole_count × 32)", and the record
/// "contain\[s\] as many bytes of that record as the data section (minus space
/// for header) permits".  The back-channel is sized to hold the preamble, one
/// whole record and ten bytes over, so both statements are testable at once:
/// the truncated record must be the first ten bytes of the record
/// GET_FLASH_SLOT_INFO gives for that slot.
pub fn get_flash_slot_info_all_partial_record(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    /// Bytes left over after the preamble and one whole record.  Ten falls
    /// inside the truncated slot's name rather than in its trailing padding,
    /// where a wrong byte would be invisible.
    const PARTIAL_BYTES: u32 = 10;

    // A session big enough for the individual records, to collect what the
    // truncated response must reproduce.
    let full = ctx.session();
    bus.enter_cmd_resp(&full)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let total = flash_slot_count(bus, &full)?;
    if total < 2 {
        return Ok(Outcome::Skip(format!(
            "the device has {total} flash slot(s); a truncated record needs a slot beyond the \
             whole one that precedes it"
        )));
    }
    let truncated = flash_slot_record(bus, &full, 1)?;

    bus.issue_cmd(&full, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    // Re-enter with a data section of exactly preamble + one record + ten.
    let data_size = PREAMBLE_SIZE + RECORD_SIZE + PARTIAL_BYTES;
    let s = enter_sized(bus, ctx, (HDR_SIZE + data_size) as u16)?;

    bus.issue_cmd(&s, group::READ, read::GET_FLASH_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_FLASH_SLOT_INFO_ALL: {e}"))?;

    bus.expect_data(
        &s,
        0,
        &[total, 1, 0x01, 0x00],
        "GET_FLASH_SLOT_INFO_ALL preamble (total_count, whole_count, partial_flag, reserved)",
    )
    .map_err(|e| {
        format!(
            "{e} — the {data_size}-byte data section holds the 4-byte preamble, one whole \
             {RECORD_SIZE}-byte record and {PARTIAL_BYTES} bytes of the next"
        )
    })?;

    // The host's arithmetic, written out as the specification states it.
    let partial_len = data_size - PREAMBLE_SIZE - RECORD_SIZE;

    // ...and the terminator the specification requires where the truncated
    // record carries a name: its final byte is 0x00, so the partial name reads
    // as a C string like every other name in the response.  The byte the
    // record really holds there is therefore not delivered.
    let mut want = truncated[..partial_len as usize].to_vec();
    want[partial_len as usize - 1] = 0x00;

    bus.expect_data(
        &s,
        PREAMBLE_SIZE + RECORD_SIZE,
        &want,
        "GET_FLASH_SLOT_INFO_ALL partial record",
    )
    .map_err(|e| {
        format!(
            "{e} — data_section_size − 4 − (whole_count × 32) = {partial_len}, and those bytes \
             must be the first {partial_len} of slot 1's record, the last replaced by the 0x00 \
             terminator"
        )
    })?;

    Ok(Outcome::Pass)
}

/// A one-byte truncated record is the `rom_type`, not a terminator.
///
/// "Where only one byte is present it is the `rom_type`, and no name follows."
/// The terminator exists so a name can be read as a C string; where there is no
/// name there is nothing to terminate, and writing one would destroy the single
/// piece of information the record carries.
///
/// Sized so the data section holds the preamble, one whole record and exactly
/// one byte over — the boundary case of the arithmetic every other partial
/// record scenario exercises in the middle.
pub fn get_flash_slot_info_all_one_byte_partial(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let full = ctx.session();
    bus.enter_cmd_resp(&full)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let total = flash_slot_count(bus, &full)?;
    if total < 2 {
        return Ok(Outcome::Skip(format!(
            "the device has {total} flash slot(s); a truncated record needs a slot beyond the \
             whole one that precedes it"
        )));
    }
    let truncated = flash_slot_record(bus, &full, 1)?;

    bus.issue_cmd(&full, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    let data_size = PREAMBLE_SIZE + RECORD_SIZE + 1;
    let s = enter_sized(bus, ctx, (HDR_SIZE + data_size) as u16)?;

    bus.issue_cmd(&s, group::READ, read::GET_FLASH_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_FLASH_SLOT_INFO_ALL: {e}"))?;

    bus.expect_data(
        &s,
        0,
        &[total, 1, 0x01, 0x00],
        "GET_FLASH_SLOT_INFO_ALL preamble (total_count, whole_count, partial_flag, reserved)",
    )?;

    bus.expect_data(
        &s,
        PREAMBLE_SIZE + RECORD_SIZE,
        &truncated[..1],
        "GET_FLASH_SLOT_INFO_ALL one-byte partial record",
    )
    .map_err(|e| {
        format!(
            "{e} — with a single byte present that byte is slot 1's rom_type; terminating a \
             name that is not there would overwrite it"
        )
    })?;

    Ok(Outcome::Pass)
}

/// GET_RAM_SLOT_INFO_ALL's four fields.
///
/// total_count and active_slot are checked against what the device reports
/// through its own plugin API when the scenario starts, so the response is
/// judged against the device's actual RAM slot configuration rather than
/// against itself.  The ROM type must identify a ROM: 0xFF is "Invalid/ROM not
/// being served", and the device is serving one.
pub fn get_ram_slot_info_all(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.issue_cmd(&s, group::READ, read::GET_RAM_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_RAM_SLOT_INFO_ALL: {e}"))?;

    // active_slot is cross-checked against what the device reported through its
    // own plugin API at start-up — two paths through the device agreeing.
    //
    // total_count is not, and deliberately: "Total number of RAM slots
    // available on the device" is what the device chooses to make available,
    // and a plugin may legitimately keep some back — this one does, since RBCP
    // rejects 0xAA in every slot argument and a slot no host can name is no use
    // to a host.  So the count is asserted by what it *means* instead: every
    // slot it promises can be named, and the first index past it cannot.
    bus.expect_data(
        &s,
        1,
        &[ctx.active_ram_slot],
        "GET_RAM_SLOT_INFO_ALL response (active_slot)",
    )?;

    let total = bus.read_data(&s, 0, 1)?[0];
    if total == 0 {
        return Err("GET_RAM_SLOT_INFO_ALL reports no RAM slots at all".to_string());
    }

    // The last slot promised, and the first one past the promise.  SLOT_PEEK
    // reads the slot it names without changing anything, so it asks the
    // question without disturbing an image.
    bus.issue_cmd(
        &s,
        group::READ,
        read::SLOT_PEEK,
        &slot_peek_args(0, 1, total - 1),
    )
    .map_err(|e| {
        format!(
            "SLOT_PEEK of slot {}: {e} — the device advertises {total} RAM slot(s), so every \
             index below {total} must be one a host can name",
            total - 1
        )
    })?;

    bus.expect_rejected(
        &s,
        group::READ,
        read::SLOT_PEEK,
        &slot_peek_args(0, 1, total),
    )
    .map_err(|e| format!("{e} — slot {total} is the first index past the {total} advertised"))?;

    let rom_type = bus.read_data(&s, 2, 1)?[0];
    if rom_type == ROM_TYPE_INVALID {
        return Err(format!(
            "GET_RAM_SLOT_INFO_ALL: rom_type is 0x{ROM_TYPE_INVALID:02X}, the ROM Types table's \
             invalid / not being served value, while the device is serving slot {}",
            ctx.active_ram_slot
        ));
    }

    bus.expect_data(&s, 3, &[0x00], "GET_RAM_SLOT_INFO_ALL reserved byte")?;

    Ok(Outcome::Pass)
}

/// GET_DEVICE_TYPE writes 24 bytes of null-terminated ASCII.
///
/// "A device must provide a type", so the field cannot be empty.
pub fn get_device_type(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.issue_cmd(&s, group::READ, read::GET_DEVICE_TYPE, &[])
        .map_err(|e| format!("GET_DEVICE_TYPE: {e}"))?;

    let field = bus.read_data(&s, 0, 24)?;
    check_ascii_field(&field, true, "GET_DEVICE_TYPE response")?;

    Ok(Outcome::Pass)
}

/// GET_DEVICE_VERSION writes 24 bytes of null-terminated ASCII.
///
/// "A device must provide a version", so the field cannot be empty.
pub fn get_device_version(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.issue_cmd(&s, group::READ, read::GET_DEVICE_VERSION, &[])
        .map_err(|e| format!("GET_DEVICE_VERSION: {e}"))?;

    let field = bus.read_data(&s, 0, 24)?;
    check_ascii_field(&field, true, "GET_DEVICE_VERSION response")?;

    Ok(Outcome::Pass)
}

/// GET_PROTOCOL_VERSION reports a version these scenarios can judge.
///
/// The three version bytes and a reserved byte that "must be zero".  During the
/// 0.x series "a host implementation written against version 0.Y.z is
/// guaranteed to interoperate correctly with any device implementing version
/// 0.Y.w where w >= z" — so a device outside those bounds is not merely
/// different, it is one whose behaviour this suite has no standing to assert.
pub fn get_protocol_version(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.issue_cmd(&s, group::READ, read::GET_PROTOCOL_VERSION, &[])
        .map_err(|e| format!("GET_PROTOCOL_VERSION: {e}"))?;

    let v = bus.read_data(&s, 0, 4)?;
    if v[0] != SPEC_MAJOR || v[1] != SPEC_MINOR || v[2] < SPEC_PATCH {
        return Err(format!(
            "the device reports RBCP {}.{}.{}; these scenarios assert version \
             {SPEC_MAJOR}.{SPEC_MINOR}.{SPEC_PATCH}, and interoperation is guaranteed only for \
             {SPEC_MAJOR}.{SPEC_MINOR}.w with w >= {SPEC_PATCH}",
            v[0], v[1], v[2]
        ));
    }

    bus.expect_data(&s, 3, &[0x00], "GET_PROTOCOL_VERSION reserved byte")?;

    Ok(Outcome::Pass)
}

/// SLOT_PEEK returns the slot's bytes, across the device's chunking.
///
/// The requested bytes are written "into the response data section", so the
/// answer must be what the device serves at those same addresses — two
/// different paths through the device, agreeing.  The range is deliberately
/// awkward: it starts on an odd address and is 40 bytes long, so it crosses the
/// 32-byte boundary the device copies in and ends part way through the next
/// chunk.  Its first and last bytes are poked to values chosen here first, so
/// the comparison is against something known.
pub fn slot_peek(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = peek_fits(ctx, PEEK_LEN) {
        return Ok(skip);
    }
    let src = peek_src(ctx);
    let slot = ctx.active_ram_slot;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // Mark both ends of the range, so a device answering with the wrong region
    // — or with a stale copy of this one — cannot match by accident.
    let first = bus.read(src)? ^ 0xFF;
    let last = bus.read(src + PEEK_LEN - 1)? ^ 0x5A;
    bus.poke_slot_verified(&s, slot, src, first)?;
    bus.poke_slot_verified(&s, slot, src + PEEK_LEN - 1, last)?;

    let served = served_bytes(bus, src, PEEK_LEN)?;

    bus.issue_cmd(
        &s,
        group::READ,
        read::SLOT_PEEK,
        &slot_peek_args(src, PEEK_LEN as u8, slot),
    )
    .map_err(|e| format!("SLOT_PEEK of {PEEK_LEN} bytes from 0x{src:06X}: {e}"))?;

    bus.expect_data(&s, 0, &served, "SLOT_PEEK response")
        .map_err(|e| {
            format!(
                "{e} — the peeked bytes must be what the device serves from 0x{src:06X}, \
                 chunking included"
            )
        })?;

    Ok(Outcome::Pass)
}

/// A SLOT_PEEK count of zero means 256 bytes.
///
/// "A count of zero indicates 256 bytes should be read."  The discrimination is
/// at the far end of the range: the source's last byte is poked to one chosen
/// value and the data section byte it must land in to its inverse, both with
/// verified writes.  A device that read no bytes, or fewer than 256, leaves the
/// second value in place; only a device that read all 256 replaces it with the
/// first.
pub fn slot_peek_count_zero_is_256(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    const FULL: u32 = 256;

    if let Some(skip) = peek_fits(ctx, FULL) {
        return Ok(skip);
    }
    let src = peek_src(ctx);
    let slot = ctx.active_ram_slot;

    let s = ctx.session();
    if s.data_size() < FULL {
        return Ok(Outcome::Skip(format!(
            "the session's data section is {} bytes, too small for the 256 a zero count asks for",
            s.data_size()
        )));
    }
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let want = bus.read(src + FULL - 1)? ^ 0xFF;
    bus.poke_slot_verified(&s, slot, src + FULL - 1, want)?;

    // Arm the destination with the opposite value, so the byte can only become
    // `want` by the device having read the whole 256.
    let dst = s.bch_start + HDR_SIZE + FULL - 1;
    bus.poke_slot_verified(&s, slot, dst, !want)?;

    bus.issue_cmd(
        &s,
        group::READ,
        read::SLOT_PEEK,
        &slot_peek_args(src, 0, slot),
    )
    .map_err(|e| format!("SLOT_PEEK with a count of zero: {e}"))?;

    let served = served_bytes(bus, src, FULL)?;
    bus.expect_data(&s, 0, &served, "SLOT_PEEK response")
        .map_err(|e| format!("{e} — a count of zero must read 256 bytes from 0x{src:06X}"))?;

    Ok(Outcome::Pass)
}

/// SLOT_PEEK fails when the response data section is too small.
///
/// "This command fails if there is insufficient space in the response data
/// section to accommodate the requested bytes."  The source range is valid, so
/// the only thing wrong with the command is that its answer would not fit.
pub fn slot_peek_exceeding_data_section_fails(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    const REQUESTED: u8 = 32;

    if let Some(skip) = peek_fits(ctx, u32::from(REQUESTED)) {
        return Ok(skip);
    }
    let src = peek_src(ctx);

    let s = enter_sized(bus, ctx, (HDR_SIZE + 16) as u16)?;

    bus.expect_rejected(
        &s,
        group::READ,
        read::SLOT_PEEK,
        &slot_peek_args(src, REQUESTED, ctx.active_ram_slot),
    )
    .map_err(|e| {
        format!(
            "{e} — {REQUESTED} bytes were requested into a {}-byte data section",
            s.data_size()
        )
    })?;

    Ok(Outcome::Pass)
}

/// SLOT_PEEK must reject a slot argument of 0xAA.
///
/// "An A4 value of 0xAA is invalid and rejected."  Every other argument is
/// valid, so the rejection can only be on account of the 0xAA.
pub fn slot_peek_rejects_slot_aa(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    if let Some(skip) = peek_fits(ctx, PEEK_LEN) {
        return Ok(skip);
    }
    let src = peek_src(ctx);

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.expect_rejected(
        &s,
        group::READ,
        read::SLOT_PEEK,
        &slot_peek_args(src, PEEK_LEN as u8, 0xAA),
    )?;

    Ok(Outcome::Pass)
}

/// A Read command in command mode must not be acted on.
///
/// "All commands in this group are valid in command-response mode only", and
/// after EXIT_CMD_RESP_ACK "the device has exited command-response mode and the
/// back-channel region is no longer maintained".  A device that ran a Read
/// command anyway would write its answer into the region it has just stopped
/// maintaining.
///
/// So the byte GET_FLASH_SLOT_COUNT writes to is armed, in command mode, with a
/// value that is not the count; the Read command is then sent, knocked and well
/// formed, in command mode; a verified poke elsewhere fences it, proving the
/// device has processed a command since; and the byte must still hold the armed
/// value rather than the count.  Both are values this device writes routinely.
pub fn not_valid_in_command_mode(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // Learn the answer the device would write, so the two values can be told
    // apart afterwards.
    let count = flash_slot_count(bus, &s)?;

    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    let dst = s.bch_start + HDR_SIZE;
    let armed = count ^ 0xFF;
    bus.poke_verified(ctx, dst, armed)
        .map_err(|e| format!("arming the response data section: {e}"))?;

    // The stimulus: a properly knocked, well-formed Read command, in command
    // mode.  Nothing about it is malformed except the mode it arrives in.
    bus.knock(ctx.command_page())?;
    bus.send_cmd(
        ctx.command_page(),
        group::READ,
        read::GET_FLASH_SLOT_COUNT,
        &[],
    )?;

    bus.fence(ctx)?;

    let got = bus.read(dst)?;
    if got == armed {
        return Ok(Outcome::Pass);
    }
    if got == count {
        return Err(format!(
            "the device acted on GET_FLASH_SLOT_COUNT in command mode: 0x{dst:06X} serves \
             0x{got:02X}, the flash slot count, rather than the armed 0x{armed:02X} — the Read \
             group is valid in command-response mode only, and after EXIT_CMD_RESP_ACK the \
             back-channel region is no longer maintained"
        ));
    }
    Err(format!(
        "0x{dst:06X} serves 0x{got:02X}, which is neither the armed 0x{armed:02X} nor the flash \
         slot count 0x{count:02X} — something other than these two writes reached it"
    ))
}
