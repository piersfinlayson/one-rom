// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM's picoboot extension: the commands this plugin adds under its own
//! magic.
//!
//! picobootx's own conformance suite covers the protocol — framing, the stall
//! and unstall sequence, ZLPs, the transfer state machine, command status.  None
//! of that is repeated here.  What this suite asserts is the part picobootx
//! cannot: the plugin's `dispatch` and `fill`, the arguments they accept and
//! refuse, and the address ranges it claims.
//!
//! A scenario stands where the core stands, calling the handlers through the
//! table the plugin registered with `picoboot_init`.  Taking the table from the
//! plugin rather than building one means the registration is under test too.
//!
//! What this cannot see is the core's side of the contract: if picobootx
//! changed whether it masks the direction bit before calling dispatch, these
//! scenarios would stay green while the device changed.  A signature change
//! still breaks the build, and picobootx's own suite covers the core's calling
//! convention against a sample implementation.

use crate::device::Device;
use crate::{Ctx, Scenario};
use onerom_plugin_tester::run::Outcome;

// Status codes, from picobootx.h.
pub const OK: i32 = 0;
pub const UNKNOWN_CMD: i32 = 1;
pub const INVALID_CMD_LENGTH: i32 = 2;
pub const INVALID_TRANSFER_LEN: i32 = 3;
pub const NOT_PERMITTED: i32 = 10;
pub const INVALID_ARG: i32 = 11;
pub const PRECONDITION_NOT_MET: i32 = 13;
pub const NOT_FOUND: i32 = 16;

// Command identifiers, from usb_custom_pbx.h.  The two that return data travel
// with picoboot's direction bit set, which is how a host sends them.
const DIR_IN: u8 = 0x80;
pub const CMD_SET_LED: u8 = 0x01;
const CMD_GET_CAPS: u8 = 0x02 | DIR_IN;
pub const CMD_GPIO_SET: u8 = 0x03;
const CMD_GPIO_QUERY: u8 = 0x04 | DIR_IN;

// The capabilities response.
const CAPS_LEN: u32 = 32;
const EXT_MAJOR: u8 = 1;
const EXT_MINOR: u8 = 0;
const FEAT_GPIO_SET: u32 = 1 << 0;
const FEAT_GPIO_QUERY: u32 = 1 << 1;
const FEAT_GPIO_HOLD: u32 = 1 << 2;
const FEAT_LED_ARGS: u32 = 1 << 3;
const MAX_HOLD_MS: u32 = 60000;

// A GPIO query entry, and the longest transfer a One ROM command may ask for.
const ENTRY_LEN: u32 = 4;
const MAX_TRANSFER_LEN: u32 = 256;

// GPIO states, mirroring ora_gpio_state_t.
pub const GPIO_LOW: u8 = 0;
pub const GPIO_HIGH: u8 = 1;
pub const GPIO_INPUT: u8 = 2;
pub const GPIO_FLAG_FORCE: u8 = 1 << 0;

/// The magic the plugin registers its commands under: "ONER", least significant
/// byte first, as usb_custom_pbx.h builds it.
const ONEROM_MAGIC: u32 = u32::from_le_bytes(*b"ONER");

/// No arguments.
const NO_ARGS: [u8; 16] = [0u8; 16];

/// A GPIO_SET argument block.
pub fn gpio_set_args(
    gpio: u8,
    state: u8,
    after_state: u8,
    flags: u8,
    duration_ms: u32,
) -> [u8; 16] {
    let mut args = [0u8; 16];
    args[0] = gpio;
    args[1] = state;
    args[2] = after_state;
    args[3] = flags;
    args[4..8].copy_from_slice(&duration_ms.to_le_bytes());
    args
}

/// A GPIO_QUERY argument block.
fn gpio_query_args(first_gpio: u8, count: u8) -> [u8; 16] {
    let mut args = [0u8; 16];
    args[0] = first_gpio;
    args[1] = count;
    args
}

/// The whole capabilities response, as the host would read it.
fn caps(dev: &mut Device) -> Result<Vec<u8>, String> {
    let st = dev.dispatch(CMD_GET_CAPS, CAPS_LEN, &NO_ARGS);
    if st != OK {
        return Err(format!("GET_CAPS was refused with status {st}"));
    }
    dev.fill_all(CAPS_LEN, CAPS_LEN)
}

fn u16_at(buf: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([buf[at], buf[at + 1]])
}

