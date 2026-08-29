// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Implementation of `onerom program`.

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_config::mcu::Variant;
use onerom_fw::{assemble_firmware, validate_sizes};

use crate::args;
use crate::firmware::{
    acquire_firmware, build_rom_image, confirm_slot_overrides, resolve_config_json,
    verify_assembled_firmware,
};
use crate::utils::{check_device, check_fire_board_optional, resolve_board};
use onerom_cli::device::select_device_by_chip_id;
use onerom_cli::pin::ResolvedPin;
use onerom_cli::plugin::{parse_plugins, resolve_plugins};
use onerom_cli::slot::{self, GlobalConfig, check_slot_confirmations, save_config};
use onerom_cli::usb::{RebootArgs, flash_program, flash_program_read, reboot};
use onerom_cli::{Error, Options};
use onerom_metadata::GPIO_RESET_DEFAULT_HOLD_MS;

// ------------------------------- Argument validation -------------------------------

fn validate_program_args(args: &args::program::ProgramArgs) -> Result<(), Error> {
    if args.msd && !args.stopped {
        return Err(Error::InvalidArgument(
            "program".to_string(),
            "--msd requires --stopped".to_string(),
        ));
    }

    // Clap cannot express "this group is required unless --no-config or --firmware
    // is set", so we enforce it here.
    if !args.no_config
        && args.config_file.is_none()
        && args.slot.is_empty()
        && args.firmware.is_none()
        && args.base_firmware.is_none()
    {
        return Err(Error::NoFirmwareSource);
    }

    Ok(())
}

// ------------------------------- Image acquisition -------------------------------

/// Acquire the complete firmware image to flash, from any of the supported sources.
async fn acquire_program_image(
    options: &Options,
    args: &args::program::ProgramArgs,
    board: &Option<Board>,
    mcu: &Variant,
    reset_host: Option<ResolvedPin>,
) -> Result<Vec<u8>, Error> {
    if let Some(firmware) = &args.firmware {
        return load_prebuilt_firmware(options, firmware);
    }

    if is_bare_base_firmware(args) {
        return load_bare_base_firmware(options, args.base_firmware.as_deref().unwrap());
    }

    build_and_assemble(options, args, board, mcu, reset_host).await
}

fn load_prebuilt_firmware(options: &Options, firmware: &str) -> Result<Vec<u8>, Error> {
    if options.verbose {
        println!("Using pre-built firmware: {firmware}");
    }
    std::fs::read(firmware).map_err(|e| Error::io(firmware, e))
}

/// Returns true when --base-firmware is given alone (no config source), meaning
/// the user wants to flash the base firmware as-is without ROM metadata.
fn is_bare_base_firmware(args: &args::program::ProgramArgs) -> bool {
    args.base_firmware.is_some() && args.config_file.is_none() && args.slot.is_empty()
}

fn load_bare_base_firmware(options: &Options, path: &str) -> Result<Vec<u8>, Error> {
    if options.verbose {
        println!("Flashing base firmware without ROM config: {path}");
    }
    std::fs::read(path).map_err(|e| Error::io(path, e))
}

async fn build_and_assemble(
    options: &Options,
    args: &args::program::ProgramArgs,
    board: &Option<Board>,
    mcu: &Variant,
    reset_host: Option<ResolvedPin>,
) -> Result<Vec<u8>, Error> {
    let board = board.as_ref().ok_or(Error::NoBoardOrDevice)?;

    // Acquire firmware first — version is needed for plugin compat checking.
    let (firmware_data, version, _version_str) =
        acquire_firmware(options, &args.base_firmware, &args.version, board, mcu).await?;

    let plugins = resolve_plugins(
        &parse_plugins(&args.plugin)?,
        &version,
        &onerom_cli::CliFetch,
    )
    .await?;

    let global_config = GlobalConfig {
        config_name: args.config_name.clone(),
        config_description: args.config_description.clone(),
        instance_name: args.instance_name.clone(),
        serial_override: args.serial_override.clone(),
        boot_logging: args.logging,
        disable_swd: args.disable_swd,
        turbo_boot: args.turbo_boot,
    };

    let config_json = resolve_config_json(
        args.config_file.as_deref(),
        &args.slot,
        args.no_config,
        board,
        &version,
        Some(&global_config),
        &plugins,
    )?;

    if let Some(path) = &args.save_config {
        save_config(path, &config_json)?;
        if options.verbose {
            println!("Saved ROM configuration to {path}");
        }
    }

    let (fw_props, metadata, image_data, desc) = build_rom_image(
        options,
        &config_json,
        version,
        *board,
        *mcu,
        args.force,
        // Runs with the config resolved and not one ROM image fetched, so a
        // request this build cannot honour costs the user nothing to discover.
        |config| {
            refuse_unservable_request(
                args,
                Some(board),
                reset_host,
                &slot::chip_types(config),
                slot::has_system_plugin(config),
            )
        },
    )
    .await?;

    validate_sizes(&fw_props, &firmware_data, &metadata, &image_data)?;

    if options.verbose && !desc.is_empty() {
        println!("ROM configuration:\n---\n{desc}\n---");
    }

    assemble_firmware(firmware_data, metadata, image_data).map_err(Into::into)
}

