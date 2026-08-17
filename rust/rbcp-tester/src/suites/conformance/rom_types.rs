// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "ROM Types".
//!
//! The protocol assigns one byte to each ROM type, and the device reports one
//! in GET_RAM_SLOT_INFO_ALL's `rom_type` field and in every GET_FLASH_SLOT_INFO
//! record.  "Note that the ROM type values above are defined by the protocol
//! independently of any specific device implementation."
//!
//! The table below is transcribed from the specification and from nothing else.
//! That is the whole value of this module: the device's numbering comes from
//! `onerom-config`'s chip type definitions, so a table taken from there — or
//! from the firmware, or from the plugin — would agree with the device by
//! construction and would assert nothing at all.  Two independently maintained
//! copies of a numbering can disagree; one copy compared with itself cannot.
//!
//! # Shape of these scenarios
//!
//! Which chip sits in which slot is fixed by the configuration under test, and
//! that is not in question here — it is the byte the device puts on the wire to
//! describe that chip which is.  So each scenario looks the chip's name up in
//! the protocol's table, asks the device over the bus, and requires the two to
//! agree.
//!
//! A value outside the table is reported the way the specification tells hosts
//! to read one — "Host implementations must handle reserved values gracefully,
//! as new ROM types may be defined in future protocol versions" — so it is
//! named as reserved, as implementation-specific, or as "invalid / ROM not
//! being served", rather than printed as an unexplained number.  It is still a
//! failure when it appears here, because the chip being described is known and
//! the table defines a value for it.
//!
//! The converse case — a chip the protocol's table does not name — is not a
//! failure of the device, which "is not required to support all ROM types
//! listed" and to which the protocol offers 0x80–0xFE for its own use.  There
//! is then no normative value to check against, so the scenario skips and says
//! which chip left it with nothing to assert.
//!
//! These scenarios run against every configuration in the matrix, and each
//! configuration serves several chips, so between them the runs cover far more
//! of the table than any single run does.

use crate::driver::{Bus, group, read};
use crate::{Ctx, Outcome};

/// The protocol's ROM type table, transcribed from the specification's "ROM
/// Types" section.
///
/// Names are written exactly as the specification writes them.  `ChipType`
/// happens to spell them the same way, so the join is a string comparison —
/// and if a chip type is ever renamed on one side only, the lookup fails to
/// find it and the scenario skips, saying so, rather than quietly matching
/// something else.
const SPEC_ROM_TYPES: &[(u8, &str)] = &[
    (0x00, "2316"),
    (0x01, "2332"),
    (0x02, "2364"),
    (0x03, "23128"),
    (0x04, "23256"),
    (0x05, "23512"),
    (0x06, "2704"),
    (0x07, "2708"),
    (0x08, "2716"),
    (0x09, "2732"),
    (0x0A, "2764"),
    (0x0B, "27128"),
    (0x0C, "27256"),
    (0x0D, "27512"),
    (0x0E, "231024"),
    (0x0F, "27C010"),
    (0x10, "27C020"),
    (0x11, "27C040"),
    (0x12, "27C080"),
    (0x13, "27C400"),
    (0x14, "6116"),
    (0x15, "27C301"),
    // 0x16–0x18 Reserved.
    (0x19, "SST39SF040"),
    (0x1A, "28C16"),
    (0x1B, "28C64"),
    (0x1C, "28C256"),
    (0x1D, "28C512"),
    (0x1E, "23QL512"),
    (0x1F, "23QL384"),
    (0x20, "23C1001"),
    (0x21, "27C200"),
    (0x22, "HM7641"),
    (0x23, "62256"),
    (0x24, "23C1010"),
    // 0x25–0x7F Reserved.
    // 0x80–0xFE Reserved for implementation-specific use.
    // 0xFF Invalid/ROM not being served.
];

/// Offset of `rom_type` in the GET_RAM_SLOT_INFO response data section.
const RAM_SLOT_ROM_TYPE: u32 = 2;