fn u32_at(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

// ---------------------------------------------------------------------------

/// The plugin registers both handlers, under its own magic.
///
/// The magic is what separates these commands from picoboot's own, and a
/// device-to-host command is stalled outright if `fill` is absent — so what was
/// registered is as much a part of the contract as what the handlers do.
fn the_registration_claims_both_handlers(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let _ = dev;
    // SAFETY: reads two values the shim recorded when the plugin registered.
    let (magic, handlers) = unsafe {
        (
            crate::ffi::usb_host_test_custom_magic(),
            crate::ffi::usb_host_test_custom_handlers(),
        )
    };
    if magic != ONEROM_MAGIC {
        return Err(format!(
            "registered under magic {magic:#010x}, not One ROM's {ONEROM_MAGIC:#010x}"
        ));
    }
    if handlers & 1 == 0 {
        return Err("no dispatch handler was registered".to_string());
    }
    if handlers & 2 == 0 {
        return Err("no fill handler was registered, so a data-IN command would stall".to_string());
    }
    Ok(Outcome::Pass)
}

/// The direction bit is the host's statement of intent, not part of the
/// command.
///
/// It has to come off before matching.  Without that, GET_CAPS and GPIO_QUERY
/// arrive as 0x82 and 0x84 and fall through to the unknown arm — so the
/// discriminating assertion is that the same identifier is understood with the
/// bit set and refused when it names nothing.
fn the_direction_bit_is_not_part_of_the_command(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let with_bit = dev.dispatch(CMD_GET_CAPS, CAPS_LEN, &NO_ARGS);
    if with_bit != OK {
        return Err(format!(
            "GET_CAPS with the direction bit set was refused with status {with_bit}"
        ));
    }

    // A command that does not exist, with the same bit set: still unknown, so
    // the bit is not making everything match.
    let absent = dev.dispatch(0x7F | DIR_IN, CAPS_LEN, &NO_ARGS);
    if absent != UNKNOWN_CMD {
        return Err(format!(
            "an unknown command with the direction bit set answered {absent}, not UNKNOWN_CMD"
        ));
    }

    Ok(Outcome::Pass)
}

/// Every One ROM command carries all sixteen argument bytes.
fn a_short_argument_block_is_refused(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let st = dev.dispatch_sized(CMD_SET_LED, 8, &NO_ARGS);
    if st != INVALID_CMD_LENGTH {
        return Err(format!(
            "a command with 8 argument bytes answered {st}, not INVALID_CMD_LENGTH"
        ));
    }

    let full = dev.dispatch_sized(CMD_SET_LED, 16, &NO_ARGS);
    if full != OK {
        return Err(format!(
            "the same command with all 16 argument bytes answered {full}, not OK"
        ));
    }

    Ok(Outcome::Pass)
}

/// A command this device does not have is refused as unknown.
///
/// The status matters as much as the refusal: a host reads UNKNOWN_CMD as "this
/// device is too old", which is what lets it fall back.
fn an_unknown_command_is_refused(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let st = dev.dispatch(0x7F, 0, &NO_ARGS);
    if st != UNKNOWN_CMD {
        return Err(format!("an unknown command answered {st}, not UNKNOWN_CMD"));
    }
    Ok(Outcome::Pass)
}

/// The capabilities say which extension this device speaks.
fn the_capabilities_report_the_extension(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    let caps = caps(dev)?;
    if caps.len() != CAPS_LEN as usize {
        return Err(format!(
            "the response was {} bytes, not the {CAPS_LEN} asked for",
            caps.len()
        ));
    }

    let struct_len = u16_at(&caps, 0);
    if u32::from(struct_len) != CAPS_LEN {
        return Err(format!(
            "struct_len is {struct_len}, so the host would read {struct_len} meaningful bytes"
        ));
    }
    if caps[2] != EXT_MAJOR || caps[3] != EXT_MINOR {
        return Err(format!(
            "the extension version is {}.{}, not {EXT_MAJOR}.{EXT_MINOR}",
            caps[2], caps[3]
        ));
    }

    let num_gpios = caps[8];
    if num_gpios != ctx.num_gpios {
        return Err(format!(
            "the device reports {num_gpios} GPIOs, not the {} this variant has",
            ctx.num_gpios
        ));
    }

    Ok(Outcome::Pass)
}

/// A device that offers bounded holds says how long one may be.
///
/// Zero would mean no opinion, which this plugin has never had — it always
/// enforces the maximum — so reporting it alongside the feature bit is what
/// lets a host refuse a longer hold before it reaches the device.
fn the_capabilities_bound_a_hold(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let caps = caps(dev)?;
    let features = u32_at(&caps, 4);
    let max_hold_ms = u32_at(&caps, 12);

    let want = FEAT_GPIO_SET | FEAT_GPIO_QUERY | FEAT_GPIO_HOLD;
    if features & want != want {
        return Err(format!(
            "features {features:#x} does not offer set, query and hold together"
        ));
    }
    if max_hold_ms != MAX_HOLD_MS {
        return Err(format!(
            "a device offering holds reports a maximum of {max_hold_ms}ms, not {MAX_HOLD_MS}ms"
        ));
    }

    Ok(Outcome::Pass)
}

/// SET_LED's extended arguments are offered when the firmware has an engine to
/// honour them.
///
/// The bit says the colour, brightness, period and hold reach the engine.  It
/// does not say this board has an RGB LED - a board without one answers
/// NOT_FOUND, and a host can only tell that apart from "too old to be asked"
/// because the bit was set.  So what the bit must follow is whether the
/// firmware resolves `ORA_ID_LED_SET`, which is asked of the firmware here
/// rather than assumed — and both ways round, since a bit set unconditionally
/// would tell a host a device predating the engine could be driven.
fn the_capabilities_offer_the_led_args(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let has_engine = dev
        .emulator()
        .plugin_lookup_valid(onerom_fw_emulator::ffi::api_id_t_ORA_ID_LED_SET);

    let caps = caps(dev)?;
    let offered = u32_at(&caps, 4) & FEAT_LED_ARGS != 0;

    match (has_engine, offered) {
        (true, false) => Err(
            "this firmware has an LED engine, but the device does not offer its arguments"
                .to_string(),
        ),
        (false, true) => Err(
            "the device offers SET_LED's arguments, but this firmware has no engine to reach"
                .to_string(),
        ),
        _ => Ok(Outcome::Pass),
    }
}

/// The capabilities are meant to grow, so a host asking for a different length
/// gets something it can make sense of.
///
/// A newer host asking for more gets zero padding, an older one asking for less
/// gets a prefix, and struct_len is what tells each how much was meaningful.
fn the_capabilities_pad_and_truncate(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let whole = caps(dev)?;

    let longer = CAPS_LEN + 16;
    let st = dev.dispatch(CMD_GET_CAPS, longer, &NO_ARGS);
    if st != OK {
        return Err(format!("a longer request was refused with status {st}"));
    }
    let padded = dev.fill_all(longer, longer)?;
    if padded.len() != longer as usize {
        return Err(format!(
            "a request for {longer} bytes produced {}",
            padded.len()
        ));
    }
    if padded[..CAPS_LEN as usize] != whole[..] {
        return Err("the padded response does not start with the capabilities".to_string());
    }
    if padded[CAPS_LEN as usize..].iter().any(|&b| b != 0) {
        return Err(format!(
            "the bytes past the structure are not zero: {:?}",
            &padded[CAPS_LEN as usize..]
        ));
    }

    let shorter = 8;
    let st = dev.dispatch(CMD_GET_CAPS, shorter, &NO_ARGS);
    if st != OK {
        return Err(format!("a shorter request was refused with status {st}"));
    }
    let prefix = dev.fill_all(shorter, shorter)?;
    if prefix[..] != whole[..shorter as usize] {
        return Err("the shorter response is not a prefix of the capabilities".to_string());
    }

    Ok(Outcome::Pass)
}

/// A capabilities request must ask for something, and not more than a command
/// may carry.
fn the_capabilities_bound_their_transfer(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let empty = dev.dispatch(CMD_GET_CAPS, 0, &NO_ARGS);
    if empty != INVALID_TRANSFER_LEN {
        return Err(format!(
            "a request for no bytes answered {empty}, not INVALID_TRANSFER_LEN"
        ));
    }

    let over = dev.dispatch(CMD_GET_CAPS, MAX_TRANSFER_LEN + 1, &NO_ARGS);
    if over != INVALID_TRANSFER_LEN {
        return Err(format!(
            "a request for {} bytes answered {over}, not INVALID_TRANSFER_LEN",
            MAX_TRANSFER_LEN + 1
        ));
    }

    let at_limit = dev.dispatch(CMD_GET_CAPS, MAX_TRANSFER_LEN, &NO_ARGS);
    if at_limit != OK {
        return Err(format!(
            "a request for exactly {MAX_TRANSFER_LEN} bytes answered {at_limit}, not OK"
        ));
    }

    Ok(Outcome::Pass)
}

/// A command with no data phase must not claim one.
fn a_command_without_data_refuses_a_transfer(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let led = dev.dispatch(CMD_SET_LED, 4, &NO_ARGS);
    if led != INVALID_CMD_LENGTH {
        return Err(format!(
            "SET_LED with a data phase answered {led}, not INVALID_CMD_LENGTH"
        ));
    }

    let args = gpio_set_args(0, GPIO_INPUT, GPIO_INPUT, GPIO_FLAG_FORCE, 0);
    let gpio = dev.dispatch(CMD_GPIO_SET, 4, &args);
    if gpio != INVALID_CMD_LENGTH {
        return Err(format!(
            "GPIO_SET with a data phase answered {gpio}, not INVALID_CMD_LENGTH"
        ));
    }

    Ok(Outcome::Pass)
}

/// Driving a GPIO takes effect when the command is dispatched, not later.
///
/// Deferring it to the task loop would leave a refusal with nowhere to go: the
/// host reads the outcome from the command's status, which has already been
/// answered by then.
fn a_gpio_set_is_applied_and_answered(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    // The highest GPIO the device has, which serving is least likely to want.
    let gpio = ctx.num_gpios - 1;

    let args = gpio_set_args(gpio, GPIO_HIGH, GPIO_INPUT, GPIO_FLAG_FORCE, 0);
    let st = dev.dispatch(CMD_GPIO_SET, 0, &args);
    if st != OK {
        return Err(format!(
            "driving GPIO {gpio} high was refused with status {st}"
        ));
    }

    // Read back through the firmware, with no pass of the plugin's loop in
    // between: the pin must already be driven.
    let (result, info) = dev.emulator().gpio_query(gpio);
    if result != onerom_fw_emulator::OraResult::Ok {
        return Err(format!("could not query GPIO {gpio}: {result:?}"));
    }
    if info.is_output == 0 || info.level == 0 {
        return Err(format!(
            "GPIO {gpio} reads back is_output {} level {} after being told to drive high",
            info.is_output, info.level
        ));
    }

    // And the opposite, so this is not passing on a pin that was already high.
    let args = gpio_set_args(gpio, GPIO_LOW, GPIO_INPUT, GPIO_FLAG_FORCE, 0);
    if dev.dispatch(CMD_GPIO_SET, 0, &args) != OK {
        return Err(format!("driving GPIO {gpio} low was refused"));
    }
    let (_, info) = dev.emulator().gpio_query(gpio);
    if info.level != 0 {
        return Err(format!(
            "GPIO {gpio} still reads high after being told to drive low"
        ));
    }

    Ok(Outcome::Pass)
}

/// A pin One ROM is using is refused unless the host insists.
///
/// The status is what carries the refusal, and NOT_PERMITTED is "understood and
/// refused" — which a host must not confuse with UNKNOWN_CMD, the answer that
/// means the device is too old.
fn a_gpio_in_use_is_refused(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    // A pin serving is using.  Found by asking the firmware rather than assumed,
    // since which pins those are is the board's business.
    let mut in_use = None;
    for gpio in 0..ctx.num_gpios {
        let (result, info) = dev.emulator().gpio_query(gpio);
        if result == onerom_fw_emulator::OraResult::Ok && info.gpio_use != 0 {
            in_use = Some(gpio);
            break;
        }
    }
    let Some(gpio) = in_use else {
        return Ok(Outcome::Skip(
            "the firmware reports no GPIO in use, so there is no pin to be refused".to_string(),
        ));
    };

    let args = gpio_set_args(gpio, GPIO_HIGH, GPIO_INPUT, 0, 0);
    let refused = dev.dispatch(CMD_GPIO_SET, 0, &args);
    if refused != NOT_PERMITTED {
        return Err(format!(
            "driving GPIO {gpio}, which One ROM is using, answered {refused}, not NOT_PERMITTED"
        ));
    }

    // The same pin, forced: the refusal is about the pin's use, not the pin.
    let args = gpio_set_args(gpio, GPIO_HIGH, GPIO_INPUT, GPIO_FLAG_FORCE, 0);
    let forced = dev.dispatch(CMD_GPIO_SET, 0, &args);
    if forced != OK {
        return Err(format!(
            "driving GPIO {gpio} with force answered {forced}, not OK"
        ));
    }

    Ok(Outcome::Pass)
}

/// A hold longer than the device offers is refused.
fn a_hold_beyond_the_maximum_is_refused(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    let gpio = ctx.num_gpios - 1;

    let args = gpio_set_args(
        gpio,
        GPIO_HIGH,
        GPIO_INPUT,
        GPIO_FLAG_FORCE,
        MAX_HOLD_MS + 1,
    );
    let over = dev.dispatch(CMD_GPIO_SET, 0, &args);
    if over != INVALID_ARG {
        return Err(format!(
            "a hold of {}ms answered {over}, not INVALID_ARG",
            MAX_HOLD_MS + 1
        ));
    }

    let args = gpio_set_args(gpio, GPIO_HIGH, GPIO_INPUT, GPIO_FLAG_FORCE, MAX_HOLD_MS);
    let at_limit = dev.dispatch(CMD_GPIO_SET, 0, &args);
    if at_limit != OK {
        return Err(format!(
            "a hold of exactly {MAX_HOLD_MS}ms answered {at_limit}, not OK"
        ));
    }

    Ok(Outcome::Pass)
}

/// A bounded hold releases the pin when it expires, and not before.
fn a_bounded_hold_releases_the_pin(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    let gpio = ctx.num_gpios - 1;
    let hold_ms = 100u32;

    let args = gpio_set_args(gpio, GPIO_HIGH, GPIO_INPUT, GPIO_FLAG_FORCE, hold_ms);
    if dev.dispatch(CMD_GPIO_SET, 0, &args) != OK {
        return Err(format!("a {hold_ms}ms hold on GPIO {gpio} was refused"));
    }

    // One millisecond short: still held, so the release is not simply happening
    // on the next pass whatever the deadline says.
    dev.advance_ms(u64::from(hold_ms) - 1);
    dev.step()?;
    let (_, info) = dev.emulator().gpio_query(gpio);
    if info.is_output == 0 || info.level == 0 {
        return Err(format!(
            "GPIO {gpio} was released {}ms into a {hold_ms}ms hold",
            hold_ms - 1
        ));
    }

    dev.advance_ms(1);
    dev.step()?;
    let (_, info) = dev.emulator().gpio_query(gpio);
    if info.is_output != 0 {
        return Err(format!(
            "GPIO {gpio} is still driven after its {hold_ms}ms hold expired"
        ));
    }

    Ok(Outcome::Pass)
}

/// A query names a run of GPIOs the device actually has.
///
/// The device is the authority on how many it has, and the host sizes its run
/// from the same number this plugin reported.
fn a_query_is_bounded_by_the_device(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    let n = ctx.num_gpios;

    let args = gpio_query_args(0, 0);
    let none = dev.dispatch(CMD_GPIO_QUERY, 0, &args);
    if none != INVALID_ARG {
        return Err(format!(
            "a run of no GPIOs answered {none}, not INVALID_ARG"
        ));
    }

    let args = gpio_query_args(n - 1, 2);
    let over = dev.dispatch(CMD_GPIO_QUERY, 2 * ENTRY_LEN, &args);
    if over != INVALID_ARG {
        return Err(format!(
            "a run ending past GPIO {n} answered {over}, not INVALID_ARG"
        ));
    }

    // The whole device, which is exactly in range.
    let args = gpio_query_args(0, n);
    let whole = dev.dispatch(CMD_GPIO_QUERY, u32::from(n) * ENTRY_LEN, &args);
    if whole != OK {
        return Err(format!("a run of every GPIO answered {whole}, not OK"));
    }

    Ok(Outcome::Pass)
}

/// A query and its response must agree on the byte count exactly.
///
/// Unlike the capabilities, this response has no growth story of its own: the
/// entry size is fixed and the run length is the host's.
fn a_query_needs_an_exact_transfer_length(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let args = gpio_query_args(0, 4);

    let short = dev.dispatch(CMD_GPIO_QUERY, 3 * ENTRY_LEN, &args);
    if short != INVALID_TRANSFER_LEN {
        return Err(format!(
            "four entries into three entries' room answered {short}, not INVALID_TRANSFER_LEN"
        ));
    }

    let exact = dev.dispatch(CMD_GPIO_QUERY, 4 * ENTRY_LEN, &args);
    if exact != OK {
        return Err(format!(
            "four entries into four entries' room answered {exact}"
        ));
    }

    Ok(Outcome::Pass)
}

/// Entries are produced whole or not at all.
///
/// A call with room for less than one entry produces nothing and asks to be
/// called again, which is what the fill contract's zero-bytes-not-done case is
/// for.  Splitting an entry across calls would hand the host a run it cannot
/// parse.
fn a_query_produces_whole_entries(dev: &mut Device, ctx: &Ctx) -> Result<Outcome, String> {
    let count = 3u8;
    let args = gpio_query_args(0, count);
    let total = u32::from(count) * ENTRY_LEN;
    if dev.dispatch(CMD_GPIO_QUERY, total, &args) != OK {
        return Err("a three-entry query was refused".to_string());
    }

    let (st, part, done) = dev.fill(ENTRY_LEN - 1);
    if st != OK {
        return Err(format!(
            "a fill with room for part of an entry answered {st}"
        ));
    }
    if !part.is_empty() {
        return Err(format!(
            "a fill with room for {} of {ENTRY_LEN} bytes produced {} bytes",
            ENTRY_LEN - 1,
            part.len()
        ));
    }
    if done {
        return Err("a fill that produced nothing reported the transfer complete".to_string());
    }

    // Asked properly, the whole run arrives, one entry at a time.
    let entries = dev.fill_all(ENTRY_LEN, total)?;
    if entries.len() != total as usize {
        return Err(format!(
            "the run is {} bytes, not the {total} asked for",
            entries.len()
        ));
    }

    // Every entry's use must be one the firmware agrees with, so this is not
    // passing on a buffer of zeros.
    for (index, entry) in entries.chunks(ENTRY_LEN as usize).enumerate() {
        let (result, info) = dev.emulator().gpio_query(index as u8);
        if result != onerom_fw_emulator::OraResult::Ok {
            return Err(format!("could not query GPIO {index}: {result:?}"));
        }
        if entry[0] != info.gpio_use {
            return Err(format!(
                "GPIO {index} is reported as use {} but the firmware says {}",
                entry[0], info.gpio_use
            ));
        }
    }
    let _ = ctx;

    Ok(Outcome::Pass)
}

/// The RAM slot the device is serving.
fn active_slot(dev: &Device) -> Result<u8, String> {
    match dev.emulator().get_active_ram_slot() {
        (onerom_fw_emulator::OraResult::Ok, Some(slot)) => Ok(slot),
        (r, _) => Err(format!("could not find the active RAM slot: {r:?}")),
    }
}

/// How big that slot is, which is what bounds the logical ROM range.
fn active_slot_size(dev: &Device) -> Result<u32, String> {
    let slot = active_slot(dev)?;
    match dev.emulator().get_ram_slot_info(slot) {
        (onerom_fw_emulator::OraResult::Ok, Some(info)) => Ok(info.size),
        (r, _) => Err(format!("could not size RAM slot {slot}: {r:?}")),
    }
}

/// The logical ROM range reads the image the device is serving.
fn the_logical_rom_range_is_readable(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    const BASE: u32 = 0x9000_0000;

    let (st, bytes) = dev.pb_read(BASE, 16);
    if st != OK {
        return Err(format!("reading the start of the ROM answered {st}"));
    }

    // Against the slot the firmware is serving, so this asserts the mapping
    // rather than that sixteen bytes came back.
    let slot = active_slot(dev)?;
    let mut want = [0u8; 16];
    let r = dev.emulator().read_ram_rom_slot(slot, 0, &mut want);
    if r != onerom_fw_emulator::OraResult::Ok {
        return Err(format!("could not read the served slot: {r:?}"));
    }
    if bytes != want {
        return Err(format!(
            "the range reported {bytes:02x?} where the served slot holds {want:02x?}"
        ));
    }

    Ok(Outcome::Pass)
}

/// A read past the end of the served image is refused.
fn the_logical_rom_range_is_bounded(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    const BASE: u32 = 0x9000_0000;

    let size = active_slot_size(dev)?;

    let (last, _) = dev.pb_read(BASE + size - 1, 1);
    if last != OK {
        return Err(format!(
            "the last byte of the image answered {last}, not OK"
        ));
    }

    let (past, _) = dev.pb_read(BASE + size, 1);
    if past == OK {
        return Err("a read one byte past the end of the image was allowed".to_string());
    }

    Ok(Outcome::Pass)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "picobootx.the_registration_claims_both_handlers",
        about: "the plugin registers dispatch and fill under One ROM's magic",
        run: the_registration_claims_both_handlers,
        before_start: None,
    },
    Scenario {
        name: "picobootx.the_direction_bit_is_not_part_of_the_command",
        about: "a command identifier is matched with the direction bit removed",
        run: the_direction_bit_is_not_part_of_the_command,
        before_start: None,
    },
    Scenario {
        name: "picobootx.a_short_argument_block_is_refused",
        about: "every One ROM command carries all sixteen argument bytes",
        run: a_short_argument_block_is_refused,
        before_start: None,
    },
    Scenario {
        name: "picobootx.an_unknown_command_is_refused",
        about: "a command this device does not have answers UNKNOWN_CMD",
        run: an_unknown_command_is_refused,
        before_start: None,
    },
    Scenario {
        name: "picobootx.the_capabilities_report_the_extension",
        about: "the capabilities carry the extension version and the device's GPIO count",
        run: the_capabilities_report_the_extension,
        before_start: None,
    },
    Scenario {
        name: "picobootx.the_capabilities_bound_a_hold",
        about: "a device offering bounded holds reports how long one may be",
        run: the_capabilities_bound_a_hold,
        before_start: None,
    },
    Scenario {
        name: "picobootx.the_capabilities_offer_the_led_args",
        about: "SET_LED's arguments are offered exactly when the firmware has an engine to honour them",
        run: the_capabilities_offer_the_led_args,
        before_start: None,
    },
    Scenario {
        name: "picobootx.the_capabilities_pad_and_truncate",
        about: "a longer request is zero padded and a shorter one is a prefix",
        run: the_capabilities_pad_and_truncate,
        before_start: None,
    },
    Scenario {
        name: "picobootx.the_capabilities_bound_their_transfer",
        about: "a capabilities request asks for something, and no more than a command carries",
        run: the_capabilities_bound_their_transfer,
        before_start: None,
    },
    Scenario {
        name: "picobootx.a_command_without_data_refuses_a_transfer",
        about: "a command with no data phase refuses one",
        run: a_command_without_data_refuses_a_transfer,
        before_start: None,
    },
    Scenario {
        name: "picobootx.a_gpio_set_is_applied_and_answered",
        about: "driving a GPIO takes effect at dispatch, so a refusal can reach the host",
        run: a_gpio_set_is_applied_and_answered,
        before_start: None,
    },
    Scenario {
        name: "picobootx.a_gpio_in_use_is_refused",
        about: "a pin One ROM is using is refused unless the host forces it",
        run: a_gpio_in_use_is_refused,
        before_start: None,
    },
    Scenario {
        name: "picobootx.a_hold_beyond_the_maximum_is_refused",
        about: "a hold longer than the device offers is refused",
        run: a_hold_beyond_the_maximum_is_refused,
        before_start: None,
    },
    Scenario {
        name: "picobootx.a_bounded_hold_releases_the_pin",
        about: "a bounded hold releases the pin when it expires, and not before",
        run: a_bounded_hold_releases_the_pin,
        before_start: None,
    },
    Scenario {
        name: "picobootx.a_query_is_bounded_by_the_device",
        about: "a GPIO query names a run the device actually has",
        run: a_query_is_bounded_by_the_device,
        before_start: None,
    },
    Scenario {
        name: "picobootx.a_query_needs_an_exact_transfer_length",
        about: "a GPIO query and its response agree on the byte count exactly",
        run: a_query_needs_an_exact_transfer_length,
        before_start: None,
    },
    Scenario {
        name: "picobootx.a_query_produces_whole_entries",
        about: "entries are produced whole, and match what the firmware reports",
        run: a_query_produces_whole_entries,
        before_start: None,
    },
    Scenario {
        name: "picobootx.the_logical_rom_range_is_readable",
        about: "the logical ROM range reads the image being served",
        run: the_logical_rom_range_is_readable,
        before_start: None,
    },
    Scenario {
        name: "picobootx.the_logical_rom_range_is_bounded",
        about: "a read past the end of the served image is refused",
        run: the_logical_rom_range_is_bounded,
        before_start: None,
    },
];