// ------------------------------- Flash operations -------------------------------

async fn verify_flash(options: &Options, data: &[u8]) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();
    if options.verbose {
        println!("Verifying {} bytes...", data.len());
    }
    let readback = flash_program_read(device, data.len() as u32).await?;
    for (i, (expected, actual)) in data.iter().zip(readback.iter()).enumerate() {
        if expected != actual {
            return Err(Error::VerifyFailed(i, *expected, *actual));
        }
    }
    println!("Verification passed");
    Ok(())
}

async fn flash_device(options: &mut Options, data: &[u8]) -> Result<(), Error> {
    reboot_to_stopped_if_running(options).await?;

    let device = options.device.as_ref().unwrap();
    if options.verbose {
        println!("Flashing {} bytes...", data.len());
    }
    flash_program(device, data).await
}

async fn reboot_to_stopped_if_running(options: &mut Options) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();
    if !device.is_running() {
        return Ok(());
    }

    if options.verbose {
        println!("Device is running, rebooting into stopped mode...");
    }
    let chip_id = device.chip_id;
    reboot(device, &RebootArgs::stopped(false, false)).await?;

    let new_device = select_device_by_chip_id(chip_id, options).await?;
    if new_device.is_running() {
        return Err(Error::DeviceStillRunning);
    }
    options.device = Some(new_device);
    Ok(())
}

fn write_firmware_file(path: &str, data: &[u8]) -> Result<(), Error> {
    std::fs::write(path, data).map_err(|e| Error::io(path, e))?;
    println!("Firmware written to {path}");
    Ok(())
}

async fn reboot_and_rescan(options: &mut Options, reboot_args: &RebootArgs) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();
    if options.verbose {
        println!("Rebooting device...");
    }
    let chip_id = device.chip_id;
    reboot(device, reboot_args).await?;

    if !reboot_args.fast {
        let device = select_device_by_chip_id(chip_id, options).await?;
        if options.verbose {
            println!("{device}");
        }
        options.device = Some(device);
    }
    Ok(())
}

// ------------------------------- Host reset -------------------------------

/// The error for an option that needs the One ROM on the USB bus while it runs.
///
/// Without a system plugin a running One ROM has no USB stack of its own, so it
/// leaves the bus the moment it starts serving. Anything the host wants to say
/// to it, or hear from it, after programming needs one.
fn without_usb(option: &str, consequence: &str) -> Error {
    Error::InvalidArgument(
        option.to_string(),
        format!(
            "The image being programmed has no USB system plugin, so the One ROM\n  \
             will not be on the USB bus once it is running{consequence}\n  \
             Add '--plugin usb', or drop {option}."
        ),
    )
}

/// Refuse a request the image being programmed cannot honour.
///
/// `--follow` and `--reset-host` both need the device back on the USB bus after
/// the flash, and `--reset-host` needs a pin One ROM is not serving with. All of
/// that is settled by what is being flashed, so it is asked of the build - once
/// from the config, which is known before any ROM image is fetched, and once
/// from a pre-built image, which is all there is to go on when the user supplied
/// one.
fn refuse_unservable_request(
    args: &args::program::ProgramArgs,
    board: Option<&Board>,
    reset_pin: Option<ResolvedPin>,
    chips: &[ChipType],
    usb_capable: bool,
) -> Result<(), Error> {
    if !usb_capable {
        if reset_pin.is_some() {
            return Err(without_usb("--reset-host", ", to take the reset."));
        }
        if args.follow {
            return Err(without_usb("--follow", ", and there is no log to follow."));
        }
    }

    // Without a board no pin can be named, let alone judged; `check_reset_pin`
    // has already said so where it matters.
    if let (Some(board), Some(pin)) = (board, reset_pin) {
        crate::control::refuse_reset_pin_in_use(board, chips, pin)?;
    }

    Ok(())
}

