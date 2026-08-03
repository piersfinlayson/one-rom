// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Group 0x03 — NV Storage".
//!
//! Seven commands over a device's dedicated non-volatile storage: one that
//! reports the capability, one that reads, and five that make up a
//! transactional write — BEGIN loads NV storage into a RAM staging buffer,
//! POKE modifies bytes of it, and COMMIT or DISCARD resolves the transaction.
//! COMMIT_BYTE performs the whole of a single-byte transaction in one command.
//!
//! # How NV storage is observed
//!
//! NV storage is not served over the bus, so the host's only view of it is
//! NV_PEEK — a device-side read answered into the response data section.  A
//! host's only way to *write* it is a commit.
//!
//! That would leave every read scenario asserting the write command it is
//! trying to be independent of, so the harness seeds NV storage directly with
//! [`Bus::seed_nv`], standing in for what a previous boot's host left there.
//! Seeding is setup; every assertion is made through the device, over the bus.
//! The two are separate paths, so their agreement is a real statement.
//!
//! Every scenario starts with NV storage erased to 0xFF, which is what the
//! specification requires of a device no host has written to.
//!
//! # Writability
//!
//! "A RAM slot must be provided by the host for the device to use as a staging
//! area" and "if the device only supports a single RAM slot, it cannot perform
//! multiple write transactions and hence GET_NV_CAPABILITY reports any NV
//! storage as read-only."  That single sentence is the *only* thing the
//! specification says about the writable flag, so it is the only thing
//! [`get_nv_capability`] asserts about it.
//!
//! Everything else about writability is asserted as self-consistency instead.
//! How a device finds somewhere to stage a transaction is its own affair — it
//! may keep storage the host never sees, or it may need the slot the host names
//! to be big enough — and a suite that predicted which would be checking the
//! implementation against a copy of itself.  So
//! [`nv_capability_matches_behaviour`] holds the device to whatever it claims,
//! in both directions, and every other write scenario skips on that claim
//! knowing it has been pinned.
//!
//! # Asserting that a transaction is gone
//!
//! Several requirements are that a transaction was discarded, which is an
//! absence.  None of them is asserted by looking for one.  A discarded
//! transaction and a retained one are told apart positively, by what the device
//! does next: NV_POKE "fails if no write transaction is in progress" and
//! NV_POKE_BEGIN "fails if a write transaction is already in progress", so the
//! two commands answer in opposite directions and [`expect_no_transaction`]
//! requires both answers.  A device that merely wedged fails the second.

use crate::driver::{Bus, CmdFailure, HDR_SIZE, Session, control, group, nv, read};
use crate::{Ctx, Outcome};

/// "The NV storage address space is a maximum of 32KB.  The location MSB in
/// NV_PEEK and NV_POKE encodes the upper address bits; values above 0x7F are
/// invalid."
const MAX_LOCATION_MSB: u8 = 0x7F;

/// The value an erased NV storage reads as, before any host has written it.
const ERASED: u8 = 0xFF;

/// NV_PEEK arguments: "A0=count, A1=location_LSB, A2=location_MSB".
fn peek_args(count: u8, location: u32) -> [u8; 3] {
    [count, location as u8, (location >> 8) as u8]
}

/// NV_POKE arguments: "A0=byte, A1=location_LSB, A2=location_MSB".
fn poke_args(byte: u8, location: u32) -> [u8; 3] {
    [byte, location as u8, (location >> 8) as u8]
}

/// NV_POKE_COMMIT_BYTE arguments: "A0=byte, A1=location_LSB, A2=location_MSB,
/// A3=RAM slot".
fn commit_byte_args(byte: u8, location: u32, slot: u8) -> [u8; 4] {
    [byte, location as u8, (location >> 8) as u8, slot]
}

