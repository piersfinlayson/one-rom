// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Specification: "Group 0x06 — LEDs".
//!
//! Four commands over the device's own LEDs: three that describe what it has and
//! what its modes need, and one that drives one.
//!
//! # LEDs are optional, and absence is a legitimate answer
//!
//! "A device that has no LEDs reports a count of zero from
//! GET_LED_CAPABILITY.  All other commands in this group return failure on such
//! a device."  So a zero count is conformant, and every scenario past
//! [`get_led_capability`] skips on it.  That one never skips, and is where the
//! "all other commands fail" rule is asserted.
//!
//! # What an LED is doing is read back through the protocol
//!
//! Unlike a pin, an LED has no second window onto it here: GET_LED_INFO is the
//! only way to see what SET_LED did.  That makes the round trip the assertion
//! throughout — set a mode, a colour, a period, then read them back — which is
//! also exactly what a host can see, so nothing is asserted that a host could
//! not itself rely on.
//!
//! # What a mode needs is the device's to say
//!
//! Which modes take a period, and the shortest period each accepts, are reported
//! per mode and per LED by GET_LED_MODE_INFO rather than fixed by the protocol.
//! So the scenarios here take the floor off the device and hold it to it —
//! [`set_led_honours_the_reported_floor`] requires the reported floor to be
//! accepted and the value below it refused, which is the whole of what makes
//! SET_LED's period predictable to a host.
//!
//! # A hold is the device's timer, not the device blocking
//!
//! "The device completes the command without waiting for the hold, which
//! outlives the command-response session."  That is the opposite of SET_AUX,
//! and [`set_led_hold_does_not_block`] holds it to both halves: the command
//! completes with the clock standing still, and moving the clock past the
//! deadline and running a frame puts the LED back.
//!
//! # What this suite does not verify
//!
//! 1. "An LED's state persists ... across RBCP_RESET.  Only a device reset
//!    restores an LED to its power-on state."  The reset half is asserted; the
//!    device reset half has no device reset in this harness.
//! 2. The shape of an animated mode — that a hue walks, that a breath fades.
//!    RBCP reports the mode in force and nothing finer, so the protocol cannot
//!    see it.  The firmware's own LED tests own that.
//! 3. A `count` above two.  This firmware has two LED channels, so the walk
//!    over them is exercised as far as a device can drive it.
//! 4. Type values 0x80-0xFE, and modes 0x06 and 0x07.  This device reports
//!    neither, so they are checked only as values the protocol allows.

use onerom_fw_emulator::ffi;

use crate::driver::{Bus, CmdFailure, HDR_SIZE, Hdr, Session, control, group, led, modify};
use crate::{Ctx, Outcome};

/// Response data section GET_LED_CAPABILITY needs, in bytes.
const CAPABILITY_BYTES: u32 = 8;

/// The same for GET_LED_INFO, whose answer is twice as long.
const INFO_BYTES: u32 = 16;

/// LED types ("LED Types").
const TYPE_MONO: u8 = 0x00;
const TYPE_RGB: u8 = 0x01;

/// LED modes ("LED Modes").
const OFF: u8 = 0x00;
const ON: u8 = 0x01;
const BLINK: u8 = 0x02;
const BREATHE: u8 = 0x03;
const CYCLE: u8 = 0x04;
const BEACON: u8 = 0x05;

/// The lowest mode the protocol reserves, and so one no device may support.
const MODE_RESERVED: u8 = 0x06;

/// One ROM's flame, from the range the protocol reserves for an
/// implementation.  Not RBCP: asserted here because this device is the one
/// under test, and a mode it accepts must round-trip like any other.
const MODE_FLAME: u8 = 0x80;

/// A colour no default could be mistaken for, and distinct in all three bytes
/// so that a device transposing two of them fails.
const TEST_COLOUR: (u8, u8, u8) = (0x11, 0x77, 0xEE);

/// A brightness inside the 1-100 range and clear of any default.
const TEST_BRIGHTNESS: u8 = 42;

/// The first brightness the protocol puts out of range.
const TOO_BRIGHT: u8 = 101;

/// GET_LED_MODE_INFO flags: bit 0 "this mode takes a period on this LED".
const PERIOD_FLAG: u8 = 0x01;

/// Where a scenario puts the millisecond counter before a hold, clear of zero
/// and of the counter's wrap.
const CLOCK_BASE_US: u64 = 4_000_000;

/// Reads of the progress byte taken before concluding a command has completed
/// promptly rather than by luck.
const SETTLE_POLLS: u32 = 8;

/// API identifiers withheld from the plugin for the scenarios that need them.
///
/// The LED calls arrived in firmware later than this plugin's minimum, so it
/// degrades where they are absent.  The emulator implements the whole API, so
/// withholding is the only way to reach that path.
pub static WITHHELD_API: &[(&str, &[u32])] = &[
    (
        "conformance.led.no_leds_without_the_led_calls",
        &[ffi::api_id_t_ORA_ID_LED_SET, ffi::api_id_t_ORA_ID_LED_GET],
    ),
    (
        "conformance.led.no_leds_without_the_led_get_call",
        &[ffi::api_id_t_ORA_ID_LED_GET],
    ),
];

/// What GET_LED_CAPABILITY reports.
#[derive(Clone, Copy)]
struct Capability {
    count: u8,
    max_period: u8,
    max_hold: u8,
}

