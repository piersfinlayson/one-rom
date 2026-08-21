// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the LED plugin API (`ORA_ID_LED_SET` and `ORA_ID_LED_GET`).
//!
//! # What these own, and what they leave alone
//!
//! The engine's shape - that a hue walks, that a breath fades, that a beacon
//! ends - is asserted by the USB plugin tester's LED suite, which drives this
//! same engine through the plugin's SET_LED command.  What that suite cannot
//! reach is the API boundary: it says only what its argument block can carry,
//! and it never calls `ora_led_set` at all.  So these tests own the contract -
//! the sizes, the refusals, the defaults and the state a caller reads back -
//! and take the animation only as far as proving a hold ends and gives the LED
//! back.
//!
//! # Where the expected answers come from
//!
//! The defaults and the floors are `onerom_metadata`'s `LED_*` constants, from
//! the metadata schema the firmware itself compiles against, so neither side
//! states a number the other does not.  Presence and the GPIO come from the
//! `GPIO_STATUS` and `GPIO_NEOPIXEL` metadata keys rather than from the engine,
//! so an engine answering from its own idea of the board fails here.
//!
//! # The clock
//!
//! Holds and frames are in the milliseconds `ora_get_plugin_uptime_ms` reports,
//! which in this process is a harness-owned counter rather than TIMER0.  Each
//! test places it where it needs it, and there is no interrupt to run a frame,
//! so a test stands where the interrupt does: move the clock to the deadline
//! the engine named, then run the frame.

use onerom_fw_emulator::{Emulator, LedState, OraResult, ffi};
use onerom_metadata::{
    GPIO_NONE, LED_BEACON_DEFAULT_PERIOD_MS, LED_BLINK_DEFAULT_PERIOD_MS, LED_BLINK_MIN_PERIOD_MS,
    LED_BREATHE_DEFAULT_PERIOD_MS, LED_BREATHE_MIN_PERIOD_MS, LED_CYCLE_DEFAULT_PERIOD_MS,
    LED_CYCLE_MIN_PERIOD_MS, LED_DEFAULT_BLUE, LED_DEFAULT_BRIGHTNESS, LED_DEFAULT_GREEN,
    LED_DEFAULT_RED, LED_FLAME_DEFAULT_PERIOD_MS, LED_MAX_HOLD_MS, LED_REQUEST_MIN_SIZE,
    LED_STATE_MIN_SIZE,
};

// The LEDs and the modes, named rather than numbered.  No cast: bindgen is
// given -fshort-enums to match the C, so these are already a byte wide.
const STATUS: u8 = ffi::ora_led_t_ORA_LED_STATUS;
const RGB: u8 = ffi::ora_led_t_ORA_LED_RGB;

const OFF: u8 = ffi::ora_led_mode_t_ORA_LED_MODE_OFF;
const ON: u8 = ffi::ora_led_mode_t_ORA_LED_MODE_ON;
const BEACON: u8 = ffi::ora_led_mode_t_ORA_LED_MODE_BEACON;
const FLAME: u8 = ffi::ora_led_mode_t_ORA_LED_MODE_FLAME;
const CYCLE: u8 = ffi::ora_led_mode_t_ORA_LED_MODE_CYCLE;
const BREATHE: u8 = ffi::ora_led_mode_t_ORA_LED_MODE_BREATHE;
const BLINK: u8 = ffi::ora_led_mode_t_ORA_LED_MODE_BLINK;

/// The first value that is not a mode, which is what a request naming one gets
/// refused for.
const NOT_A_MODE: u8 = BLINK + 1;

/// The first LED channel this firmware does not have.
const NOT_AN_LED: u8 = RGB + 1;

/// Where the clock sits for a test that does not care where it sits - clear of
/// zero and clear of the wrap, so no arithmetic here is near an edge by
/// accident.
const BASE_MS: u32 = 10_000;

/// A period both LEDs accept for blink, distinctive enough that a request which
/// wrongly took effect moves it.
const ARMED_PERIOD_MS: u16 = 1_000;

fn ms_to_us(ms: u32) -> u64 {
    u64::from(ms) * 1_000
}