/// The number of RAM slots the device offers a host, read over the bus.
///
/// Not the firmware's count: a plugin may offer fewer than the hardware has,
/// keeping the rest for itself, and what a host may name is what the device
/// says it may name.
fn advertised_slots(bus: &mut Bus, s: &Session) -> Result<u8, String> {
    bus.issue_cmd(s, group::READ, read::GET_RAM_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_RAM_SLOT_INFO_ALL: {e}"))?;
    Ok(bus.read_data(s, 0, 1)?[0])
}

/// Whether the device says it can write NV storage, read over the bus.
fn advertised_writable(bus: &mut Bus, s: &Session) -> Result<bool, String> {
    bus.issue_cmd(s, group::NV_STORAGE, nv::GET_NV_CAPABILITY, &[])
        .map_err(|e| format!("GET_NV_CAPABILITY: {e}"))?;
    Ok(bus.read_data(s, 2, 1)?[0] != 0)
}

/// A RAM slot to name in a write transaction, if the device can perform one.
///
/// The device's own writable flag decides, because how it stages a transaction
/// is its business — it may lend itself slots the host never sees, or it may
/// need the one named here to be large enough, and predicting which would be
/// mirroring the implementation instead of testing it.
///
/// Trusting that flag is safe only because
/// [`nv_capability_matches_behaviour`] pins it to what the device actually
/// does, in both directions.  Without that, a device that wrongly claimed
/// read-only would turn this whole suite into skips and nothing would notice.
fn staging_slot(bus: &mut Bus, s: &Session, ctx: &Ctx) -> Result<Option<u8>, String> {
    let Some(slot) = ctx.inactive_ram_slot() else {
        return Ok(None);
    };
    Ok(advertised_writable(bus, s)?.then_some(slot))
}

/// Why a write scenario cannot run on this device.
fn needs_staging_slot(ctx: &Ctx) -> Outcome {
    if ctx.inactive_ram_slot().is_none() {
        return Outcome::Skip(format!(
            "the device has one RAM slot (a {} served from slot {}), so there is no slot to name \
             in a write transaction",
            ctx.chip_type.name(),
            ctx.active_ram_slot
        ));
    }
    Outcome::Skip(
        "the device reports NV storage read-only, so it has no write transaction to test"
            .to_string(),
    )
}

/// What GET_NV_CAPABILITY claims about writability is what the device does.
///
/// The specification constrains the flag in one direction only — "if the device
/// only supports a single RAM slot, it cannot perform multiple write
/// transactions and hence GET_NV_CAPABILITY reports any NV storage as
/// read-only" — and says nothing that lets a host predict it otherwise.  How a
/// device stages a transaction is its own business: it may lend itself storage
/// the host never sees, or it may need the slot the host names to be large
/// enough.
///
/// So rather than predict the flag, this requires the device to agree with
/// itself.  Whatever it claims, it is held to: a device claiming writable must
/// accept NV_POKE_BEGIN on at least one of the slots it advertises, and one
/// claiming read-only must refuse every last one of them.
///
/// # Why this scenario carries the rest of the suite
///
/// Every other write scenario here skips when the device reports read-only,
/// which is only safe because this pins the claim to observable behaviour in
/// both directions.  Without it a device that wrongly claimed read-only would
/// reduce the whole group to skips and nothing would say so; and one that
/// wrongly claimed writable — as this implementation did on a board whose RAM
/// slots are too small to stage in — would fail every transaction while
/// advertising that it could perform them.
pub fn nv_capability_matches_behaviour(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let writable = advertised_writable(bus, &s)?;
    let total = advertised_slots(bus, &s)?;
    if total == 0 {
        return Err("GET_RAM_SLOT_INFO_ALL reports no RAM slots at all".to_string());
    }

    // Every slot the device offers, except the one it is serving — which
    // NV_POKE_BEGIN must refuse whatever its writability.
    let mut accepted = Vec::new();
    for slot in (0..total).filter(|&slot| slot != ctx.active_ram_slot) {
        match bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[slot]) {
            Ok(()) => {
                accepted.push(slot);
                bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_DISCARD, &[])
                    .map_err(|e| format!("NV_POKE_DISCARD after staging in slot {slot}: {e}"))?;
            }
            Err(CmdFailure::Failed) => (),
            Err(e) => return Err(format!("NV_POKE_BEGIN with slot {slot}: {e}")),
        }
    }

    if writable && accepted.is_empty() {
        return Err(format!(
            "GET_NV_CAPABILITY reports NV storage writable, but NV_POKE_BEGIN was refused for \
             every one of the {total} RAM slot(s) the device advertises — a host told it may \
             write has no way left to do so"
        ));
    }
    if !writable && !accepted.is_empty() {
        return Err(format!(
            "GET_NV_CAPABILITY reports NV storage read-only, but NV_POKE_BEGIN was accepted for \
             slot(s) {accepted:?} — a host told it may not write is being allowed to start a \
             transaction anyway"
        ));
    }

    Ok(Outcome::Pass)
}

/// A device reporting NV storage read-only must refuse to write it.
///
/// "Fails if NV storage is not writable, if a write transaction is already in
/// progress, or if the RAM slot specified is invalid, active or too small."
/// Writability is listed first, and it is the only one of the three a host can
/// establish in advance — GET_NV_CAPABILITY tells it so.
///
/// The byte asked for is one NV storage already holds, which is the case a
/// device is most tempted to answer cheaply: nothing needs writing, so nothing
/// is at risk.  But a host that gets status-OK from a write command has been
/// told the write happened, and on a read-only device no write can have
/// happened — the next one, with a byte that differs, will fail.  A host cannot
/// tell those two apart from the response, so the specification requires the
/// same answer for both.
///
/// Runs only where the device reports NV storage read-only.
pub fn nv_poke_commit_byte_refused_when_read_only(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x44;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    if advertised_writable(bus, &s)? {
        return Ok(Outcome::Skip(
            "the device reports NV storage writable, so this rule does not apply to it".to_string(),
        ));
    }

    const VALUE: u8 = 0x71;
    bus.seed_nv(LOCATION, &[VALUE]);

    bus.expect_rejected(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE_COMMIT_BYTE,
        &commit_byte_args(VALUE, LOCATION, 0),
    )
    .map_err(|e| {
        format!(
            "{e} — GET_NV_CAPABILITY reports NV storage read-only, and NV_POKE_COMMIT_BYTE \
             \"fails if NV storage is not writable\".  The byte asked for is the one already at \
             that location, so a device short-circuiting on an unchanged byte answers status-OK \
             here — which tells the host a write succeeded on a device that cannot write"
        )
    })?;

    Ok(Outcome::Pass)
}

/// Read `len` bytes of NV storage with NV_PEEK and return them.
fn nv_peek(bus: &mut Bus, s: &Session, location: u32, len: u8) -> Result<Vec<u8>, String> {
    bus.issue_cmd(s, group::NV_STORAGE, nv::NV_PEEK, &peek_args(len, location))
        .map_err(|e| format!("NV_PEEK of {len} byte(s) from 0x{location:04X}: {e}"))?;
    bus.read_data(s, 0, if len == 0 { 256 } else { u32::from(len) })
}

/// Peek NV storage and require it to hold `want`, saying why if it does not.
fn expect_nv(
    bus: &mut Bus,
    s: &Session,
    location: u32,
    want: &[u8],
    why: &str,
) -> Result<(), String> {
    let got = nv_peek(bus, s, location, want.len() as u8)?;
    if got != want {
        return Err(format!(
            "NV_PEEK of 0x{location:04X} read {got:02X?}, expected {want:02X?} — {why}; the \
             device's own view of that range is {:02X?}",
            bus.nv_bytes(location, want.len())
        ));
    }
    Ok(())
}

/// Require that no write transaction is in progress.
///
/// Positive on both sides, and neither half alone would do.  NV_POKE "fails if
/// no write transaction is in progress", so its failure says a transaction is
/// gone — but so would a device that had wedged, or that rejects every NV
/// command.  NV_POKE_BEGIN "fails if a write transaction is already in
/// progress", so its *success* says the device is alive, taking NV commands,
/// and had nothing staged.  The transaction that opens is then closed again, so
/// this leaves the device as it found it.
fn expect_no_transaction(bus: &mut Bus, s: &Session, slot: u8, what: &str) -> Result<(), String> {
    bus.expect_rejected(s, group::NV_STORAGE, nv::NV_POKE, &poke_args(0x5A, 0))
        .map_err(|e| {
            format!("{e} — {what}, so NV_POKE must fail with no write transaction in progress")
        })?;

    bus.issue_cmd(s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[slot])
        .map_err(|e| {
            format!(
                "NV_POKE_BEGIN after {what}: {e} — a transaction that was not discarded would \
                 make this fail, and a device that had merely stopped answering would too"
            )
        })?;
    bus.issue_cmd(s, group::NV_STORAGE, nv::NV_POKE_DISCARD, &[])
        .map_err(|e| format!("NV_POKE_DISCARD tidying up after {what}: {e}"))?;
    Ok(())
}

