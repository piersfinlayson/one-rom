// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! EXTENSIONS.md: "Group 0x80 — Device Info".
//!
//! Commissioning values are written to OTP with `onerom_metadata::otp`'s
//! writer, as `onerom hardware commission` does. Each metadata command is
//! checked against the plugin API call it corresponds to.
//!
//! Where a response is shorter than the data section, the byte after it is
//! set first and checked afterwards, so a device writing more than the
//! response fails.

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_metadata::otp::{CommissioningValues, NewCommissioningInstance, RowWrite};
use onerom_metadata::{
    COMMISSIONING_VERSION_KEY, OTP_COMMISSIONING_AREA_FIRST_ROW, OTP_KEY_NONE, OTP_PAGE_ROWS,
    OTP_STORE_MAGIC, OTP_STORE_VERSION, OneromOtpKey,
};

use crate::driver::{Bus, HDR_SIZE, Session, control, group};
use crate::{Ctx, Outcome};

const GROUP: u8 = 0x80;
const GET_COMMISSIONING_VERSION: u8 = 0x00;
const GET_COMMISSIONING_VALUE: u8 = 0x01;
const GET_METADATA_STR: u8 = 0x02;
const GET_METADATA_UINT: u8 = 0x03;
const GET_METADATA_ARRAY: u8 = 0x04;

/// Offset of the value in a Value Response and of the elements in a
/// GET_METADATA_ARRAY response.
const RESP_HDR: u32 = 4;

const DATE: &str = "20261015";
const SIGNER: u16 = 2;
const MANUFACTURER: &str = "piers.rocks";
const SIG_BYTE: u8 = 0xa5;

const BOARD_KEY: u16 = OneromOtpKey::OtpKeyCommissioningBoard as u16;
const SIG_KEY: u16 = OneromOtpKey::OtpKeyCommissioningSig as u16;
const MANUFACTURER_KEY: u16 = OneromOtpKey::OtpKeyCommissioningManufacturer as u16;
const DATE_KEY: u16 = OneromOtpKey::OtpKeyCommissioningDate as u16;
const SIGNER_KEY: u16 = OneromOtpKey::OtpKeyCommissioningSigner as u16;

/// A commissioning key the writer doesn't write.
const UNWRITTEN_KEY: u16 = 0x1234;

const STR_KEYS: [u32; 3] = [
    ffi::ora_metadata_key_t_ORA_METADATA_KEY_UNIT_NAME,
    ffi::ora_metadata_key_t_ORA_METADATA_KEY_SERIAL_OVERRIDE,
    ffi::ora_metadata_key_t_ORA_METADATA_KEY_HW_REV,
];
const HW_REV: u32 = ffi::ora_metadata_key_t_ORA_METADATA_KEY_HW_REV;
const BOARD_SIZE: u32 = ffi::ora_metadata_key_t_ORA_METADATA_KEY_BOARD_SIZE;
const UINT_KEYS: [u32; 3] = [
    BOARD_SIZE,
    ffi::ora_metadata_key_t_ORA_METADATA_KEY_RP_VARIANT,
    ffi::ora_metadata_key_t_ORA_METADATA_KEY_NUM_PHYS_PINS,
];
const CHIP_ID: u32 = ffi::ora_metadata_key_t_ORA_METADATA_KEY_CHIP_ID;

/// A metadata key no firmware defines.
const UNKNOWN_METADATA_KEY: u32 = 0x0000_7FFF;

/// The array key the shim's metadata stand-in returns a scenario's elements
/// for. No firmware defines it.
pub const STAND_IN_KEY: u32 = 0x0000_7FFE;

/// Elements of [`STAND_IN_KEY`]. The second is too large for a width of 1.
const WIDE_ELEMS: [u32; 2] = [0x01, 0x100];

/// The scenarios run with the shim's metadata stand-in and the elements it
/// returns for [`STAND_IN_KEY`].
pub static STAND_IN: &[(&str, &[u32])] = &[
    (
        "extensions.device_info.reserved_key_byte_is_rejected",
        &WIDE_ELEMS,
    ),
    (
        "extensions.device_info.metadata_array_fails_on_a_wide_element",
        &WIDE_ELEMS,
    ),
];

/// A commissioning key with a most significant byte of 0xAA.
const RESERVED_COMMISSIONING_KEY: u16 = 0xAA01;