/// A zeroed request for `led` in `mode`, with the size a caller built against
/// this header declares.
fn request(led: u8, mode: u8) -> ffi::ora_led_request_t {
    let mut req = Emulator::led_request();
    req.led = led;
    req.mode = mode;
    req
}

/// The GPIO the board's metadata says an LED is on, or `GPIO_NONE`.
///
/// This is the independent answer the engine's own `gpio` and `present` fields
/// are checked against.
fn metadata_gpio(emu: &Emulator, led: u8) -> Result<u8, String> {
    let key = if led == RGB {
        ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_NEOPIXEL
    } else {
        ffi::ora_metadata_key_t_ORA_METADATA_KEY_GPIO_STATUS
    };
    let (result, value) = emu.get_metadata_uint(key);
    if !result.is_ok() {
        return Err(format!("GPIO metadata for LED {led}: {result:?}"));
    }
    let value = value.ok_or_else(|| format!("GPIO metadata for LED {led}: OK but no value"))?;

    Ok(value as u8)
}

fn present(emu: &Emulator, led: u8) -> Result<bool, String> {
    Ok(metadata_gpio(emu, led)? != GPIO_NONE)
}

/// Whether this board has an RGB LED, for the caller deciding whether the
/// RGB-only tests can run at all.
pub fn has_rgb(emu: &Emulator) -> bool {
    metadata_gpio(emu, RGB).is_ok_and(|g| g != GPIO_NONE)
}

/// The live status-LED state, as the other plugin reads it.
fn status_led_state(emu: &Emulator) -> Result<bool, String> {
    let (result, value) =
        emu.get_metadata_uint(ffi::ora_metadata_key_t_ORA_METADATA_KEY_STATUS_LED_STATE);
    if !result.is_ok() {
        return Err(format!("STATUS_LED_STATE: {result:?}"));
    }
    let value = value.ok_or("STATUS_LED_STATE: OK but no value")?;

    Ok(value != 0)
}

/// `ora_led_get`, where anything but success is the test's failure.
fn state(emu: &Emulator, led: u8) -> Result<LedState, String> {
    let (result, state) = emu.led_get(led);
    if !result.is_ok() {
        return Err(format!("led_get({led}): {result:?}"));
    }

    Ok(state)
}

/// `ora_led_set`, where anything but success is the test's failure.
fn set(emu: &Emulator, req: &ffi::ora_led_request_t) -> Result<(), String> {
    let result = emu.led_set(req);
    if !result.is_ok() {
        return Err(format!(
            "led_set(led {}, mode {}): {result:?}",
            req.led, req.mode
        ));
    }

    Ok(())
}

/// Put an LED into a state a later assertion can be made against, and return
/// it.
fn arm(emu: &Emulator, led: u8) -> Result<LedState, String> {
    let mut req = request(led, BLINK);
    req.period_ms = ARMED_PERIOD_MS;
    set(emu, &req)?;

    state(emu, led)
}

/// Run `body` against an engine that starts from a known state and is put back
/// afterwards.
///
/// Both LEDs and the clock are shared with everything else in this process, so
/// a test neither inherits what the test before it left nor leaves its own
/// state behind.  Setting the status LED after the reset re-establishes the
/// channel and `status_led_enabled` together, which a reset alone does not: the
/// reset forgets what the channels were doing without driving either LED.
fn with_engine<F>(emu: &Emulator, body: F) -> Result<(), String>
where
    F: FnOnce(&Emulator) -> Result<(), String>,
{
    let boot_state = status_led_state(emu)?;

    emu.led_reset();
    emu.set_timer_us(ms_to_us(BASE_MS));

    let result = body(emu);

    emu.led_reset();
    emu.set_timer_us(ms_to_us(BASE_MS));
    emu.set_status_led(boot_state);

    result
}

