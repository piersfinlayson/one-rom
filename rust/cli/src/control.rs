// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::args::control::GpioState;
use crate::{
    args,
    utils::{
        active_chip_type, check_device, check_device_running, check_fire_board_optional,
        check_live_read_write, resolve_board_optional,
    },
};
use onerom_cli::device::{Device, select_device};
use onerom_cli::gpio;
use onerom_cli::hint;
use onerom_cli::picobootx::{ONEROM_FEAT_GPIO_QUERY, ONEROM_GPIO_FLAG_FORCE};
use onerom_cli::pin::{Pin, ResolvedPin};
use onerom_cli::reset::{self, PinObjection};
use onerom_cli::usb::{
    Caps, FLASH_BASE, GpioSetArgs, GpioUse, LedSubCmd, RebootArgs, SetLedArgs, flash_erase,
    get_caps, gpio_query, gpio_set, read_memory, reboot, set_led, set_rgb, write_memory,
};
use onerom_cli::{Error, Options};
use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_config::mcu::PinTolerance;
use onerom_fw_parser::ParsedDevice;
use onerom_gen::FlashChips;
use std::io::Write;

/// Send one status LED request, reporting it when the CLI is verbose.
///
/// Every `control led` subcommand comes through here, so the device lookup and
/// what the user is told stay in one place. The capability check lives in
/// `set_led`, which pays for it only when the request carries a period or a
/// hold.
async fn led_request(
    options: &Options,
    args: &impl crate::args::CommandTrait,
    sub_cmd: LedSubCmd,
    period: Option<u16>,
    hold: Option<u32>,
    said: &str,
) -> Result<(), Error> {
    check_device(options, args, true)?;
    let device = options.device.as_ref().unwrap();
    let mut led_args = SetLedArgs::status(sub_cmd);
    led_args.period_ms = period.unwrap_or(0);
    led_args.hold_ms = hold.unwrap_or(0);
    set_led(device, led_args).await?;
    if options.verbose {
        println!("{said}");
    }
    Ok(())
}

pub async fn cmd_led_on(
    options: &Options,
    args: &args::control::ControlLedOnArgs,
) -> Result<(), Error> {
    led_request(options, args, LedSubCmd::On, None, args.hold, "LED on").await
}

pub async fn cmd_led_off(
    options: &Options,
    args: &args::control::ControlLedOffArgs,
) -> Result<(), Error> {
    led_request(options, args, LedSubCmd::Off, None, args.hold, "LED off").await
}

pub async fn cmd_led_beacon(
    options: &Options,
    args: &args::control::ControlLedBeaconArgs,
) -> Result<(), Error> {
    led_request(
        options,
        args,
        LedSubCmd::Beacon,
        args.period,
        args.hold,
        "LED beacon started",
    )
    .await
}

pub async fn cmd_led_flame(
    options: &Options,
    args: &args::control::ControlLedFlameArgs,
) -> Result<(), Error> {
    led_request(
        options,
        args,
        LedSubCmd::Flame,
        args.period,
        args.hold,
        "LED flame started",
    )
    .await
}

pub async fn cmd_led_blink(
    options: &Options,
    args: &args::control::ControlLedBlinkArgs,
) -> Result<(), Error> {
    led_request(
        options,
        args,
        LedSubCmd::Blink,
        args.period,
        args.hold,
        "LED blinking",
    )
    .await
}

/// Send one RGB request, reporting it when the CLI is verbose.
///
/// Every `control rgb` subcommand comes through here, so the capability check,
/// the device lookup and what the user is told stay in one place.
async fn rgb_request(
    options: &Options,
    args: &impl crate::args::CommandTrait,
    led_args: SetLedArgs,
    said: &str,
) -> Result<(), Error> {
    check_device(options, args, true)?;
    let device = options.device.as_ref().unwrap();
    let caps = get_caps(device).await?;
    set_rgb(device, &caps, led_args).await?;
    if options.verbose {
        println!("{said}");
    }
    Ok(())
}

pub async fn cmd_rgb_on(
    options: &Options,
    args: &args::control::ControlRgbOnArgs,
) -> Result<(), Error> {
    let (red, green, blue) = args.colour.rgb();
    let mut led_args = SetLedArgs::rgb(LedSubCmd::On, red, green, blue);
    led_args.brightness = args.brightness.unwrap_or(0);
    led_args.hold_ms = args.hold.unwrap_or(0);
    rgb_request(options, args, led_args, "RGB LED on").await
}

pub async fn cmd_rgb_off(
    options: &Options,
    args: &args::control::ControlRgbOffArgs,
) -> Result<(), Error> {
    let mut led_args = SetLedArgs::rgb(LedSubCmd::Off, 0, 0, 0);
    led_args.hold_ms = args.hold.unwrap_or(0);
    rgb_request(options, args, led_args, "RGB LED off").await
}

pub async fn cmd_rgb_beacon(
    options: &Options,
    args: &args::control::ControlRgbBeaconArgs,
) -> Result<(), Error> {
    let (red, green, blue) = args.colour.rgb();
    let mut led_args = SetLedArgs::rgb(LedSubCmd::Beacon, red, green, blue);
    led_args.brightness = args.brightness.unwrap_or(0);
    led_args.period_ms = args.period.unwrap_or(0);
    led_args.hold_ms = args.hold.unwrap_or(0);
    rgb_request(options, args, led_args, "RGB LED beacon started").await
}