/// What GET_LED_INFO reports, named rather than indexed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Info {
    kind: u8,
    mode: u8,
    red: u8,
    green: u8,
    blue: u8,
    brightness: u8,
    period: u8,
    modes: u8,
}

/// SET_LED's eight arguments: "A0=mode, A1=red, A2=green, A3=blue,
/// A4=brightness, A5=period, A6=hold, A7=led".
fn set_args(
    mode: u8,
    colour: (u8, u8, u8),
    brightness: u8,
    period: u8,
    hold: u8,
    led: u8,
) -> [u8; 8] {
    [
        mode, colour.0, colour.1, colour.2, brightness, period, hold, led,
    ]
}

/// SET_LED naming only a mode, which is what a host that wants the device's own
/// colour, brightness and period sends.
fn plain(mode: u8, led: u8) -> [u8; 8] {
    set_args(mode, (0, 0, 0), 0, 0, 0, led)
}

fn capability(bus: &mut Bus, s: &Session) -> Result<Capability, String> {
    bus.issue_cmd(s, group::LED, led::GET_LED_CAPABILITY, &[])
        .map_err(|e| format!("GET_LED_CAPABILITY: {e}"))?;
    let d = bus.read_data(s, 0, 3)?;
    Ok(Capability {
        count: d[0],
        max_period: d[1],
        max_hold: d[2],
    })
}

fn info(bus: &mut Bus, s: &Session, n: u8) -> Result<Info, String> {
    bus.issue_cmd(s, group::LED, led::GET_LED_INFO, &[n])
        .map_err(|e| format!("GET_LED_INFO for LED {n}: {e}"))?;
    let d = bus.read_data(s, 0, INFO_BYTES)?;
    Ok(Info {
        kind: d[0],
        mode: d[1],
        red: d[2],
        green: d[3],
        blue: d[4],
        brightness: d[5],
        period: d[6],
        modes: d[8],
    })
}

/// Whether an LED supports a mode, from the bitmap it reports.
fn supports(i: &Info, mode: u8) -> bool {
    mode < 8 && (i.modes & (1 << mode)) != 0
}

/// What GET_LED_MODE_INFO reports for one mode on one LED.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ModeInfo {
    takes_period: bool,
    min_period: u8,
    flags: u8,
}

fn mode_info(bus: &mut Bus, s: &Session, mode: u8, n: u8) -> Result<ModeInfo, String> {
    bus.issue_cmd(s, group::LED, led::GET_LED_MODE_INFO, &[mode, n])
        .map_err(|e| format!("GET_LED_MODE_INFO for mode 0x{mode:02X} on LED {n}: {e}"))?;
    let d = bus.read_data(s, 0, 2)?;
    Ok(ModeInfo {
        takes_period: (d[0] & PERIOD_FLAG) != 0,
        min_period: d[1],
        flags: d[0],
    })
}

/// The lowest period a mode accepts, which is its reported floor or one where
/// it reports none — zero being "the mode's own default" rather than a period.
fn lowest_period(m: &ModeInfo) -> u8 {
    m.min_period.max(1)
}

/// A session on a device that has at least one LED, or the reason to skip.
fn session_with_an_led(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Result<(Session, Capability), Outcome>, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;
    let cap = capability(bus, &s)?;
    if cap.count == 0 {
        return Ok(Err(Outcome::Skip(
            "the device has no LEDs, which the specification permits".into(),
        )));
    }
    Ok(Ok((s, cap)))
}

/// The lowest-numbered LED of the given type, which is how the specification
/// tells a host to find one: "A host wanting a colour uses the lowest-numbered
/// LED of type RGB."
fn find_type(
    bus: &mut Bus,
    s: &Session,
    count: u8,
    want: u8,
) -> Result<Option<(u8, Info)>, String> {
    for n in 0..count {
        let i = info(bus, s, n)?;
        if i.kind == want {
            return Ok(Some((n, i)));
        }
    }
    Ok(None)
}

