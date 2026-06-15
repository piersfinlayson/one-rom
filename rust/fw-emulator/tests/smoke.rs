// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Smoke test: boot the firmware emulator and verify flag query calls work.
//!
//! Run with:
//!   cargo test -- --test-threads=1
//!
//! The --test-threads=1 constraint is important: firmware_main writes global
//! C state, so parallel test cases that each call Emulator::boot() will race.
//! A static Mutex guard can replace this once the suite grows.

use onerom_fw_emulator::Emulator;

/// Boot the firmware and confirm the limp-mode query doesn't crash.
///
/// The expected value (false) reflects a clean boot with no fault injection.
/// Adjust if the firmware stubs set limp mode for a different reason.
#[test]
fn boot_limp_mode_is_false() {
    Emulator::set_logging(false);
    let emu = Emulator::boot();
    assert!(
        !emu.limp_mode(),
        "firmware reported limp mode after a clean boot"
    );
}

/// Boot the firmware and confirm the PIOs-enabled query doesn't crash.
///
/// The assertion is intentionally loose (just checking it's a bool) until
/// we know what value firmware_main leaves this flag in on the host stub.
/// Tighten once behaviour is confirmed.
#[test]
fn boot_pios_enabled_is_readable() {
    Emulator::set_logging(false);
    let emu = Emulator::boot();
    let _enabled: bool = emu.pios_enabled();
    // no assertion yet — prove the call works without panicking
}
