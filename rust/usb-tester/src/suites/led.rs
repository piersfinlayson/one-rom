// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The LEDs a SET_LED addresses, and the modes that outlive the command that
//! started them.
//!
//! The command names a channel, and both the channels a device has belong to
//! the firmware's LED engine.  The plugin forwards and reports what the engine
//! answered - it holds no LED state of its own - so a command is applied as it
//! is answered, and a refusal reaches the host rather than being discovered a
//! pass later.
//!
//! A mode that carries on afterwards - a beacon, a flame, a fade - is driven by
//! the engine from a timer interrupt.  This process has no interrupt, so a
//! scenario stands where it does: `engine_frame` moves the clock to the
//! deadline the engine named and runs the frame.
//!
//! What the status LED is doing is read back through the firmware's own
//! `status_led_enabled`, which is the live state and the channel the other
//! plugin reads — so a scenario asserts the same thing another plugin would
//! see, rather than the plugin's private copy.  What the RGB LED is doing is
//! read back through `ORA_ID_LED_GET`, for the same reason: it is the engine's
//! state, not the plugin's account of what it asked for.
//!
//! What this suite cannot see is the pixel.  The engine's colour is asserted
//! where the plugin put it, not on the wire the WS2812 reads, so a fault in the
//! engine's own PIO output would leave these green.  Nor can it see a board's
//! RGB LED where there is none: the engine refuses every request for an LED the
//! board does not have, so the scenarios that need one skip there and only the
//! refusal is covered.

use onerom_fw_emulator::OraResult;

use crate::device::{Device, LedState};
use crate::{Ctx, Scenario};
use onerom_plugin_tester::run::Outcome;

use super::picobootx::{CMD_SET_LED, INVALID_ARG, NOT_FOUND, OK};

// Sub-commands, from usb_custom_pbx.h.
const LED_OFF: u8 = 0x00;
const LED_ON: u8 = 0x01;
const LED_BEACON: u8 = 0x02;
const LED_FLAME: u8 = 0x03;
const LED_CYCLE: u8 = 0x04;
const LED_BREATHE: u8 = 0x05;
const LED_BLINK: u8 = 0x06;

// The default periods the engine applies when a request names none, from
// pioled.c.
const CYCLE_PERIOD_MS: u16 = 5000;
const BREATHE_PERIOD_MS: u16 = 5000;
const BLINK_PERIOD_MS: u16 = 1000;

// The steps a repetition is divided into, from pioled.c.
const CYCLE_STEPS: usize = 100;
const BREATHE_STEPS: usize = 100;

// The LEDs a SET_LED can address, from usb_custom_pbx.h.
const LED_ID_STATUS: u8 = 0x00;
const LED_ID_RGB: u8 = 0x01;

// The longest hold SET_LED accepts, from usb_custom_pbx.h.
const MAX_HOLD_MS: u32 = 60000;

// LED_QUERY and its response, from usb_custom_pbx.h.
const CMD_LED_QUERY: u8 = 0x05 | 0x80;
const LED_STATE_LEN: u32 = 16;

// Sub-commands this suite names, from onerom_led_subcmd_t.
const SUB_OFF: u8 = 0x00;
const SUB_ON: u8 = 0x01;
const SUB_BEACON: u8 = 0x02;
const SUB_CYCLE: u8 = 0x04;
const SUB_BREATHE: u8 = 0x05;
const SUB_BLINK: u8 = 0x06;
const SUB_FLAME: u8 = 0x03;

// What the firmware reports for an LED the board does not have.
const GPIO_NONE: u8 = 0xFF;

// The beacon's shape, from usb_led.h.
const BEACON_DURATION_MS: u64 = 2500;
const BEACON_TOGGLE_MS: u64 = 50;

/// A SET_LED argument block for the status LED.
fn set_led_args(sub_cmd: u8) -> [u8; 16] {
    let mut args = [0u8; 16];
    args[0] = LED_ID_STATUS;
    args[1] = sub_cmd;
    args
}

/// A SET_LED argument block for the RGB LED.
///
/// Every field after the sub-command reads as "the device chooses" when zero,
/// so a scenario says the ones it means to assert on and leaves the rest.  The
/// offsets are the wire's, which is what makes a plugin reading a field from
/// the wrong byte a failure here.
fn rgb_args(
    sub_cmd: u8,
    colour: (u8, u8, u8),
    brightness: u8,
    period_ms: u16,
    hold_ms: u32,
) -> [u8; 16] {
    let mut args = [0u8; 16];
    args[0] = LED_ID_RGB;
    args[1] = sub_cmd;
    args[4] = colour.0;
    args[5] = colour.1;
    args[6] = colour.2;
    args[7] = brightness;
    args[8..10].copy_from_slice(&period_ms.to_le_bytes());
    args[10..14].copy_from_slice(&hold_ms.to_le_bytes());
    args
}

/// Send a SET_LED for the RGB LED, which the plugin applies as it answers.
fn set_rgb(dev: &mut Device, args: &[u8; 16]) -> Result<(), String> {
    let st = dev.dispatch(CMD_SET_LED, 0, args);
    if st != OK {
        return Err(format!(
            "a SET_LED for the RGB LED was refused with status {st}"
        ));
    }
    Ok(())
}

/// What the RGB LED is doing, as the firmware's engine reports it.
fn rgb_state(dev: &Device) -> Result<LedState, String> {
    dev.led(LED_ID_RGB)
}

/// A scenario that needs an RGB LED, on a board that has none.
///
/// Which boards those are is the firmware's answer rather than a list kept
/// here, so a board that gains the LED runs these without a change.
fn no_rgb_led() -> Outcome {
    Outcome::Skip(
        "this board has no RGB LED, so the engine refuses every request for one".to_string(),
    )
}

/// How an LED reads, for a message.
fn describe(led: &LedState) -> String {
    format!(
        "mode {} colour {:02x},{:02x},{:02x} brightness {} period {}ms",
        led.mode, led.red, led.green, led.blue, led.brightness, led.period_ms
    )
}

/// What the device's status LED is doing, as another plugin would read it.
fn led_state(dev: &Device) -> Result<bool, String> {
    match dev.status_led() {
        (OraResult::Ok, Some(state)) => Ok(state != 0),
        (r, _) => Err(format!("could not read the status LED state: {r:?}")),
    }
}

/// Send a SET_LED for the status LED, which the engine applies as it answers.
///
/// A task loop pass follows anyway, so a scenario built on this is not asserting
/// anything about which of the two moved the LED.  The scenario that does assert
/// that dispatches for itself.
fn set_led(dev: &mut Device, sub_cmd: u8) -> Result<(), String> {
    let st = dev.dispatch(CMD_SET_LED, 0, &set_led_args(sub_cmd));
    if st != OK {
        return Err(format!("SET_LED {sub_cmd} was refused with status {st}"));
    }
    dev.step()
}

// ---------------------------------------------------------------------------

