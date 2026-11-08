// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `ora_get_commissioning_value` and
//! `ora_get_commissioning_instance`.
//!
//! Each case writes the commissioning area with `onerom_metadata::otp`'s
//! writer, as `onerom hardware commission` does, then reads it back through the
//! call. OTP outlives a boot in this process, so the test clears it when it's
//! done.

use onerom_config::hw::{Board, BoardSize};
use onerom_fw_emulator::{Emulator, OraResult};
use onerom_metadata::otp::{CommissioningValues, NewCommissioningInstance, RowWrite};
use onerom_metadata::{
    COMMISSIONING_VERSION_KEY, OTP_COMMISSIONING_AREA_FIRST_ROW, OTP_KEY_NONE, OTP_PAGE_ROWS,
    OTP_STORE_VERSION, OneromBoardSize, OneromOtpKey,
};

const FIRST: u16 = OTP_COMMISSIONING_AREA_FIRST_ROW;

const BOARD: Board = Board::Fire24A;
const DATE: &str = "20261014";
const SIGNER: u16 = 2;
const REVISION: &str = "A1";
const BATCH: &str = "Y1-2345";

/// A key the writer doesn't write.
const UNWRITTEN_KEY: u16 = 0x1234;

const BOARD_KEY: u16 = OneromOtpKey::OtpKeyCommissioningBoard as u16;
const SIG_KEY: u16 = OneromOtpKey::OtpKeyCommissioningSig as u16;
const MANUFACTURER_KEY: u16 = OneromOtpKey::OtpKeyCommissioningManufacturer as u16;
const DATE_KEY: u16 = OneromOtpKey::OtpKeyCommissioningDate as u16;
const SIGNER_KEY: u16 = OneromOtpKey::OtpKeyCommissioningSigner as u16;
const REVISION_KEY: u16 = OneromOtpKey::OtpKeyCommissioningRevision as u16;
const BATCH_KEY: u16 = OneromOtpKey::OtpKeyCommissioningBatch as u16;
const SIZE_KEY: u16 = OneromOtpKey::OtpKeyCommissioningSize as u16;

/// An instance at `first_row`, with `manufacturer` and a signature filled
/// with `sig_byte`.
fn instance(manufacturer: &str, sig_byte: u8, first_row: u16) -> Vec<RowWrite> {
    let values = CommissioningValues::builder(BOARD, manufacturer, DATE, SIGNER)
        .size(BoardSize::M)
        .revision(REVISION)
        .batch(BATCH)
        .build()
        .expect("valid commissioning values");
    NewCommissioningInstance::new(&values, first_row)
        .expect("an instance that fits")
        .writes(&[sig_byte; 64])
}

/// The first row of the page after the last of `writes`.
fn next_page(writes: &[RowWrite]) -> u16 {
    let last = writes.iter().map(|w| w.row).max().expect("writes");
    (last / OTP_PAGE_ROWS + 1) * OTP_PAGE_ROWS
}

fn write(writes: &[RowWrite]) {
    for w in writes {
        Emulator::set_otp_ecc(w.row, &[w.value]);
    }
}

/// Every key's value in an instance from [`instance`].
fn expected(manufacturer: &str, sig_byte: u8) -> Vec<(u16, &'static str, Vec<u8>)> {
    vec![
        (BOARD_KEY, "BOARD", BOARD.name().as_bytes().to_vec()),
        (SIG_KEY, "SIG", vec![sig_byte; 64]),
        (
            MANUFACTURER_KEY,
            "MANUFACTURER",
            manufacturer.as_bytes().to_vec(),
        ),
        (DATE_KEY, "DATE", DATE.as_bytes().to_vec()),
        (SIGNER_KEY, "SIGNER", SIGNER.to_le_bytes().to_vec()),
        (REVISION_KEY, "REVISION", REVISION.as_bytes().to_vec()),
        (BATCH_KEY, "BATCH", BATCH.as_bytes().to_vec()),
        (
            SIZE_KEY,
            "SIZE",
            u16::from(OneromBoardSize::BoardSizeM as u8)
                .to_le_bytes()
                .to_vec(),
        ),
        (
            COMMISSIONING_VERSION_KEY,
            "version",
            OTP_STORE_VERSION.to_le_bytes().to_vec(),
        ),
    ]
}

fn expect_value(
    emu: &Emulator,
    case: &str,
    key: u16,
    name: &str,
    want: &[u8],
) -> Result<(), String> {
    match emu.get_commissioning_value(key) {
        (OraResult::Ok, Some(got)) if got == want => Ok(()),
        (result, got) => Err(format!(
            "{case}: {name} got {result:?} {got:02x?}, expected {want:02x?}"
        )),
    }
}

fn expect_absent(emu: &Emulator, case: &str, key: u16, name: &str) -> Result<(), String> {
    match emu.get_commissioning_value(key) {
        (OraResult::NotSupported, _) => Ok(()),
        (result, got) => Err(format!(
            "{case}: {name} got {result:?} {got:02x?}, expected NotSupported"
        )),
    }
}

fn expect_instance(
    emu: &Emulator,
    case: &str,
    manufacturer: &str,
    sig_byte: u8,
) -> Result<(), String> {
    for (key, name, want) in expected(manufacturer, sig_byte) {
        expect_value(emu, case, key, name, &want)?;
    }
    expect_absent(emu, case, UNWRITTEN_KEY, "an unwritten key")
}

fn expect_uncommissioned(emu: &Emulator, case: &str) -> Result<(), String> {
    for (key, name, _) in expected("", 0) {
        expect_absent(emu, case, key, name)?;
    }
    Ok(())
}