/// Require that every command in the group fails, which is what a device with
/// no LEDs must do with all but the capability command.
fn expect_zero_led_rule(bus: &mut Bus, s: &Session) -> Result<(), String> {
    bus.expect_rejected(s, group::LED, led::GET_LED_INFO, &[0])?;
    bus.expect_rejected(s, group::LED, led::GET_LED_MODE_INFO, &[ON, 0])?;
    bus.expect_rejected(s, group::LED, led::SET_LED, &plain(ON, 0))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

/// The device answers GET_LED_CAPABILITY, and its answer is self-consistent.
///
/// "Fails if the response data section is smaller than 8 bytes", and these
/// sessions are larger, so the command must succeed on every device — a device
/// with no LEDs included, whose answer is a count of zero.  Where the count is
/// zero this is also where "all other commands in this group return failure on
/// such a device" is asserted.
///
/// The count "never exceeds 170", 0xAA not being a valid LED number.
pub fn get_led_capability(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    bus.expect_data(&s, 3, &[0x00; 5], "GET_LED_CAPABILITY reserved bytes 3-7")?;

    if cap.count > 170 {
        return Err(format!(
            "the device reports {} LEDs; an LED number is a final argument, so 0xAA is not \
             one and a device may have at most 170",
            cap.count
        ));
    }

    if cap.count == 0 {
        expect_zero_led_rule(bus, &s)?;
    }

    Ok(Outcome::Pass)
}

/// Every LED the device advertises describes itself.
///
/// LEDs are numbered contiguously from zero, so a count of n means LEDs 0 to
/// n-1 all answer.  Each reports a type the protocol defines or leaves to the
/// implementation — 0x02 to 0x7F are reserved and 0xFF is Invalid, so neither
/// may appear — a mode in force that is not Invalid, a supported-modes bitmap
/// that includes that mode, and zero in its reserved bytes.
///
/// A mono LED must not claim a mode built out of a colour, which is the one
/// rule the protocol states about the bitmap's contents.
pub fn led_info_describes_every_led(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    for n in 0..cap.count {
        let i = info(bus, &s, n)?;

        if (0x02..=0x7F).contains(&i.kind) || i.kind == 0xFF {
            return Err(format!(
                "LED {n} reports type 0x{:02X}; RBCP 0.1.2 defines 0x00 (Monochrome) and 0x01 \
                 (RGB), reserves 0x02-0x7F, leaves 0x80-0xFE to the implementation and calls \
                 0xFF Invalid",
                i.kind
            ));
        }
        if i.mode == 0xFF {
            return Err(format!(
                "LED {n} reports mode 0xFF, which the protocol calls Invalid"
            ));
        }
        if !supports(&i, i.mode) && i.mode < 0x80 {
            return Err(format!(
                "LED {n} is in mode 0x{:02X} but its bitmap 0x{:02X} does not claim it",
                i.mode, i.modes
            ));
        }
        if (i.modes & (1 << MODE_RESERVED)) != 0 || (i.modes & (1 << 0x07)) != 0 {
            return Err(format!(
                "LED {n} claims mode 0x06 or 0x07 in its bitmap 0x{:02X}; both are reserved",
                i.modes
            ));
        }
        if i.kind == TYPE_MONO && (supports(&i, CYCLE) || supports(&i, BREATHE)) {
            return Err(format!(
                "LED {n} is monochrome but claims Cycle or Breathe in its bitmap 0x{:02X}; \
                 both are built out of a colour",
                i.modes
            ));
        }
        if i.brightness > 100 {
            return Err(format!(
                "LED {n} reports brightness {}, above 100",
                i.brightness
            ));
        }
        if cap.max_period != 0 && i.period > cap.max_period {
            return Err(format!(
                "LED {n} reports period {} above the {} GET_LED_CAPABILITY allows",
                i.period, cap.max_period
            ));
        }

        bus.expect_data(
            &s,
            7,
            &[0x00],
            &format!("GET_LED_INFO reserved byte 7 for LED {n}"),
        )?;
        bus.expect_data(
            &s,
            9,
            &[0x00; 7],
            &format!("GET_LED_INFO reserved bytes 9-15 for LED {n}"),
        )?;
    }

    Ok(Outcome::Pass)
}

/// A monochrome LED reports the colour it shows.
///
/// "The colour fields describe the colour the LED shows when lit: the colour in
/// force on an LED whose colour the host sets, and the LED's own colour on one
/// whose it cannot."  All-zero is the device declining to say, which is
/// conformant — so this asserts the colour is stated where the device does
/// state one, and stands down where it does not.
pub fn mono_led_reports_the_colour_it_shows(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    let Some((n, i)) = find_type(bus, &s, cap.count, TYPE_MONO)? else {
        return Ok(Outcome::Skip("the device has no monochrome LED".into()));
    };

    if (i.red, i.green, i.blue) == (0, 0, 0) {
        return Ok(Outcome::Skip(format!(
            "LED {n} states no colour, which the specification permits for an LED whose \
             colour the device does not know"
        )));
    }

    // A mono LED has no brightness, and zero is how the protocol says so
    // rather than being a brightness of nothing.
    if i.brightness != 0 {
        return Err(format!(
            "monochrome LED {n} reports brightness {}; it has none, and zero is what says so",
            i.brightness
        ));
    }

    Ok(Outcome::Pass)
}

/// SET_LED changes the mode GET_LED_INFO reports, for every mode an LED claims.
///
/// The bitmap is the device's own statement of what it supports, so this holds
/// it to it: each claimed mode must be accepted and must come back as the mode
/// in force.
pub fn set_led_drives_every_mode_it_claims(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    for n in 0..cap.count {
        let claimed = info(bus, &s, n)?.modes;
        for mode in [OFF, ON, BLINK, BREATHE, CYCLE, BEACON] {
            if (claimed & (1 << mode)) == 0 {
                continue;
            }
            bus.issue_cmd(&s, group::LED, led::SET_LED, &plain(mode, n))
                .map_err(|e| format!("SET_LED mode 0x{mode:02X} on LED {n}: {e}"))?;
            let now = info(bus, &s, n)?;
            if now.mode != mode {
                return Err(format!(
                    "LED {n} was set to mode 0x{mode:02X} and reports 0x{:02X}",
                    now.mode
                ));
            }
        }
    }

    Ok(Outcome::Pass)
}

/// A colour and brightness the host names come back unchanged.
///
/// The round trip is the whole of what a host can see, and it is what makes
/// the colour bytes mean anything.  Only an RGB LED has a colour to set.
pub fn set_led_round_trips_colour_and_brightness(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    let Some((n, _)) = find_type(bus, &s, cap.count, TYPE_RGB)? else {
        return Ok(Outcome::Skip("the device has no RGB LED".into()));
    };

    bus.issue_cmd(
        &s,
        group::LED,
        led::SET_LED,
        &set_args(ON, TEST_COLOUR, TEST_BRIGHTNESS, 0, 0, n),
    )
    .map_err(|e| format!("SET_LED on LED {n}: {e}"))?;

    let got = info(bus, &s, n)?;
    if (got.red, got.green, got.blue) != TEST_COLOUR {
        return Err(format!(
            "LED {n} was set to ({:#04X}, {:#04X}, {:#04X}) and reports ({:#04X}, {:#04X}, \
             {:#04X})",
            TEST_COLOUR.0, TEST_COLOUR.1, TEST_COLOUR.2, got.red, got.green, got.blue
        ));
    }
    if got.brightness != TEST_BRIGHTNESS {
        return Err(format!(
            "LED {n} was set to brightness {TEST_BRIGHTNESS} and reports {}",
            got.brightness
        ));
    }

    Ok(Outcome::Pass)
}

/// All three colour bytes zero means the device chooses, not black.
///
/// "All three zero means the host names no colour, and the device chooses one."
/// So a device given zeros must not report them back as the colour in force —
/// it names one of its own.  Asserted from a known colour, so that what comes
/// back is the device's answer rather than what was already there.
pub fn all_zero_colour_is_the_device_s_own(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    let Some((n, _)) = find_type(bus, &s, cap.count, TYPE_RGB)? else {
        return Ok(Outcome::Skip("the device has no RGB LED".into()));
    };

    bus.issue_cmd(
        &s,
        group::LED,
        led::SET_LED,
        &set_args(ON, TEST_COLOUR, TEST_BRIGHTNESS, 0, 0, n),
    )
    .map_err(|e| format!("SET_LED arming LED {n}: {e}"))?;

    bus.issue_cmd(&s, group::LED, led::SET_LED, &plain(ON, n))
        .map_err(|e| format!("SET_LED with no colour on LED {n}: {e}"))?;

    let got = info(bus, &s, n)?;
    if (got.red, got.green, got.blue) == (0, 0, 0) {
        return Err(format!(
            "LED {n} was set to mode On with all three colour bytes zero and reports the \
             colour as zero; all zero means the device chooses one, and is not black"
        ));
    }

    Ok(Outcome::Pass)
}

/// A period the host names comes back in the units it named it in.
///
/// The period is "in units of 100ms" in both directions, so a host that sets
/// one and reads it back gets the same number.
pub fn set_led_round_trips_a_period(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    let mut tried = false;
    for n in 0..cap.count {
        let i = info(bus, &s, n)?;
        for mode in [BLINK, BREATHE, CYCLE, BEACON] {
            if !supports(&i, mode) {
                continue;
            }
            let m = mode_info(bus, &s, mode, n)?;
            if !m.takes_period {
                continue;
            }
            let floor = lowest_period(&m);
            if floor > cap.max_period {
                return Err(format!(
                    "LED {n} mode 0x{mode:02X} reports a floor of {floor} and the device a \
                     maximum of {}; nothing a host can ask for would be accepted",
                    cap.max_period
                ));
            }
            // A period clear of the floor, so a device that quietly substituted
            // its own default or its own minimum is caught.
            let want = floor.saturating_add(1).min(cap.max_period);

            bus.issue_cmd(
                &s,
                group::LED,
                led::SET_LED,
                &set_args(mode, (0, 0, 0), 0, want, 0, n),
            )
            .map_err(|e| format!("SET_LED mode 0x{mode:02X} period {want} on LED {n}: {e}"))?;

            let got = info(bus, &s, n)?;
            if got.period != want {
                return Err(format!(
                    "LED {n} mode 0x{mode:02X} was set to a period of {want} and reports {}",
                    got.period
                ));
            }
            tried = true;
        }
    }

    if tried {
        Ok(Outcome::Pass)
    } else {
        Ok(Outcome::Skip(
            "no mode on this device takes a period".into(),
        ))
    }
}

/// Every supported mode describes what it needs, and the description holds.
///
/// GET_LED_MODE_INFO's reserved flag bits and reserved bytes must be zero, a
/// mode that takes no period must report a floor of zero, and a floor must be
/// inside the maximum GET_LED_CAPABILITY reports.  Off and On "never take one",
/// which is the only part of the answer the protocol fixes.
pub fn mode_info_describes_every_supported_mode(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    for n in 0..cap.count {
        let i = info(bus, &s, n)?;
        for mode in [OFF, ON, BLINK, BREATHE, CYCLE, BEACON] {
            if !supports(&i, mode) {
                continue;
            }
            let m = mode_info(bus, &s, mode, n)?;

            if (m.flags & !PERIOD_FLAG) != 0 {
                return Err(format!(
                    "LED {n} mode 0x{mode:02X} reports flags 0x{:02X}; bits 1-7 are reserved",
                    m.flags
                ));
            }
            if (mode == OFF || mode == ON) && m.takes_period {
                return Err(format!(
                    "LED {n} reports that mode 0x{mode:02X} takes a period; Off and On never \
                     take one"
                ));
            }
            if !m.takes_period && m.min_period != 0 {
                return Err(format!(
                    "LED {n} mode 0x{mode:02X} takes no period but reports a floor of {}",
                    m.min_period
                ));
            }
            if m.min_period > cap.max_period {
                return Err(format!(
                    "LED {n} mode 0x{mode:02X} reports a floor of {} above the device maximum \
                     of {}",
                    m.min_period, cap.max_period
                ));
            }
            bus.expect_data(
                &s,
                2,
                &[0x00; 6],
                &format!("GET_LED_MODE_INFO reserved bytes 2-7 for mode 0x{mode:02X} of LED {n}"),
            )?;
        }
    }

    Ok(Outcome::Pass)
}

/// A period below the floor is refused, and the floor itself is accepted.
///
/// The floor is what makes SET_LED's period predictable: a host that reads it
/// can name a period that will work, and one below it "is outside the range
/// GET_LED_MODE_INFO reports for that mode".
pub fn set_led_honours_the_reported_floor(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    for n in 0..cap.count {
        let i = info(bus, &s, n)?;
        for mode in [BLINK, BREATHE, CYCLE, BEACON] {
            if !supports(&i, mode) {
                continue;
            }
            let m = mode_info(bus, &s, mode, n)?;
            if m.min_period < 2 {
                // A floor of zero or one bars nothing: one is the smallest
                // period a host can name, zero meaning the mode's default.
                continue;
            }

            bus.issue_cmd(
                &s,
                group::LED,
                led::SET_LED,
                &set_args(mode, (0, 0, 0), 0, m.min_period, 0, n),
            )
            .map_err(|e| {
                format!(
                    "{e} — LED {n} mode 0x{mode:02X} reports a floor of {}, which it must \
                     accept",
                    m.min_period
                )
            })?;

            bus.expect_rejected(
                &s,
                group::LED,
                led::SET_LED,
                &set_args(mode, (0, 0, 0), 0, m.min_period - 1, 0, n),
            )
            .map_err(|e| {
                format!(
                    "{e} — LED {n} mode 0x{mode:02X} reports a floor of {}",
                    m.min_period
                )
            })?;

            return Ok(Outcome::Pass);
        }
    }

    Ok(Outcome::Skip(
        "no mode on this device reports a floor a host could fall below".into(),
    ))
}

/// GET_LED_MODE_INFO refuses a mode the LED does not support, and an absent LED.
pub fn mode_info_rejects_what_it_cannot_describe(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    bus.expect_rejected(&s, group::LED, led::GET_LED_MODE_INFO, &[MODE_RESERVED, 0])
        .map_err(|e| format!("{e} — mode 0x06 is reserved by the protocol"))?;
    bus.expect_rejected(&s, group::LED, led::GET_LED_MODE_INFO, &[ON, cap.count])
        .map_err(|e| format!("{e} — the device has {} LEDs", cap.count))?;
    bus.expect_rejected(&s, group::LED, led::GET_LED_MODE_INFO, &[ON, 0xAA])?;

    for n in 0..cap.count {
        let i = info(bus, &s, n)?;
        for mode in [CYCLE, BREATHE, BLINK, BEACON] {
            if supports(&i, mode) {
                continue;
            }
            bus.expect_rejected(&s, group::LED, led::GET_LED_MODE_INFO, &[mode, n])
                .map_err(|e| {
                    format!(
                        "{e} — LED {n}'s bitmap 0x{:02X} does not claim mode 0x{mode:02X}",
                        i.modes
                    )
                })?;
        }
    }

    Ok(Outcome::Pass)
}

/// SET_LED completes without waiting for its hold, and the hold puts the LED
/// back.
///
/// "The device completes the command without waiting for the hold, which
/// outlives the command-response session."  Both halves: the command completes
/// with the clock standing still — a device timing the hold the way SET_AUX
/// does could not — and moving the clock past the deadline restores what was in
/// force when the command arrived.
pub fn set_led_hold_does_not_block(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };
    if cap.max_hold == 0 {
        return Ok(Outcome::Skip(
            "the device offers no timed holds, which the specification permits".into(),
        ));
    }

    // Armed with a repeating mode where one is available, so that the restore
    // covers the period as well as the mode - the specification has a bounded
    // mode put back "the mode, colour, brightness and period".
    let n = 0;
    let armed = info(bus, &s, n)?;
    let arm = match [BLINK, BREATHE, CYCLE]
        .into_iter()
        .find(|&m| supports(&armed, m))
    {
        Some(mode) => {
            let m = mode_info(bus, &s, mode, n)?;
            let period = lowest_period(&m).saturating_add(1).min(cap.max_period);
            set_args(mode, (0, 0, 0), 0, period, 0, n)
        }
        None => plain(OFF, n),
    };

    bus.set_clock_us(CLOCK_BASE_US);
    bus.issue_cmd(&s, group::LED, led::SET_LED, &arm)
        .map_err(|e| format!("SET_LED arming LED {n}: {e}"))?;
    let before = info(bus, &s, n)?;

    // Nothing here moves the clock but a scenario, so it stands still for the
    // whole of this command: a device that waited for the hold the way SET_AUX
    // does would never complete it, and issue_cmd would report that.
    let hold = cap.max_hold.min(10);
    bus.issue_cmd(
        &s,
        group::LED,
        led::SET_LED,
        &set_args(ON, (0, 0, 0), 0, 0, hold, n),
    )
    .map_err(|e| format!("SET_LED with a hold of {hold} units on LED {n}: {e}"))?;

    let held = info(bus, &s, n)?;
    if held.mode != ON {
        return Err(format!(
            "LED {n} was set to mode 0x{ON:02X} for {hold} units and reports 0x{:02X}; a held \
             mode takes the LED for the length of the hold",
            held.mode
        ));
    }
    for _ in 0..SETTLE_POLLS {
        bus.read_hdr(&s, Hdr::Progress)?;
    }

    // Stand where the engine's timer interrupt stands: past the deadline, then
    // run the frame it would have run.  Advanced rather than set, so the
    // arithmetic cannot land before a deadline the device took from a clock
    // some other scenario left elsewhere.
    bus.advance_clock_us(u64::from(hold) * 100_000 + 1_000);
    bus.led_frame();

    let after = info(bus, &s, n)?;
    if after != before {
        return Err(format!(
            "LED {n} was {before:?}, was set to mode 0x{ON:02X} for {hold} units, and after \
             the hold reports {after:?}; the device restores the mode, colour, brightness and \
             period that were in force when the command arrived"
        ));
    }

    Ok(Outcome::Pass)
}