pub async fn cmd_rgb_flame(
    options: &Options,
    args: &args::control::ControlRgbFlameArgs,
) -> Result<(), Error> {
    let (red, green, blue) = args.colour.rgb();
    let mut led_args = SetLedArgs::rgb(LedSubCmd::Flame, red, green, blue);
    led_args.brightness = args.brightness.unwrap_or(0);
    led_args.period_ms = args.period.unwrap_or(0);
    led_args.hold_ms = args.hold.unwrap_or(0);
    rgb_request(options, args, led_args, "RGB LED flame started").await
}

pub async fn cmd_rgb_cycle(
    options: &Options,
    args: &args::control::ControlRgbCycleArgs,
) -> Result<(), Error> {
    // Cycle chooses its own colours, so the request carries none.
    let mut led_args = SetLedArgs::rgb(LedSubCmd::Cycle, 0, 0, 0);
    led_args.brightness = args.brightness.unwrap_or(0);
    led_args.period_ms = args.period.unwrap_or(0);
    led_args.hold_ms = args.hold.unwrap_or(0);
    rgb_request(options, args, led_args, "RGB LED cycling").await
}

pub async fn cmd_rgb_breathe(
    options: &Options,
    args: &args::control::ControlRgbBreatheArgs,
) -> Result<(), Error> {
    let (red, green, blue) = args.colour.rgb();
    let mut led_args = SetLedArgs::rgb(LedSubCmd::Breathe, red, green, blue);
    led_args.brightness = args.brightness.unwrap_or(0);
    led_args.period_ms = args.period.unwrap_or(0);
    led_args.hold_ms = args.hold.unwrap_or(0);
    rgb_request(options, args, led_args, "RGB LED breathing").await
}

pub async fn cmd_rgb_blink(
    options: &Options,
    args: &args::control::ControlRgbBlinkArgs,
) -> Result<(), Error> {
    let (red, green, blue) = args.colour.rgb();
    let mut led_args = SetLedArgs::rgb(LedSubCmd::Blink, red, green, blue);
    led_args.brightness = args.brightness.unwrap_or(0);
    led_args.period_ms = args.period.unwrap_or(0);
    led_args.hold_ms = args.hold.unwrap_or(0);
    rgb_request(options, args, led_args, "RGB LED blinking").await
}

pub async fn cmd_reboot(
    options: &Options,
    args: &args::control::ControlRebootArgs,
) -> Result<(), Error> {
    check_device(options, args, false)?;
    let device = options.device.as_ref().unwrap();
    assert!(
        !(args.stopped && args.running),
        "Cannot specify both --stopped and --running"
    );
    let reboot_args = args.into();
    if options.verbose {
        println!("Rebooting device:\n  {device}");
    } else {
        println!("Rebooting device...");
    }
    let serial = device.serial.clone();
    reboot(device, &reboot_args).await?;
    println!("Rebooted device into {} mode", reboot_args.mode);

    if options.verbose {
        // Rescan device to show new mode
        let selector = serial.as_deref();
        let device = select_device(selector, options).await?;
        println!("{device}");
    }

    Ok(())
}

/// Name a GPIO as richly as the local metadata allows, e.g.
/// `GPIO16 (A7)`, `GPIO9 (X1 pad)`, `GPIO29 (status LED, NeoPixel)`.
///
/// Every name here comes from the board and the chip being served. The device
/// reports only a coarse use category and deliberately never a role name, so if
/// the board could not be resolved this degrades to the bare `GPIO<N>`.
fn describe_gpio(board: Option<&Board>, chip: Option<ChipType>, gpio: u8) -> String {
    let mut notes: Vec<String> = Vec::new();
    if let Some(board) = board {
        if let Some(chip) = chip
            && let Some(function) = gpio::rom_function(board, chip, gpio)
        {
            notes.push(function);
        }
        // A GPIO can carry two peripherals - fire-24-f drives its status LED
        // and its NeoPixel from GPIO 29 - and a refusal that named only one
        // would understate what driving it disturbs.
        notes.extend(
            gpio::system_functions(board, gpio)
                .into_iter()
                .map(String::from),
        );
        if let Some(role) = gpio::header_role(board, gpio) {
            notes.push(format!("{role} pad"));
        }
    }

    if notes.is_empty() {
        format!("GPIO{gpio}")
    } else {
        format!("GPIO{gpio} ({})", notes.join(", "))
    }
}

/// What One ROM is doing with a GPIO, and what taking it over costs.
///
/// The two halves come from the device's `use` category, which reports the
/// *consequence* of forcing a pin rather than the pin's role - the role is named
/// separately by [`describe_gpio`] from board metadata.
fn describe_use(gpio_use: GpioUse) -> (&'static str, &'static str) {
    match gpio_use {
        GpioUse::Free => ("not in use by One ROM", ""),
        GpioUse::ServingRead => (
            "ROM serving reads it",
            "Forcing it is reversible: serving keeps reading the pin, and setting it \
             back to z restores it.",
        ),
        GpioUse::ServingDriven => (
            "ROM serving drives it",
            "Forcing it takes the pin away from the PIO that drives it, and serving \
             stays broken until the device is rebooted.",
        ),
        GpioUse::SystemPin => (
            "it is a One ROM system pin",
            "Driving it will disturb whatever the board uses it for.",
        ),
        GpioUse::InputForced => ("ROM serving ignores it", ""),
    }
}