/// GET_NV_CAPABILITY reports the size, and the one thing the specification
/// says about writability.
///
/// The size is checked against the storage the harness stands in for, which is
/// independent of anything the device computes.  The reserved byte "must be
/// zero".
///
/// The writable flag is *only* checked where the specification constrains it:
/// "if the device only supports a single RAM slot, it cannot perform multiple
/// write transactions and hence GET_NV_CAPABILITY reports any NV storage as
/// read-only."  Nothing licenses a host to predict it otherwise — a device with
/// several slots may still have good reason to say read-only — so the other
/// direction is left to [`nv_capability_matches_behaviour`], which requires the
/// device to be consistent with itself rather than with a guess made here.
///
/// The slot count is read back over the bus, not taken from the firmware's own
/// count: what a host may name is what the device tells it, and a plugin may
/// legitimately offer fewer slots than the hardware has.
pub fn get_nv_capability(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let total = advertised_slots(bus, &s)?;

    bus.issue_cmd(&s, group::NV_STORAGE, nv::GET_NV_CAPABILITY, &[])
        .map_err(|e| format!("GET_NV_CAPABILITY: {e}"))?;

    bus.expect_data(
        &s,
        0,
        &[ctx.nv_size as u8, (ctx.nv_size >> 8) as u8],
        "GET_NV_CAPABILITY size",
    )?;
    bus.expect_data(&s, 3, &[0x00], "GET_NV_CAPABILITY reserved byte")?;

    if total == 1 {
        bus.expect_data(&s, 2, &[0x00], "GET_NV_CAPABILITY writable")
            .map_err(|e| {
                format!(
                    "{e} — the device advertises a single RAM slot, and one with a single slot \
                     cannot free it to stage a transaction in, so it must report NV storage \
                     read-only"
                )
            })?;
    }

    Ok(Outcome::Pass)
}

/// Untouched NV storage reads as 0xFF.
///
/// "Before having been written by any host, the entire NV storage on any device
/// is initialized to 0xFF."  Read at both ends and across a boundary in the
/// middle, so a device answering from the wrong region — or from a partially
/// initialised one — is caught.
///
/// On a device this is a property of erased flash; here the harness erases its
/// stand-in before every scenario, so what this asserts of the device is that
/// it reads that storage back faithfully rather than substituting anything of
/// its own.
pub fn erased_storage_reads_as_ff(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    const LEN: u8 = 32;
    for location in [0, ctx.nv_size / 2 - 1, ctx.nv_size - u32::from(LEN)] {
        expect_nv(
            bus,
            &s,
            location,
            &[ERASED; LEN as usize],
            "NV storage that no host has written to is 0xFF throughout",
        )?;
    }

    Ok(Outcome::Pass)
}

/// NV_PEEK answers from NV storage, at the location and length asked for.
///
/// "Reads one or more bytes directly from NV storage at the specified location
/// and writes them into the response data section."  The range is deliberately
/// awkward — an odd location, and a length that is neither a power of two nor a
/// multiple of any plausible chunk — and its two ends carry different values
/// from its middle, so a device answering with the right length from the wrong
/// place cannot match.
pub fn nv_peek_reads_storage(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x101;
    const LEN: usize = 37;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // A pattern whose every byte differs from its neighbours and from 0xFF, so
    // an off-by-one in either direction shows up.
    let want: Vec<u8> = (0..LEN).map(|i| (i as u8).wrapping_mul(7) ^ 0x5A).collect();
    bus.seed_nv(LOCATION, &want);

    expect_nv(
        bus,
        &s,
        LOCATION,
        &want,
        "NV_PEEK reads directly from NV storage at the location it names",
    )?;

    // The byte below the range must not have come along with it: 0xFF is what
    // the erase left there, and the pattern deliberately never contains it.
    expect_nv(
        bus,
        &s,
        LOCATION - 1,
        &[ERASED],
        "the byte below the seeded range was not written, so NV_PEEK must not shift its answer",
    )?;

    Ok(Outcome::Pass)
}

/// A count of zero means 256 bytes.
///
/// "A count of zero indicates 256 bytes should be read."  The 256th byte is
/// given a value of its own, so a device reading 255 — or treating zero as
/// zero — is caught on the byte that distinguishes them.
pub fn nv_peek_count_zero_is_256(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x40;
    const LEN: usize = 256;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    if s.data_size() < LEN as u32 {
        return Ok(Outcome::Skip(format!(
            "a 256-byte peek does not fit in this session's {}-byte data section",
            s.data_size()
        )));
    }

    let want: Vec<u8> = (0..LEN).map(|i| (i as u8) ^ 0x3C).collect();
    bus.seed_nv(LOCATION, &want);

    let got = nv_peek(bus, &s, LOCATION, 0)?;
    if got != want {
        let i = (0..LEN).find(|&i| got[i] != want[i]).unwrap_or(0);
        return Err(format!(
            "a count of zero read byte {i} as 0x{:02X}, expected 0x{:02X} — a count of zero \
             indicates 256 bytes, and byte 255 is 0x{:02X}",
            got[i], want[i], want[255]
        ));
    }

    Ok(Outcome::Pass)
}

