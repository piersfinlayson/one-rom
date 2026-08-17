// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The status LED, and the modes that outlive the command that started them.
//!
//! `ONEROM_CMD_SET_LED` is deferred to the task loop rather than applied at
//! dispatch, because a mode is a state machine that carries on afterwards and
//! nothing about it can be refused.  So every scenario here is in two parts:
//! what the command did, and what the passes after it did.
//!
//! What the LED is doing is read back through the firmware's own
//! `status_led_enabled`, which is the live state and the channel the other
//! plugin reads — so a scenario asserts the same thing another plugin would
//! see, rather than the plugin's private copy.

use onerom_fw_emulator::OraResult;

use crate::device::Device;
use crate::{Ctx, Scenario};
use onerom_plugin_tester::run::Outcome;

use super::picobootx::{CMD_SET_LED, OK};

// Sub-commands, from usb_custom_pbx.h.
const LED_OFF: u8 = 0x00;
const LED_ON: u8 = 0x01;
const LED_BEACON: u8 = 0x02;

// The beacon's shape, from usb_led.h.
const BEACON_DURATION_MS: u64 = 2500;
const BEACON_TOGGLE_MS: u64 = 50;

/// A SET_LED argument block.
fn set_led_args(sub_cmd: u8) -> [u8; 16] {
    let mut args = [0u8; 16];
    // args[0] is the LED identifier, of which a device has one.
    args[1] = sub_cmd;
    args
}

/// What the device's status LED is doing, as another plugin would read it.
fn led_state(dev: &Device) -> Result<bool, String> {
    match dev.status_led() {
        (OraResult::Ok, Some(state)) => Ok(state != 0),
        (r, _) => Err(format!("could not read the status LED state: {r:?}")),
    }
}

/// Send a SET_LED and let the task loop apply it.
fn set_led(dev: &mut Device, sub_cmd: u8) -> Result<(), String> {
    let st = dev.dispatch(CMD_SET_LED, 0, &set_led_args(sub_cmd));
    if st != OK {
        return Err(format!("SET_LED {sub_cmd} was refused with status {st}"));
    }
    dev.step()
}

// ---------------------------------------------------------------------------

/// The LED goes on and off, and the command alone does not move it.
///
/// Dispatch records what was asked for and the task loop applies it, so the
/// discriminating half is the state between the two: a plugin that drove the
/// pin from dispatch would already have moved it.
fn the_led_is_driven_from_the_task_loop(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    set_led(dev, LED_OFF)?;
    if led_state(dev)? {
        return Err("the LED is on after being told to go off".to_string());
    }

    let st = dev.dispatch(CMD_SET_LED, 0, &set_led_args(LED_ON));
    if st != OK {
        return Err(format!("SET_LED on was refused with status {st}"));
    }
    if led_state(dev)? {
        return Err("the LED moved during dispatch, before the task loop ran".to_string());
    }

    dev.step()?;
    if !led_state(dev)? {
        return Err("the LED is still off a pass after being told to go on".to_string());
    }

    Ok(Outcome::Pass)
}

/// A beacon blinks, rather than settling on a level.
///
/// The toggle interval is what makes it a beacon rather than a light, so the
/// assertion is that the state changes across it and does not change within it.
fn a_beacon_blinks(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    set_led(dev, LED_BEACON)?;
    if !led_state(dev)? {
        return Err("a beacon did not light the LED to begin with".to_string());
    }

    // One millisecond short of a toggle: still where it was.
    dev.advance_ms(BEACON_TOGGLE_MS - 1);
    dev.step()?;
    if !led_state(dev)? {
        return Err(format!(
            "the beacon toggled {}ms into a {BEACON_TOGGLE_MS}ms interval",
            BEACON_TOGGLE_MS - 1
        ));
    }

    dev.advance_ms(1);
    dev.step()?;
    if led_state(dev)? {
        return Err("the beacon did not toggle when its interval elapsed".to_string());
    }

    // And back again, so this is not a beacon that turns off once.
    dev.advance_ms(BEACON_TOGGLE_MS);
    dev.step()?;
    if !led_state(dev)? {
        return Err("the beacon did not toggle a second time".to_string());
    }

    Ok(Outcome::Pass)
}

