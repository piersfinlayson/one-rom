// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! ONEROM_CMD_SET_STANDBY, read back through `FIRMWARE_FLAG_STANDBY` in
//! runtime info.
//!
//! This harness doesn't drive the bus, so the data pins in standby are left to
//! the plugin API tester and the PIO tester.

use onerom_fw_emulator::Emulator;
use onerom_fw_emulator::ffi::api_id_t_ORA_ID_SET_STANDBY;
use onerom_plugin_tester::run::Outcome;

use crate::device::Device;
use crate::{Ctx, Scenario};

use super::picobootx::{
    CMD_SET_STANDBY, FEAT_STANDBY, INVALID_ARG, NOT_PERMITTED, OK, caps, u32_at,
};

/// A SET_STANDBY argument block.
fn standby_args(standby: u8) -> [u8; 16] {
    let mut args = [0u8; 16];
    args[0] = standby;
    args
}

/// Send SET_STANDBY and fail unless it is answered with `status` and One ROM
/// is then in standby exactly when `standby` is true.
fn set_standby(
    dev: &mut Device,
    args: &[u8; 16],
    status: i32,
    standby: bool,
) -> Result<(), String> {
    let st = dev.dispatch(CMD_SET_STANDBY, 0, args);
    if st != status {
        return Err(format!(
            "SET_STANDBY {} answered {st}, not {status}",
            args[0]
        ));
    }
    if dev.in_standby() != standby {
        return Err(format!(
            "One ROM is {} after SET_STANDBY {}",
            if standby { "serving" } else { "in standby" },
            args[0]
        ));
    }
    Ok(())
}

/// Standby turns on and off as the command is answered, and a command for the
/// state One ROM is already in succeeds.
///
/// The state is read without a pass of the plugin's loop in between, so a
/// change deferred to the loop fails here.  The sequence starts with off so
/// both changes are made whichever state the set boots in.
fn the_command_turns_standby_on_and_off(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    for standby in [0u8, 1, 1, 0, 0] {
        set_standby(dev, &standby_args(standby), OK, standby == 1)?;
    }
    Ok(Outcome::Pass)
}

/// A standby value other than 0 or 1 is refused as INVALID_ARG and leaves One
/// ROM as it was, in standby or serving.
fn a_standby_value_other_than_0_or_1_is_refused(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    for start in [0u8, 1] {
        set_standby(dev, &standby_args(start), OK, start == 1)?;
        for standby in [2u8, 0xFF] {
            set_standby(dev, &standby_args(standby), INVALID_ARG, start == 1)?;
        }
    }
    Ok(Outcome::Pass)
}

/// The reserved argument bytes are ignored, as usb_custom_pbx.h requires of
/// every reserved field.
fn the_reserved_bytes_are_ignored(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    for standby in [1u8, 0] {
        let mut args = [0xFFu8; 16];
        args[0] = standby;
        set_standby(dev, &args, OK, standby == 1)?;
    }
    Ok(Outcome::Pass)
}

/// Withhold `ORA_ID_SET_STANDBY`, as on firmware older than the call.
fn withhold_set_standby(_emu: &Emulator) {
    let ids = [api_id_t_ORA_ID_SET_STANDBY];
    // SAFETY: the shim copies the identifiers before returning.
    unsafe {
        onerom_plugin_tester::ffi::ora_host_test_withhold_api(ids.as_ptr(), ids.len() as u32)
    };
}

/// On firmware without `ORA_ID_SET_STANDBY` standby isn't offered and
/// SET_STANDBY returns NOT_PERMITTED, leaving One ROM as it was.
fn the_command_is_not_permitted_without_the_api_call(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let caps = caps(dev)?;
    if u32_at(&caps, 4) & FEAT_STANDBY != 0 {
        return Err("ONEROM_FEAT_STANDBY is set, but ORA_ID_SET_STANDBY is withheld".to_string());
    }

    let booted = dev.in_standby();
    for standby in [0u8, 1] {
        set_standby(dev, &standby_args(standby), NOT_PERMITTED, booted)?;
    }
    Ok(Outcome::Pass)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "standby.the_command_turns_standby_on_and_off",
        about: "standby turns on and off at dispatch, and a repeat of either succeeds",
        run: the_command_turns_standby_on_and_off,
        before_start: None,
    },
    Scenario {
        name: "standby.a_standby_value_other_than_0_or_1_is_refused",
        about: "SET_STANDBY with a value other than 0 or 1 answers INVALID_ARG and changes nothing",
        run: a_standby_value_other_than_0_or_1_is_refused,
        before_start: None,
    },
    Scenario {
        name: "standby.the_reserved_bytes_are_ignored",
        about: "SET_STANDBY's reserved bytes are ignored",
        run: the_reserved_bytes_are_ignored,
        before_start: None,
    },
    Scenario {
        name: "standby.the_command_is_not_permitted_without_the_api_call",
        about: "without ORA_ID_SET_STANDBY, standby isn't offered and SET_STANDBY answers NOT_PERMITTED",
        run: the_command_is_not_permitted_without_the_api_call,
        before_start: Some(withhold_set_standby),
    },
];