/// A location MSB above 0x7F is rejected.
///
/// "The location MSB must not exceed 0x7F; if it does, the device rejects the
/// command."  0x80 is the smallest violation; 0xAA is the one the rule exists
/// for — "this constraint ensures that 0xAA is always detectable as a reset
/// signal in the final argument position of both commands", and A2 is NV_PEEK's
/// final argument.
///
/// # What this cannot establish
///
/// Only that such a location is refused, not that it is refused *by this rule*.
/// An MSB of 0x80 puts the location at 32768 or above, and "the NV storage
/// address space is a maximum of 32KB", so every such location is also out of
/// range — on this device and on any conformant one.  A device that dropped the
/// MSB check entirely and kept only the range check passes here, and this
/// scenario was confirmed not to catch that.
///
/// That is a property of the specification rather than a gap in the scenario:
/// the two rules cannot be separated by any host, so what a host may rely on is
/// exactly what is asserted here.  The MSB rule earns its place by making the
/// refusal cheap and certain in the argument position where 0xAA must stay
/// detectable, not by forbidding anything the range rule permits.
pub fn nv_peek_rejects_location_msb_above_7f(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    for msb in [MAX_LOCATION_MSB + 1, 0xAA] {
        let bad = u32::from(msb) << 8;
        bus.expect_rejected(&s, group::NV_STORAGE, nv::NV_PEEK, &peek_args(1, bad))
            .map_err(|e| format!("{e} — the location MSB was 0x{msb:02X}"))?;
    }

    // The largest MSB the rule permits must still be accepted where the
    // location is in range, so the refusals above are about the MSB and not
    // about NV_PEEK refusing every location it is given.
    nv_peek(bus, &s, ctx.nv_size - 1, 1)
        .map_err(|e| format!("{e} — the last location in NV storage is in range"))?;

    Ok(Outcome::Pass)
}

/// A peek running past the end of NV storage fails.
///
/// "Fails if ... the requested range exceeds the NV storage size."  The range
/// overruns by one byte, so a bound that is off by one is caught rather than
/// only a bound that is missing; the same peek one byte lower must succeed, so
/// the failure is attributable to the overrun and not to the length.
pub fn nv_peek_beyond_storage_fails(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    const LEN: u8 = 16;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let last_ok = ctx.nv_size - u32::from(LEN);
    nv_peek(bus, &s, last_ok, LEN)
        .map_err(|e| format!("{e} — a peek ending exactly at the end of NV storage is in range"))?;

    bus.expect_rejected(
        &s,
        group::NV_STORAGE,
        nv::NV_PEEK,
        &peek_args(LEN, last_ok + 1),
    )
    .map_err(|e| {
        format!(
            "{e} — {LEN} bytes from 0x{:04X} runs one byte past this device's {}-byte NV storage",
            last_ok + 1,
            ctx.nv_size
        )
    })?;

    Ok(Outcome::Pass)
}

/// A peek larger than the response data section fails.
///
/// "Fails if there is insufficient space in the response data section to
/// accommodate the requested bytes."  The session is sized so that a peek one
/// byte longer than the section overruns it, and the peek that exactly fills
/// the section must succeed — so the failure is about the space and not about
/// the count.
pub fn nv_peek_exceeding_data_section_fails(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    /// A data section small enough that both peeks fit in a byte count.
    const BCH_SIZE: u16 = HDR_SIZE as u16 + 32;

    let s = Session {
        bch_size: BCH_SIZE,
        ..ctx.session()
    };
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP with a {BCH_SIZE}-byte back-channel: {e}"))?;

    let fits = s.data_size() as u8;
    nv_peek(bus, &s, 0, fits)
        .map_err(|e| format!("{e} — {fits} bytes exactly fills the response data section"))?;

    bus.expect_rejected(&s, group::NV_STORAGE, nv::NV_PEEK, &peek_args(fits + 1, 0))
        .map_err(|e| {
            format!(
                "{e} — {} bytes is one more than the {fits}-byte response data section holds",
                fits + 1
            )
        })?;

    Ok(Outcome::Pass)
}

/// A transaction begins, stages a change, and is discarded.
///
/// "NV_POKE_DISCARD ... discards the staging buffer without writing to NV
/// storage and frees the staging buffer."  Both halves are asserted: NV storage
/// still reads what was seeded — discriminated against the value the poke
/// staged, which is a value the device would write on a commit — and the
/// transaction is gone, positively, by what the device does next.
pub fn nv_poke_discard_abandons_the_transaction(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x20;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let Some(slot) = staging_slot(bus, &s, ctx)? else {
        return Ok(needs_staging_slot(ctx));
    };

    let seeded = 0x5Au8;
    let staged = 0xA5u8;
    bus.seed_nv(LOCATION, &[seeded]);

    bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[slot])
        .map_err(|e| format!("NV_POKE_BEGIN with staging slot {slot}: {e}"))?;
    bus.issue_cmd(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE,
        &poke_args(staged, LOCATION),
    )
    .map_err(|e| format!("NV_POKE of 0x{staged:02X}: {e}"))?;
    bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_DISCARD, &[])
        .map_err(|e| format!("NV_POKE_DISCARD: {e}"))?;

    expect_nv(
        bus,
        &s,
        LOCATION,
        &[seeded],
        "a discarded transaction is not written to NV storage, so the location must still hold \
         the seeded value rather than the staged one",
    )?;
    expect_no_transaction(bus, &s, slot, "NV_POKE_DISCARD frees the staging buffer")?;

    Ok(Outcome::Pass)
}

/// Only one write transaction may be in progress at a time.
///
/// "Only one write transaction may be in progress at a time", and NV_POKE_BEGIN
/// "fails if ... a write transaction is already in progress".  The second BEGIN
/// names the same slot the first accepted, so the only thing that can make the
/// device refuse it is the transaction already open.
pub fn nv_poke_begin_rejects_a_second_transaction(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let Some(slot) = staging_slot(bus, &s, ctx)? else {
        return Ok(needs_staging_slot(ctx));
    };

    bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[slot])
        .map_err(|e| format!("first NV_POKE_BEGIN with staging slot {slot}: {e}"))?;
    bus.expect_rejected(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[slot])
        .map_err(|e| format!("{e} — a transaction with slot {slot} is already in progress"))?;

    Ok(Outcome::Pass)
}