/// The chip ID rows of the example in EXTENSIONS.md, chip ID E126C9F97C10ADAC.
const CHIP_ID_ROWS: [u16; 4] = [0xadac, 0x7c10, 0xc9f9, 0xe126];
const CHIP_ID_BYTES: [u8; 8] = [0xE1, 0x26, 0xC9, 0xF9, 0x7C, 0x10, 0xAD, 0xAC];

fn value_args(count: u8, offset: u16, key: u16) -> [u8; 5] {
    let [o0, o1] = offset.to_le_bytes();
    let [k0, k1] = key.to_le_bytes();
    [count, o0, o1, k0, k1]
}

fn str_args(count: u8, offset: u16, key: u32) -> [u8; 7] {
    let [o0, o1] = offset.to_le_bytes();
    let [k0, k1, k2, k3] = key.to_le_bytes();
    [count, o0, o1, k0, k1, k2, k3]
}

fn array_args(count: u8, index: u8, width: u8, key: u32) -> [u8; 7] {
    let [k0, k1, k2, k3] = key.to_le_bytes();
    [count, index, width, k0, k1, k2, k3]
}

/// Write a commissioning instance for `ctx`'s board and return the rows
/// written.
fn commission(ctx: &Ctx) -> Vec<RowWrite> {
    let values = CommissioningValues::new(ctx.board, MANUFACTURER, DATE, SIGNER)
        .expect("valid commissioning values");
    let writes = NewCommissioningInstance::new(&values, OTP_COMMISSIONING_AREA_FIRST_ROW)
        .expect("an instance that fits")
        .writes(&[SIG_BYTE; 64]);
    for w in &writes {
        Emulator::set_otp_ecc(w.row, &[w.value]);
    }
    writes
}

/// Every commissioning key's value in an instance from [`commission`].
fn expected_values(ctx: &Ctx) -> Vec<(u16, &'static str, Vec<u8>)> {
    vec![
        (BOARD_KEY, "BOARD", ctx.board.name().as_bytes().to_vec()),
        (SIG_KEY, "SIG", vec![SIG_BYTE; 64]),
        (
            MANUFACTURER_KEY,
            "MANUFACTURER",
            MANUFACTURER.as_bytes().to_vec(),
        ),
        (DATE_KEY, "DATE", DATE.as_bytes().to_vec()),
        (SIGNER_KEY, "SIGNER", SIGNER.to_le_bytes().to_vec()),
        (
            COMMISSIONING_VERSION_KEY,
            "version",
            OTP_STORE_VERSION.to_le_bytes().to_vec(),
        ),
    ]
}

fn enter(bus: &mut Bus, ctx: &Ctx) -> Result<Session, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    Ok(s)
}

/// Change data section byte `offset` and return its new value.
fn arm(bus: &mut Bus, ctx: &Ctx, s: &Session, offset: u32) -> Result<u8, String> {
    let addr = s.bch_start + HDR_SIZE + offset;
    let armed = bus.read(addr)? ^ 0x5A;
    bus.poke_slot_verified(s, ctx.active_ram_slot, addr, armed)?;
    Ok(armed)
}

fn expect_armed(
    bus: &mut Bus,
    s: &Session,
    offset: u32,
    armed: u8,
    what: &str,
) -> Result<(), String> {
    let got = bus.read_data(s, offset, 1)?[0];
    if got != armed {
        return Err(format!(
            "{what}: data section byte {offset} changed from 0x{armed:02X} to 0x{got:02X}"
        ));
    }
    Ok(())
}

/// Issue a command with a Value Response and require `len`, `bytes` and an
/// unchanged byte after them.
fn expect_value(
    bus: &mut Bus,
    ctx: &Ctx,
    s: &Session,
    (cmd, args): (u8, &[u8]),
    len: usize,
    bytes: &[u8],
    what: &str,
) -> Result<(), String> {
    let after = RESP_HDR + bytes.len() as u32;
    let armed = arm(bus, ctx, s, after)?;
    bus.issue_cmd(s, GROUP, cmd, args)
        .map_err(|e| format!("{what}: {e}"))?;
    let mut want = (len as u16).to_le_bytes().to_vec();
    want.extend_from_slice(&[0, 0]);
    want.extend_from_slice(bytes);
    bus.expect_data(s, 0, &want, what)?;
    expect_armed(bus, s, after, armed, what)
}