pub(crate) fn needs_force(gpio_use: GpioUse) -> bool {
    !matches!(gpio_use, GpioUse::Free | GpioUse::InputForced)
}

/// Say what a 3.3V-only pad risks, ahead of asking whether to go on.
///
/// Shared so that a pin vetted before programming and a pin driven directly are
/// warned about in the same words.
fn warn_three_volt_three(name: &str) {
    println!("Warning: {name} is 3.3V-only (an RP2350 ADC pin), not 5V-tolerant.");
    println!("  More than 3.3V on this pad can damage the MCU - including whatever the");
    println!("  net sits at once One ROM releases the pin.");
}

/// Ask before doing something the user may not have meant. `--yes` (and, where
/// the command has one, `--force`) answers for them.
fn confirm_gpio(options: &Options, force: bool) -> Result<bool, Error> {
    if options.yes || force {
        println!(
            "Auto-accepted ({})",
            if options.yes { "--yes" } else { "--force" }
        );
        return Ok(true);
    }

    print!("Proceed anyway? (y/N): ");
    std::io::stdout()
        .flush()
        .map_err(|e| Error::Other(e.to_string()))?;

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| Error::Other(e.to_string()))?;

    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// One `control` command's request to drive a pin.
///
/// The pin arrives already resolved, and the board it was resolved against comes
/// with it: both callers need the board anyway - to resolve a `--pin` pad name
/// before there is anything to drive - so passing it on costs nothing and keeps
/// the "which GPIO is this?" question settled in one place.
struct DriveRequest<'a> {
    /// The pin to drive.
    pin: ResolvedPin,

    /// The board the pin was resolved against, where one is known. `None` costs
    /// the pin's name and its 5V-tolerance check, not the operation.
    board: Option<&'a Board>,

    /// The state to drive now.
    state: GpioState,

    /// The state to apply when `hold_ms` expires.
    after: GpioState,

    /// Milliseconds to hold `state` for; 0 latches indefinitely.
    hold_ms: u32,

    /// Whether to override the device's in-use refusal.
    force: bool,

    /// Whether the caller has already warned about a 3.3V-only pad and had the
    /// user accept it. `program --reset-host` vets its pin before it flashes
    /// anything, and asking a second time once the device is back on the bus
    /// would be asking about a decision already taken.
    tolerance_confirmed: bool,

    /// How the caller's command overrides the in-use refusal; `control reset`
    /// has no `--force` of its own and points elsewhere.
    force_hint: &'a str,
}

/// Ask the device what it is using a GPIO for, and the user whether to go on.
///
/// Shared by `control pin`, `control reset` and `program --reset-host` - they
/// differ in the write they then make and in what they print, not in what is
/// asked here.
///
/// Returns the device's capabilities, which the caller needs for the write, or
/// `None` if the user declined a warning - in which case nothing has been
/// driven.
async fn vet_gpio(options: &Options, req: &DriveRequest<'_>) -> Result<Option<Caps>, Error> {
    let device = options.device.as_ref().unwrap();
    let gpio = req.pin.gpio();

    // The capability probe is also how a device too old for GPIO control says
    // so, in place of an unexplained USB stall.
    let caps = get_caps(device).await?;

    // Naming is best effort: a board this build does not recognise costs the
    // pin's name, not the operation.
    let board = req.board;
    let chip = active_chip_type(device);
    let name = describe_gpio(board, chip, gpio);

    // Ask before driving, so a refusal can say what the pin is doing rather
    // than only that it is busy. The firmware gates the write regardless, so
    // skipping this on a device that cannot answer loses the explanation, not
    // the protection.
    let gpio_use = if caps.has_feature(ONEROM_FEAT_GPIO_QUERY) {
        gpio_query(device, &caps, gpio, 1)
            .await?
            .first()
            .and_then(|entry| entry.gpio_use())
    } else {
        None
    };

    if let Some(gpio_use) = gpio_use
        && needs_force(gpio_use)
    {
        let (doing, consequence) = describe_use(gpio_use);
        if !req.force {
            return Err(Error::GpioInUseNamed(
                name.to_string(),
                doing.to_string(),
                consequence.to_string(),
                req.force_hint.to_string(),
            ));
        }
        println!("Warning: {name} is in use by One ROM: {doing}.");
        println!("  {consequence}");
    }

    // Static board metadata, not a measurement: the RP2350's ADC pins are the
    // only pads that are not 5V-tolerant. Nothing here knows or asks what is
    // wired to the pad.
    if let Some(board) = board
        && !req.tolerance_confirmed
        && board.gpio_tolerance(gpio) == Some(PinTolerance::ThreeVolt3)
    {
        warn_three_volt_three(&name);
        if !confirm_gpio(options, req.force)? {
            println!("Aborted");
            return Ok(None);
        }
    }

    Ok(Some(caps))
}