/// GET_FLASH_SLOT_INFO_ALL's data section: a 4-byte preamble, then 32-byte
/// records, each beginning with its `rom_type`.  `whole_count` is the second
/// byte of the preamble.
const PREAMBLE_LEN: u32 = 4;
const RECORD_LEN: u32 = 32;
const WHOLE_COUNT_OFFSET: u32 = 1;

/// How the specification's table accounts for a byte a device reported.
enum SpecClass {
    /// A value the table assigns, to the named ROM type.
    Defined(&'static str),
    /// Within one of the table's Reserved ranges — 0x16–0x18 or 0x25–0x7F.
    Reserved,
    /// 0x80–0xFE, "Reserved for implementation-specific use".
    ImplementationSpecific,
    /// 0xFF, "Invalid/ROM not being served".
    NotServed,
}

impl std::fmt::Display for SpecClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Defined(name) => write!(f, "the protocol's value for {name}"),
            Self::Reserved => f.write_str("a value the protocol reserves and does not define"),
            Self::ImplementationSpecific => f.write_str("reserved for implementation-specific use"),
            Self::NotServed => f.write_str("the protocol's \"invalid / ROM not being served\""),
        }
    }
}

fn classify(value: u8) -> SpecClass {
    if let Some((_, name)) = SPEC_ROM_TYPES.iter().find(|(v, _)| *v == value) {
        return SpecClass::Defined(name);
    }
    match value {
        0xFF => SpecClass::NotServed,
        0x80..=0xFE => SpecClass::ImplementationSpecific,
        _ => SpecClass::Reserved,
    }
}

/// The protocol's value for a chip, if the table names it.
fn spec_value(chip: &str) -> Option<u8> {
    SPEC_ROM_TYPES
        .iter()
        .find(|(_, name)| *name == chip)
        .map(|(v, _)| *v)
}

/// The chips the device holds in flash, in the order it offers them to a host.
///
/// The configuration under test determines this: each chip set becomes one
/// flash slot, in configuration order, and plugin sets are excluded from what a
/// host is shown.  Taking the *chips* from the configuration is not circular —
/// what is under test is the byte the device uses to describe each of them.
fn flash_slots(ctx: &Ctx) -> Vec<(u8, &'static str)> {
    ctx.config
        .chip_sets
        .iter()
        .filter_map(|set| set.chips.first())
        .map(|chip| chip.chip_type.resolved())
        .filter(|chip_type| !chip_type.is_plugin())
        .enumerate()
        .map(|(i, chip_type)| (i as u8, chip_type.name()))
        .collect()
}

/// Why a reported byte disagrees with the protocol's table.
fn mismatch(what: &str, got: u8, want: u8, chip: &str) -> String {
    format!(
        "{what}: the device reports ROM type 0x{got:02X} — {} — where the protocol's ROM \
         Types table gives 0x{want:02X} for {chip}",
        classify(got)
    )
}

/// The chip a scenario cannot check, because the protocol names no value for
/// it.  A device "is not required to support all ROM types listed", and the
/// converse holds too: a device may serve a chip the protocol has not yet
/// named, and 0x80–0xFE exists for exactly that.
fn no_protocol_value(chip: &str) -> Outcome {
    Outcome::Skip(format!(
        "the protocol's ROM Types table names no value for {chip}, so the device's report \
         has nothing normative to be checked against"
    ))
}

/// The ROM type the device reports for the slot it is serving must be the
/// protocol's value for that chip.
///
/// "GET_RAM_SLOT_INFO Response Format" puts `rom_type` — "ROM type currently
/// being served" — at offset 2 of the data section.  The device is serving the
/// chip named by the configuration under test, and has just demonstrated it by
/// answering out of the back-channel over the bus, so 0xFF ("invalid / ROM not
/// being served") is as wrong an answer here as any other wrong value.
pub fn served_type_from_ram_slot_info(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let chip = ctx.chip_type.name();
    let Some(want) = spec_value(chip) else {
        return Ok(no_protocol_value(chip));
    };

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::READ, read::GET_RAM_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_RAM_SLOT_INFO_ALL: {e}"))?;

    let got = bus.read_data(&s, RAM_SLOT_ROM_TYPE, 1)?[0];
    if got != want {
        return Err(mismatch("GET_RAM_SLOT_INFO_ALL rom_type", got, want, chip));
    }

    Ok(Outcome::Pass)
}