pub fn test_commissioning_value(emu: &Emulator) -> Result<(), String> {
    let result = check_commissioning_value(emu);
    Emulator::clear_otp();
    result
}

/// [`test_commissioning_value`], less clearing OTP.
fn check_commissioning_value(emu: &Emulator) -> Result<(), String> {
    Emulator::clear_otp();
    expect_uncommissioned(emu, "OTP unwritten")?;

    // An instance interrupted before its signature isn't complete.
    let first = instance("piers.rocks", 0xa5, FIRST);
    Emulator::clear_otp();
    write(&first[..first.len() - 1]);
    expect_uncommissioned(emu, "an interrupted instance")?;

    Emulator::clear_otp();
    write(&first);
    expect_instance(emu, "one instance", "piers.rocks", 0xa5)?;

    // A later interrupted instance leaves the complete one in use.
    let second = instance("onerom.org", 0x5a, next_page(&first));
    write(&second[..second.len() - 1]);
    expect_instance(emu, "an interrupted second instance", "piers.rocks", 0xa5)?;

    // A later complete instance replaces it.
    write(&second);
    expect_instance(emu, "a second instance", "onerom.org", 0x5a)?;

    // An instance with a version this firmware can't read ends the walk, and
    // only its version is found.
    let unknown = OTP_STORE_VERSION + 1;
    Emulator::set_otp_ecc(next_page(&second), &[first[0].value, unknown]);
    expect_value(
        emu,
        "an unknown version",
        COMMISSIONING_VERSION_KEY,
        "version",
        &unknown.to_le_bytes(),
    )?;
    for (key, name, _) in expected("", 0) {
        if key != COMMISSIONING_VERSION_KEY {
            expect_absent(emu, "an unknown version", key, name)?;
        }
    }
    println!("  Commissioning values: uncommissioned, interrupted, replaced and unknown version");

    Emulator::clear_otp();
    write(&first);
    let invalid = [
        (
            "key OTP_KEY_NONE",
            emu.get_commissioning_value(OTP_KEY_NONE).0,
        ),
        (
            "a NULL value pointer",
            emu.get_commissioning_value_null_out(BOARD_KEY, true, false),
        ),
        (
            "a NULL length pointer",
            emu.get_commissioning_value_null_out(BOARD_KEY, false, true),
        ),
    ];
    for (label, result) in invalid {
        if result != OraResult::InvalidArg {
            return Err(format!("{label}: got {result:?}, expected InvalidArg"));
        }
    }
    println!("  Commissioning values: invalid arguments");
    Ok(())
}

/// `writes`' rows from the first to the last, two bytes each, low byte first.
/// These are the bytes `ora_get_commissioning_instance` returns for the instance
/// `writes` writes.
fn instance_bytes(writes: &[RowWrite]) -> Vec<u8> {
    let mut rows = writes.to_vec();
    rows.sort_by_key(|w| w.row);
    rows.iter().flat_map(|w| w.value.to_le_bytes()).collect()
}

fn expect_whole_instance(emu: &Emulator, case: &str, writes: &[RowWrite]) -> Result<(), String> {
    let want = instance_bytes(writes);
    match emu.get_commissioning_instance() {
        (OraResult::Ok, Some(got)) if got == want => Ok(()),
        (result, got) => Err(format!(
            "{case}: got {result:?} {got:02x?}, expected {want:02x?}"
        )),
    }
}

fn expect_no_instance(emu: &Emulator, case: &str) -> Result<(), String> {
    match emu.get_commissioning_instance() {
        (OraResult::NotSupported, _) => Ok(()),
        (result, got) => Err(format!(
            "{case}: got {result:?} {got:02x?}, expected NotSupported"
        )),
    }
}

pub fn test_commissioning_instance(emu: &Emulator) -> Result<(), String> {
    let result = check_commissioning_instance(emu);
    Emulator::clear_otp();
    result
}

/// [`test_commissioning_instance`], less clearing OTP.
fn check_commissioning_instance(emu: &Emulator) -> Result<(), String> {
    Emulator::clear_otp();
    expect_no_instance(emu, "OTP unwritten")?;

    // An instance interrupted before its signature isn't complete.
    let first = instance("piers.rocks", 0xa5, FIRST);
    write(&first[..first.len() - 1]);
    expect_no_instance(emu, "an interrupted instance")?;

    Emulator::clear_otp();
    write(&first);
    expect_whole_instance(emu, "one instance", &first)?;

    // A later interrupted instance leaves the complete one in use.
    let second = instance("onerom.org", 0x5a, next_page(&first));
    write(&second[..second.len() - 1]);
    expect_whole_instance(emu, "an interrupted second instance", &first)?;

    // A later complete instance replaces it.
    write(&second);
    expect_whole_instance(emu, "a second instance", &second)?;

    // An instance with a version this firmware can't read means none can be
    // used.
    let unknown = OTP_STORE_VERSION + 1;
    Emulator::set_otp_ecc(next_page(&second), &[first[0].value, unknown]);
    expect_no_instance(emu, "an unknown version")?;
    println!("  Commissioning instance: uncommissioned, interrupted, replaced and unknown version");

    Emulator::clear_otp();
    write(&first);
    let invalid = [
        (
            "a NULL instance pointer",
            emu.get_commissioning_instance_null_out(true, false),
        ),
        (
            "a NULL length pointer",
            emu.get_commissioning_instance_null_out(false, true),
        ),
        (
            "both pointers NULL",
            emu.get_commissioning_instance_null_out(true, true),
        ),
    ];
    for (label, result) in invalid {
        if result != OraResult::InvalidArg {
            return Err(format!("{label}: got {result:?}, expected InvalidArg"));
        }
    }
    println!("  Commissioning instance: invalid arguments");
    Ok(())
}