/// A beacon ends, and leaves the LED as it found it.
///
/// The state before the beacon is what it restores, so a device whose LED was
/// off is dark again afterwards and one whose LED was on is lit — and a beacon
/// that simply stopped mid-blink would leave it at whichever half it reached.
fn a_beacon_restores_what_it_interrupted(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    for (before, name) in [(LED_OFF, "off"), (LED_ON, "on")] {
        set_led(dev, before)?;
        let was = led_state(dev)?;

        set_led(dev, LED_BEACON)?;
        dev.advance_ms(BEACON_DURATION_MS + BEACON_TOGGLE_MS);
        dev.step()?;

        let now = led_state(dev)?;
        if now != was {
            return Err(format!(
                "a beacon over an LED that was {name} left it {}",
                if now { "on" } else { "off" }
            ));
        }

        // And it has stopped, rather than being caught at the right half of a
        // blink it is still running.
        dev.advance_ms(BEACON_TOGGLE_MS * 4);
        dev.step()?;
        if led_state(dev)? != was {
            return Err(format!(
                "the beacon was still blinking after its {BEACON_DURATION_MS}ms ran out"
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// A beacon restarted while one is running still restores what the first
/// interrupted.
///
/// The restart arrives with the LED lit by the beacon itself, so a device that
/// captured the state again at that point would save the blink rather than the
/// LED the host left behind — and would finish by lighting an LED that started
/// dark.
fn a_restart_keeps_the_state_from_before_the_first_beacon(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    set_led(dev, LED_OFF)?;
    if led_state(dev)? {
        return Err("the LED is on after being told to go off".to_string());
    }

    set_led(dev, LED_BEACON)?;
    if !led_state(dev)? {
        return Err("a beacon did not light the LED to begin with".to_string());
    }

    // Restarted inside the first toggle interval, so the LED is still lit and
    // the state a second capture would take is the opposite of the right one.
    dev.advance_ms(BEACON_TOGGLE_MS - 30);
    set_led(dev, LED_BEACON)?;
    if !led_state(dev)? {
        return Err("the restarted beacon did not leave the LED lit".to_string());
    }

    dev.advance_ms(BEACON_DURATION_MS + BEACON_TOGGLE_MS);
    dev.step()?;
    if led_state(dev)? {
        return Err(
            "the restarted beacon left the LED on, which is the blink it interrupted rather \
             than the dark LED the first beacon found"
                .to_string(),
        );
    }

    Ok(Outcome::Pass)
}

/// Once a beacon has finished, the plugin stops driving the LED.
///
/// The LED is shared, so "finished" has to mean the plugin has let go of it,
/// not merely that it left the right value behind: a device still in beacon
/// mode goes on writing its restored state every pass, and would overwrite
/// whatever another plugin does with the LED next.
fn a_finished_beacon_stops_driving_the_led(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    set_led(dev, LED_ON)?;
    set_led(dev, LED_BEACON)?;
    dev.advance_ms(BEACON_DURATION_MS + BEACON_TOGGLE_MS);
    dev.step()?;
    if !led_state(dev)? {
        return Err("the beacon did not restore the lit LED it interrupted".to_string());
    }

    // Another plugin takes the LED the other way.
    dev.set_status_led_elsewhere(false);
    if led_state(dev)? {
        return Err("the LED did not follow the other plugin that drove it".to_string());
    }

    dev.step_n(5)?;
    if led_state(dev)? {
        return Err(
            "the plugin lit the LED again after the beacon had finished, so it never left \
             beacon mode"
                .to_string(),
        );
    }

    Ok(Outcome::Pass)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "led.the_led_is_driven_from_the_task_loop",
        about: "SET_LED is recorded at dispatch and applied by the task loop",
        run: the_led_is_driven_from_the_task_loop,
        before_start: None,
    },
    Scenario {
        name: "led.a_beacon_blinks",
        about: "a beacon changes the LED across its toggle interval, not within it",
        run: a_beacon_blinks,
        before_start: None,
    },
    Scenario {
        name: "led.a_beacon_restores_what_it_interrupted",
        about: "a beacon ends by putting the LED back where it found it",
        run: a_beacon_restores_what_it_interrupted,
        before_start: None,
    },
    Scenario {
        name: "led.a_restart_keeps_the_state_from_before_the_first_beacon",
        about: "a beacon restarted mid-beacon restores what the first one interrupted",
        run: a_restart_keeps_the_state_from_before_the_first_beacon,
        before_start: None,
    },
    Scenario {
        name: "led.a_finished_beacon_stops_driving_the_led",
        about: "a finished beacon leaves the shared LED to whoever drives it next",
        run: a_finished_beacon_stops_driving_the_led,
        before_start: None,
    },
];