/// Drive one GPIO to `req`'s state, having vetted it first.
///
/// Returns `false` if the user declined a warning, in which case nothing was
/// driven.
async fn drive_gpio(options: &Options, req: DriveRequest<'_>) -> Result<bool, Error> {
    let Some(caps) = vet_gpio(options, &req).await? else {
        return Ok(false);
    };
    let device = options.device.as_ref().unwrap();

    let args = GpioSetArgs {
        gpio: req.pin.gpio(),
        state: req.state.into(),
        after_state: req.after.into(),
        flags: if req.force { ONEROM_GPIO_FLAG_FORCE } else { 0 },
        duration_ms: req.hold_ms,
    };
    gpio_set(device, &caps, args).await?;

    Ok(true)
}

/// Pulse a host system's reset line low, and say so.
///
/// Shared by `control reset` and `program --reset-host`. The pin is vetted
/// against the device the way any driven pin is, and the pulse itself is
/// [`reset::pulse`], which is where "low, then released, never high" lives.
///
/// `tolerance_confirmed` says the caller has already put the 3.3V-only warning
/// to the user.
pub async fn pulse_reset(
    options: &Options,
    pin: ResolvedPin,
    board: Option<&Board>,
    hold_ms: u32,
    tolerance_confirmed: bool,
) -> Result<(), Error> {
    let force_hint = format!(
        "Use '{}' to drive it anyway.",
        hint::force_pin_low(pin.pin())
    );
    let req = DriveRequest {
        pin,
        board,
        state: GpioState::Low,
        after: GpioState::Z,
        hold_ms,
        force: false,
        force_hint: &force_hint,
        tolerance_confirmed,
    };
    let Some(caps) = vet_gpio(options, &req).await? else {
        return Ok(());
    };

    let device = options.device.as_ref().unwrap();
    reset::pulse(device, &caps, pin.gpio(), hold_ms).await?;

    println!("{}", reset_asserted_line(pin, hold_ms));

    Ok(())
}

pub(crate) fn reset_asserted_line(pin: ResolvedPin, hold_ms: u32) -> String {
    format!(
        "Asserted reset on {pin} for {hold_ms}ms - the device times the pulse and releases the pin"
    )
}

/// Refuse a reset pin a ROM slot of `image` uses.
///
/// The device refuses to give up a pin it is serving with, so this is the same
/// refusal, made against the image about to be flashed rather than the one
/// already running.
pub fn refuse_reset_pin_in_use(image: &ParsedDevice, pin: ResolvedPin) -> Result<(), Error> {
    let slots = onerom_cli::image::slots_using(image, pin.gpio());
    let Some((last, rest)) = slots.split_last() else {
        return Ok(());
    };
    let slots = match rest {
        [] => format!("slot {last}"),
        _ => {
            let rest: Vec<String> = rest.iter().map(ToString::to_string).collect();
            format!("slots {} and {last}", rest.join(", "))
        }
    };
    Err(reset_pin_in_use(
        pin,
        &format!(
            "{pin} is used by {slots} of the image being programmed, so it can't reset the host."
        ),
    ))
}

/// The `--reset-host` error for `pin`, which `reason` says can't be used.
fn reset_pin_in_use(pin: ResolvedPin, reason: &str) -> Error {
    Error::InvalidArgument(
        "--reset-host".to_string(),
        format!(
            "{reason}\n  \
             Use '{}' to drive it anyway.",
            hint::force_pin_low(pin.pin())
        ),
    )
}

/// Vet a reset pin before anything is programmed, and resolve it.
///
/// Everything asked here comes from board metadata, so it is asked before the
/// device is touched and before a single ROM image is fetched: a pad this board
/// does not have, a pin the board uses itself, or a pad that cannot take 5V is
/// the user's mistake, and finding it later means finding it with the host
/// system already waiting to be reset. Whether the image's slots use the pin is
/// [`refuse_reset_pin_in_use`].
///
/// Returns the resolved pin, and whether the user was asked about a 3.3V-only
/// pad - which [`pulse_reset`] needs, so that accepting once is accepting.
pub fn check_reset_pin(
    options: &Options,
    pin: &Pin,
    board: Option<&Board>,
) -> Result<(ResolvedPin, bool), Error> {
    let resolved = pin.resolve(board)?;

    let Some(board) = board else {
        // Without a board there is nothing to ask: the pin was named as a GPIO
        // (a pad would have failed to resolve), and every objection is raised
        // from board metadata. The device still gates the write.
        return Ok((resolved, false));
    };

    let objections = reset::vet_pin(board, resolved.gpio());
    for objection in &objections {
        if let PinObjection::InUse(uses) = objection {
            return Err(reset_pin_in_use(
                resolved,
                &match uses.split_last() {
                    Some((last, rest)) if !rest.is_empty() => format!(
                        "{resolved} is wired to the board's {} and {last}, so it can't reset the host.",
                        rest.join(", ")
                    ),
                    _ => format!(
                        "{resolved} is wired to the board's {}, so it can't reset the host.",
                        uses.join(", ")
                    ),
                },
            ));
        }
    }

    let mut tolerance_confirmed = false;
    if objections.contains(&PinObjection::NotFiveVoltTolerant) {
        warn_three_volt_three(&describe_gpio(Some(board), None, resolved.gpio()));
        if !confirm_gpio(options, false)? {
            return Err(Error::Aborted(
                "The reset pin is 3.3V-only, and nothing has been programmed.".to_string(),
            ));
        }
        tolerance_confirmed = true;
    }

    Ok((resolved, tolerance_confirmed))
}

