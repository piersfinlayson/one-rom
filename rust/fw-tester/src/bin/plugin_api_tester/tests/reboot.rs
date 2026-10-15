// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `ora_reboot`.
//!
//! The stub records the bootrom reboot and never reboots, so each test reads
//! back what the firmware passed to the bootrom.  A failed call must pass
//! nothing.

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_metadata::{REBOOT_MAX_DELAY_MS, REBOOT_MIN_DELAY_MS, REBOOT_REQUEST_MIN_SIZE};

// No cast: bindgen is given -fshort-enums to match the C, so these are already
// a byte wide.
const NORMAL: u8 = ffi::ora_reboot_mode_t_ORA_REBOOT_MODE_NORMAL;
const BOOTLOADER: u8 = ffi::ora_reboot_mode_t_ORA_REBOOT_MODE_BOOTLOADER;

/// A delay clear of both limits.
const DELAY_MS: u32 = 500;

fn request(mode: u8, delay_ms: u32) -> ffi::ora_reboot_request_t {
    ffi::ora_reboot_request_t {
        size: std::mem::size_of::<ffi::ora_reboot_request_t>() as u8,
        mode,
        reserved: [0; 2],
        delay_ms,
    }
}

/// Call `ora_reboot` with `req` and check it reboots into `bootloader` after
/// `delay_ms`.
fn expect_reboot(
    emu: &Emulator,
    what: &str,
    req: &ffi::ora_reboot_request_t,
    bootloader: bool,
    delay_ms: u32,
    errors: &mut Vec<String>,
) {
    // Clear any record left by an earlier test.
    emu.rom_reboot();

    let result = emu.reboot(req);
    if result != OraResult::Ok {
        errors.push(format!("{what}: expected Ok, got {result:?}"));
    }
    let want = Some((bootloader, delay_ms));
    let got = emu.rom_reboot();
    if got != want {
        errors.push(format!(
            "{what}: expected bootrom reboot {want:?}, got {got:?}"
        ));
    }
}

/// Check `result` is `want` and the firmware asked the bootrom for nothing.
fn expect_failed(
    emu: &Emulator,
    what: &str,
    result: OraResult,
    want: OraResult,
    errors: &mut Vec<String>,
) {
    if result != want {
        errors.push(format!("{what}: expected {want:?}, got {result:?}"));
    }
    if let Some(got) = emu.rom_reboot() {
        errors.push(format!("{what}: failed but rebooted {got:?}"));
    }
}

fn finish(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Each mode reboots to where it says, after the delay requested.
pub fn test_reboot_modes(emu: &Emulator) -> Result<(), String> {
    let mut errors = Vec::new();
    expect_reboot(
        emu,
        "normal",
        &request(NORMAL, DELAY_MS),
        false,
        DELAY_MS,
        &mut errors,
    );
    expect_reboot(
        emu,
        "bootloader",
        &request(BOOTLOADER, DELAY_MS),
        true,
        DELAY_MS,
        &mut errors,
    );
    finish(errors)
}

/// A delay of zero becomes the shortest delay, and one over the maximum fails.
pub fn test_reboot_delay(emu: &Emulator) -> Result<(), String> {
    let mut errors = Vec::new();
    expect_reboot(
        emu,
        "delay 0",
        &request(NORMAL, 0),
        false,
        REBOOT_MIN_DELAY_MS,
        &mut errors,
    );
    expect_reboot(
        emu,
        "maximum delay",
        &request(NORMAL, REBOOT_MAX_DELAY_MS),
        false,
        REBOOT_MAX_DELAY_MS,
        &mut errors,
    );

    emu.rom_reboot();
    let result = emu.reboot(&request(NORMAL, REBOOT_MAX_DELAY_MS + 1));
    expect_failed(
        emu,
        "over maximum delay",
        result,
        OraResult::InvalidArg,
        &mut errors,
    );
    finish(errors)
}

/// A NULL request, a short size and an unknown mode fail, and a larger size
/// than this firmware's structure is read as far as this firmware's fields go.
pub fn test_reboot_rejects(emu: &Emulator) -> Result<(), String> {
    let mut errors = Vec::new();
    emu.rom_reboot();

    let result = emu.reboot_null();
    expect_failed(
        emu,
        "NULL request",
        result,
        OraResult::InvalidArg,
        &mut errors,
    );

    for size in [0u8, REBOOT_REQUEST_MIN_SIZE - 1] {
        let mut req = request(NORMAL, DELAY_MS);
        req.size = size;
        let result = emu.reboot(&req);
        expect_failed(
            emu,
            &format!("size {size}"),
            result,
            OraResult::InvalidSize,
            &mut errors,
        );
    }

    for mode in [BOOTLOADER + 1, 0xFF] {
        let result = emu.reboot(&request(mode, DELAY_MS));
        expect_failed(
            emu,
            &format!("mode {mode}"),
            result,
            OraResult::NotSupported,
            &mut errors,
        );
    }

    for size in [REBOOT_REQUEST_MIN_SIZE, 255] {
        let mut req = request(BOOTLOADER, DELAY_MS);
        req.size = size;
        expect_reboot(
            emu,
            &format!("size {size}"),
            &req,
            true,
            DELAY_MS,
            &mut errors,
        );
    }
    finish(errors)
}

/// A bootrom reboot that can't be scheduled returns `Error`.
pub fn test_reboot_bootrom_fails(emu: &Emulator) -> Result<(), String> {
    let mut errors = Vec::new();
    emu.rom_reboot();
    emu.set_rom_reboot_fails(true);
    let result = emu.reboot(&request(NORMAL, DELAY_MS));
    emu.set_rom_reboot_fails(false);
    expect_failed(
        emu,
        "bootrom reboot fails",
        result,
        OraResult::Error,
        &mut errors,
    );
    finish(errors)
}

/// The deprecated `ora_reboot_bootsel` reboots into the bootloader after 10ms.
pub fn test_reboot_bootsel(emu: &Emulator) -> Result<(), String> {
    emu.rom_reboot();
    emu.reboot_bootsel();
    let want = Some((true, 10));
    let got = emu.rom_reboot();
    if got != want {
        return Err(format!("expected bootrom reboot {want:?}, got {got:?}"));
    }
    Ok(())
}
