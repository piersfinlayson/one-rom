// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `ora_firmware_state_query`.
//!
//! A test build copies the ROM image with `memcpy` and plugin launch is
//! compiled out. `FIRMWARE_STATE_ROM_LOADED` is set when boot returns, and
//! `FIRMWARE_STATE_PLUGINS_STARTED` is only set by
//! `Emulator::set_plugins_started`.
//!
//! On a device `ROM_LOADED` is also reached when the copy channel has nothing
//! left to copy. The emulator doesn't have a copy channel, so that isn't
//! tested.

use std::cell::Cell;
use std::rc::Rc;

use onerom_fw_emulator::{Emulator, OraResult, ffi};
use onerom_metadata::onerom_firmware_state_t as State;

const ROM_LOADED: u32 = State::FIRMWARE_STATE_ROM_LOADED;
const PLUGINS_STARTED: u32 = State::FIRMWARE_STATE_PLUGINS_STARTED;
const STARTUP_DONE: u32 = State::FIRMWARE_STATE_STARTUP_DONE;
const WAIT: u32 = ffi::ORA_FIRMWARE_STATE_QUERY_FLAG_WAIT;

/// The state furthest from those declared in the schema.
const UNKNOWN_STATE: u32 = 1 << 31;

/// The hook sets `PLUGINS_STARTED` so a query that waits in error returns. A
/// count above 0 means the query waited.
fn counting_hook(emu: &Emulator) -> Rc<Cell<u32>> {
    let calls = Rc::new(Cell::new(0u32));
    let counter = Rc::clone(&calls);
    emu.set_yield_hook(move || {
        counter.set(counter.get() + 1);
        Emulator::set_plugins_started();
    });
    calls
}

fn expect(
    emu: &Emulator,
    errors: &mut Vec<String>,
    states: u32,
    flags: u32,
    want: (OraResult, u32),
) {
    let calls = counting_hook(emu);
    let got = emu.firmware_state_query(states, flags);
    emu.clear_yield_hook();
    if got != want {
        errors.push(format!(
            "query({states:#x}, {flags:#x}): got {:?}/{:#x}, want {:?}/{:#x}",
            got.0, got.1, want.0, want.1
        ));
    }
    if calls.get() != 0 {
        errors.push(format!(
            "query({states:#x}, {flags:#x}) waited, where it returns at once"
        ));
    }
}

pub fn test_states_after_boot(emu: &Emulator) -> Result<(), String> {
    let mut errors = Vec::new();

    let recorded = emu.firmware_states();
    if recorded != ROM_LOADED {
        return Err(format!(
            "runtime info records {recorded:#x} after boot, want ROM_LOADED alone \
             ({ROM_LOADED:#x})"
        ));
    }

    expect(emu, &mut errors, 0, 0, (OraResult::Ok, ROM_LOADED));
    expect(emu, &mut errors, ROM_LOADED, 0, (OraResult::Ok, ROM_LOADED));
    for states in [
        PLUGINS_STARTED,
        STARTUP_DONE,
        ROM_LOADED | PLUGINS_STARTED,
        ROM_LOADED | PLUGINS_STARTED | STARTUP_DONE,
    ] {
        expect(
            emu,
            &mut errors,
            states,
            0,
            (OraResult::NotReady, ROM_LOADED),
        );
    }

    let after = emu.firmware_states();
    if after != recorded {
        errors.push(format!(
            "runtime info moved from {recorded:#x} to {after:#x} across queries, which only \
             read it"
        ));
    }

    if errors.is_empty() {
        println!("  ROM_LOADED reached after boot, PLUGINS_STARTED and STARTUP_DONE not");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub fn test_unknown_state(emu: &Emulator) -> Result<(), String> {
    let mut errors = Vec::new();

    // STARTUP_DONE is the last state declared in the schema.
    let next = STARTUP_DONE << 1;
    for states in [
        next,
        UNKNOWN_STATE,
        ROM_LOADED | UNKNOWN_STATE,
        PLUGINS_STARTED | next,
    ] {
        for flags in [0, WAIT] {
            expect(
                emu,
                &mut errors,
                states,
                flags,
                (OraResult::NotSupported, ROM_LOADED),
            );
        }
    }

    if errors.is_empty() {
        println!("  unknown states refused at once, with and without the wait flag");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub fn test_null_states_out(emu: &Emulator) -> Result<(), String> {
    let mut errors = Vec::new();
    for (states, want) in [
        (ROM_LOADED, OraResult::Ok),
        (PLUGINS_STARTED, OraResult::NotReady),
        (UNKNOWN_STATE, OraResult::NotSupported),
    ] {
        let got = emu.firmware_state_query_null_out(states, 0);
        if got != want {
            errors.push(format!(
                "query({states:#x}) with NULL out: got {got:?}, want {want:?}"
            ));
        }
    }

    if errors.is_empty() {
        println!("  NULL states_out accepted for OK, NOT_READY and NOT_SUPPORTED");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub fn test_reserved_flags(emu: &Emulator) -> Result<(), String> {
    let mut errors = Vec::new();
    let reserved = !WAIT;
    expect(
        emu,
        &mut errors,
        ROM_LOADED,
        reserved,
        (OraResult::Ok, ROM_LOADED),
    );
    expect(
        emu,
        &mut errors,
        PLUGINS_STARTED,
        reserved,
        (OraResult::NotReady, ROM_LOADED),
    );

    if errors.is_empty() {
        println!("  reserved flag bits ignored");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// `STARTUP_DONE` is set along with `PLUGINS_STARTED`, as `ROM_LOADED` is
/// already set.
pub fn test_wait(emu: &Emulator) -> Result<(), String> {
    const SET_ON_CALL: u32 = 3;
    let all = ROM_LOADED | PLUGINS_STARTED | STARTUP_DONE;
    let mut errors = Vec::new();

    let calls = Rc::new(Cell::new(0u32));
    let counter = Rc::clone(&calls);
    emu.set_yield_hook(move || {
        counter.set(counter.get() + 1);
        if counter.get() == SET_ON_CALL {
            Emulator::set_plugins_started();
        }
    });
    let got = emu.firmware_state_query(PLUGINS_STARTED | STARTUP_DONE, WAIT);
    emu.clear_yield_hook();

    if got != (OraResult::Ok, all) {
        errors.push(format!(
            "wait: got {:?}/{:#x}, want Ok/{all:#x}",
            got.0, got.1
        ));
    }
    if calls.get() != SET_ON_CALL {
        errors.push(format!(
            "wait handed over {} time(s), and the state was set on call {SET_ON_CALL}",
            calls.get()
        ));
    }
    let recorded = emu.firmware_states();
    if recorded != all {
        errors.push(format!(
            "runtime info records {recorded:#x} once plugins started, want {all:#x}"
        ));
    }

    expect(emu, &mut errors, all, WAIT, (OraResult::Ok, all));
    expect(emu, &mut errors, STARTUP_DONE, 0, (OraResult::Ok, all));

    if errors.is_empty() {
        println!("  wait returned once PLUGINS_STARTED and STARTUP_DONE were reached");
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
