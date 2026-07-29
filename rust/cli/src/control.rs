// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use crate::args::control::GpioState;
use crate::board_view::{gpio_header_role, gpio_rom_function, gpio_system_functions};
use crate::{
    args,
    utils::{
        active_chip_type, check_device, check_device_running, check_fire_board_optional,
        check_live_read_write, resolve_board_optional,
    },
};
use onerom_cli::device::{Device, select_device};
use onerom_cli::picobootx::{ONEROM_FEAT_GPIO_QUERY, ONEROM_GPIO_FLAG_FORCE};
use onerom_cli::pin::ResolvedPin;
use onerom_cli::usb::{
    FLASH_BASE, GpioSetArgs, GpioUse, LedSubCmd, RebootArgs, flash_erase, get_caps, gpio_query,
    gpio_set, read_memory, reboot, set_led, write_memory,
};
use onerom_cli::{Error, Options};
use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_config::mcu::PinTolerance;
use std::io::Write;

pub async fn cmd_led_on(
    options: &Options,
    args: &args::control::ControlLedOnArgs,
) -> Result<(), Error> {
    check_device(options, args, true)?;
    let device = options.device.as_ref().unwrap();
    set_led(device, 0, LedSubCmd::On).await?;
    if options.verbose {
        println!("LED on");
    }
    Ok(())
}

pub async fn cmd_led_off(
    options: &Options,
    args: &args::control::ControlLedOffArgs,
) -> Result<(), Error> {
    check_device(options, args, true)?;
    let device = options.device.as_ref().unwrap();
    set_led(device, 0, LedSubCmd::Off).await?;
    if options.verbose {
        println!("LED off");
    }
    Ok(())
}

pub async fn cmd_led_beacon(
    options: &Options,
    args: &args::control::ControlLedBeaconArgs,
) -> Result<(), Error> {
    check_device(options, args, true)?;
    let device = options.device.as_ref().unwrap();
    set_led(device, 0, LedSubCmd::Beacon).await?;
    if options.verbose {
        println!("LED beacon started");
    }
    Ok(())
}

pub async fn cmd_led_flame(
    options: &Options,
    args: &args::control::ControlLedFlameArgs,
) -> Result<(), Error> {
    check_device(options, args, true)?;
    let device = options.device.as_ref().unwrap();
    set_led(device, 0, LedSubCmd::Flame).await?;
    if options.verbose {
        println!("LED flame started");
    }
    Ok(())
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
        let device = select_device(selector, options.unrecognised, &options.vid_pid).await?;
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
            && let Some(function) = gpio_rom_function(board, chip, gpio)
        {
            notes.push(function);
        }
        // A GPIO can carry two peripherals - fire-24-f drives its status LED
        // and its NeoPixel from GPIO 29 - and a refusal that named only one
        // would understate what driving it disturbs.
        notes.extend(
            gpio_system_functions(board, gpio)
                .into_iter()
                .map(String::from),
        );
        if let Some(role) = gpio_header_role(board, gpio) {
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
    }
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

    /// How the caller's command overrides the in-use refusal; `control reset`
    /// has no `--force` of its own and points elsewhere.
    force_hint: &'a str,
}

/// Drive one GPIO, having first asked the device what it is using it for.
///
/// Shared by `control pin` and `control reset` - the two differ in the request
/// they build and what they print afterwards, not in what happens here.
///
/// Returns `false` if the user declined a warning, in which case nothing was
/// driven.
async fn drive_gpio(options: &Options, req: DriveRequest<'_>) -> Result<bool, Error> {
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
        && gpio_use != GpioUse::Free
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
        && board.gpio_tolerance(gpio) == Some(PinTolerance::ThreeVolt3)
    {
        println!("Warning: {name} is 3.3V-only (an RP2350 ADC pin), not 5V-tolerant.");
        println!("  More than 3.3V on this pad can damage the MCU - including whatever the");
        println!("  net sits at once One ROM releases the pin.");
        if !confirm_gpio(options, req.force)? {
            println!("Aborted");
            return Ok(false);
        }
    }

    let args = GpioSetArgs {
        gpio,
        state: req.state.into(),
        after_state: req.after.into(),
        flags: if req.force { ONEROM_GPIO_FLAG_FORCE } else { 0 },
        duration_ms: req.hold_ms,
    };
    gpio_set(device, &caps, args).await?;

    Ok(true)
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
            "A reset pulse must have a duration.\n  \
             Use 'onerom control pin --pin <PIN> --state low' to latch a pin low indefinitely."
                .to_string(),
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

    let driven = drive_gpio(
        options,
        DriveRequest {
            pin,
            board: board.as_ref(),
            state: GpioState::Low,
            // Released, never driven high: a reset net has its own pull-up and
            // may have other drivers on it.
            after: GpioState::Z,
            hold_ms: args.hold,
            force: false,
            force_hint:
                "Use 'onerom control pin --pin <PIN> --state low --hold <MS> --force' to drive it anyway.",
        },
    )
    .await?;

    if driven {
        println!(
            "Asserted reset on {pin} for {}ms - the device times the pulse and releases the pin",
            args.hold
        );
    }

    Ok(())
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

const FLASH_SIZE: u32 = 2 * 1024 * 1024;
const SECTOR_SIZE: u32 = 4096;

fn build_erase_ranges(args: &args::control::ControlEraseArgs) -> Result<Vec<(u32, u32)>, Error> {
    if args.all {
        return Ok(vec![(0, FLASH_SIZE)]);
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

fn validate_erase_ranges(ranges: &[(u32, u32)]) -> Result<(), Error> {
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
        if offset + size > FLASH_SIZE {
            return Err(Error::InvalidArgument(
                "erase".to_string(),
                format!("Range {offset:#x}+{size:#x} exceeds flash size {FLASH_SIZE:#x}"),
            ));
        }
    }
    Ok(())
}

fn confirm_erase(options: &Options, device: &Device, ranges: &[(u32, u32)]) -> Result<bool, Error> {
    let total_kb = ranges.iter().map(|(_, s)| s).sum::<u32>() / 1024;
    println!(
        "This will erase {total_kb}KB across {} range(s) on device:\n  {device}",
        ranges.len()
    );
    if options.verbose {
        for (offset, size) in ranges {
            println!(
                "  {size:#x} bytes ({}KB) at {:#010x}",
                size / 1024,
                FLASH_BASE + offset
            );
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
    let new_device = select_device(selector, options.unrecognised, &options.vid_pid).await?;

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

    let total_kb = ranges.iter().map(|(_, s)| s).sum::<u32>() / 1024;
    println!("Erased {total_kb}KB of flash");
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

    let ranges = build_erase_ranges(args)?;
    validate_erase_ranges(&ranges)?;

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