/// GET_FLASH_SLOT_INFO must report the protocol's value for the chip in the
/// slot asked about.
///
/// Every flash slot is asked about in turn, so one run covers as many entries
/// of the table as the configuration has distinct chips — and asking slot by
/// slot is what pins each answer to a particular chip, rather than to whichever
/// chip the device happened to describe.
pub fn flash_slot_types_from_flash_slot_info(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let slots = flash_slots(ctx);
    if let Some((_, chip)) = slots.iter().find(|(_, chip)| spec_value(chip).is_none()) {
        return Ok(no_protocol_value(chip));
    }
    if slots.is_empty() {
        return Ok(Outcome::Skip(
            "the configuration under test offers no flash slots to a host".to_string(),
        ));
    }

    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    for (slot, chip) in slots {
        let want = spec_value(chip).expect("checked above");
        bus.issue_cmd(&s, group::READ, read::GET_FLASH_SLOT_INFO, &[slot])
            .map_err(|e| format!("GET_FLASH_SLOT_INFO for slot {slot}: {e}"))?;

        // "GET_FLASH_SLOT_INFO Response Format": rom_type is the first byte of
        // the data section, the 31-byte name following it.
        let got = bus.read_data(&s, 0, 1)?[0];
        if got != want {
            return Err(mismatch(
                &format!("GET_FLASH_SLOT_INFO for flash slot {slot}"),
                got,
                want,
                chip,
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// GET_FLASH_SLOT_INFO_ALL's records must carry the same protocol values.
///
/// The records are the same 32-byte shape as GET_FLASH_SLOT_INFO's — "0 | 1 |
/// rom_type" — and follow the 4-byte preamble "in slot index order", so record
/// `i` describes flash slot `i`.  A device that numbered chips consistently but
/// built this response from a different source, or emitted the records out of
/// order, disagrees with the table here while agreeing with it slot by slot.
pub fn flash_slot_types_from_flash_slot_info_all(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let slots = flash_slots(ctx);
    if let Some((_, chip)) = slots.iter().find(|(_, chip)| spec_value(chip).is_none()) {
        return Ok(no_protocol_value(chip));
    }

    let s = ctx.session();

    // Only records that fit can be checked; the preamble's partial record
    // carries no rom_type unless it is at least one byte long, and its content
    // is the business of the response-format scenarios rather than this one.
    let capacity = ((s.data_size() - PREAMBLE_LEN) / RECORD_LEN) as usize;
    let expected = slots.len().min(capacity);
    if expected == 0 {
        return Ok(Outcome::Skip(
            "the back-channel data section has no room for a complete flash slot record"
                .to_string(),
        ));
    }

    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    bus.issue_cmd(&s, group::READ, read::GET_FLASH_SLOT_INFO_ALL, &[])
        .map_err(|e| format!("GET_FLASH_SLOT_INFO_ALL: {e}"))?;

    // The device's own count of complete records bounds what can be read as a
    // record at all, so a short answer is reported as such rather than as a
    // string of wrong ROM types read out of whatever the image holds there.
    let whole_count = usize::from(bus.read_data(&s, WHOLE_COUNT_OFFSET, 1)?[0]);
    if whole_count < expected {
        return Err(format!(
            "GET_FLASH_SLOT_INFO_ALL returned {whole_count} complete records; the device \
             offers {} flash slots and the data section has room for {capacity}, so {expected} \
             records must be present for their ROM types to be read",
            slots.len()
        ));
    }

    for (slot, chip) in slots.into_iter().take(expected) {
        let want = spec_value(chip).expect("checked above");
        let offset = PREAMBLE_LEN + u32::from(slot) * RECORD_LEN;
        let got = bus.read_data(&s, offset, 1)?[0];
        if got != want {
            return Err(mismatch(
                &format!("GET_FLASH_SLOT_INFO_ALL record {slot}"),
                got,
                want,
                chip,
            ));
        }
    }

    Ok(Outcome::Pass)
}