/// SET_LED refuses a mode the LED does not claim, and an out-of-range
/// brightness.
///
/// "Fails if ... mode is not one that LED supports ... if brightness exceeds
/// 100."  A reserved mode is refused by every device, and a mono LED refuses
/// the two modes built out of a colour.
pub fn set_led_rejects_what_it_cannot_do(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    bus.expect_rejected(&s, group::LED, led::SET_LED, &plain(MODE_RESERVED, 0))
        .map_err(|e| format!("{e} — mode 0x06 is reserved by the protocol"))?;

    bus.expect_rejected(
        &s,
        group::LED,
        led::SET_LED,
        &set_args(ON, (0, 0, 0), TOO_BRIGHT, 0, 0, 0),
    )
    .map_err(|e| format!("{e} — brightness {TOO_BRIGHT} is above 100"))?;

    for n in 0..cap.count {
        let i = info(bus, &s, n)?;
        for mode in [CYCLE, BREATHE, BLINK, BEACON] {
            if supports(&i, mode) {
                continue;
            }
            bus.expect_rejected(&s, group::LED, led::SET_LED, &plain(mode, n))
                .map_err(|e| {
                    format!(
                        "{e} — LED {n}'s bitmap 0x{:02X} does not claim mode 0x{mode:02X}",
                        i.modes
                    )
                })?;
        }
    }

    Ok(Outcome::Pass)
}