fn expect_rejected(
    bus: &mut Bus,
    s: &Session,
    cmd: u8,
    args: &[u8],
    what: &str,
) -> Result<(), String> {
    bus.expect_rejected(s, GROUP, cmd, args)
        .map_err(|e| format!("{what}: {e}"))
}

/// GET_COMMISSIONING_VERSION returns 0 on a One ROM that isn't commissioned.
pub fn version_is_zero_uncommissioned(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = enter(bus, ctx)?;
    bus.issue_cmd(&s, GROUP, GET_COMMISSIONING_VERSION, &[])
        .map_err(|e| format!("GET_COMMISSIONING_VERSION: {e}"))?;
    bus.expect_data(&s, 0, &[0, 0, 0, 0], "GET_COMMISSIONING_VERSION response")?;
    Ok(Outcome::Pass)
}

/// GET_COMMISSIONING_VERSION returns the commissioning data's version.
pub fn version_reports_the_commissioned_version(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    commission(ctx);
    let s = enter(bus, ctx)?;
    bus.issue_cmd(&s, GROUP, GET_COMMISSIONING_VERSION, &[])
        .map_err(|e| format!("GET_COMMISSIONING_VERSION: {e}"))?;
    let [v0, v1] = OTP_STORE_VERSION.to_le_bytes();
    bus.expect_data(&s, 0, &[v0, v1, 0, 0], "GET_COMMISSIONING_VERSION response")?;
    Ok(Outcome::Pass)
}

/// GET_COMMISSIONING_VALUE reads every key's whole value with a count of
/// zero, which indicates 256 bytes.
pub fn value_reads_every_key(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    commission(ctx);
    let s = enter(bus, ctx)?;
    for (key, name, want) in expected_values(ctx) {
        expect_value(
            bus,
            ctx,
            &s,
            (GET_COMMISSIONING_VALUE, &value_args(0, 0, key)),
            want.len(),
            &want,
            &format!("GET_COMMISSIONING_VALUE for {name}"),
        )?;
    }
    Ok(Outcome::Pass)
}

/// GET_COMMISSIONING_VALUE returns the smaller of count and the bytes after
/// offset. An offset at the end of the value returns no bytes.
pub fn value_reads_from_an_offset(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    commission(ctx);
    let s = enter(bus, ctx)?;
    let value = MANUFACTURER.as_bytes();
    let len = value.len();
    for (count, offset) in [(1u8, 0u16), (3, 2), (8, len as u16 - 2), (8, len as u16)] {
        let from = usize::from(offset);
        let n = usize::from(count).min(len - from);
        expect_value(
            bus,
            ctx,
            &s,
            (
                GET_COMMISSIONING_VALUE,
                &value_args(count, offset, MANUFACTURER_KEY),
            ),
            len,
            &value[from..from + n],
            &format!("GET_COMMISSIONING_VALUE for MANUFACTURER, count {count}, offset {offset}"),
        )?;
    }
    Ok(Outcome::Pass)
}

/// GET_COMMISSIONING_VALUE fails where the value isn't present, and where
/// offset is beyond the end of the value.
pub fn value_fails_where_absent(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = enter(bus, ctx)?;
    for (key, name, _) in expected_values(ctx) {
        expect_rejected(
            bus,
            &s,
            GET_COMMISSIONING_VALUE,
            &value_args(0, 0, key),
            &format!("GET_COMMISSIONING_VALUE for {name} uncommissioned"),
        )?;
    }

    commission(ctx);
    let past = MANUFACTURER.len() as u16 + 1;
    for (args, what) in [
        (value_args(0, 0, UNWRITTEN_KEY), "an unwritten key"),
        (value_args(0, 0, OTP_KEY_NONE), "key OTP_KEY_NONE"),
        (
            value_args(1, past, MANUFACTURER_KEY),
            "an offset beyond the end of MANUFACTURER",
        ),
    ] {
        expect_rejected(
            bus,
            &s,
            GET_COMMISSIONING_VALUE,
            &args,
            &format!("GET_COMMISSIONING_VALUE for {what}"),
        )?;
    }
    Ok(Outcome::Pass)
}