/// NV_POKE_BEGIN refuses the slot being served, and a slot of 0xAA.
///
/// "Fails if ... the RAM slot specified is invalid, active or too small", and
/// "an A0 value of 0xAA is invalid and rejected".  The active slot is the one
/// case a host might plausibly get wrong, since the specification warns that
/// "any RAM slot specified will be overwritten by the device": staging into the
/// slot being served would destroy the ROM the machine is running from.
pub fn nv_poke_begin_rejects_bad_slots(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.expect_rejected(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE_BEGIN,
        &[ctx.active_ram_slot],
    )
    .map_err(|e| {
        format!(
            "{e} — slot {} is the slot being served, and staging into it would overwrite the \
             ROM the host is running from",
            ctx.active_ram_slot
        )
    })?;

    bus.expect_rejected(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[0xAA])
        .map_err(|e| format!("{e} — an A0 value of 0xAA is invalid and rejected"))?;

    let total = advertised_slots(bus, &s)?;
    bus.expect_rejected(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[total])
        .map_err(|e| {
            format!("{e} — slot {total} is one past the {total} RAM slot(s) the device advertises")
        })?;

    Ok(Outcome::Pass)
}

/// NV_POKE and NV_POKE_DISCARD fail with no transaction in progress.
///
/// Both are specified the same way — "fails if no write transaction is in
/// progress" — and neither has begun one here.  A fresh session is the cleanest
/// statement of that: the device has just entered command-response mode and
/// nothing has staged anything.
pub fn nv_poke_and_discard_need_a_transaction(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.expect_rejected(&s, group::NV_STORAGE, nv::NV_POKE, &poke_args(0x5A, 0))
        .map_err(|e| format!("{e} — no write transaction has been begun"))?;
    bus.expect_rejected(&s, group::NV_STORAGE, nv::NV_POKE_DISCARD, &[])
        .map_err(|e| format!("{e} — no write transaction has been begun"))?;

    Ok(Outcome::Pass)
}

/// NV_POKE rejects a location MSB above 0x7F, and one past the end.
///
/// "The location MSB must not exceed 0x7F; if it does, the device rejects the
/// command.  Fails if ... the location exceeds the NV storage size."  A poke
/// inside the storage is made first, so the rejections below are attributable
/// to the location rather than to the transaction.
///
/// As with [`nv_peek_rejects_location_msb_above_7f`], the two rules cannot be
/// told apart by a host: an MSB above 0x7F puts the location beyond the 32KB
/// the address space is capped at, so it is out of range too.  Both are
/// asserted because both are what a host may rely on.
pub fn nv_poke_rejects_bad_locations(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let Some(slot) = staging_slot(bus, &s, ctx)? else {
        return Ok(needs_staging_slot(ctx));
    };

    bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[slot])
        .map_err(|e| format!("NV_POKE_BEGIN with staging slot {slot}: {e}"))?;

    bus.issue_cmd(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE,
        &poke_args(0x5A, ctx.nv_size - 1),
    )
    .map_err(|e| format!("NV_POKE at the last location in NV storage: {e} — that is in range"))?;

    bus.expect_rejected(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE,
        &poke_args(0x5A, u32::from(MAX_LOCATION_MSB + 1) << 8),
    )
    .map_err(|e| format!("{e} — the location MSB was 0x{:02X}", MAX_LOCATION_MSB + 1))?;

    bus.expect_rejected(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE,
        &poke_args(0x5A, ctx.nv_size),
    )
    .map_err(|e| {
        format!(
            "{e} — location 0x{:04X} is one past this device's {}-byte NV storage",
            ctx.nv_size, ctx.nv_size
        )
    })?;

    Ok(Outcome::Pass)
}

/// NV_PEEK reads NV storage even while a transaction is staged over it.
///
/// "NV_PEEK always reads directly from NV storage, regardless of whether a
/// write transaction is in progress.  This allows the host to inspect the
/// actual state of NV storage after a failed commit."
///
/// So the location is seeded with one value, a transaction stages a different
/// one over it, and the peek must answer with the first.  Both are values the
/// device writes routinely — the staged one is exactly what a commit would put
/// there — so this discriminates rather than merely finding a byte unchanged,
/// and the NV_POKE the device completed is itself the fence.
pub fn nv_peek_reads_storage_during_a_transaction(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x33;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let Some(slot) = staging_slot(bus, &s, ctx)? else {
        return Ok(needs_staging_slot(ctx));
    };

    let in_storage = 0x11u8;
    let staged = 0x22u8;
    bus.seed_nv(LOCATION, &[in_storage]);

    bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[slot])
        .map_err(|e| format!("NV_POKE_BEGIN with staging slot {slot}: {e}"))?;
    bus.issue_cmd(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE,
        &poke_args(staged, LOCATION),
    )
    .map_err(|e| format!("NV_POKE of 0x{staged:02X}: {e}"))?;

    expect_nv(
        bus,
        &s,
        LOCATION,
        &[in_storage],
        "NV_PEEK always reads directly from NV storage, so a byte staged but not committed must \
         not appear in its answer",
    )?;

    Ok(Outcome::Pass)
}