/// An implementation-specific mode round-trips where the device has one.
///
/// Not RBCP: modes 0x80-0xFE are "reserved for implementation-specific use",
/// and the bitmap cannot report them, so a host issues one knowing the device.
/// One ROM's flame is at 0x80.  A device without it must refuse rather than
/// silently drive something else, which is the half that matters to a host.
pub fn implementation_specific_mode_is_honoured_or_refused(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let (s, _) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    let n = 0;
    let before = info(bus, &s, n)?;
    match bus.issue_cmd(&s, group::LED, led::SET_LED, &plain(MODE_FLAME, n)) {
        Ok(()) => {
            let got = info(bus, &s, n)?;
            if got.mode != MODE_FLAME {
                return Err(format!(
                    "LED {n} accepted mode 0x{MODE_FLAME:02X} and reports 0x{:02X}",
                    got.mode
                ));
            }
        }
        Err(CmdFailure::Failed) => {
            let got = info(bus, &s, n)?;
            if got.mode != before.mode {
                return Err(format!(
                    "LED {n} refused mode 0x{MODE_FLAME:02X} but moved from 0x{:02X} to \
                     0x{:02X}",
                    before.mode, got.mode
                ));
            }
        }
        Err(e) => return Err(format!("SET_LED mode 0x{MODE_FLAME:02X}: {e}")),
    }

    Ok(Outcome::Pass)
}