/// Verify what `ora_led_get` says about each LED's presence and wiring, against
/// the board's metadata, and that it refuses what it cannot describe.
pub fn test_led_presence(emu: &Emulator) -> Result<(), String> {
    with_engine(emu, |emu| {
        let mut errors = Vec::new();
        let mut described = Vec::new();

        for led in [STATUS, RGB] {
            let expected_gpio = metadata_gpio(emu, led)?;
            let expected_present = u8::from(expected_gpio != GPIO_NONE);

            let (result, s) = emu.led_get(led);
            if !result.is_ok() {
                errors.push(format!("led {led}: get failed: {result:?}"));
                continue;
            }
            if s.led != led {
                errors.push(format!("led {led}: described led {} instead", s.led));
            }
            if s.gpio != expected_gpio {
                errors.push(format!(
                    "led {led}: on GPIO {}, metadata says {}",
                    s.gpio, expected_gpio
                ));
            }
            if s.present != expected_present {
                errors.push(format!(
                    "led {led}: present {}, metadata says {}",
                    s.present, expected_present
                ));
            }
            // A reserved byte carrying whatever the caller's structure held is
            // what would reach a host through a plugin forwarding it.
            if s.reserved != 0 {
                errors.push(format!("led {led}: reserved byte is {:#04x}", s.reserved));
            }
            if usize::from(s.size) != size_of::<ffi::ora_led_state_t>() {
                errors.push(format!(
                    "led {led}: wrote {} bytes, expected {}",
                    s.size,
                    size_of::<ffi::ora_led_state_t>()
                ));
            }

            described.push(format!(
                "{}: {}",
                if led == RGB { "rgb" } else { "status" },
                if expected_present == 1 {
                    format!("GPIO {expected_gpio}")
                } else {
                    "absent".to_string()
                }
            ));
        }

        // An LED this firmware does not number is refused rather than described
        // as absent, which is what makes the two answers above distinguishable.
        for led in [NOT_AN_LED, 255] {
            let (result, _) = emu.led_get(led);
            if result != OraResult::InvalidArg {
                errors.push(format!(
                    "led {led} (not a channel): expected InvalidArg, got {result:?}"
                ));
            }
        }

        let result = emu.led_get_null(STATUS);
        if result != OraResult::InvalidArg {
            errors.push(format!(
                "null state_out: expected InvalidArg, got {result:?}"
            ));
        }

        if errors.is_empty() {
            println!("  {}", described.join(", "));
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    })
}

/// Verify the size contract both LED structures carry: a caller declaring less
/// than the structure first shipped at is refused, one declaring more is served
/// what this firmware knows and told how much that was.
pub fn test_led_size_contract(emu: &Emulator) -> Result<(), String> {
    with_engine(emu, |emu| {
        if !present(emu, STATUS)? {
            return Err("board has no status LED to exercise the sizes on".to_string());
        }

        let mut errors = Vec::new();
        let state_size = size_of::<ffi::ora_led_state_t>() as u8;

        // ── ora_led_get ──────────────────────────────────────────────────────
        for size in [0u8, LED_STATE_MIN_SIZE - 1] {
            let (result, _) = emu.led_get_sized(STATUS, size);
            if result != OraResult::InvalidSize {
                errors.push(format!(
                    "get with size {size}: expected InvalidSize, got {result:?}"
                ));
            }
        }

        for size in [LED_STATE_MIN_SIZE, state_size, 255] {
            let (result, s) = emu.led_get_sized(STATUS, size);
            let expected = size.min(state_size);
            if !result.is_ok() {
                errors.push(format!("get with size {size}: {result:?}"));
                continue;
            }
            if s.size != expected {
                errors.push(format!(
                    "get with size {size}: reported writing {}, expected {expected}",
                    s.size
                ));
            }
            // 0xFF is the sentinel the wrapper pre-fills with, so this says the
            // firmware wrote the field rather than left it.
            if s.led != STATUS {
                errors.push(format!(
                    "get with size {size}: led field left at {:#04x}",
                    s.led
                ));
            }
        }

        // ── ora_led_set ──────────────────────────────────────────────────────
        //
        // A refused request must leave the LED alone, so each is fenced against
        // a state that any request taking effect would move.
        let armed = arm(emu, STATUS)?;

        for size in [0u8, LED_REQUEST_MIN_SIZE - 1] {
            let mut req = request(STATUS, ON);
            req.size = size;
            let result = emu.led_set(&req);
            if result != OraResult::InvalidSize {
                errors.push(format!(
                    "set with size {size}: expected InvalidSize, got {result:?}"
                ));
            }
            if state(emu, STATUS)? != armed {
                errors.push(format!("set with size {size}: refused but changed the LED"));
            }
        }

        // A caller that knows a larger structure than this firmware does is
        // read for the fields this firmware knows, which is what keeps a plugin
        // built against a later header working here.
        for size in [LED_REQUEST_MIN_SIZE, 255] {
            arm(emu, STATUS)?;
            let mut req = request(STATUS, ON);
            req.size = size;
            let result = emu.led_set(&req);
            if !result.is_ok() {
                errors.push(format!("set with size {size}: {result:?}"));
                continue;
            }
            if state(emu, STATUS)?.mode != ON {
                errors.push(format!("set with size {size}: accepted but did nothing"));
            }
        }

        if errors.is_empty() {
            println!(
                "  request floor {LED_REQUEST_MIN_SIZE}, state floor {LED_STATE_MIN_SIZE}, \
                 this build's state {state_size}"
            );
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    })
}

/// Verify every way `ora_led_set` refuses a request, that a refusal leaves the
/// LED where it was, and that the values either side of each limit are the ones
/// the limit says.
pub fn test_led_rejects(emu: &Emulator) -> Result<(), String> {
    with_engine(emu, |emu| {
        if !present(emu, STATUS)? {
            return Err("board has no status LED to exercise the refusals on".to_string());
        }

        let mut errors = Vec::new();
        let armed = arm(emu, STATUS)?;

        // Each of these is refused for one reason, and the request is otherwise
        // one the firmware would have taken.
        let not_a_channel = request(NOT_AN_LED, ON);
        let not_a_mode = request(STATUS, NOT_A_MODE);
        let cycle_on_status = request(STATUS, CYCLE);
        let breathe_on_status = request(STATUS, BREATHE);
        let mut too_bright = request(STATUS, ON);
        too_bright.brightness = 101;
        let mut held_too_long = request(STATUS, ON);
        held_too_long.hold_ms = LED_MAX_HOLD_MS + 1;
        let mut blink_too_fast = request(STATUS, BLINK);
        blink_too_fast.period_ms = LED_BLINK_MIN_PERIOD_MS - 1;

        let refusals: &[(&str, &ffi::ora_led_request_t, OraResult)] = &[
            (
                "a channel this firmware has no LED for",
                &not_a_channel,
                OraResult::InvalidArg,
            ),
            (
                "a mode that is not a mode",
                &not_a_mode,
                OraResult::InvalidArg,
            ),
            (
                "cycle on the status LED",
                &cycle_on_status,
                OraResult::InvalidArg,
            ),
            (
                "breathe on the status LED",
                &breathe_on_status,
                OraResult::InvalidArg,
            ),
            ("brightness above 100", &too_bright, OraResult::InvalidArg),
            (
                "a hold beyond the limit",
                &held_too_long,
                OraResult::InvalidArg,
            ),
            (
                "a period below the mode's own",
                &blink_too_fast,
                OraResult::InvalidArg,
            ),
        ];

        for (what, req, expected) in refusals {
            let result = emu.led_set(req);
            if result != *expected {
                errors.push(format!("{what}: expected {expected:?}, got {result:?}"));
            }
            if state(emu, STATUS)? != armed {
                errors.push(format!("{what}: refused but changed the LED"));
            }
        }

        if emu.led_set_null() != OraResult::InvalidArg {
            errors.push("a null request: expected InvalidArg".to_string());
        }
        if state(emu, STATUS)? != armed {
            errors.push("a null request: refused but changed the LED".to_string());
        }

        // The values at each limit are accepted, which is what says the
        // refusals above are the limit rather than the neighbourhood of it.
        let mut brightest = request(STATUS, ON);
        brightest.brightness = 100;
        let mut longest_hold = request(STATUS, ON);
        longest_hold.hold_ms = LED_MAX_HOLD_MS;
        let mut fastest_blink = request(STATUS, BLINK);
        fastest_blink.period_ms = LED_BLINK_MIN_PERIOD_MS;

        let accepted: &[(&str, &ffi::ora_led_request_t)] = &[
            ("brightness of exactly 100", &brightest),
            ("a hold of exactly the limit", &longest_hold),
            ("a period of exactly the mode's own", &fastest_blink),
        ];

        for (what, req) in accepted {
            emu.led_reset();
            let result = emu.led_set(req);
            if !result.is_ok() {
                errors.push(format!("{what}: expected OK, got {result:?}"));
            }
        }

        // An LED the board does not have is refused as unsupported, which is a
        // different answer from a request that was malformed.
        emu.led_reset();
        let result = emu.led_set(&request(RGB, ON));
        let expected = if present(emu, RGB)? {
            OraResult::Ok
        } else {
            OraResult::NotSupported
        };
        if result != expected {
            errors.push(format!(
                "the RGB LED on this board: expected {expected:?}, got {result:?}"
            ));
        }

        if errors.is_empty() {
            println!(
                "  {} refusals and {} limits",
                refusals.len() + 1,
                accepted.len()
            );
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    })
}

/// Verify what the firmware fills in for a request that leaves a field at zero,
/// and that it keeps what a request does state.
pub fn test_led_defaults(emu: &Emulator) -> Result<(), String> {
    with_engine(emu, |emu| {
        if !present(emu, STATUS)? {
            return Err("board has no status LED to exercise the defaults on".to_string());
        }

        let mut errors = Vec::new();

        // The status LED is lit or dark.  A request carrying a colour and a
        // brightness for it records neither, so a reader is not told it is lit
        // some colour at some brightness.
        let mut coloured = request(STATUS, ON);
        coloured.brightness = 100;
        coloured.red = 1;
        coloured.green = 2;
        coloured.blue = 3;
        set(emu, &coloured)?;
        let s = state(emu, STATUS)?;
        if (s.brightness, s.red, s.green, s.blue) != (0, 0, 0, 0) {
            errors.push(format!(
                "the status LED recorded brightness {} and colour {},{},{}",
                s.brightness, s.red, s.green, s.blue
            ));
        }

        // Each mode's own period, where a request names none.
        let periods: &[(u8, u16, &str)] = &[
            (OFF, 0, "off"),
            (ON, 0, "on"),
            (BLINK, LED_BLINK_DEFAULT_PERIOD_MS, "blink"),
            (BEACON, LED_BEACON_DEFAULT_PERIOD_MS, "beacon"),
            (FLAME, LED_FLAME_DEFAULT_PERIOD_MS, "flame"),
        ];
        for (mode, expected, name) in periods {
            emu.led_reset();
            set(emu, &request(STATUS, *mode))?;
            let got = state(emu, STATUS)?.period_ms;
            if got != *expected {
                errors.push(format!(
                    "{name} with no period: {got} ms, expected {expected}"
                ));
            }
        }

        // A period that is stated is the one that runs.
        emu.led_reset();
        let mut stated = request(STATUS, BLINK);
        stated.period_ms = ARMED_PERIOD_MS;
        set(emu, &stated)?;
        let got = state(emu, STATUS)?.period_ms;
        if got != ARMED_PERIOD_MS {
            errors.push(format!("blink at {ARMED_PERIOD_MS} ms: reported {got} ms"));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    })
}

/// Verify the colour, brightness and period the RGB LED takes when a request
/// names none, and that it keeps what one does name.
///
/// Only run where the board has an RGB LED - see [`has_rgb`].
pub fn test_led_rgb_defaults(emu: &Emulator) -> Result<(), String> {
    with_engine(emu, |emu| {
        let mut errors = Vec::new();

        // A request naming no colour gets the firmware's rather than a dark
        // LED, and no brightness gets the firmware's rather than none.
        set(emu, &request(RGB, ON))?;
        let s = state(emu, RGB)?;
        if s.brightness != LED_DEFAULT_BRIGHTNESS {
            errors.push(format!(
                "an unnamed brightness read back {}, expected {LED_DEFAULT_BRIGHTNESS}",
                s.brightness
            ));
        }
        if (s.red, s.green, s.blue) != (LED_DEFAULT_RED, LED_DEFAULT_GREEN, LED_DEFAULT_BLUE) {
            errors.push(format!(
                "an unnamed colour read back {},{},{}, expected {LED_DEFAULT_RED},\
                 {LED_DEFAULT_GREEN},{LED_DEFAULT_BLUE}",
                s.red, s.green, s.blue
            ));
        }

        // What a request does name is what runs.
        let mut named = request(RGB, ON);
        named.brightness = 60;
        named.red = 0x10;
        named.green = 0x20;
        named.blue = 0x30;
        set(emu, &named)?;
        let s = state(emu, RGB)?;
        if (s.brightness, s.red, s.green, s.blue) != (60, 0x10, 0x20, 0x30) {
            errors.push(format!(
                "a named colour read back {} at {},{},{}",
                s.brightness, s.red, s.green, s.blue
            ));
        }

        // The two modes only the RGB LED has, and their own periods.
        let periods: &[(u8, u16, u16, &str)] = &[
            (
                CYCLE,
                LED_CYCLE_DEFAULT_PERIOD_MS,
                LED_CYCLE_MIN_PERIOD_MS,
                "cycle",
            ),
            (
                BREATHE,
                LED_BREATHE_DEFAULT_PERIOD_MS,
                LED_BREATHE_MIN_PERIOD_MS,
                "breathe",
            ),
        ];
        for (mode, default_ms, min_ms, name) in periods {
            emu.led_reset();
            set(emu, &request(RGB, *mode))?;
            let got = state(emu, RGB)?.period_ms;
            if got != *default_ms {
                errors.push(format!(
                    "{name} with no period: {got} ms, expected {default_ms}"
                ));
            }

            let mut fastest = request(RGB, *mode);
            fastest.period_ms = *min_ms;
            if !emu.led_set(&fastest).is_ok() {
                errors.push(format!("{name} at its shortest period was refused"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    })
}

/// Verify that the status LED's live state is one thing, whichever call moved
/// it.
///
/// `ora_set_status_led`, `ora_led_set` and `ORA_METADATA_KEY_STATUS_LED_STATE`
/// are three entry points to the same LED, and the metadata key is the channel
/// the other plugin reads.  A firmware where one of them kept its own idea
/// would pass a test that only ever asked through one of them.
pub fn test_led_status_channel(emu: &Emulator) -> Result<(), String> {
    with_engine(emu, |emu| {
        if !present(emu, STATUS)? {
            return Err("board has no status LED".to_string());
        }

        let mut errors = Vec::new();

        // Moved through the older call, read through the newer one and through
        // the coordination channel.
        for on in [true, false, true] {
            emu.set_status_led(on);
            let mode = state(emu, STATUS)?.mode;
            let expected_mode = if on { ON } else { OFF };
            if mode != expected_mode {
                errors.push(format!(
                    "set_status_led({on}): led_get says mode {mode}, expected {expected_mode}"
                ));
            }
            if status_led_state(emu)? != on {
                errors.push(format!("set_status_led({on}): the channel says otherwise"));
            }
        }

        // Moved through the newer call, read through the coordination channel.
        for (mode, expected_lit, name) in [(ON, true, "on"), (OFF, false, "off")] {
            set(emu, &request(STATUS, mode))?;
            if status_led_state(emu)? != expected_lit {
                errors.push(format!(
                    "led_set({name}): the channel says {}",
                    !expected_lit
                ));
            }
        }

        // A mode that is neither on nor off still reports what the LED is doing
        // now, and a blink starts lit.
        let mut blink = request(STATUS, BLINK);
        blink.period_ms = ARMED_PERIOD_MS;
        set(emu, &blink)?;
        if !status_led_state(emu)? {
            errors.push("led_set(blink): the channel says the LED is dark".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    })
}

/// Verify that a held mode ends and gives the LED back to what it was doing,
/// and that a second hold arriving during the first does not move where it goes
/// back to.
pub fn test_led_hold_restores(emu: &Emulator) -> Result<(), String> {
    with_engine(emu, |emu| {
        if !present(emu, STATUS)? {
            return Err("board has no status LED".to_string());
        }

        const FIRST_HOLD_MS: u32 = 500;
        const SECOND_HOLD_MS: u32 = 600;
        const BETWEEN_MS: u32 = 100;

        let armed = arm(emu, STATUS)?;

        // A held mode takes the LED, and names a deadline for giving it back.
        let mut held = request(STATUS, ON);
        held.hold_ms = FIRST_HOLD_MS;
        set(emu, &held)?;

        if state(emu, STATUS)?.mode != ON {
            return Err("a held mode did not take the LED".to_string());
        }
        let deadline = emu
            .led_next_deadline_ms()
            .ok_or("a held mode named no deadline to end at")?;
        if deadline != BASE_MS + FIRST_HOLD_MS {
            return Err(format!(
                "a {FIRST_HOLD_MS} ms hold from {BASE_MS} ms ends at {deadline} ms"
            ));
        }

        // A second held request during the first leaves alone what the LED goes
        // back to, so it returns to what it was doing before either of them.
        emu.set_timer_us(ms_to_us(BASE_MS + BETWEEN_MS));
        let mut second = request(STATUS, OFF);
        second.hold_ms = SECOND_HOLD_MS;
        set(emu, &second)?;

        // Past both, and the frame the timer interrupt would have run.
        emu.set_timer_us(ms_to_us(BASE_MS + BETWEEN_MS + SECOND_HOLD_MS + 1));
        emu.led_frame();

        let after = state(emu, STATUS)?;
        if after != armed {
            return Err(format!(
                "after two holds the LED came back to mode {} at {} ms, expected mode {} at {} ms",
                after.mode, after.period_ms, armed.mode, armed.period_ms
            ));
        }

        // And having given the LED back, the engine has nothing left owing but
        // the blink it returned to.
        let deadline = emu
            .led_next_deadline_ms()
            .ok_or("the blink it came back to is not being driven")?;
        println!("  came back to blink, next frame at {deadline} ms");

        Ok(())
    })
}

/// Verify that a hold whose expiry falls on the millisecond counter's wrap
/// still ends.
///
/// The expiry is stored as a millisecond, and zero is that field's "no hold is
/// running".  A hold arriving at the one millisecond where `now + hold_ms` is
/// exactly zero would be recorded as no hold at all, and the LED would sit in
/// the held mode for good.  One millisecond in 2^32 is not reachable by waiting
/// - the counter wraps every 49.7 days - so it is reached by placing the clock.
pub fn test_led_hold_at_the_wrap(emu: &Emulator) -> Result<(), String> {
    with_engine(emu, |emu| {
        if !present(emu, STATUS)? {
            return Err("board has no status LED".to_string());
        }

        const HOLD_MS: u32 = 1_000;

        // The millisecond at which this hold's expiry lands on the sentinel.
        let now_ms = 0u32.wrapping_sub(HOLD_MS);
        emu.set_timer_us(ms_to_us(now_ms));

        let armed = arm(emu, STATUS)?;

        let mut held = request(STATUS, ON);
        held.hold_ms = HOLD_MS;
        set(emu, &held)?;

        // A held mode that does not repeat has nothing else to schedule, so the
        // deadline the engine names here is the hold's own.
        let deadline = emu.led_next_deadline_ms().ok_or(
            "a hold expiring on the counter's wrap was recorded as no hold at all, so the \
             LED would never come back",
        )?;

        // Past it, and the frame the timer interrupt would have run.
        emu.set_timer_us(ms_to_us(now_ms) + ms_to_us(HOLD_MS) + ms_to_us(2));
        emu.led_frame();

        let after = state(emu, STATUS)?;
        if after != armed {
            return Err(format!(
                "after a hold across the wrap the LED is in mode {}, expected mode {}",
                after.mode, armed.mode
            ));
        }
        println!("  hold from {now_ms} ms ended at {deadline} ms");

        Ok(())
    })
}