/// With an unsupported commissioning version, GET_COMMISSIONING_VERSION
/// returns it and GET_COMMISSIONING_VALUE fails for every key.
pub fn value_fails_on_an_unsupported_version(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let first = commission(ctx);
    let last = first.iter().map(|w| w.row).max().expect("writes");
    let next = (last / OTP_PAGE_ROWS + 1) * OTP_PAGE_ROWS;
    let unknown = OTP_STORE_VERSION + 1;
    Emulator::set_otp_ecc(next, &[first[0].value, unknown]);

    let s = enter(bus, ctx)?;
    bus.issue_cmd(&s, GROUP, GET_COMMISSIONING_VERSION, &[])
        .map_err(|e| format!("GET_COMMISSIONING_VERSION: {e}"))?;
    let [v0, v1] = unknown.to_le_bytes();
    bus.expect_data(&s, 0, &[v0, v1, 0, 0], "GET_COMMISSIONING_VERSION response")?;

    for (key, name, _) in expected_values(ctx) {
        if key == COMMISSIONING_VERSION_KEY {
            continue;
        }
        expect_rejected(
            bus,
            &s,
            GET_COMMISSIONING_VALUE,
            &value_args(0, 0, key),
            &format!("GET_COMMISSIONING_VALUE for {name} with version {unknown}"),
        )?;
    }
    Ok(Outcome::Pass)
}

/// GET_METADATA_STR reads a string from an offset. An offset at the end of the
/// string returns no bytes.
pub fn metadata_str_reads_a_string(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let value = match bus.metadata_str(HW_REV) {
        (OraResult::Ok, Some(v)) if !v.is_empty() => v.into_bytes(),
        (r, v) => return Err(format!("get_metadata_str for HW_REV returned {r:?} {v:?}")),
    };
    let len = value.len();
    let s = enter(bus, ctx)?;
    for (count, offset) in [
        (0u8, 0u16),
        (1, 0),
        (2, 1),
        (8, len as u16 - 1),
        (8, len as u16),
    ] {
        let from = usize::from(offset);
        let want = if count == 0 { 256 } else { usize::from(count) };
        let n = want.min(len - from);
        expect_value(
            bus,
            ctx,
            &s,
            (GET_METADATA_STR, &str_args(count, offset, HW_REV)),
            len,
            &value[from..from + n],
            &format!("GET_METADATA_STR for HW_REV, count {count}, offset {offset}"),
        )?;
    }
    Ok(Outcome::Pass)
}

/// GET_METADATA_STR reads an unset string as length 0.
pub fn metadata_str_unset_has_length_zero(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let Some(key) = STR_KEYS
        .into_iter()
        .find(|&k| bus.metadata_str(k) == (OraResult::Ok, None))
    else {
        return Ok(Outcome::Skip("every string key is set".into()));
    };
    let s = enter(bus, ctx)?;
    expect_value(
        bus,
        ctx,
        &s,
        (GET_METADATA_STR, &str_args(8, 0, key)),
        0,
        &[],
        &format!("GET_METADATA_STR for unset key 0x{key:02X}"),
    )?;
    expect_rejected(
        bus,
        &s,
        GET_METADATA_STR,
        &str_args(8, 1, key),
        &format!("GET_METADATA_STR for unset key 0x{key:02X} at offset 1"),
    )?;
    Ok(Outcome::Pass)
}

/// GET_METADATA_STR fails where the key is unknown or isn't a string, and
/// where offset is beyond the end of the string.
pub fn metadata_str_fails(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let len = match bus.metadata_str(HW_REV) {
        (OraResult::Ok, Some(v)) => v.len() as u16,
        (r, v) => return Err(format!("get_metadata_str for HW_REV returned {r:?} {v:?}")),
    };
    let s = enter(bus, ctx)?;
    for (args, what) in [
        (str_args(0, 0, UNKNOWN_METADATA_KEY), "an unknown key"),
        (str_args(0, 0, BOARD_SIZE), "number key BOARD_SIZE"),
        (str_args(0, 0, CHIP_ID), "array key CHIP_ID"),
        (
            str_args(1, len + 1, HW_REV),
            "an offset beyond the end of HW_REV",
        ),
    ] {
        expect_rejected(
            bus,
            &s,
            GET_METADATA_STR,
            &args,
            &format!("GET_METADATA_STR for {what}"),
        )?;
    }
    Ok(Outcome::Pass)
}