/// Leaving command-response mode discards a staged transaction, by every route.
///
/// "If command-response mode exits for any reason while a write transaction is
/// in progress — whether via EXIT_CMD_RESP_ACK, EXIT_CMD_RESP_SILENT,
/// SWITCH_AND_EXIT, or RBCP_RESET — the device silently discards the staging
/// buffer.  Exit commands are never rejected on account of an in-progress
/// transaction.  RBCP_RESET in particular must unconditionally discard any
/// in-progress transaction, as it is a recovery mechanism."
///
/// All four routes are exercised in one scenario because they are one
/// requirement, and the assertion for each is the same: re-enter, then require
/// that no transaction is in progress — positively, from what the device does
/// next rather than from the absence of anything.
///
/// SWITCH_AND_EXIT names the slot already active, so the back-channel the
/// re-entry uses stays where this scenario can read it.
pub fn exiting_command_response_mode_discards_the_transaction(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let Some(slot) = staging_slot(bus, &s, ctx)? else {
        return Ok(needs_staging_slot(ctx));
    };

    // Each case below opens its own session, so leave the one that was needed
    // to ask the device whether it can write at all.
    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK after the capability check: {e}"))?;

    for (what, group_byte, cmd, args) in [
        (
            "EXIT_CMD_RESP_ACK",
            group::CONTROL,
            control::EXIT_CMD_RESP_ACK,
            &[][..],
        ),
        (
            "EXIT_CMD_RESP_SILENT",
            group::CONTROL,
            control::EXIT_CMD_RESP_SILENT,
            &[][..],
        ),
        (
            "SWITCH_AND_EXIT",
            group::CONTROL,
            control::SWITCH_AND_EXIT,
            &[ctx.active_ram_slot][..],
        ),
        ("RBCP_RESET", group::RESET, 0xAA, &[][..]),
    ] {
        bus.enter_cmd_resp(&s)
            .map_err(|e| format!("ENTER_CMD_RESP before the {what} case: {e}"))?;

        bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[slot])
            .map_err(|e| format!("NV_POKE_BEGIN before {what}: {e}"))?;

        // Sent, not polled.  Three of the four are silent, and the fourth is
        // the exit whose completion this scenario is not what is under test.
        bus.send_cmd(s.command_page, group_byte, cmd, args)?;

        bus.enter_cmd_resp(&s).map_err(|e| {
            format!(
                "ENTER_CMD_RESP after {what}: {e} — exit commands are never rejected on account \
                 of an in-progress transaction, so the device must have left the mode"
            )
        })?;

        expect_no_transaction(
            bus,
            &s,
            slot,
            &format!("{what} silently discards the staging buffer"),
        )?;

        bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
            .map_err(|e| format!("tidying up after the {what} case: {e}"))?;
    }

    Ok(Outcome::Pass)
}

/// NV_POKE_COMMIT_BYTE returns early when the byte already holds that value.
///
/// The specification does not require this, but it warns that a commit "is
/// likely to involve the device erasing flash — which is a long (ms)
/// operation", and this device avoids it where there is nothing to write.
///
/// What makes that assertable is that the device having touched flash at all
/// is visible: the harness stands in for the bootrom's flash routines and
/// counts the calls.  So this is not "the byte still holds its value" — which
/// would be equally true of a device that erased and reprogrammed it — but
/// that the erase and program never happened.  The location is seeded first,
/// so the byte the device matches against is one this scenario chose.
pub fn nv_poke_commit_byte_returns_early_for_an_unchanged_byte(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x77;
    const VALUE: u8 = 0x6C;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let Some(slot) = staging_slot(bus, &s, ctx)? else {
        return Ok(needs_staging_slot(ctx));
    };

    bus.seed_nv(LOCATION, &[VALUE]);
    expect_nv(
        bus,
        &s,
        LOCATION,
        &[VALUE],
        "the location is seeded with the value the commit will ask for",
    )?;

    bus.issue_cmd(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE_COMMIT_BYTE,
        &commit_byte_args(VALUE, LOCATION, slot),
    )
    .map_err(|e| format!("NV_POKE_COMMIT_BYTE of a byte that is already 0x{VALUE:02X}: {e}"))?;

    let log = bus.flash_log();
    if log.erase_calls != 0 || log.program_calls != 0 {
        return Err(format!(
            "NV_POKE_COMMIT_BYTE of a byte already holding 0x{VALUE:02X} performed {} erase(s) \
             and {} program(s) — the byte was unchanged, so it had nothing to write, and a \
             commit \"is likely to involve the device erasing flash, which is a long (ms) \
             operation\"",
            log.erase_calls, log.program_calls
        ));
    }

    expect_nv(
        bus,
        &s,
        LOCATION,
        &[VALUE],
        "the byte was already this value, so the commit had nothing to change",
    )?;
    expect_no_transaction(
        bus,
        &s,
        slot,
        "NV_POKE_COMMIT_BYTE leaves no transaction open",
    )?;

    Ok(Outcome::Pass)
}

/// NV_POKE_COMMIT writes the staging buffer to NV storage and frees it.
///
/// "Commits the staging buffer to NV storage and frees the staging buffer."
/// Three bytes are staged — the first of the region, the last, and one in the
/// middle — so a commit that wrote a prefix, a page, or a single byte is caught
/// on one of the others; every one of them is given a value that differs both
/// from what NV storage held and from the 0xFF an erase leaves, so neither a
/// commit that did nothing nor one that only erased can match.
///
/// The bytes are read back with NV_PEEK, over the bus, rather than out of the
/// harness's own model of the storage.
pub fn nv_poke_commit_writes_the_staging_buffer(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let Some(slot) = staging_slot(bus, &s, ctx)? else {
        return Ok(needs_staging_slot(ctx));
    };

    // Distinct values at distinct places, none of them 0xFF and none of them
    // what the location already holds.
    let staged = [
        (0u32, 0xC3u8),
        (ctx.nv_size / 2, 0x7Eu8),
        (ctx.nv_size - 1, 0x19u8),
    ];
    for (location, value) in staged {
        bus.seed_nv(location, &[value ^ 0xFF]);
    }

    bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[slot])
        .map_err(|e| format!("NV_POKE_BEGIN with staging slot {slot}: {e}"))?;
    for (location, value) in staged {
        bus.issue_cmd(
            &s,
            group::NV_STORAGE,
            nv::NV_POKE,
            &poke_args(value, location),
        )
        .map_err(|e| format!("NV_POKE of 0x{value:02X} at 0x{location:04X}: {e}"))?;
    }

    bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_COMMIT, &[])
        .map_err(|e| format!("NV_POKE_COMMIT: {e}"))?;

    for (location, value) in staged {
        expect_nv(
            bus,
            &s,
            location,
            &[value],
            "NV_POKE_COMMIT writes the whole staging buffer to NV storage",
        )?;
    }

    expect_no_transaction(bus, &s, slot, "NV_POKE_COMMIT frees the staging buffer")?;

    Ok(Outcome::Pass)
}

