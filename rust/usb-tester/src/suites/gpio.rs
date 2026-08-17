// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Bounded GPIO holds: the part of GPIO control the plugin owns rather than the
//! firmware.
//!
//! `ora_gpio_set` owns the safety model and refuses a pin One ROM is using.
//! What the plugin owns is the timing of a bounded hold, and the rule the whole
//! feature exists for: a pin must never be left asserted with nothing scheduled
//! to release it.  Every scenario here is a way that could go wrong — a hold
//! superseded, a hold cancelled, the release slots exhausted — and what the pin
//! is doing afterwards is read back through the firmware.
//!
//! The commands themselves are covered in the picobootx suite.  This is what
//! happens over the passes that follow one.

use onerom_fw_emulator::OraResult;

use crate::device::Device;
use crate::{Ctx, Scenario};
use onerom_plugin_tester::run::Outcome;

use super::picobootx::{
    CMD_GPIO_SET, GPIO_FLAG_FORCE, GPIO_HIGH, GPIO_INPUT, GPIO_LOW, OK, PRECONDITION_NOT_MET,
    gpio_set_args,
};

/// Release slots the plugin has, from usb_gpio.h.
const RELEASES: u8 = 8;

/// Where the device's uptime returns to zero, 49.7 days in.
const WRAP_MS: u64 = 1u64 << 32;

/// What a pin is doing, as the firmware reports it.
fn pin(dev: &Device, gpio: u8) -> Result<(bool, bool), String> {
    let (result, info) = dev.emulator().gpio_query(gpio);
    if result != OraResult::Ok {
        return Err(format!("could not query GPIO {gpio}: {result:?}"));
    }
    Ok((info.is_output != 0, info.level != 0))
}

/// Drive a pin, optionally for a bounded period.
fn drive(
    dev: &mut Device,
    gpio: u8,
    state: u8,
    after_state: u8,
    duration_ms: u32,
) -> Result<i32, String> {
    let args = gpio_set_args(gpio, state, after_state, GPIO_FLAG_FORCE, duration_ms);
    Ok(dev.dispatch(CMD_GPIO_SET, 0, &args))
}

/// The highest GPIOs, which serving is least likely to want.
fn free_pins(ctx: &Ctx, count: u8) -> Vec<u8> {
    ((ctx.num_gpios - count)..ctx.num_gpios).collect()
}

// ---------------------------------------------------------------------------

/// A hold ends in the state the command named, not simply released.
///
/// `after_state` is the host's say in where the pin is left, so a hold that
/// always released to an input would be wrong in a way a test that only checked
/// "no longer driven" would miss.
fn a_hold_ends_in_the_state_it_was_given(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    let gpio = free_pins(ctx, 1)[0];

    // High for 50ms, then low — driven, not released.
    if drive(dev, gpio, GPIO_HIGH, GPIO_LOW, 50)? != OK {
        return Err(format!("a bounded hold on GPIO {gpio} was refused"));
    }
    dev.advance_ms(50);
    dev.step()?;

    let (is_output, level) = pin(dev, gpio)?;
    if !is_output {
        return Err(format!(
            "GPIO {gpio} was released to an input, but the hold named a driven level"
        ));
    }
    if level {
        return Err(format!(
            "GPIO {gpio} is still high, not the low it was to end at"
        ));
    }

    Ok(Outcome::Pass)
}

/// A second command on the same pin supersedes the first hold.
///
/// The pin must not be released by the hold the second command replaced —
/// which is what would happen if the plugin claimed a second slot rather than
/// reusing the pin's own.
fn a_later_command_supersedes_a_hold(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    let gpio = free_pins(ctx, 1)[0];

    if drive(dev, gpio, GPIO_HIGH, GPIO_INPUT, 50)? != OK {
        return Err("the first hold was refused".to_string());
    }
    // Replaced before it expires, with a longer one.
    dev.advance_ms(20);
    dev.step()?;
    if drive(dev, gpio, GPIO_HIGH, GPIO_INPUT, 200)? != OK {
        return Err("the replacement hold was refused".to_string());
    }

    // Past the first hold's deadline: the pin must still be driven, or the
    // superseded release fired.
    dev.advance_ms(40);
    dev.step()?;
    let (is_output, _) = pin(dev, gpio)?;
    if !is_output {
        return Err(format!(
            "GPIO {gpio} was released at the superseded hold's deadline"
        ));
    }

    // Past the replacement's: now it goes.
    dev.advance_ms(200);
    dev.step()?;
    let (is_output, _) = pin(dev, gpio)?;
    if is_output {
        return Err(format!(
            "GPIO {gpio} outlived the hold that replaced the first"
        ));
    }

    Ok(Outcome::Pass)
}

/// A command with no duration latches the pin and cancels any pending release.
///
/// Otherwise the earlier hold's deadline would release a pin the host has since
/// asked to hold indefinitely.
fn an_unbounded_command_cancels_a_hold(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    let gpio = free_pins(ctx, 1)[0];

    if drive(dev, gpio, GPIO_HIGH, GPIO_INPUT, 50)? != OK {
        return Err("the bounded hold was refused".to_string());
    }
    if drive(dev, gpio, GPIO_HIGH, GPIO_INPUT, 0)? != OK {
        return Err("the unbounded command was refused".to_string());
    }

    dev.advance_ms(500);
    dev.step()?;

    let (is_output, level) = pin(dev, gpio)?;
    if !is_output || !level {
        return Err(format!(
            "GPIO {gpio} was released at the cancelled hold's deadline"
        ));
    }

    Ok(Outcome::Pass)
}