// ------------------------------- program command -------------------------------

pub async fn cmd_program(
    options: &mut Options,
    args: &args::program::ProgramArgs,
) -> Result<(), Error> {
    validate_program_args(args)?;
    check_device(options, args, false)?;

    // Board must be resolved before acquire_program_image so it is available
    // for chip type validation when parsing --slot arguments.
    let board = resolve_board(options, &args.board)?;
    check_fire_board_optional(&board)?;
    let mcu = Variant::RP2350;

    if let Some(b) = &board
        && !args.slot.is_empty()
    {
        let confirmations = check_slot_confirmations(&args.slot, b)?;
        confirm_slot_overrides(options, &confirmations).await?;
    }

    // Everything about the reset pin that board metadata alone can settle - the
    // pad exists, One ROM does not use it for the board's own peripherals, it can
    // take 5V - is settled here, before a byte is read or fetched. What the image
    // decides is asked of the image, below.
    let reset_host = match &args.reset_host {
        Some(pin) => Some(crate::control::check_reset_pin(
            options,
            pin,
            board.as_ref(),
            &[],
        )?),
        None => None,
    };

    let data =
        acquire_program_image(options, args, &board, &mcu, reset_host.map(|(pin, _)| pin)).await?;
    let image = verify_assembled_firmware(options, &data, args.force, board).await?;

    // A pre-built image has no config to ask, so it is asked of the parse. A
    // built one has already answered, before its ROMs were fetched.
    if args.firmware.is_some() || is_bare_base_firmware(args) {
        // An image whose metadata did not parse cannot answer - and
        // verify_assembled_firmware has already said so, or been forced past -
        // so it is left alone rather than refused on a reading nothing stands
        // behind.
        let usb_capable = !image.parse_errors().is_empty() || image.is_usb_run_capable();
        refuse_unservable_request(
            args,
            board.as_ref(),
            reset_host.map(|(pin, _)| pin),
            &onerom_cli::image::chip_types(&image),
            usb_capable,
        )?;
    }

    loop {
        if let Some(out) = &args.output {
            write_firmware_file(out, &data)?;
        }

        println!("Programming device - DO NOT DISCONNECT");
        flash_device(options, &data).await?;

        if args.verify {
            verify_flash(options, &data).await?;
        }

        reboot_and_rescan(options, &args.into()).await?;
        println!("Programming complete");

        if args.scan_slots {
            if let Some(device) = options.device.as_ref() {
                println!("Reading device after programming...");
                crate::inspect::output_slot_info(device, options, "")
                    .await
                    .inspect_err(|_| log::error!("Failed to read slots after programming"))?;
            } else {
                eprintln!("Failed to read device after programming");
                return Err(Error::NoDevice);
            }
        }

        // Before --follow, which does not return until the user stops watching.
        if let Some((pin, tolerance_confirmed)) = reset_host {
            crate::control::pulse_reset(
                options,
                pin,
                board.as_ref(),
                GPIO_RESET_DEFAULT_HOLD_MS,
                tolerance_confirmed,
            )
            .await?;
        }

        if args.follow {
            let device = options.device.as_ref().ok_or(Error::NoDevice)?;
            crate::monitor::follow(device, None, options.verbose).await?;
        }

        if !args.batch {
            break;
        }

        println!("Press any key to program next device, q to exit...");
        let key = crate::utils::read_char()?;
        if key.code == crossterm::event::KeyCode::Char('q') {
            println!("Exiting batch programming mode");
            break;
        }

        // Try and get a new device
        match onerom_cli::device::select_device(None, options).await {
            Ok(device) => {
                options.device = Some(device);
            }
            Err(e) => {
                eprintln!("Error selecting next device for programming:\n  {e}");
                println!("Exiting batch programming mode");
                break;
            }
        }
    }

    Ok(())
}