/// A commit erases before it programs, and restores XIP before it does.
///
/// Not a requirement of the protocol — it is a requirement of the hardware, and
/// the reason the specification warns that a commit "is likely to involve the
/// device erasing flash".  Flash can only clear bits, so a program over
/// unerased storage yields the bitwise AND of the two rather than what was
/// asked for; and the routine that performs the erase runs with the flash
/// unreadable, so a program issued before XIP is restored reads its source from
/// a bus that is not answering.
///
/// Neither fault shows in the committed bytes when the staged value happens to
/// be a subset of the bits already there, so the sequence is asserted directly:
/// the harness's model refuses a call that arrives in the wrong state and says
/// so.  The staged value is chosen to *set* a bit that NV storage does not
/// have, so the outcome catches a missing erase as well.
pub fn nv_poke_commit_erases_before_programming(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x2A;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let Some(slot) = staging_slot(bus, &s, ctx)? else {
        return Ok(needs_staging_slot(ctx));
    };

    // 0x0F is already there; 0xF0 shares none of its bits, so programming it
    // without erasing first would leave 0x00.
    bus.seed_nv(LOCATION, &[0x0F]);

    bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_BEGIN, &[slot])
        .map_err(|e| format!("NV_POKE_BEGIN with staging slot {slot}: {e}"))?;
    bus.issue_cmd(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE,
        &poke_args(0xF0, LOCATION),
    )
    .map_err(|e| format!("NV_POKE: {e}"))?;
    bus.issue_cmd(&s, group::NV_STORAGE, nv::NV_POKE_COMMIT, &[])
        .map_err(|e| format!("NV_POKE_COMMIT: {e}"))?;

    let log = bus.flash_log();
    if log.bad_erase != 0 {
        return Err(
            "the erase arrived with XIP still active, or named a range outside the NV region — \
             the erase routine runs with flash unreadable and must be entered only after \
             flash_exit_xip"
                .to_string(),
        );
    }
    if log.bad_program != 0 {
        return Err(
            "the program arrived before XIP was restored, or named a range outside the NV \
             region — flash_range_program reads its source over a bus that is not answering \
             until flash_select_xip_read_mode has run"
                .to_string(),
        );
    }
    if log.erase_calls != 1 || log.program_calls != 1 {
        return Err(format!(
            "the commit made {} erase(s) and {} program(s); it must make exactly one of each",
            log.erase_calls, log.program_calls
        ));
    }

    // The order, not merely the presence.  A device that programmed before
    // restoring XIP would produce exactly the right bytes here and fail on
    // hardware, because flash_range_program reads its source over a bus that
    // is not answering yet.
    let sequence = [
        ("connect_internal_flash", log.connect_seq),
        ("flash_exit_xip", log.exit_xip_seq),
        ("flash_range_erase", log.erase_seq),
        ("flash_select_xip_read_mode", log.select_xip_seq),
        ("flash_range_program", log.program_seq),
    ];
    if let Some(w) = sequence.windows(2).find(|w| w[0].1 >= w[1].1) {
        return Err(format!(
            "the commit called {} before {} — the sequence must be {}, because the erase runs \
             with flash unreadable and the program reads its source back over XIP",
            w[1].0,
            w[0].0,
            sequence
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(" → ")
        ));
    }
    if log.select_xip_clkdiv != u32::from(bus.xip_clkdiv()) {
        return Err(format!(
            "XIP was restored with a clock divisor of {}, not the {} that was in force before \
             the erase — the divisor is read before XIP is disabled precisely so that it can be \
             put back",
            log.select_xip_clkdiv,
            bus.xip_clkdiv()
        ));
    }
    if log.staged_fn_addr == 0 {
        return Err(
            "the commit never formed a pointer to a staged erase routine — the routine has to be \
             copied out of flash before flash becomes unreadable, so calling it in place would \
             fault on a device"
                .to_string(),
        );
    }

    expect_nv(
        bus,
        &s,
        LOCATION,
        &[0xF0],
        "0x0F was already there and shares no bits with 0xF0, so a program without a preceding \
         erase would leave 0x00",
    )?;

    Ok(Outcome::Pass)
}

/// NV_POKE_COMMIT fails with no transaction in progress.
///
/// "Fails if no write transaction is in progress."  A device that committed
/// anyway would erase and reprogram NV storage from a staging buffer that
/// holds whatever the last slot user left there, so the flash calls are
/// required to be absent as well as the command required to fail.
pub fn nv_poke_commit_needs_a_transaction(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x55;
    const VALUE: u8 = 0x81;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.seed_nv(LOCATION, &[VALUE]);

    bus.expect_rejected(&s, group::NV_STORAGE, nv::NV_POKE_COMMIT, &[])
        .map_err(|e| format!("{e} — no write transaction has been begun"))?;

    let log = bus.flash_log();
    if log.erase_calls != 0 || log.program_calls != 0 {
        return Err(format!(
            "a commit with no transaction in progress performed {} erase(s) and {} program(s) \
             — there was no staging buffer to write",
            log.erase_calls, log.program_calls
        ));
    }
    expect_nv(
        bus,
        &s,
        LOCATION,
        &[VALUE],
        "a rejected commit writes nothing, so the seeded byte must survive it",
    )?;

    Ok(Outcome::Pass)
}

/// NV_POKE_COMMIT_BYTE performs the whole of a single-byte transaction.
///
/// "For the common case of updating a single byte, NV_POKE_COMMIT_BYTE performs
/// the full transaction — BEGIN, POKE, COMMIT — as a single command."  So the
/// byte lands in NV storage without any of the three being issued separately,
/// the neighbouring bytes come through the erase-and-reprogram unchanged — which
/// is the whole point of staging the region rather than writing one byte — and
/// no transaction is left open.
pub fn nv_poke_commit_byte_performs_the_whole_transaction(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x1C0;
    const VALUE: u8 = 0x5B;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let Some(slot) = staging_slot(bus, &s, ctx)? else {
        return Ok(needs_staging_slot(ctx));
    };

    // The target and its neighbours, all different, and none of them the value
    // about to be written.
    let neighbours: Vec<u8> = (0..8u8).map(|i| i.wrapping_mul(0x11) ^ 0x24).collect();
    bus.seed_nv(LOCATION - 4, &neighbours);
    bus.seed_nv(LOCATION, &[VALUE ^ 0xFF]);

    bus.issue_cmd(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE_COMMIT_BYTE,
        &commit_byte_args(VALUE, LOCATION, slot),
    )
    .map_err(|e| format!("NV_POKE_COMMIT_BYTE of 0x{VALUE:02X} at 0x{LOCATION:04X}: {e}"))?;

    let mut want = neighbours.clone();
    want[4] = VALUE;
    expect_nv(
        bus,
        &s,
        LOCATION - 4,
        &want,
        "NV_POKE_COMMIT_BYTE stages the whole region, so its neighbours survive the erase and \
         reprogram that the one byte requires",
    )?;

    expect_no_transaction(
        bus,
        &s,
        slot,
        "NV_POKE_COMMIT_BYTE frees the staging buffer",
    )?;

    Ok(Outcome::Pass)
}