pub async fn cmd_reset(
    options: &Options,
    args: &args::control::ControlResetArgs,
) -> Result<(), Error> {
    check_device_running(options, args)?;

    // A reset pulse with no end is not a reset. `--hold 0` reaches the device as
    // "latch indefinitely", which for a reset line means holding the host system
    // down for ever, so it is rejected here rather than silently honoured.
    if args.hold == 0 {
        return Err(Error::InvalidArgument(
            "--hold".to_string(),
            format!(
                "A reset pulse must have a duration.\n  \
                 Use '{}' to latch a pin low indefinitely.",
                hint::latch_pin_low(args.pin)
            ),
        ));
    }

    // A pad name is meaningless without a board, so the pin is resolved here,
    // before the device is touched: a --pin this board has no pad for is the
    // user's mistake, not something to discover half way through driving it.
    let board = resolve_board_optional(options, &args.board)?;
    // The device being driven is a Fire, so an Ice --board would name its pads
    // against the wrong hardware - silently, which is the worst of the options.
    check_fire_board_optional(&board)?;
    let pin = args.pin.resolve(board.as_ref())?;

    pulse_reset(options, pin, board.as_ref(), args.hold, false).await
}

pub async fn cmd_select(
    options: &Options,
    args: &args::control::ControlSelectArgs,
) -> Result<(), Error> {
    check_device(options, args, true)?;
    let _device = options.device.as_ref().unwrap();
    Err(Error::Unimplemented("control select".to_string()))
}

pub async fn cmd_pin(options: &Options, args: &args::control::ControlPinArgs) -> Result<(), Error> {
    check_device_running(options, args)?;

    // No --hold means latch indefinitely, and --then has nothing to revert to;
    // clap already requires --hold alongside it. With --hold and no --then the
    // pin is released.
    let hold_ms = args.hold.unwrap_or(0);
    let after = args.then.unwrap_or(GpioState::Z);

    // See cmd_reset: the pin is resolved against the board before anything is
    // driven, because a pad name has no GPIO until a board says so, and an Ice
    // board is not the hardware being driven.
    let board = resolve_board_optional(options, &args.board)?;
    check_fire_board_optional(&board)?;
    let pin = args.pin.resolve(board.as_ref())?;

    let driven = drive_gpio(
        options,
        DriveRequest {
            pin,
            board: board.as_ref(),
            state: args.state,
            after,
            hold_ms,
            force: args.force,
            force_hint: "Use --force to drive it anyway.",
            tolerance_confirmed: false,
        },
    )
    .await?;

    if driven {
        if hold_ms == 0 {
            println!("Set {pin} {}", args.state);
        } else {
            println!(
                "Set {pin} {} for {hold_ms}ms - the device times the hold and then sets it {after}",
                args.state
            );
        }
    }

    Ok(())
}

// Resolve poke input — either a single byte value or the contents of a file.
//
// The ArgGroup on the args structs guarantees exactly one of these is Some.
fn poke_data(value: Option<u8>, input: Option<&String>) -> Result<Vec<u8>, Error> {
    if let Some(byte) = value {
        Ok(vec![byte])
    } else if let Some(path) = input {
        std::fs::read(path).map_err(|e| Error::Other(e.to_string()))
    } else {
        // Clap ArgGroup ensures this is unreachable, but be explicit
        Err(Error::Other("No data source specified".to_string()))
    }
}

pub async fn cmd_poke_memory(
    options: &Options,
    args: &args::control::ControlPokeMemoryArgs,
) -> Result<(), Error> {
    check_device(options, args, false)?;
    let device = options.device.as_ref().unwrap();

    let data = poke_data(args.byte, args.input.as_ref())?;
    write_memory(device, args.address, &data).await?;

    if options.verbose {
        println!("Wrote {} byte(s) to 0x{:08x}", data.len(), args.address);
    }

    Ok(())
}

pub async fn cmd_poke_live(
    options: &Options,
    args: &args::control::ControlPokeLiveArgs,
) -> Result<(), Error> {
    check_device(options, args, true)?;
    let data = poke_data(args.byte, args.input.as_ref())?;
    let (address, _length) =
        check_live_read_write(options, args.address, Some(data.len() as u32), args)?;
    let device = options.device.as_ref().unwrap();

    if args.delta {
        let current = read_memory(device, address, data.len() as u32).await?;

        // Build runs of consecutive changed bytes
        let mut runs: Vec<(u32, Vec<u8>)> = Vec::new();
        for (i, b) in data.iter().copied().enumerate() {
            if current.get(i).copied().unwrap_or(!b) != b {
                let addr = address + i as u32;
                #[allow(clippy::collapsible_if)]
                if let Some((start, bytes)) = runs.last_mut() {
                    if *start + bytes.len() as u32 == addr {
                        bytes.push(b);
                        continue;
                    }
                }
                runs.push((addr, vec![b]));
            }
        }

        let dry_run_str = if args.dry_run { "[dry-run] " } else { "" };

        // Write the deltas
        let delta_count: usize = runs.iter().map(|(_, b)| b.len()).sum();
        for (addr, bytes) in &runs {
            if options.verbose {
                println!(
                    "{dry_run_str}Writing {} byte(s) to 0x{addr:08x}",
                    bytes.len()
                );
            }
            if !args.dry_run {
                write_memory(device, *addr, bytes).await?;
            }
        }

        if runs.is_empty() {
            println!("{dry_run_str}No differences found - no data written.");
        } else {
            if options.verbose {
                println!("{dry_run_str}{} contiguous blocks written", runs.len())
            }
            println!(
                "{dry_run_str}Applied {delta_count} delta byte(s) of {} to live ROM offset 0x{:08x}",
                data.len(),
                args.address
            );
        }
    } else {
        write_memory(device, address, &data).await?;
        println!(
            "Wrote {} byte(s) to live ROM offset 0x{:08x}",
            data.len(),
            args.address
        );
    }

    Ok(())
}

