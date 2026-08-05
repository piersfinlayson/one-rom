// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Implementation of `onerom program`.

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
use onerom_cli::plugin::{parse_plugins, resolve_plugins};
use onerom_cli::slot::{GlobalConfig, check_slot_confirmations, save_config};
use onerom_cli::usb::{RebootArgs, flash_program, flash_program_read, reboot};
use onerom_cli::{Error, Options};

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
) -> Result<Vec<u8>, Error> {
    if let Some(firmware) = &args.firmware {
        return load_prebuilt_firmware(options, firmware);
    }

    if is_bare_base_firmware(args) {
        return load_bare_base_firmware(options, args.base_firmware.as_deref().unwrap());
    }

    build_and_assemble(options, args, board, mcu).await
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

    let (fw_props, metadata, image_data, desc) =
        build_rom_image(options, &config_json, version, *board, *mcu, args.force).await?;

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

    let new_device =
        select_device_by_chip_id(chip_id, options.unrecognised, &options.vid_pid).await?;
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
        let device =
            select_device_by_chip_id(chip_id, options.unrecognised, &options.vid_pid).await?;
        if options.verbose {
            println!("{device}");
        }
        options.device = Some(device);
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

    let data = acquire_program_image(options, args, &board, &mcu).await?;
    verify_assembled_firmware(options, &data, args.force, board).await?;

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
        match onerom_cli::device::select_device(None, options.unrecognised, &options.vid_pid).await
        {
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