/// A commit that has happened survives into the next session.
///
/// NV storage is "dedicated non-volatile storage": what a commit wrote is still
/// there after the host leaves command-response mode and comes back, which is
/// the whole reason a bootloader would use it — the specification's own example
/// stores "the last-selected slot index in NV storage using
/// NV_POKE_COMMIT_BYTE, and read\[s\] it back on boot".
pub fn a_commit_outlives_the_session(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x9;
    const VALUE: u8 = 0x3E;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let Some(slot) = staging_slot(bus, &s, ctx)? else {
        return Ok(needs_staging_slot(ctx));
    };

    bus.issue_cmd(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE_COMMIT_BYTE,
        &commit_byte_args(VALUE, LOCATION, slot),
    )
    .map_err(|e| format!("NV_POKE_COMMIT_BYTE: {e}"))?;

    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("second ENTER_CMD_RESP: {e}"))?;

    expect_nv(
        bus,
        &s,
        LOCATION,
        &[VALUE],
        "NV storage is non-volatile, so a committed byte is still there in the next session",
    )?;

    Ok(Outcome::Pass)
}

/// NV_POKE_COMMIT_BYTE must reject a slot argument of 0xAA.
///
/// "An A3 value of 0xAA is invalid and rejected."  The byte differs from what
/// the location holds, so the early return above cannot answer for it: the
/// rejection has to be on account of the slot.
pub fn nv_poke_commit_byte_rejects_slot_aa(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x88;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    bus.seed_nv(LOCATION, &[0x10]);
    bus.expect_rejected(
        &s,
        group::NV_STORAGE,
        nv::NV_POKE_COMMIT_BYTE,
        &commit_byte_args(0x20, LOCATION, 0xAA),
    )
    .map_err(|e| {
        format!(
            "{e} — A3, the RAM slot, was 0xAA, and the byte differs from the 0x10 already at \
             that location so the unchanged-byte early return cannot account for the outcome"
        )
    })?;

    Ok(Outcome::Pass)
}

/// An NV command in command mode must not be acted on.
///
/// "All commands in this group are valid in command-response mode only."  The
/// response data section is armed in command mode with a value that is not the
/// capability response; GET_NV_CAPABILITY is then sent, knocked and well
/// formed, in command mode; a verified poke fences it; and the byte must still
/// hold the armed value rather than the answer.
pub fn not_valid_in_command_mode(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // The answer the device would write, so the two can be told apart.
    bus.issue_cmd(&s, group::NV_STORAGE, nv::GET_NV_CAPABILITY, &[])
        .map_err(|e| format!("GET_NV_CAPABILITY: {e}"))?;
    let answer = bus.read_data(&s, 0, 1)?[0];

    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    let dst = s.bch_start + HDR_SIZE;
    let armed = answer ^ 0xFF;
    bus.poke_verified(ctx, dst, armed)
        .map_err(|e| format!("arming the response data section: {e}"))?;

    bus.knock(ctx.command_page())?;
    bus.send_cmd(
        ctx.command_page(),
        group::NV_STORAGE,
        nv::GET_NV_CAPABILITY,
        &[],
    )?;

    bus.fence(ctx)?;

    let got = bus.read(dst)?;
    if got == armed {
        return Ok(Outcome::Pass);
    }
    if got == answer {
        return Err(format!(
            "the device acted on GET_NV_CAPABILITY in command mode: 0x{dst:06X} serves \
             0x{got:02X}, the first byte of the capability response, rather than the armed \
             0x{armed:02X} — the NV Storage group is valid in command-response mode only"
        ));
    }
    Err(format!(
        "0x{dst:06X} serves 0x{got:02X}, which is neither the armed 0x{armed:02X} nor the \
         capability response's 0x{answer:02X} — something other than these two writes reached it"
    ))
}

/// The Read group's own peek must not answer from NV storage, nor NV_PEEK from
/// a slot.
///
/// SLOT_PEEK "read\[s\] one or more bytes from the specified RAM slot" and
/// NV_PEEK "reads one or more bytes directly from NV storage": two commands,
/// two separate stores, and nothing in the specification connects them.  The
/// same location is given a different value in each, and each command must
/// answer with its own.
pub fn nv_peek_and_slot_peek_read_different_stores(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    const LOCATION: u32 = 0x60;

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    // The slot address is above the back-channel, so poking it disturbs
    // nothing; the NV location is an NV offset and unrelated to it.
    let slot_addr = ctx.scratch_addr();
    let in_nv = 0x4Du8;
    let in_slot = 0xB2u8;

    bus.seed_nv(LOCATION, &[in_nv]);
    bus.poke_slot_verified(&s, ctx.active_ram_slot, slot_addr, in_slot)?;

    expect_nv(
        bus,
        &s,
        LOCATION,
        &[in_nv],
        "NV_PEEK reads NV storage, not the RAM slot",
    )?;

    let got = bus.peek_slot(&s, ctx.active_ram_slot, slot_addr, 1)?[0];
    if got != in_slot {
        return Err(format!(
            "SLOT_PEEK of 0x{slot_addr:06X} read 0x{got:02X}, expected the 0x{in_slot:02X} poked \
             there — NV storage holds 0x{in_nv:02X}, and the two are separate stores"
        ));
    }

    Ok(Outcome::Pass)
}