const SECTOR_SIZE: u32 = 4096;

/// Each of `chips` as a range from `FLASH_BASE`, its offset and length.
fn chip_ranges(chips: &FlashChips) -> Vec<(u32, u32)> {
    [Some(chips.first()), chips.second()]
        .into_iter()
        .flatten()
        .map(|chip| (chip.start - FLASH_BASE, chip.end - chip.start))
        .collect()
}

/// The ranges `args` asks to erase, each as its offset from `FLASH_BASE` and
/// its length. `--all` is each of `chips`.
pub(crate) fn build_erase_ranges(
    args: &args::control::ControlEraseArgs,
    chips: &FlashChips,
) -> Result<Vec<(u32, u32)>, Error> {
    if args.all {
        return Ok(chip_ranges(chips));
    }

    let offsets: Vec<u32> = if !args.address.is_empty() {
        args.address
            .iter()
            .map(|&a| {
                if a < FLASH_BASE {
                    Err(Error::InvalidArgument(
                        "erase".to_string(),
                        format!("Address {a:#010x} is below flash base {FLASH_BASE:#010x}"),
                    ))
                } else {
                    Ok(a - FLASH_BASE)
                }
            })
            .collect::<Result<_, _>>()?
    } else {
        args.offset.clone()
    };

    if offsets.len() != args.length.len() {
        return Err(Error::InvalidArgument(
            "erase".to_string(),
            format!(
                "Got {} offset/address(es) but {} length(s)",
                offsets.len(),
                args.length.len()
            ),
        ));
    }

    Ok(offsets
        .into_iter()
        .zip(args.length.iter().copied())
        .collect())
}

/// Refuses a range that isn't whole sectors within one of `chips`.
pub(crate) fn validate_erase_ranges(
    ranges: &[(u32, u32)],
    chips: &FlashChips,
) -> Result<(), Error> {
    let flash = chip_ranges(chips);
    for (offset, size) in ranges {
        if offset % SECTOR_SIZE != 0 {
            return Err(Error::InvalidArgument(
                "erase".to_string(),
                format!("Offset {offset:#x} is not {SECTOR_SIZE}-byte aligned"),
            ));
        }
        if *size == 0 || size % SECTOR_SIZE != 0 {
            return Err(Error::InvalidArgument(
                "erase".to_string(),
                format!("Size {size:#x} must be a non-zero multiple of {SECTOR_SIZE:#x}"),
            ));
        }
        let within = |&(start, len): &(u32, u32)| {
            *offset >= start && u64::from(*offset) + u64::from(*size) <= u64::from(start + len)
        };
        if !flash.iter().any(within) {
            let chips = flash
                .iter()
                .map(|(start, len)| format!("{:#x}+{len:#x}", FLASH_BASE + start))
                .collect::<Vec<_>>()
                .join(" and ");
            return Err(Error::InvalidArgument(
                "erase".to_string(),
                format!(
                    "Range {:#x}+{size:#x} is outside this One ROM's flash\n  Flash: {chips}",
                    // An --offset can be any u32.
                    u64::from(FLASH_BASE) + u64::from(*offset)
                ),
            ));
        }
    }
    Ok(())
}

/// The kilobytes `ranges` cover together.
fn erase_kb(ranges: &[(u32, u32)]) -> u32 {
    ranges.iter().map(|(_, size)| size).sum::<u32>() / 1024
}

/// The line asking to erase `ranges`, which the device's line follows.
/// `--verbose` adds the number of ranges.
pub(crate) fn erase_question(ranges: &[(u32, u32)], verbose: bool) -> String {
    if verbose {
        format!(
            "This will erase {}KB across {} range(s) on device:",
            erase_kb(ranges),
            ranges.len()
        )
    } else {
        format!("This will erase {}KB on device:", erase_kb(ranges))
    }
}

/// The lines `--verbose` shows for `ranges` beneath the device's line.
pub(crate) fn erase_range_lines(ranges: &[(u32, u32)]) -> Vec<String> {
    ranges
        .iter()
        .map(|(offset, size)| {
            format!(
                "  {size:#x} bytes ({}KB) at {:#010x}",
                size / 1024,
                FLASH_BASE + offset
            )
        })
        .collect()
}

/// The line reporting `ranges` erased.
pub(crate) fn erased_line(ranges: &[(u32, u32)]) -> String {
    format!("Erased {}KB of flash", erase_kb(ranges))
}

fn confirm_erase(options: &Options, device: &Device, ranges: &[(u32, u32)]) -> Result<bool, Error> {
    println!("{}\n  {device}", erase_question(ranges, options.verbose));
    if options.verbose {
        for line in erase_range_lines(ranges) {
            println!("{line}");
        }
    }

    if options.yes {
        println!("Auto-accepted (--yes)");
        return Ok(true);
    }

    print!("Are you sure? (y/N): ");
    std::io::stdout()
        .flush()
        .map_err(|e| Error::Other(e.to_string()))?;

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| Error::Other(e.to_string()))?;

    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}