/// GET_METADATA_UINT reads a number.
pub fn metadata_uint_reads_a_number(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let mut want = Vec::new();
    for key in UINT_KEYS {
        match bus.metadata_uint(key) {
            (OraResult::Ok, Some(v)) => want.push((key, v)),
            (r, _) => return Err(format!("get_metadata_uint for 0x{key:02X} returned {r:?}")),
        }
    }
    let s = enter(bus, ctx)?;
    for (key, value) in want {
        let [k0, k1, k2, k3] = key.to_le_bytes();
        bus.issue_cmd(&s, GROUP, GET_METADATA_UINT, &[k0, k1, k2, k3])
            .map_err(|e| format!("GET_METADATA_UINT for 0x{key:02X}: {e}"))?;
        bus.expect_data(
            &s,
            0,
            &value.to_le_bytes(),
            &format!("GET_METADATA_UINT response for 0x{key:02X}"),
        )?;
    }
    Ok(Outcome::Pass)
}

/// GET_METADATA_UINT fails where the key is unknown or isn't a number.
pub fn metadata_uint_fails(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = enter(bus, ctx)?;
    for (key, what) in [
        (UNKNOWN_METADATA_KEY, "an unknown key"),
        (HW_REV, "string key HW_REV"),
        (CHIP_ID, "array key CHIP_ID"),
    ] {
        expect_rejected(
            bus,
            &s,
            GET_METADATA_UINT,
            &key.to_le_bytes(),
            &format!("GET_METADATA_UINT for {what}"),
        )?;
    }
    Ok(Outcome::Pass)
}

/// Issue GET_METADATA_ARRAY for CHIP_ID and require `elems` at `width` and an
/// unchanged byte after them.
fn expect_chip_id(
    bus: &mut Bus,
    ctx: &Ctx,
    s: &Session,
    count: u8,
    index: u8,
    width: u8,
    elems: &[u8],
) -> Result<(), String> {
    let what =
        format!("GET_METADATA_ARRAY for CHIP_ID, count {count}, index {index}, width {width}");
    let after = RESP_HDR + (elems.len() * usize::from(width)) as u32;
    let armed = arm(bus, ctx, s, after)?;
    bus.issue_cmd(
        s,
        GROUP,
        GET_METADATA_ARRAY,
        &array_args(count, index, width, CHIP_ID),
    )
    .map_err(|e| format!("{what}: {e}"))?;
    let mut want = vec![elems.len() as u8, 0, 0, 0];
    for &e in elems {
        want.extend_from_slice(&u32::from(e).to_le_bytes()[..usize::from(width)]);
    }
    bus.expect_data(s, 0, &want, &what)?;
    expect_armed(bus, s, after, armed, &what)
}

/// GET_METADATA_ARRAY reads the chip ID, as in EXTENSIONS.md's example.
pub fn metadata_array_reads_the_chip_id(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    Emulator::set_otp_ecc(0, &CHIP_ID_ROWS);
    let s = enter(bus, ctx)?;
    expect_chip_id(bus, ctx, &s, 8, 0, 1, &CHIP_ID_BYTES)?;
    Ok(Outcome::Pass)
}

/// GET_METADATA_ARRAY writes elements at each width from index, returning
/// fewer than count where the array is shorter. A count of zero indicates 256.
pub fn metadata_array_reads_from_an_index(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    Emulator::set_otp_ecc(0, &CHIP_ID_ROWS);
    let s = enter(bus, ctx)?;
    for (count, index, width) in [
        (0u8, 0u8, 1u8),
        (2, 3, 1),
        (5, 6, 1),
        (8, 0, 2),
        (3, 2, 4),
        (8, 7, 4),
    ] {
        let from = usize::from(index);
        let want = if count == 0 { 256 } else { usize::from(count) };
        let n = want.min(CHIP_ID_BYTES.len() - from);
        expect_chip_id(
            bus,
            ctx,
            &s,
            count,
            index,
            width,
            &CHIP_ID_BYTES[from..from + n],
        )?;
    }
    Ok(Outcome::Pass)
}