/// The LED goes on and off as the command is answered, not a pass later.
///
/// The engine holds the mode, so the plugin has nothing to apply afterwards.
/// The discriminating half is the state between the command and the next pass:
/// a plugin that recorded the request instead would not have moved the LED yet.
fn the_status_led_is_set_as_the_command_is_answered(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    // Armed with a dispatch and a task loop pass, so a plugin that recorded the
    // command and applied it later is armed too.
    set_led(dev, LED_OFF)?;
    dev.step()?;
    if led_state(dev)? {
        return Err("the LED is on after being told to go off".to_string());
    }

    let st = dev.dispatch(CMD_SET_LED, 0, &set_led_args(LED_ON));
    if st != OK {
        return Err(format!("SET_LED on was refused with status {st}"));
    }

    // No pass in between: the engine holds the mode, so the command applied it.
    if !led_state(dev)? {
        return Err(
            "the LED is still off, so the command was recorded rather than applied".to_string(),
        );
    }

    // And a pass does not move it again.
    dev.step()?;
    if !led_state(dev)? {
        return Err("a task loop pass turned the LED back off".to_string());
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
    dev.led_frame();
    if !led_state(dev)? {
        return Err(format!(
            "the beacon toggled {}ms into a {BEACON_TOGGLE_MS}ms interval",
            BEACON_TOGGLE_MS - 1
        ));
    }

    dev.advance_ms(1);
    dev.led_frame();
    if led_state(dev)? {
        return Err("the beacon did not toggle when its interval elapsed".to_string());
    }

    // And back again, so this is not a beacon that turns off once.
    dev.advance_ms(BEACON_TOGGLE_MS);
    dev.led_frame();
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
        run_beacon_out(dev)?;

        let now = led_state(dev)?;
        if now != was {
            return Err(format!(
                "a beacon over an LED that was {name} left it {}",
                if now { "on" } else { "off" }
            ));
        }

        // And it has stopped, rather than being caught at the right half of a
        // blink it is still running.  The frame runs whether or not anything is
        // due, which is what makes a beacon still going show itself.
        dev.advance_ms(BEACON_TOGGLE_MS * 4);
        dev.led_frame();
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

    run_beacon_out(dev)?;
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
    run_beacon_out(dev)?;
    if !led_state(dev)? {
        return Err("the beacon did not restore the lit LED it interrupted".to_string());
    }

    // Another plugin takes the LED the other way.
    dev.set_status_led_elsewhere(false);
    if led_state(dev)? {
        return Err("the LED did not follow the other plugin that drove it".to_string());
    }

    dev.advance_ms(BEACON_TOGGLE_MS * 5);
    dev.led_frame();
    dev.step_n(5)?;
    if led_state(dev)? {
        return Err(
            "the LED was lit again after the beacon had finished, so the engine never left \
             beacon mode"
                .to_string(),
        );
    }

    Ok(Outcome::Pass)
}

/// A board without the RGB LED refuses the command rather than swallowing it.
///
/// The engine answers NOT_SUPPORTED for an LED the board does not have, and the
/// plugin owes the host NOT_FOUND for it — the same answer as a channel that
/// does not exist, and one a host reads as "this device has no such LED" rather
/// than "this device is too old to be asked", which is what UNKNOWN_CMD would
/// have said.  Which way round it goes is the board's business, so both are
/// asserted, and the board is asked of the firmware.
fn the_rgb_led_is_refused_where_the_board_has_none(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let before = rgb_state(dev)?;
    let st = dev.dispatch(
        CMD_SET_LED,
        0,
        &rgb_args(LED_FLAME, (0x10, 0x20, 0x30), 40, 750, 0),
    );

    if before.present {
        if st != OK {
            return Err(format!(
                "a board with an RGB LED refused a SET_LED for it with status {st}"
            ));
        }
        if rgb_state(dev)? == before {
            return Err(format!(
                "the command was accepted, but the engine is still at {}",
                describe(&before)
            ));
        }
        return Ok(Outcome::Pass);
    }

    if st != NOT_FOUND {
        return Err(format!(
            "a board with no RGB LED answered {st}, not NOT_FOUND"
        ));
    }
    if rgb_state(dev)? != before {
        return Err("a refused request moved the engine's RGB channel anyway".to_string());
    }

    // And this device is not refusing everything: the LED it does have still
    // does as it is told.
    set_led(dev, LED_ON)?;
    if !led_state(dev)? {
        return Err("the status LED is off after being told to go on".to_string());
    }

    Ok(Outcome::Pass)
}

/// The colour, brightness and mode the wire carried reach the engine.
///
/// Nothing in the request is a value the device would have chosen for itself —
/// the three colour components differ from each other, the brightness is not
/// the engine's default and the period is not zero — so every field has to have
/// crossed to be read back.  A plugin that dropped one, or took it from the
/// wrong byte, leaves the engine holding the device's own answer instead.
fn the_rgb_led_takes_the_colour_and_mode_from_the_wire(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    let colour = (0x10u8, 0x20u8, 0x30u8);
    set_rgb(dev, &rgb_args(LED_FLAME, colour, 40, 750, 0))?;

    let led = rgb_state(dev)?;
    if led.mode != LED_FLAME {
        return Err(format!(
            "the engine holds mode {} after a request for flame, sub-command {LED_FLAME}",
            led.mode
        ));
    }
    if (led.red, led.green, led.blue) != colour {
        return Err(format!(
            "the engine holds colour {:02x},{:02x},{:02x} where the wire carried \
             {:02x},{:02x},{:02x}",
            led.red, led.green, led.blue, colour.0, colour.1, colour.2
        ));
    }
    if led.brightness != 40 {
        return Err(format!(
            "the engine holds brightness {}, not the 40 the wire carried",
            led.brightness
        ));
    }
    if led.period_ms != 750 {
        return Err(format!(
            "the engine holds a period of {}ms, not the 750ms the wire carried",
            led.period_ms
        ));
    }

    Ok(Outcome::Pass)
}

/// A second request takes the RGB LED somewhere else.
///
/// The two differ in every field, so nothing the first left behind satisfies
/// the second — which is what a scenario that passed by doing nothing would be
/// relying on.
fn a_second_request_moves_the_rgb_led(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    let first_colour = (0x10u8, 0x20u8, 0x30u8);
    set_rgb(dev, &rgb_args(LED_ON, first_colour, 40, 250, 0))?;
    let first = rgb_state(dev)?;
    if first.mode != LED_ON || (first.red, first.green, first.blue) != first_colour {
        return Err(format!(
            "the first request left the engine at {}",
            describe(&first)
        ));
    }

    let second_colour = (0x40u8, 0x50u8, 0x60u8);
    set_rgb(dev, &rgb_args(LED_FLAME, second_colour, 80, 500, 0))?;
    let second = rgb_state(dev)?;

    if second == first {
        return Err(format!(
            "a second, different request left the engine exactly where the first did: {}",
            describe(&second)
        ));
    }
    if second.mode != LED_FLAME
        || (second.red, second.green, second.blue) != second_colour
        || second.brightness != 80
        || second.period_ms != 500
    {
        return Err(format!(
            "the engine holds {} after the second request, which asked for flame at \
             {:02x},{:02x},{:02x}, brightness 80, period 500ms",
            describe(&second),
            second_colour.0,
            second_colour.1,
            second_colour.2
        ));
    }

    Ok(Outcome::Pass)
}

/// A host that names no colour gets red.
///
/// Every field of the request reads as "the device chooses" when zero, and for
/// the colour that choice is red rather than an LED left dark.  The engine is
/// armed with a colour of its own first, so red afterwards is the device
/// answering rather than where it happened to start.
fn an_unnamed_colour_reads_as_red(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    let colour = (0x10u8, 0x20u8, 0x30u8);
    set_rgb(dev, &rgb_args(LED_ON, colour, 40, 0, 0))?;
    let armed = rgb_state(dev)?;
    if (armed.red, armed.green, armed.blue) != colour {
        return Err(format!(
            "the engine holds colour {:02x},{:02x},{:02x} where the wire carried \
             {:02x},{:02x},{:02x}",
            armed.red, armed.green, armed.blue, colour.0, colour.1, colour.2
        ));
    }

    set_rgb(dev, &rgb_args(LED_ON, (0, 0, 0), 40, 0, 0))?;
    let led = rgb_state(dev)?;
    if (led.red, led.green, led.blue) != (0xFF, 0x00, 0x00) {
        return Err(format!(
            "a request naming no colour left the engine at {:02x},{:02x},{:02x}, not red",
            led.red, led.green, led.blue
        ));
    }

    Ok(Outcome::Pass)
}

/// A channel this device does not have is refused, and moves no LED.
///
/// NOT_FOUND is "understood, and this device has no such LED", which a host
/// must not confuse with UNKNOWN_CMD — the answer that means the device is too
/// old to be asked.  Both LEDs are read across the refusal, since a plugin that
/// recorded the request before checking the channel would drive one of them on
/// the next pass.
fn a_channel_this_device_does_not_have_is_refused(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    set_led(dev, LED_ON)?;
    if !led_state(dev)? {
        return Err("the LED is off after being told to go on".to_string());
    }
    let rgb_before = rgb_state(dev)?;

    // A channel above the RGB LED, asking for something both LEDs would show.
    let mut args = set_led_args(LED_OFF);
    args[0] = LED_ID_RGB + 1;
    let st = dev.dispatch(CMD_SET_LED, 0, &args);
    if st != NOT_FOUND {
        return Err(format!(
            "a SET_LED for channel {} answered {st}, not NOT_FOUND",
            args[0]
        ));
    }

    dev.step()?;
    if !led_state(dev)? {
        return Err(
            "a SET_LED for a channel the device does not have turned the status LED off"
                .to_string(),
        );
    }
    if rgb_state(dev)? != rgb_before {
        return Err(format!(
            "a SET_LED for a channel the device does not have left the RGB LED at {}, \
             where it was {}",
            describe(&rgb_state(dev)?),
            describe(&rgb_before)
        ));
    }

    // The same request on a channel the device does have is obeyed, so this is
    // a refusal of the channel rather than of the command.
    set_led(dev, LED_OFF)?;
    if led_state(dev)? {
        return Err("the status LED stayed on when its own channel was told to go off".to_string());
    }

    Ok(Outcome::Pass)
}

/// A hold longer than the device offers is refused, and the LED carries on.
///
/// The refusal happens before the engine is called, so what it must not do is
/// take the LED off whatever it was doing on the way to saying no.  Exactly the
/// maximum is accepted, which is what says the refusal is about the length
/// rather than about holds.
/// A brightness above 100 is refused, and the LED keeps what it had.
///
/// The engine scales each of the three colour bytes by this percentage, so a
/// value above 100 would carry a full colour past 255 and out of its byte.  The
/// discriminating half is the fence: 100 itself must be taken, so a device
/// refusing every brightness passes neither clause.
/// Run the engine on to its next frame, as TIMER0 alarm 1 does on a device.
///
/// Moving the clock exactly to the deadline the engine named, rather than by an
/// interval a scenario worked out for itself, is what puts the engine's own
/// arithmetic under test alongside what it does when the frame arrives.
fn engine_frame(dev: &mut Device) -> Result<(), String> {
    let due = dev
        .led_deadline_ms()
        .ok_or_else(|| "the engine wants no further frame".to_string())?;
    let now = dev.uptime_ms();

    dev.advance_ms(u64::from(due.wrapping_sub(now)));
    dev.led_frame();
    Ok(())
}

/// Run a beacon out to its end, one engine frame at a time.
///
/// The engine decides when each toggle is due and when the run is over, so this
/// follows the deadlines it names rather than jumping the clock past the whole
/// thing - which would skip every toggle and leave the beacon to notice its own
/// end on one late frame.
fn run_beacon_out(dev: &mut Device) -> Result<(), String> {
    // Bounded by wake-ups rather than by the beacon's own length, because a
    // wake-up is not a fixed slice of it: a board that shares the pin between
    // the two LEDs wakes to hand the pin back as well as to toggle.  The bound
    // is only there so a beacon that never ends fails rather than hangs.
    for _ in 0..256 {
        if dev.led_deadline_ms().is_none() {
            return Ok(());
        }
        engine_frame(dev)?;
    }

    Err("the beacon was still running after 256 frames".to_string())
}

/// Run frames until the engine sends the LED another colour.
///
/// One wake-up is not one animation step everywhere: on a board that shares the
/// pin between the two LEDs the engine also wakes to hand the pin back, so a
/// scenario counting wake-ups would count those too.  What every board has in
/// common is that an animation step is a colour going out.
fn engine_step(dev: &mut Device) -> Result<(), String> {
    let (_, before) = dev.led_pixel();

    for _ in 0..4 {
        engine_frame(dev)?;
        if dev.led_pixel().1 != before {
            return Ok(());
        }
    }

    Err("four frames passed without the engine sending a colour".to_string())
}

/// The colour the RGB LED is showing, as the chip reads it.
/// Let the first colour out.
///
/// The engine holds the first colour after power-up behind the WS2812's reset,
/// so a scenario that wants to watch what follows it has to let it go first.
/// `the_first_colour_waits_for_the_reset` is what asserts that behaviour - here
/// it is only got out of the way, and only when nothing has gone out yet.
fn deliver_first_colour(dev: &mut Device) -> Result<(), String> {
    if dev.led_pixel().1 == 0 {
        engine_frame(dev)?;
    }

    Ok(())
}

fn pixel(dev: &Device) -> u32 {
    dev.led_pixel().0
}

/// A beacon on the RGB LED blinks, and gives the LED back when it is done.
///
/// Three things, each of which has failed in some device somewhere: that it
/// blinks at all, that the blinking alternates rather than driving one way, and
/// that a bounded identify ends and returns what it interrupted.
fn an_rgb_beacon_blinks_and_ends(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    // Arm with a mode a beacon has to give back, and one whose colour is not
    // one the beacon would produce for itself.
    set_rgb(dev, &rgb_args(LED_ON, (0xFF, 0x00, 0x00), 100, 0, 0))?;
    deliver_first_colour(dev)?;
    let armed = pixel(dev);

    set_rgb(dev, &rgb_args(LED_BEACON, (0x00, 0x00, 0xFF), 100, 0, 0))?;
    let lit = pixel(dev);
    if lit == 0 {
        return Err("a beacon did not light the RGB LED to begin with".to_string());
    }

    engine_step(dev)?;
    let toggled = pixel(dev);
    if toggled == lit {
        return Err(format!(
            "the beacon showed {toggled:#08x} again rather than toggling"
        ));
    }

    engine_step(dev)?;
    if pixel(dev) != lit {
        return Err("the beacon did not toggle back, so it drives one way".to_string());
    }

    // Run it out.  This counts wake-ups rather than colours, because the end of
    // a beacon is the engine having nothing further it wants.
    run_beacon_out(dev)?;

    let after = rgb_state(dev)?;
    if after.mode != LED_ON {
        return Err(format!(
            "the beacon left the RGB LED at {}, not the mode it interrupted",
            describe(&after)
        ));
    }
    if pixel(dev) != armed {
        return Err(format!(
            "the beacon gave back {:#08x}, not the {armed:#08x} it interrupted",
            pixel(dev)
        ));
    }

    Ok(Outcome::Pass)
}

/// Cycle walks the hues and comes back round.
///
/// The discriminating half is the return: a device that changed colour every
/// frame without following a circle would pass the first clause and fail this
/// one.
fn cycle_walks_the_hues(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    set_rgb(dev, &rgb_args(LED_CYCLE, (0, 0, 0), 100, 1000, 0))?;

    engine_step(dev)?;
    let first = pixel(dev);

    engine_step(dev)?;
    let second = pixel(dev);
    if second == first {
        return Err(format!("cycle showed {first:#08x} twice running"));
    }

    // A full rotation from the first frame, so the hue that comes back is the
    // one it started on.
    for _ in 0..(CYCLE_STEPS - 1) {
        engine_step(dev)?;
    }

    let round = pixel(dev);
    if round != first {
        return Err(format!(
            "a full rotation ended on {round:#08x}, not the {first:#08x} it began on"
        ));
    }

    Ok(Outcome::Pass)
}

/// Breathe fades up and back down.
///
/// Asserted on the total light rather than a particular colour, since that is
/// what a fade is.  A device that only rose, or only fell, fails one clause.
fn breathe_fades_up_and_down(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    // A colour on all three channels, so a fade shows in the sum whichever
    // channel the engine scales first.
    set_rgb(
        dev,
        &rgb_args(LED_BREATHE, (0xFF, 0xFF, 0xFF), 100, 1000, 0),
    )?;
    deliver_first_colour(dev)?;

    let mut levels = Vec::new();
    for _ in 0..BREATHE_STEPS {
        engine_step(dev)?;
        let p = pixel(dev);
        levels.push((p & 0xFF) + ((p >> 8) & 0xFF) + ((p >> 16) & 0xFF));
    }

    let peak = levels.iter().copied().max().unwrap_or(0);
    let peak_at = levels.iter().position(|l| *l == peak).unwrap_or(0);

    if peak == 0 {
        return Err("breathe never lit the RGB LED".to_string());
    }
    if peak_at == 0 || peak_at + 1 >= levels.len() {
        return Err(format!(
            "breathe peaked at frame {peak_at} of {}, so it did not fade both ways",
            levels.len()
        ));
    }
    if levels[peak_at - 1] >= peak || levels[peak_at + 1] >= peak {
        return Err("breathe's peak is not a peak - it does not rise then fall".to_string());
    }

    Ok(Outcome::Pass)
}

/// A hold gives the LED back when it expires.
///
/// The engine times this, so a scenario moves the clock to the deadline the
/// engine named rather than counting frames of its own.
fn a_hold_gives_the_rgb_led_back(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    set_rgb(dev, &rgb_args(LED_ON, (0xFF, 0x00, 0x00), 100, 0, 0))?;
    deliver_first_colour(dev)?;
    let before = rgb_state(dev)?;
    let before_pixel = pixel(dev);

    set_rgb(dev, &rgb_args(LED_ON, (0x00, 0x00, 0xFF), 100, 0, 5000))?;
    let held = rgb_state(dev)?;
    if held.blue != 0xFF {
        return Err(format!(
            "the held request left the RGB LED at {}, not the colour it asked for",
            describe(&held)
        ));
    }

    // Frames until the hold expires.  On a board that shares the pin there are
    // handovers in between, which are wake-ups this is not waiting for.
    for _ in 0..8 {
        engine_frame(dev)?;
        if rgb_state(dev)?.red == before.red {
            break;
        }
        if dev.led_deadline_ms().is_none() {
            break;
        }
    }

    let after = rgb_state(dev)?;
    if after.red != before.red || after.green != before.green || after.blue != before.blue {
        return Err(format!(
            "the hold gave back {}, where it took over from {}",
            describe(&after),
            describe(&before)
        ));
    }
    if pixel(dev) != before_pixel {
        return Err(format!(
            "the hold gave back {:#08x}, not the {before_pixel:#08x} it took over from",
            pixel(dev)
        ));
    }

    Ok(Outcome::Pass)
}

/// A status LED command while the state machine holds the shared pin waits for
/// it, rather than reaching the pin or being lost.
///
/// On a device the pin's function select decides this: while the block has the
/// pin an SIO write changes nothing, and the level the status LED asked for
/// arrives when the engine hands the pin back.  The two halves are what make
/// this a test rather than an observation - the pin not moving would also be
/// true of firmware that threw the request away, and the pin moving later would
/// also be true of firmware that drove it early and got lucky.
fn a_status_led_command_waits_for_the_shared_pin(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let rgb = rgb_state(dev)?;
    let status = dev.led(LED_ID_STATUS)?;

    if !rgb.present || rgb.gpio != status.gpio {
        return Ok(Outcome::Skip(
            "this board wires the two LEDs to different pins".to_string(),
        ));
    }

    // Start from a dark status LED.  Nothing has driven the RGB LED yet, so the
    // pin is SIO's and this write lands on it.  A dark LED schedules nothing,
    // so there is no frame to run here.
    set_led(dev, LED_OFF)?;

    // Take the pin for a pixel.  It is owed back, but not yet.
    set_rgb(dev, &rgb_args(LED_ON, (0x10, 0x20, 0x30), 40, 0, 0))?;
    let held = dev.gpio_level(rgb.gpio);

    // Arm: ask for the status LED while the block still has the pin.
    set_led(dev, LED_ON)?;

    if dev.gpio_level(rgb.gpio) != held {
        return Err(format!(
            "the pin went to {} while the state machine held it, so an SIO \
             write reached a pad it could not have",
            dev.gpio_level(rgb.gpio)
        ));
    }

    // The engine owes the pin back.  The request was not dropped - a lit status
    // LED drives the pin low - so it lands as the pin changes hands.
    engine_frame(dev)?;

    if dev.gpio_level(rgb.gpio) != 0 {
        return Err(format!(
            "the pin reads {} once the engine handed it back, so the status \
             LED command was lost rather than deferred",
            dev.gpio_level(rgb.gpio)
        ));
    }

    Ok(Outcome::Pass)
}

/// On a board that wires both LEDs to one pin, the status LED keeps working.
///
/// The engine takes the pin for as long as a pixel takes and owes it straight
/// back, so a status LED command lands as it does on any other board.  The
/// discriminating half is driving the RGB LED first: before this was so, the
/// pin stayed with the state machine and the status LED stopped moving
/// altogether once anything had touched the pixel.
fn the_status_led_survives_the_rgb_led_on_a_shared_pin(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let rgb = rgb_state(dev)?;
    let status = dev.led(LED_ID_STATUS)?;

    if !rgb.present || rgb.gpio != status.gpio {
        return Ok(Outcome::Skip(
            "this board wires the two LEDs to different pins".to_string(),
        ));
    }

    // Take the pixel, which is what used to leave the status LED stranded.
    set_rgb(dev, &rgb_args(LED_ON, (0x10, 0x20, 0x30), 40, 0, 0))?;

    // The engine owes the pin back, so run the frame that hands it over.
    engine_frame(dev)?;

    for want in [true, false, true] {
        set_led(dev, if want { LED_ON } else { LED_OFF })?;

        // The flag says what the LED is meant to be doing.  The pad says what
        // the pin is actually at, and the two part company exactly when the
        // state machine has kept the pin - which is the failure this covers.
        // A lit status LED drives the pin low.
        if led_state(dev)? != want {
            return Err(format!(
                "the status LED would not go {} after the RGB LED had the pin",
                if want { "on" } else { "off" }
            ));
        }
        if dev.gpio_level(rgb.gpio) != u8::from(!want) {
            return Err(format!(
                "the shared pin reads {} with the status LED {}, so the state \
                 machine still holds it",
                dev.gpio_level(rgb.gpio),
                if want { "on" } else { "off" }
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// A period given on the wire is the one the engine repeats at.
///
/// The discriminating half is that a period the device would never choose for
/// itself comes back, so a plugin dropping the field would leave the mode's own
/// default in its place.
fn a_period_crosses_to_the_engine(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    set_rgb(dev, &rgb_args(LED_BREATHE, (0x10, 0x20, 0x30), 40, 1777, 0))?;
    let state = rgb_state(dev)?;

    if state.mode != LED_BREATHE {
        return Err(format!(
            "the engine holds {} after a request for breathe",
            describe(&state)
        ));
    }
    if state.period_ms != 1777 {
        return Err(format!(
            "the engine repeats every {}ms, not the 1777ms asked for",
            state.period_ms
        ));
    }

    Ok(Outcome::Pass)
}

/// A period of zero leaves the mode to choose, and each mode chooses its own.
///
/// Three modes, three different defaults, so a device answering with one
/// constant fails.  This is what a host that predates the field sends, and it
/// is the half that keeps those hosts working.
fn an_unnamed_period_is_the_modes_own(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    for (mode, expected, name) in [
        (LED_CYCLE, CYCLE_PERIOD_MS, "cycle"),
        (LED_BREATHE, BREATHE_PERIOD_MS, "breathe"),
        (LED_BLINK, BLINK_PERIOD_MS, "blink"),
    ] {
        set_rgb(dev, &rgb_args(mode, (0x10, 0x20, 0x30), 40, 0, 0))?;
        let state = rgb_state(dev)?;

        if state.period_ms != expected {
            return Err(format!(
                "{name} with no period repeats every {}ms, not its own {expected}ms",
                state.period_ms
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// A mode built out of a colour is refused for the status LED, and moves it not
/// at all.
///
/// Cycle, breathe and blink need a colour, and the status LED has none.  The
/// refusal has to reach the host: the status LED's modes are applied by the
/// task loop rather than as the command is answered, so a mode queued and then
/// quietly dropped there would have told the host it worked.  Both halves are
/// asserted - the status the host is given, and that the LED did not move,
/// armed lit and armed dark so it cannot match where it already was.
fn a_colour_mode_is_refused_for_the_status_led(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    // Blink is deliberately absent: it alternates an LED with dark, which the
    // status LED does as readily as the RGB one.  Only the two modes built out
    // of a colour are refused.
    for (mode, name) in [(LED_CYCLE, "cycle"), (LED_BREATHE, "breathe")] {
        for armed in [true, false] {
            set_led(dev, if armed { LED_ON } else { LED_OFF })?;

            let mut args = rgb_args(mode, (0x10, 0x20, 0x30), 40, 0, 0);
            args[0] = LED_ID_STATUS;

            let st = dev.dispatch(CMD_SET_LED, 0, &args);
            if st != INVALID_ARG {
                return Err(format!(
                    "{name} for the status LED answered {st}, not INVALID_ARG"
                ));
            }

            dev.step()?;
            if led_state(dev)? != armed {
                return Err(format!(
                    "{name} moved the status LED, which it has no colour for"
                ));
            }
        }
    }

    set_led(dev, LED_ON)?;
    if !led_state(dev)? {
        return Err("the status LED then refused a mode it does have".to_string());
    }

    Ok(Outcome::Pass)
}

fn a_brightness_above_a_hundred_is_refused(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    set_rgb(dev, &rgb_args(LED_ON, (0x10, 0x20, 0x30), 40, 0, 0))?;
    let before = rgb_state(dev)?;

    let over = dev.dispatch(CMD_SET_LED, 0, &rgb_args(LED_ON, (0xFF, 0, 0), 101, 0, 0));
    if over != INVALID_ARG {
        return Err(format!(
            "a brightness of 101 answered {over}, not INVALID_ARG"
        ));
    }
    let after = rgb_state(dev)?;
    if after != before {
        return Err(format!(
            "a refused brightness left the RGB LED at {}, where it was {}",
            describe(&after),
            describe(&before)
        ));
    }

    set_rgb(dev, &rgb_args(LED_ON, (0xFF, 0, 0), 100, 0, 0))?;
    let at_limit = rgb_state(dev)?;
    if at_limit.brightness != 100 {
        return Err(format!(
            "a brightness of exactly 100 left the RGB LED at {}",
            describe(&at_limit)
        ));
    }

    Ok(Outcome::Pass)
}

fn a_hold_beyond_the_maximum_leaves_the_rgb_led_alone(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    let colour = (0x10u8, 0x20u8, 0x30u8);
    set_rgb(dev, &rgb_args(LED_FLAME, colour, 40, 750, 0))?;
    let before = rgb_state(dev)?;
    if before.mode != LED_FLAME {
        return Err(format!(
            "the engine holds {} after a request for flame",
            describe(&before)
        ));
    }

    let over = dev.dispatch(
        CMD_SET_LED,
        0,
        &rgb_args(LED_OFF, (0, 0, 0), 100, 0, MAX_HOLD_MS + 1),
    );
    if over != INVALID_ARG {
        return Err(format!(
            "a hold of {}ms answered {over}, not INVALID_ARG",
            MAX_HOLD_MS + 1
        ));
    }
    let after = rgb_state(dev)?;
    if after != before {
        return Err(format!(
            "a refused hold left the RGB LED at {}, where it was {}",
            describe(&after),
            describe(&before)
        ));
    }

    set_rgb(dev, &rgb_args(LED_OFF, (0, 0, 0), 100, 0, MAX_HOLD_MS))?;
    let at_limit = rgb_state(dev)?;
    if at_limit.mode != LED_OFF {
        return Err(format!(
            "a hold of exactly {MAX_HOLD_MS}ms left the RGB LED at {}",
            describe(&at_limit)
        ));
    }

    Ok(Outcome::Pass)
}

/// The RGB LED is set as the command is answered, not on a later pass.
///
/// This is the opposite of the status LED, and for the reason the status LED is
/// not: the engine holds the mode itself, so the call does not outlive the
/// command and its answer is one the host wants to hear.  The discriminating
/// half is the read with no pass of the plugin's loop in between — a plugin
/// that queued this the way it queues the status LED would still be holding the
/// mode it was armed with.
fn the_rgb_led_is_set_as_the_command_is_answered(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    // Armed with a pass of the task loop after it, so a plugin that queued this
    // one is armed too and the read below is the only thing telling them apart.
    set_rgb(dev, &rgb_args(LED_ON, (0x10, 0x20, 0x30), 40, 0, 0))?;
    dev.step()?;
    let armed = rgb_state(dev)?;
    if armed.mode != LED_ON {
        return Err(format!(
            "the engine holds {} after a request for on",
            describe(&armed)
        ));
    }

    set_rgb(dev, &rgb_args(LED_FLAME, (0x40, 0x50, 0x60), 80, 0, 0))?;
    let applied = rgb_state(dev)?;
    if applied == armed {
        return Err(format!(
            "the RGB LED is still at {} once the command has been answered, so the \
             request was recorded rather than applied",
            describe(&applied)
        ));
    }
    if applied.mode != LED_FLAME {
        return Err(format!(
            "the engine holds {} once the command has been answered",
            describe(&applied)
        ));
    }

    // And the task loop does not have a second go at it.
    dev.step()?;
    let later = rgb_state(dev)?;
    if later != applied {
        return Err(format!(
            "a pass of the task loop moved the RGB LED to {}, from the {} the command \
             had already set",
            describe(&later),
            describe(&applied)
        ));
    }

    Ok(Outcome::Pass)
}

/// Read a LED_QUERY response, as a host would.
fn query(dev: &mut Device, led_id: u8) -> Result<Vec<u8>, String> {
    let mut args = [0u8; 16];
    args[0] = led_id;

    let st = dev.dispatch(CMD_LED_QUERY, LED_STATE_LEN, &args);
    if st != OK {
        return Err(format!(
            "a LED_QUERY for channel {led_id} was refused with status {st}"
        ));
    }
    dev.fill_all(LED_STATE_LEN, LED_STATE_LEN)
}

/// A LED_QUERY reports what a SET_LED just asked for.
///
/// Armed with a mode and a period the device would not otherwise be in, so a
/// response of zeros - which is what a command producing nothing would leave
/// behind - fails instead of passing.
fn a_query_reports_what_was_set(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let mut args = set_led_args(SUB_BEACON);
    args[8..10].copy_from_slice(&300u16.to_le_bytes());
    args[10..14].copy_from_slice(&9_000u32.to_le_bytes());
    set_rgb(dev, &args)?;

    let buf = query(dev, LED_ID_STATUS)?;

    let struct_len = u16::from_le_bytes([buf[0], buf[1]]);
    if struct_len as usize != buf.len() {
        return Err(format!(
            "struct_len says {struct_len} of the {} bytes transferred are meaningful",
            buf.len()
        ));
    }
    if buf[2] != LED_ID_STATUS {
        return Err(format!("asked about channel 0, answered about {}", buf[2]));
    }
    if buf[3] == 0 {
        return Err(
            "every board has a status LED, and the query says this one has none".to_string(),
        );
    }
    if buf[4] != SUB_BEACON {
        return Err(format!("set beacon, the query reports mode {}", buf[4]));
    }
    let period = u16::from_le_bytes([buf[11], buf[12]]);
    if period != 300 {
        return Err(format!("set a 300ms period, the query reports {period}"));
    }

    Ok(Outcome::Pass)
}

/// A second, different command produces a different answer.
///
/// A query hard-wired to one response would pass the scenario above.  This is
/// what makes it say something.
fn a_query_follows_the_led(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    set_rgb(dev, &set_led_args(SUB_FLAME))?;
    let before = query(dev, LED_ID_STATUS)?;

    set_rgb(dev, &set_led_args(SUB_OFF))?;
    let after = query(dev, LED_ID_STATUS)?;

    if before[4] == after[4] {
        return Err(format!(
            "flame and off both report mode {}, so the query is not reading the LED",
            after[4]
        ));
    }
    if after[4] != SUB_OFF {
        return Err(format!("set off, the query reports mode {}", after[4]));
    }

    Ok(Outcome::Pass)
}

/// An LED the board does not have is answered about, not refused.
///
/// The difference is the point.  A refusal is what a device too old to be asked
/// gives, so answering with `present` clear is the only way a host can tell a
/// user "this board has no RGB LED" rather than "something went wrong".
fn a_query_answers_for_an_led_the_board_lacks(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let buf = query(dev, LED_ID_RGB)?;

    // The firmware is the authority on which LEDs the board has, so the
    // expectation comes from it rather than from a list kept here.
    let present = rgb_state(dev)?.present;

    if (buf[3] != 0) != present {
        return Err(format!(
            "the firmware says present={present}, the query says present={}",
            buf[3] != 0
        ));
    }
    if buf[2] != LED_ID_RGB {
        return Err(format!("asked about channel 1, answered about {}", buf[2]));
    }
    if !present && buf[9] != GPIO_NONE {
        return Err(format!(
            "a board with no RGB LED reported it on GPIO {}",
            buf[9]
        ));
    }

    Ok(Outcome::Pass)
}

/// A channel this device does not know is refused, not answered about some
/// other LED.
fn a_query_for_an_unknown_channel_is_refused(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let mut args = [0u8; 16];
    args[0] = 7;

    let st = dev.dispatch(CMD_LED_QUERY, LED_STATE_LEN, &args);
    if st == OK {
        return Err("a query for channel 7 was accepted".to_string());
    }
    if st != NOT_FOUND {
        return Err(format!("expected NOT_FOUND for channel 7, got status {st}"));
    }

    Ok(Outcome::Pass)
}

/// The response pads and truncates, so it can grow without a protocol change.
///
/// The same contract GET_CAPS holds to: a shorter request gets a prefix that
/// still declares the full struct_len, and a longer one is zero padded.
fn a_query_pads_and_truncates(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let mut args = [0u8; 16];
    args[0] = LED_ID_STATUS;

    let st = dev.dispatch(CMD_LED_QUERY, 4, &args);
    if st != OK {
        return Err(format!("a 4 byte request was refused with status {st}"));
    }
    let short = dev.fill_all(4, 4)?;
    if short.len() != 4 {
        return Err(format!("asked for 4 bytes, got {}", short.len()));
    }
    let declared = u16::from_le_bytes([short[0], short[1]]);
    if u32::from(declared) != LED_STATE_LEN {
        return Err(format!(
            "a truncated response declared struct_len {declared}, not {LED_STATE_LEN}"
        ));
    }

    let over = LED_STATE_LEN + 8;
    let st = dev.dispatch(CMD_LED_QUERY, over, &args);
    if st != OK {
        return Err(format!("an over-long request was refused with status {st}"));
    }
    let long = dev.fill_all(over, over)?;
    if long.len() != over as usize {
        return Err(format!("asked for {over} bytes, got {}", long.len()));
    }
    if long[LED_STATE_LEN as usize..].iter().any(|&b| b != 0) {
        return Err("bytes past the structure were not zero padded".to_string());
    }

    Ok(Outcome::Pass)
}

/// The engine knows what the status LED is doing on a device nothing has
/// touched.
///
/// Every other scenario in this suite sets a mode before it reads one, so the
/// only state none of them covers is the one every device boots into.  The
/// firmware lights the status LED at boot from RUNTIME->status_led_enabled
/// (main.c), without going through the engine, so the engine's channel can sit
/// at its zeroed value - off - while the LED is on.
///
/// The live state is the authority here, not a constant kept in this test: it
/// is what a second plugin reads, and what the engine itself writes on every
/// change.
fn the_engine_knows_the_boot_state(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let lit = led_state(dev)?;
    let reported = dev.led(LED_ID_STATUS)?;

    let expected = if lit { SUB_ON } else { SUB_OFF };

    if reported.mode != expected {
        return Err(format!(
            "the status LED is {}, and the engine reports mode {} rather than {}",
            if lit { "on" } else { "off" },
            reported.mode,
            expected
        ));
    }

    Ok(Outcome::Pass)
}

/// A beacon on a device nothing has touched puts the status LED back where it
/// found it.
///
/// `a_beacon_restores_what_it_interrupted` sets a state before beaconing, so it
/// never sees the state a device boots into.  What a bounded mode saves is the
/// engine's own idea of the channel, and on a fresh boot that idea is the
/// zeroed one - so a beacon can capture "off" from an LED that is on and turn
/// it off for good when it ends.
fn a_beacon_on_a_fresh_boot_restores_the_boot_state(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let before = led_state(dev)?;

    set_led(dev, SUB_BEACON)?;

    // Past the beacon's own duration, so it has ended and restored.
    run_beacon_out(dev)?;

    let after = led_state(dev)?;

    if after != before {
        return Err(format!(
            "the status LED was {} before the beacon and {} after it",
            if before { "on" } else { "off" },
            if after { "on" } else { "off" }
        ));
    }

    Ok(Outcome::Pass)
}

/// The first colour after power-up waits for the WS2812's reset.
///
/// A WS2812 takes a frame only after the line has been held low for its reset
/// time.  The engine claims the state machine on the first request that needs
/// it, which puts the line low microseconds before the frame would go - not a
/// reset, so the chip reads the bits out of step and latches a colour nobody
/// asked for.
///
/// Only the first request is exposed to it.  After that the line rests low
/// between frames, so any later request has long since had its reset.  That is
/// why this scenario asserts on a device nothing has driven yet, and why
/// nothing else in this suite sees it: they all set a colour before they look.
///
/// A board where the two LEDs share a pin is not exempt.  It escapes today only
/// because the status LED holds that line low from boot, which is a property of
/// the board rather than anything the engine arranged.
fn the_first_colour_waits_for_the_reset(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    let (_, before) = dev.led_pixel();
    if before != 0 {
        return Err(format!(
            "{before} colours had already gone out before this scenario asked for one"
        ));
    }

    set_rgb(dev, &rgb_args(SUB_ON, (0x00, 0x00, 0xFF), 100, 0, 0))?;

    // The claim has just taken the line low.  Nothing may go out until that low
    // has lasted the reset time.
    let (_, sent) = dev.led_pixel();
    if sent != 0 {
        return Err(
            "the first colour went out as the request was answered, with no reset before it"
                .to_string(),
        );
    }

    // And it must actually be coming, rather than dropped.
    if dev.led_deadline_ms().is_none() {
        return Err("nothing was sent and the engine is waiting for nothing".to_string());
    }

    engine_frame(dev)?;

    let (value, count) = dev.led_pixel();
    if count != 1 {
        return Err(format!(
            "after the reset the engine had sent {count} colours, not one"
        ));
    }

    // Blue at full brightness, in the green-red-blue order the chip reads.
    if value != 0x0000FF {
        return Err(format!(
            "the colour after the reset was {value:#08x}, not the blue that was asked for"
        ));
    }

    Ok(Outcome::Pass)
}

/// A period too short for the mode is refused, not quietly rounded up.
///
/// Below its minimum a mode cannot run at the period asked for - the frames
/// would have to be closer together than the engine schedules - so accepting
/// the value would have the device report a period it was not running at.  The
/// minimums come from the mode's step count, so they differ per mode and are
/// listed here rather than derived, which is what makes a changed step count
/// show up as a failure.
fn a_period_below_the_minimum_is_refused(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    // mode, its minimum, and a period one below it.
    let cases = [
        (SUB_CYCLE, 1000u16),
        (SUB_BREATHE, 1000),
        (SUB_BLINK, 50),
        (SUB_BEACON, 50),
        (SUB_FLAME, 500),
    ];

    for (mode, min) in cases {
        // One below the minimum is refused.
        let st = dev.dispatch(
            CMD_SET_LED,
            0,
            &rgb_args(mode, (0xFF, 0xFF, 0xFF), 100, min - 1, 0),
        );
        if st != INVALID_ARG {
            return Err(format!(
                "mode {mode} took a period of {}ms, answering {st} rather than refusing it",
                min - 1
            ));
        }

        // And the minimum itself is taken, so the bound is where it is said to
        // be rather than somewhere above it.
        let st = dev.dispatch(
            CMD_SET_LED,
            0,
            &rgb_args(mode, (0xFF, 0xFF, 0xFF), 100, min, 0),
        );
        if st != OK {
            return Err(format!(
                "mode {mode} refused a period of {min}ms, its own minimum, with status {st}"
            ));
        }

        let reported = rgb_state(dev)?;
        if reported.period_ms != min {
            return Err(format!(
                "mode {mode} took {min}ms and reports {}ms",
                reported.period_ms
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// A period of zero still means the mode's own default.
///
/// The bound is on what a caller states, so the "let the device choose" value
/// has to survive it.
fn a_zero_period_is_still_the_default(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    if !rgb_state(dev)?.present {
        return Ok(no_rgb_led());
    }

    set_rgb(dev, &rgb_args(SUB_CYCLE, (0xFF, 0xFF, 0xFF), 100, 0, 0))?;

    let reported = rgb_state(dev)?;
    if reported.period_ms == 0 {
        return Err("a zero period was taken literally rather than as the default".to_string());
    }
    if reported.period_ms < 1000 {
        return Err(format!(
            "the default cycle period is {}ms, below the minimum the engine enforces",
            reported.period_ms
        ));
    }

    Ok(Outcome::Pass)
}

/// The status LED blinks, and keeps blinking.
///
/// Blink is not a colour mode, so it applies to an LED that has no colour.  A
/// scenario for it needs the LED to be seen changing under its own steam rather
/// than being told to change: what is asserted is that the engine alternates
/// it, and that it is still doing so after the point a beacon would have given
/// up, since blink is the one toggling mode that does not end itself.
fn the_status_led_blinks_indefinitely(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let mut args = set_led_args(SUB_BLINK);
    args[8..10].copy_from_slice(&100u16.to_le_bytes());

    let st = dev.dispatch(CMD_SET_LED, 0, &args);
    if st != OK {
        return Err(format!(
            "blink for the status LED was refused with status {st}"
        ));
    }
    dev.step()?;

    let first = led_state(dev)?;

    // It alternates.
    engine_frame(dev)?;
    let second = led_state(dev)?;
    if second == first {
        return Err(format!(
            "the status LED stayed {} across a frame, so blink drives it one way",
            if first { "on" } else { "off" }
        ));
    }

    // And back.
    engine_frame(dev)?;
    if led_state(dev)? != first {
        return Err("the status LED did not come back round".to_string());
    }

    // Past where a beacon would have ended, it is still going.  A beacon runs
    // BEACON_DURATION_MS and then restores, and blink must not.
    let deadline = dev.uptime_ms() + (BEACON_DURATION_MS as u32) + 1000;
    let mut toggles = 0;
    let mut last = led_state(dev)?;
    while dev.uptime_ms() < deadline {
        engine_frame(dev)?;
        let now = led_state(dev)?;
        if now != last {
            toggles += 1;
            last = now;
        }
    }

    if toggles == 0 {
        return Err(format!(
            "the status LED stopped blinking within {BEACON_DURATION_MS}ms, as a beacon would"
        ));
    }

    let reported = dev.led(LED_ID_STATUS)?;
    if reported.mode != SUB_BLINK {
        return Err(format!(
            "the engine reports mode {} rather than still blinking",
            reported.mode
        ));
    }

    Ok(Outcome::Pass)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "led.the_status_led_is_set_as_the_command_is_answered",
        about: "SET_LED is applied as it is answered, not a pass later",
        run: the_status_led_is_set_as_the_command_is_answered,
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
    Scenario {
        name: "led.the_rgb_led_is_refused_where_the_board_has_none",
        about: "a board with no RGB LED answers NOT_FOUND, and one with it obeys",
        run: the_rgb_led_is_refused_where_the_board_has_none,
        before_start: None,
    },
    Scenario {
        name: "led.the_rgb_led_takes_the_colour_and_mode_from_the_wire",
        about: "the colour, brightness, period and mode a SET_LED carried reach the engine",
        run: the_rgb_led_takes_the_colour_and_mode_from_the_wire,
        before_start: None,
    },
    Scenario {
        name: "led.a_second_request_moves_the_rgb_led",
        about: "a second, different request leaves the engine somewhere else",
        run: a_second_request_moves_the_rgb_led,
        before_start: None,
    },
    Scenario {
        name: "led.an_unnamed_colour_reads_as_red",
        about: "a request naming no colour lights the RGB LED white",
        run: an_unnamed_colour_reads_as_red,
        before_start: None,
    },
    Scenario {
        name: "led.a_channel_this_device_does_not_have_is_refused",
        about: "a channel above the RGB LED answers NOT_FOUND and moves no LED",
        run: a_channel_this_device_does_not_have_is_refused,
        before_start: None,
    },
    Scenario {
        name: "led.an_rgb_beacon_blinks_and_ends",
        about: "a beacon blinks, alternates, and gives the LED back",
        run: an_rgb_beacon_blinks_and_ends,
        before_start: None,
    },
    Scenario {
        name: "led.cycle_walks_the_hues",
        about: "cycle changes colour each frame and comes back round",
        run: cycle_walks_the_hues,
        before_start: None,
    },
    Scenario {
        name: "led.breathe_fades_up_and_down",
        about: "breathe rises to a peak and falls again",
        run: breathe_fades_up_and_down,
        before_start: None,
    },
    Scenario {
        name: "led.a_hold_gives_the_rgb_led_back",
        about: "a hold expires back to the mode and colour before it",
        run: a_hold_gives_the_rgb_led_back,
        before_start: None,
    },
    Scenario {
        name: "led.a_status_led_command_waits_for_the_shared_pin",
        about: "a status LED command lands when the state machine gives the pin back",
        run: a_status_led_command_waits_for_the_shared_pin,
        before_start: None,
    },
    Scenario {
        name: "led.the_status_led_survives_the_rgb_led_on_a_shared_pin",
        about: "a board with one pin for both LEDs still obeys status LED commands",
        run: the_status_led_survives_the_rgb_led_on_a_shared_pin,
        before_start: None,
    },
    Scenario {
        name: "led.a_period_crosses_to_the_engine",
        about: "a period on the wire is the one the engine repeats at",
        run: a_period_crosses_to_the_engine,
        before_start: None,
    },
    Scenario {
        name: "led.an_unnamed_period_is_the_modes_own",
        about: "a period of zero leaves each mode to its own default",
        run: an_unnamed_period_is_the_modes_own,
        before_start: None,
    },
    Scenario {
        name: "led.a_colour_mode_is_refused_for_the_status_led",
        about: "cycle, breathe and blink are refused, and do not move the status LED",
        run: a_colour_mode_is_refused_for_the_status_led,
        before_start: None,
    },
    Scenario {
        name: "led.a_brightness_above_a_hundred_is_refused",
        about: "a brightness above 100 is refused, and 100 itself is taken",
        run: a_brightness_above_a_hundred_is_refused,
        before_start: None,
    },
    Scenario {
        name: "led.a_hold_beyond_the_maximum_leaves_the_rgb_led_alone",
        about: "a hold longer than the device offers is refused, and the LED carries on",
        run: a_hold_beyond_the_maximum_leaves_the_rgb_led_alone,
        before_start: None,
    },
    Scenario {
        name: "led.the_rgb_led_is_set_as_the_command_is_answered",
        about: "the RGB LED is driven at dispatch, unlike the status LED",
        run: the_rgb_led_is_set_as_the_command_is_answered,
        before_start: None,
    },
    Scenario {
        name: "led.a_query_reports_what_was_set",
        about: "a LED_QUERY reports the mode and period a SET_LED just asked for",
        run: a_query_reports_what_was_set,
        before_start: None,
    },
    Scenario {
        name: "led.a_query_follows_the_led",
        about: "a second, different command produces a different answer",
        run: a_query_follows_the_led,
        before_start: None,
    },
    Scenario {
        name: "led.a_query_answers_for_an_led_the_board_lacks",
        about: "an LED the board does not have is answered about, with present clear",
        run: a_query_answers_for_an_led_the_board_lacks,
        before_start: None,
    },
    Scenario {
        name: "led.a_query_for_an_unknown_channel_is_refused",
        about: "a channel this device does not know is refused, not answered about another LED",
        run: a_query_for_an_unknown_channel_is_refused,
        before_start: None,
    },
    Scenario {
        name: "led.a_query_pads_and_truncates",
        about: "a shorter request gets a prefix that still declares struct_len, a longer one is padded",
        run: a_query_pads_and_truncates,
        before_start: None,
    },
    Scenario {
        name: "led.the_engine_knows_the_boot_state",
        about: "the engine agrees with the live status-LED state on an untouched device",
        run: the_engine_knows_the_boot_state,
        before_start: None,
    },
    Scenario {
        name: "led.a_beacon_on_a_fresh_boot_restores_the_boot_state",
        about: "a beacon on an untouched device puts the status LED back where it found it",
        run: a_beacon_on_a_fresh_boot_restores_the_boot_state,
        before_start: None,
    },
    Scenario {
        name: "led.the_first_colour_waits_for_the_reset",
        about: "the first colour after power-up is held back until the WS2812's reset has passed",
        run: the_first_colour_waits_for_the_reset,
        before_start: None,
    },
    Scenario {
        name: "led.a_period_below_the_minimum_is_refused",
        about: "each mode refuses a period shorter than it can run, and takes its own minimum",
        run: a_period_below_the_minimum_is_refused,
        before_start: None,
    },
    Scenario {
        name: "led.a_zero_period_is_still_the_default",
        about: "zero still means the mode's own default, and that default clears the minimum",
        run: a_zero_period_is_still_the_default,
        before_start: None,
    },
    Scenario {
        name: "led.the_status_led_blinks_indefinitely",
        about: "blink applies to the status LED, and does not end itself the way a beacon does",
        run: the_status_led_blinks_indefinitely,
        before_start: None,
    },
];