async fn ensure_stopped(options: &mut Options) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();
    if !device.is_running() {
        return Ok(());
    }

    if options.verbose {
        println!("Device is running, rebooting into stopped mode...");
    }
    let serial = device.serial.clone();
    reboot(device, &RebootArgs::stopped(false, false)).await?;

    let selector = serial.as_deref();
    let new_device = select_device(selector, options).await?;

    if new_device.is_running() {
        return Err(Error::DeviceStillRunning);
    }
    options.device = Some(new_device);
    Ok(())
}

async fn erase_ranges(options: &Options, ranges: &[(u32, u32)]) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();

    println!("Erasing flash - DO NOT DISCONNECT");

    for (offset, size) in ranges {
        if options.verbose {
            let address = FLASH_BASE + offset;
            println!("  Erasing {size:#x} bytes at {address:#010x}");
        }
        flash_erase(device, *offset, *size).await?;
    }

    println!("{}", erased_line(ranges));
    Ok(())
}

async fn reboot_after_erase(
    options: &Options,
    args: &args::control::ControlEraseArgs,
) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();

    let reboot_args = args.into();
    reboot(device, &reboot_args).await?;
    if !reboot_args.is_none() {
        println!("Rebooted device into {} mode", reboot_args.mode);
    }

    Ok(())
}