/// With every release slot in use, a further hold is refused rather than
/// asserted.
///
/// A pin driven with nothing scheduled to release it is the one outcome bounded
/// holds exist to rule out, so the refusal has to come before anything is
/// driven.
fn a_hold_with_no_slot_left_is_refused(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    let pins = free_pins(ctx, RELEASES + 1);
    if pins.len() < usize::from(RELEASES) + 1 {
        return Ok(Outcome::Skip(format!(
            "the device has {} GPIOs, too few to fill {RELEASES} release slots and ask for another",
            ctx.num_gpios
        )));
    }

    for &gpio in &pins[..usize::from(RELEASES)] {
        if drive(dev, gpio, GPIO_HIGH, GPIO_INPUT, 10_000)? != OK {
            return Err(format!("filling the slots: GPIO {gpio} was refused"));
        }
    }

    let spare = pins[usize::from(RELEASES)];
    let st = drive(dev, spare, GPIO_HIGH, GPIO_INPUT, 10_000)?;
    if st != PRECONDITION_NOT_MET {
        return Err(format!(
            "a hold with no slot left answered {st}, not PRECONDITION_NOT_MET"
        ));
    }

    // And it was refused before the pin was touched.
    let (is_output, _) = pin(dev, spare)?;
    if is_output {
        return Err(format!(
            "GPIO {spare} was driven by a hold that was refused, so nothing will release it"
        ));
    }

    // A pin that already holds a slot is not refused, however full they are:
    // the slot is reused rather than claimed again.
    let held = pins[0];
    if drive(dev, held, GPIO_HIGH, GPIO_INPUT, 10_000)? != OK {
        return Err(format!(
            "GPIO {held}, which already holds a slot, was refused when the slots were full"
        ));
    }

    Ok(Outcome::Pass)
}

/// A hold placed just before the clock wraps runs for the length it was given.
///
/// A deadline past the wrap is a small number while the clock is still a large
/// one, so for that last stretch the two cannot be compared as they stand: a
/// device reading them as plain unsigned values sees `now` above the deadline
/// and releases the pin the moment the command lands, most of a hold early.
/// That is why the check before the wrap matters more than the one after it.
fn a_hold_survives_the_clock_wrapping(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    let gpio = free_pins(ctx, 1)[0];

    // 20ms short of the wrap, holding for 50 - so the deadline lands 30ms the
    // far side of it.  Placed against the clock the device actually has, not
    // against a zero this scenario would otherwise be assuming: overshoot the
    // wrap and every check below still passes, on a device that gets this
    // wrong as much as on one that gets it right.
    dev.advance_ms(WRAP_MS - 20 - u64::from(dev.uptime_ms()));
    if drive(dev, gpio, GPIO_HIGH, GPIO_LOW, 50)? != OK {
        return Err(format!("a hold across the wrap on GPIO {gpio} was refused"));
    }

    dev.step()?;
    let (is_output, level) = pin(dev, gpio)?;
    if !is_output || !level {
        return Err(format!(
            "GPIO {gpio} was released immediately, before the clock had even wrapped"
        ));
    }

    // Over the wrap, with 30ms of the hold left to run.
    dev.advance_ms(20);
    dev.step()?;
    let (is_output, level) = pin(dev, gpio)?;
    if !is_output || !level {
        return Err(format!("GPIO {gpio} was released by the wrap itself"));
    }

    // And the deadline, now an ordinary one, still ends the hold.
    dev.advance_ms(30);
    dev.step()?;
    let (is_output, level) = pin(dev, gpio)?;
    if !is_output || level {
        return Err(format!(
            "GPIO {gpio} did not end low at a deadline the other side of the wrap"
        ));
    }

    Ok(Outcome::Pass)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "gpio.a_hold_ends_in_the_state_it_was_given",
        about: "a hold leaves the pin in the after-state the command named",
        run: a_hold_ends_in_the_state_it_was_given,
        before_start: None,
    },
    Scenario {
        name: "gpio.a_later_command_supersedes_a_hold",
        about: "a second command on a pin replaces its hold rather than adding one",
        run: a_later_command_supersedes_a_hold,
        before_start: None,
    },
    Scenario {
        name: "gpio.an_unbounded_command_cancels_a_hold",
        about: "a command with no duration latches the pin and drops its pending release",
        run: an_unbounded_command_cancels_a_hold,
        before_start: None,
    },
    Scenario {
        name: "gpio.a_hold_with_no_slot_left_is_refused",
        about: "with every release slot taken, a further hold is refused before driving",
        run: a_hold_with_no_slot_left_is_refused,
        before_start: None,
    },
    Scenario {
        name: "gpio.a_hold_survives_the_clock_wrapping",
        about: "a hold whose deadline is past the 49.7-day wrap runs its full length",
        run: a_hold_survives_the_clock_wrapping,
        before_start: None,
    },
];