/// Both commands taking an LED number refuse one the device does not have, and
/// refuse 0xAA.
///
/// "Fails if the LED is not one the device has", and "An A0 value of 0xAA is
/// invalid and rejected" for GET_LED_INFO, A7 for SET_LED.  LEDs are numbered
/// contiguously from zero, so the count is itself the first number that is not
/// one.
pub fn commands_reject_an_absent_led(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    bus.expect_rejected(&s, group::LED, led::GET_LED_INFO, &[cap.count])
        .map_err(|e| format!("{e} — the device has {} LEDs", cap.count))?;
    bus.expect_rejected(&s, group::LED, led::SET_LED, &plain(ON, cap.count))
        .map_err(|e| format!("{e} — the device has {} LEDs", cap.count))?;

    bus.expect_rejected(&s, group::LED, led::GET_LED_INFO, &[0xAA])?;
    bus.expect_rejected(&s, group::LED, led::SET_LED, &plain(ON, 0xAA))?;

    Ok(Outcome::Pass)
}

/// Each query command needs room for its whole answer.
///
/// GET_LED_CAPABILITY "fails if the response data section is smaller than 8
/// bytes" and GET_LED_INFO "smaller than 16 bytes", so a section between the
/// two must serve the first and refuse the second.  A device with no LEDs
/// answers the capability command too, so the first half holds either way.
pub fn query_commands_need_room_for_their_answer(
    bus: &mut Bus,
    ctx: &Ctx,
) -> Result<Outcome, String> {
    let between = (INFO_BYTES - 4) as u16;
    let s = bus.enter_sized(ctx, HDR_SIZE as u16 + between)?;

    bus.issue_cmd(&s, group::LED, led::GET_LED_CAPABILITY, &[])
        .map_err(|e| format!("GET_LED_CAPABILITY in a {between}-byte data section: {e}"))?;
    bus.expect_rejected(&s, group::LED, led::GET_LED_INFO, &[0])
        .map_err(|e| format!("{e} — the data section is {between} bytes and the answer is 16"))?;

    // ENTER_CMD_RESP "is not supported when in command-response mode", so the
    // second session needs the first one closed.
    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    let short = (CAPABILITY_BYTES - 4) as u16;
    let s = bus.enter_sized(ctx, HDR_SIZE as u16 + short)?;
    for (cmd, args) in [
        (led::GET_LED_CAPABILITY, &[][..]),
        (led::GET_LED_MODE_INFO, &[ON, 0][..]),
    ] {
        bus.expect_rejected(&s, group::LED, cmd, args)
            .map_err(|e| format!("{e} — the data section is {short} bytes and the answer is 8"))?;
    }

    Ok(Outcome::Pass)
}