pub async fn cmd_erase(
    options: &mut Options,
    args: &args::control::ControlEraseArgs,
) -> Result<(), Error> {
    check_device(options, args, false)?;

    let chips = options.device.as_ref().unwrap().flash_chips();
    let ranges = build_erase_ranges(args, &chips)?;
    validate_erase_ranges(&ranges, &chips)?;

    if !confirm_erase(options, options.device.as_ref().unwrap(), &ranges)? {
        println!("Aborted");
        return Ok(());
    }

    if !args.no_reboot {
        ensure_stopped(options).await?;
    } else if options.verbose {
        println!("Not rebooting before erase");
    }
    erase_ranges(options, &ranges).await?;
    if !args.no_reboot {
        reboot_after_erase(options, args).await
    } else {
        if options.verbose {
            println!("Not rebooting after erase");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_board::{holds, image_2364_sets};
    use onerom_cli::LogLevel;
    use onerom_cli::pin::parse_pin;

    /// fire-24-f, whose header is characterised and whose select pads sit behind
    /// the RP2350A's ADC pins, so every case below is reachable on one board.
    fn board() -> Board {
        Board::try_from_str("fire-24-f").unwrap()
    }

    fn options(yes: bool) -> Options {
        Options {
            verbose: false,
            log_level: LogLevel::Warn,
            yes,
            unrecognised: false,
            device: None,
            vid_pid: Vec::new(),
        }
    }

    fn check(pin: &str, board: Option<&Board>) -> Result<(ResolvedPin, bool), Error> {
        check_reset_pin(&options(true), &parse_pin(pin).unwrap(), board)
    }

    #[test]
    fn a_free_pad_is_accepted_and_resolved() {
        // X1 is an expansion pad: no system function, and 5V-tolerant.
        let (pin, confirmed) = check("x1", Some(&board())).unwrap();
        assert_eq!(pin.gpio(), 9);
        assert!(!confirmed);
    }

    #[test]
    fn a_pin_the_board_uses_itself_is_refused() {
        // GPIO29 drives fire-24-f's status LED and its RGB LED.
        let err = check("gpio29", Some(&board())).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Status LED"), "{msg}");
        assert!(msg.contains("RGB LED"), "{msg}");
    }

    #[test]
    fn a_three_volt_three_pad_has_to_be_accepted() {
        // SEL_A is GPIO26, an ADC pin, so it is not 5V-tolerant. --yes answers
        // the warning, and the answer is carried out so the pulse does not ask
        // again.
        let (pin, confirmed) = check("sel_a", Some(&board())).unwrap();
        assert_eq!(pin.gpio(), 26);
        assert!(confirmed);

        // Declining is not exercised here: it is a stdin read, which would hang
        // a `cargo test` run on a terminal. What it returns is Error::Aborted,
        // so nothing is programmed.
    }

    #[test]
    fn a_gpio_named_without_a_board_is_taken_as_given() {
        // Nothing below the resolve can be asked without board metadata, and the
        // device still gates the write.
        let (pin, confirmed) = check("gpio16", None).unwrap();
        assert_eq!(pin.gpio(), 16);
        assert!(!confirmed);
    }

    /// The ROM slots an image of 2364 `sets` for `board` has using `pin`.
    async fn slots_using(board: Board, sets: &[&str], pin: &str) -> Vec<usize> {
        let image = image_2364_sets(board, sets, &[]);
        let image = onerom_cli::image::parse_firmware(&image).await;
        let pin = parse_pin(pin).unwrap().resolve(Some(&board)).unwrap();
        onerom_cli::image::slots_using(&image, pin.gpio())
    }

    #[tokio::test]
    async fn a_pin_a_slot_serves_with_is_found() {
        // GPIO16 is A7 of a 2364 on fire-24-f.
        assert_eq!(slots_using(board(), &["single"], "gpio16").await, [0]);
        assert!(slots_using(board(), &["single"], "x1").await.is_empty());
    }

    /// A banked set reads X1 to select its bank, and the slot that uses it
    /// needn't be the first.
    #[tokio::test]
    async fn x1_is_found_where_a_banked_set_uses_it() {
        let sets = ["single", "banked", "single", "banked"];
        assert_eq!(slots_using(board(), &sets, "x1").await, [1, 3]);
    }

    /// fire-28-c's X1 is wired to GPIOs 9 and 28, and a banked set reads it on
    /// 28.
    #[tokio::test]
    async fn either_gpio_of_a_dual_wired_x_pin_is_found() {
        for pin in ["x1", "gpio9", "gpio28"] {
            assert_eq!(
                slots_using(Board::Fire28C, &["banked"], pin).await,
                [0],
                "{pin}"
            );
        }
        assert!(
            slots_using(Board::Fire28C, &["single"], "gpio9")
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_reset_pin_a_slot_uses_is_refused() {
        let image = image_2364_sets(board(), &["single", "banked", "banked"], &[]);
        let image = onerom_cli::image::parse_firmware(&image).await;
        let pin = |name: &str| parse_pin(name).unwrap().resolve(Some(&board())).unwrap();

        let err = refuse_reset_pin_in_use(&image, pin("x1")).unwrap_err();
        let msg = err.to_string();
        assert!(holds(&msg, &["--reset-host", "x1", "1", "2"]), "{msg}");
        assert!(refuse_reset_pin_in_use(&image, pin("x2")).is_ok());
    }

    /// `words`, which start `onerom control erase`, parsed.
    fn erase_args(words: &str) -> args::control::ControlEraseArgs {
        use clap::Parser;
        let cli = crate::args::Cli::try_parse_from(words.split_whitespace()).unwrap();
        let crate::args::Commands::Control(control) = cli.command else {
            panic!("not control");
        };
        let args::control::ControlCommands::Erase(args) = control.command else {
            panic!("not control erase");
        };
        args
    }

    fn chips(size: onerom_config::hw::BoardSize) -> FlashChips {
        FlashChips::new(onerom_config::mcu::Variant::RP2350, size)
    }

    #[test]
    fn all_erases_every_chip() {
        use onerom_config::hw::BoardSize;
        let args = erase_args("onerom control erase --all");
        let m = build_erase_ranges(&args, &chips(BoardSize::M)).unwrap();
        assert_eq!(m, [(0, 0x20_0000)]);
        let l = build_erase_ranges(&args, &chips(BoardSize::L)).unwrap();
        assert_eq!(l, [(0, 0x20_0000), (0x100_0000, 0x20_0000)]);
    }

    /// An address on the second chip is an offset of 0x1000000 and more.
    #[test]
    fn a_range_on_the_second_chip_needs_a_board_with_one() {
        use onerom_config::hw::BoardSize;
        let args = erase_args("onerom control erase --address 0x11000000 --length 0x1000");
        let ranges = build_erase_ranges(&args, &chips(BoardSize::L)).unwrap();
        assert_eq!(ranges, [(0x100_0000, 0x1000)]);
        assert!(validate_erase_ranges(&ranges, &chips(BoardSize::L)).is_ok());
        assert!(validate_erase_ranges(&ranges, &chips(BoardSize::M)).is_err());
    }

    #[test]
    fn a_range_outside_a_chip_is_refused() {
        use onerom_config::hw::BoardSize;
        let l = chips(BoardSize::L);
        // The last sector of each chip.
        assert!(validate_erase_ranges(&[(0x1f_f000, 0x1000)], &l).is_ok());
        assert!(validate_erase_ranges(&[(0x11f_f000, 0x1000)], &l).is_ok());
        // Past the end of each chip, between them, and across the end of the
        // first.
        for range in [
            (0x20_0000, 0x1000),
            (0x120_0000, 0x1000),
            (0x80_0000, 0x1000),
            (0x1f_f000, 0x2000),
        ] {
            assert!(validate_erase_ranges(&[range], &l).is_err(), "{range:x?}");
        }
    }

    #[test]
    fn the_erase_lines_contain_the_total() {
        use onerom_config::hw::BoardSize;
        let args = erase_args("onerom control erase --all");
        let ranges = build_erase_ranges(&args, &chips(BoardSize::L)).unwrap();
        assert!(holds(&erase_question(&ranges, true), &["4096KB", "2"]));
        assert!(holds(&erase_question(&ranges, false), &["4096KB"]));
        assert!(holds(&erased_line(&ranges), &["4096KB"]));
        let lines = erase_range_lines(&ranges);
        assert!(holds(&lines[0], &["0x10000000"]));
        assert!(holds(&lines[1], &["0x11000000"]));
    }

    #[test]
    fn a_pad_named_without_a_board_is_refused() {
        let err = check("sel_a", None).unwrap_err();
        assert!(err.to_string().contains("--board"), "{err}");
    }

    #[test]
    fn only_free_and_input_forced_pins_drive_unforced() {
        assert!(!needs_force(GpioUse::Free));
        assert!(!needs_force(GpioUse::InputForced));
        assert!(needs_force(GpioUse::ServingRead));
        assert!(needs_force(GpioUse::ServingDriven));
        assert!(needs_force(GpioUse::SystemPin));
    }
}