/// GET_METADATA_ARRAY fails where the key is unknown or isn't an array, where
/// index is beyond the last element, and where width isn't 1, 2 or 4.
pub fn metadata_array_fails(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = enter(bus, ctx)?;
    for (args, what) in [
        (array_args(1, 0, 1, UNKNOWN_METADATA_KEY), "an unknown key"),
        (array_args(1, 0, 1, HW_REV), "string key HW_REV"),
        (array_args(1, 0, 1, BOARD_SIZE), "number key BOARD_SIZE"),
        (
            array_args(1, 8, 1, CHIP_ID),
            "index 8 of the 8-byte CHIP_ID",
        ),
        (
            array_args(1, 9, 1, CHIP_ID),
            "index 9 of the 8-byte CHIP_ID",
        ),
        (array_args(1, 0, 0, CHIP_ID), "width 0"),
        (array_args(1, 0, 3, CHIP_ID), "width 3"),
        (array_args(1, 0, 8, CHIP_ID), "width 8"),
    ] {
        expect_rejected(
            bus,
            &s,
            GET_METADATA_ARRAY,
            &args,
            &format!("GET_METADATA_ARRAY for {what}"),
        )?;
    }
    Ok(Outcome::Pass)
}

/// GET_METADATA_ARRAY fails where an element is too large for width.
///
/// The firmware's arrays hold bytes, which fit any width, so the scenario reads
/// the shim's metadata stand-in array, [`WIDE_ELEMS`].
pub fn metadata_array_fails_on_a_wide_element(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = enter(bus, ctx)?;
    let what = "GET_METADATA_ARRAY for the stand-in array at width 2";
    bus.issue_cmd(
        &s,
        GROUP,
        GET_METADATA_ARRAY,
        &array_args(2, 0, 2, STAND_IN_KEY),
    )
    .map_err(|e| format!("{what}: {e}"))?;
    bus.expect_data(&s, 0, &[2, 0, 0, 0, 0x01, 0x00, 0x00, 0x01], what)?;

    let armed = arm(bus, ctx, &s, 0)?;
    for (args, what) in [
        (
            array_args(2, 0, 1, STAND_IN_KEY),
            "both elements at width 1",
        ),
        (array_args(1, 1, 1, STAND_IN_KEY), "element 1 at width 1"),
    ] {
        expect_rejected(
            bus,
            &s,
            GET_METADATA_ARRAY,
            &args,
            &format!("GET_METADATA_ARRAY for {what}"),
        )?;
    }
    expect_armed(bus, &s, 0, armed, "GET_METADATA_ARRAY for a wide element")?;
    Ok(Outcome::Pass)
}

/// Each command fails where the response data section is one byte smaller than
/// EXTENSIONS.md requires and succeeds where it is exactly that size.
pub fn commands_fail_without_room_for_the_response(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    commission(ctx);
    let cases: [(u8, Vec<u8>, u32, &str); 5] = [
        (
            GET_COMMISSIONING_VERSION,
            vec![],
            4,
            "GET_COMMISSIONING_VERSION",
        ),
        (
            GET_COMMISSIONING_VALUE,
            value_args(8, 0, MANUFACTURER_KEY).to_vec(),
            4 + 8,
            "GET_COMMISSIONING_VALUE with count 8",
        ),
        (
            GET_METADATA_STR,
            str_args(8, 0, HW_REV).to_vec(),
            4 + 8,
            "GET_METADATA_STR with count 8",
        ),
        (
            GET_METADATA_UINT,
            BOARD_SIZE.to_le_bytes().to_vec(),
            4,
            "GET_METADATA_UINT",
        ),
        (
            GET_METADATA_ARRAY,
            array_args(4, 0, 2, CHIP_ID).to_vec(),
            4 + 4 * 2,
            "GET_METADATA_ARRAY with count 4 and width 2",
        ),
    ];
    for (cmd, args, need, what) in cases {
        for (size, ok) in [(need - 1, false), (need, true)] {
            let s = bus.enter_sized(ctx, (HDR_SIZE + size) as u16)?;
            let result = bus.issue_cmd(&s, GROUP, cmd, &args);
            match (ok, result) {
                (true, Ok(())) => {}
                (false, Err(crate::driver::CmdFailure::Failed)) => {}
                (_, r) => {
                    return Err(format!(
                        "{what} in a {size}-byte data section returned {r:?}, a {need}-byte \
                         response requires it to {}",
                        if ok { "succeed" } else { "fail" }
                    ));
                }
            }
            bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
                .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;
        }
    }
    Ok(Outcome::Pass)
}