/// All four commands are refused in command mode, and cost the host nothing.
///
/// "All commands in this group are valid in command-response mode only", so
/// neither a query nor a SET_LED may act.  Two assertions, because the group's
/// two kinds of command fail differently.
///
/// The queries must not answer, which the armed data section says: a device
/// that answered would write its response where the host last asked for one.
///
/// SET_LED must not act, which the LED itself says.  An RGB LED is put in a
/// known state before the session ends, the command-mode SET_LED names a
/// different colour and brightness, and the LED must still hold the first when
/// the next session reads it back.  That is the half with teeth - the data
/// section is no longer maintained once the session is over, so a device that
/// acted on the query would likely miss it anyway.
///
/// Each command is knocked in its own right.  In command mode the device is
/// waiting for a knock, so a bare command frame is read as junk and never
/// dispatched.
///
/// The other half of the requirement is deliberately not asserted here.  A
/// refused command "is nonetheless framed like any other: the device consumes
/// its argument bytes before discarding it", and a knocked command afterwards
/// cannot see a missing discard, because the knock is matched by the firmware's
/// address monitor as a sliding window and leftover argument bytes slide past
/// it.  `argument_counts_are_consumed_exactly` covers consumption in
/// command-response mode, where the token makes it observable.
pub fn not_valid_in_command_mode(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    // An RGB LED, so that colour and brightness read back.  Where the board has
    // none the command loop below still runs - reaching the device's refusal is
    // the point, and only the SET_LED half of the assertion needs the LED.
    let armed_led = match find_type(bus, &s, cap.count, TYPE_RGB)? {
        Some((n, _)) => {
            bus.issue_cmd(
                &s,
                group::LED,
                led::SET_LED,
                &set_args(ON, TEST_COLOUR, TEST_BRIGHTNESS, 0, 0, n),
            )
            .map_err(|e| format!("SET_LED arming LED {n}: {e}"))?;
            Some((n, info(bus, &s, n)?))
        }
        None => None,
    };

    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;

    let dst = s.bch_start + HDR_SIZE;
    let armed = bus.read(dst)? ^ 0xFF;
    bus.poke_verified(ctx, dst, armed)
        .map_err(|e| format!("arming the response data section: {e}"))?;

    // A colour and brightness the armed LED is not already showing, so a
    // SET_LED that took effect cannot be mistaken for one that did not.
    let n = armed_led.map_or(0, |(n, _)| n);
    for (cmd, args) in [
        (led::GET_LED_CAPABILITY, &[][..]),
        (led::GET_LED_INFO, &[n][..]),
        (led::GET_LED_MODE_INFO, &[ON, n][..]),
        (
            led::SET_LED,
            &set_args(ON, (0xFF, 0x00, 0x00), 99, 0, 0, n)[..],
        ),
    ] {
        bus.knock(s.command_page)?;
        bus.send_cmd(s.command_page, group::LED, cmd, args)?;
    }

    if bus.read(dst)? != armed {
        return Err("a command-mode LED query wrote to the response data section".into());
    }

    if let Some((n, want)) = armed_led {
        let s = ctx.session();
        bus.enter_cmd_resp(&s)
            .map_err(|e| format!("re-entering command-response mode: {e}"))?;
        let got = info(bus, &s, n)?;
        if got != want {
            return Err(format!(
                "a SET_LED issued in command mode took effect on LED {n}: it was {want:?} and \
                 is now {got:?}"
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// The device takes exactly the argument bytes each command declares.
///
/// A device taking one too few leaves a byte to be read as the next frame's
/// GROUP, and one too many swallows the next frame's first byte.  Either
/// desynchronises the session undetectably, so this sends each command and then
/// a NOP, and requires the NOP to arrive.
pub fn argument_counts_are_consumed_exactly(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, _) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    for (cmd, args) in [
        (led::GET_LED_CAPABILITY, &[][..]),
        (led::GET_LED_INFO, &[0][..]),
        (led::GET_LED_MODE_INFO, &[ON, 0][..]),
        (led::SET_LED, &plain(OFF, 0)[..]),
    ] {
        let _ = bus.issue_cmd(&s, group::LED, cmd, args);
        bus.issue_cmd(&s, group::CONTROL, control::NOP, &[])
            .map_err(|e| {
                format!(
                    "the NOP after 0x06/0x{cmd:02X} did not arrive ({e}), so the device did not \
                     consume that command's arguments exactly"
                )
            })?;
    }

    Ok(Outcome::Pass)
}

/// An LED's state survives leaving command-response mode, and RBCP_RESET.
///
/// "An LED's state persists across the end of a command-response session and
/// across RBCP_RESET.  Only a device reset restores an LED to its power-on
/// state."
pub fn led_state_survives_the_session(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    let Some((n, _)) = find_type(bus, &s, cap.count, TYPE_RGB)? else {
        return Ok(Outcome::Skip("the device has no RGB LED".into()));
    };

    bus.issue_cmd(
        &s,
        group::LED,
        led::SET_LED,
        &set_args(ON, TEST_COLOUR, TEST_BRIGHTNESS, 0, 0, n),
    )
    .map_err(|e| format!("SET_LED on LED {n}: {e}"))?;
    let want = info(bus, &s, n)?;

    bus.issue_cmd(&s, group::CONTROL, control::EXIT_CMD_RESP_ACK, &[])
        .map_err(|e| format!("EXIT_CMD_RESP_ACK: {e}"))?;
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("re-entering command-response mode: {e}"))?;
    if info(bus, &s, n)? != want {
        return Err(format!(
            "LED {n} did not survive the end of the session: {want:?} became {:?}",
            info(bus, &s, n)?
        ));
    }

    bus.reset(s.command_page)?;
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("re-entering command-response mode after RBCP_RESET: {e}"))?;
    if info(bus, &s, n)? != want {
        return Err(format!(
            "LED {n} did not survive RBCP_RESET: {want:?} became {:?}",
            info(bus, &s, n)?
        ));
    }

    Ok(Outcome::Pass)
}

/// Without the firmware's LED calls the device reports no LEDs.
///
/// A plugin degrades where a call its minimum firmware version does not
/// guarantee is missing.  The protocol already provides for it — a count of
/// zero — so the group goes quiet rather than the plugin refusing to run, and
/// every other command in the group fails.
pub fn no_leds_without_the_led_calls(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    if cap.count != 0 {
        return Err(format!(
            "the plugin reports {} LEDs with the firmware's LED calls withheld; without them \
             it has no engine to reach",
            cap.count
        ));
    }
    expect_zero_led_rule(bus, &s)?;

    Ok(Outcome::Pass)
}

/// A device that can drive an LED but not read one back reports no LEDs.
///
/// The other half of the degradation [`no_leds_without_the_led_calls`] covers,
/// and not the same path: there the plugin has no engine at all, here it has one
/// it can write to but cannot ask about.  RBCP numbers only the LEDs a board
/// actually carries, and reading a channel back is how the plugin finds out
/// which those are — so a channel it cannot read is one it cannot advertise,
/// however willing the engine is to be told about it.
///
/// The distinction matters because a device getting this wrong looks fine from
/// the capability command and falls apart from GET_LED_INFO onwards: it would be
/// offering a host LEDs whose type, mode and colour it has no way to report.
/// Reached by withholding `ORA_ID_LED_GET` alone — see [`WITHHELD_API`] — since
/// the emulator implements the whole API.
pub fn no_leds_without_the_led_get_call(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let s = ctx.session();
    bus.enter_cmd_resp(&s)
        .map_err(|e| format!("ENTER_CMD_RESP: {e}"))?;

    let cap = capability(bus, &s)?;
    if cap.count != 0 {
        return Err(format!(
            "the plugin reports {} LED(s) with the firmware's LED read call withheld; it cannot \
             tell which channels the board carries, nor answer GET_LED_INFO for any of them",
            cap.count
        ));
    }
    expect_zero_led_rule(bus, &s)?;

    Ok(Outcome::Pass)
}

/// A slot switch does not disturb the LEDs.
///
/// Nothing in the protocol ties an LED to a slot, and a bootloader sets its
/// colour and then switches — so this asserts the pairing that flow depends on.
pub fn led_state_survives_a_slot_switch(bus: &mut Bus, ctx: &Ctx) -> Result<Outcome, String> {
    let (s, cap) = match session_with_an_led(bus, ctx)? {
        Ok(v) => v,
        Err(o) => return Ok(o),
    };

    let Some((n, _)) = find_type(bus, &s, cap.count, TYPE_RGB)? else {
        return Ok(Outcome::Skip("the device has no RGB LED".into()));
    };

    bus.issue_cmd(
        &s,
        group::LED,
        led::SET_LED,
        &set_args(ON, TEST_COLOUR, TEST_BRIGHTNESS, 0, 0, n),
    )
    .map_err(|e| format!("SET_LED on LED {n}: {e}"))?;
    let want = info(bus, &s, n)?;

    bus.issue_cmd(
        &s,
        group::MODIFY,
        modify::SWITCH_SLOT,
        &[ctx.active_ram_slot],
    )
    .map_err(|e| format!("SWITCH_SLOT: {e}"))?;

    let got = info(bus, &s, n)?;
    if got != want {
        return Err(format!(
            "LED {n} changed across a slot switch: {want:?} became {got:?}"
        ));
    }

    Ok(Outcome::Pass)
}