/// Write a complete commissioning instance holding only
/// [`RESERVED_COMMISSIONING_KEY`] and the signature.
fn commission_reserved_key() {
    let mut rows = vec![
        OTP_STORE_MAGIC,
        OTP_STORE_VERSION,
        RESERVED_COMMISSIONING_KEY,
        2,
        0x1234,
        SIG_KEY,
        64,
    ];
    rows.extend([u16::from_le_bytes([SIG_BYTE; 2]); 32]);
    Emulator::set_otp_ecc(OTP_COMMISSIONING_AREA_FIRST_ROW, &rows);
}

/// A key whose most significant byte is 0xAA is rejected.
///
/// Without the plugin's check each command would succeed. OTP holds the
/// commissioning key and the shim's metadata stand-in returns a value for the
/// metadata key.
pub fn reserved_key_byte_is_rejected(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    commission_reserved_key();
    if bus.commissioning_value(RESERVED_COMMISSIONING_KEY).0 != OraResult::Ok {
        return Err("the firmware doesn't return the reserved commissioning key".into());
    }
    let s = enter(bus, ctx)?;
    let key = 0xAA00_0000 | HW_REV;
    for (cmd, args, what) in [
        (
            GET_COMMISSIONING_VALUE,
            value_args(1, 0, RESERVED_COMMISSIONING_KEY).to_vec(),
            "GET_COMMISSIONING_VALUE with A4 0xAA",
        ),
        (
            GET_METADATA_STR,
            str_args(1, 0, key).to_vec(),
            "GET_METADATA_STR with A6 0xAA",
        ),
        (
            GET_METADATA_UINT,
            key.to_le_bytes().to_vec(),
            "GET_METADATA_UINT with A3 0xAA",
        ),
        (
            GET_METADATA_ARRAY,
            array_args(1, 0, 1, key).to_vec(),
            "GET_METADATA_ARRAY with A6 0xAA",
        ),
    ] {
        expect_rejected(bus, &s, cmd, &args, what)?;
    }
    Ok(Outcome::Pass)
}

/// Each command with valid arguments.
fn every_command() -> [(u8, Vec<u8>); 5] {
    [
        (GET_COMMISSIONING_VERSION, vec![]),
        (
            GET_COMMISSIONING_VALUE,
            value_args(0, 0, BOARD_KEY).to_vec(),
        ),
        (GET_METADATA_STR, str_args(0, 0, HW_REV).to_vec()),
        (GET_METADATA_UINT, BOARD_SIZE.to_le_bytes().to_vec()),
        (GET_METADATA_ARRAY, array_args(8, 0, 1, CHIP_ID).to_vec()),
    ]
}

/// No command writes a response in command mode.
///
/// The response data section is set first and checked afterwards. Each
/// command follows its own knock, as command mode doesn't dispatch a command
/// without one.
pub fn not_valid_in_command_mode(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    commission(ctx);
    let s = enter(bus, ctx)?;
    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    let dst = s.bch_start + HDR_SIZE;
    let armed = bus.read(dst)? ^ 0xFF;
    bus.poke_verified(ctx, dst, armed)
        .map_err(|e| format!("setting the response data section: {e}"))?;

    for (cmd, args) in every_command() {
        bus.knock(s.command_page)?;
        bus.send_cmd(s.command_page, GROUP, cmd, &args)?;
    }

    if bus.read(dst)? != armed {
        return Err("a command-mode Device Info command wrote to the response data section".into());
    }
    Ok(Outcome::Pass)
}

/// Each command reads exactly its argument bytes. A NOP sent after each
/// command must arrive.
pub fn argument_counts_are_consumed_exactly(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = enter(bus, ctx)?;
    for (cmd, args) in every_command() {
        let _ = bus.issue_cmd(&s, GROUP, cmd, &args);
        bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
            .map_err(|e| {
                format!(
                    "the NOP after 0x80/0x{cmd:02X} did not arrive ({e}), so the device did not \
                     consume that command's arguments exactly"
                )
            })?;
    }
    Ok(Outcome::Pass)
}
