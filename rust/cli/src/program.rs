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
use onerom_app::read_commissioning;
use onerom_cli::device::select_device_by_chip_id;
use onerom_cli::otp::{PicobootOtp, escape_controls};
use onerom_cli::pin::ResolvedPin;
use onerom_cli::plugin::{parse_plugins, resolve_plugins};
use onerom_cli::slot::{self, GlobalConfig, check_slot_confirmations, save_config};
use onerom_cli::usb::{RebootArgs, flash_program, flash_program_read, reboot};
use onerom_cli::{Device, DeviceState, Error, Options};
use onerom_fw_parser::ParsedDevice;
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

/// Flashes `data`. `image_board` is the board the image is for. `force`
/// programs a board commissioned as another board. A refused image leaves the
/// One ROM as it was.
async fn flash_device(
    options: &mut Options,
    data: &[u8],
    image_board: Option<Board>,
    force: bool,
) -> Result<(), Error> {
    let stopped = reboot_to_stopped(options, &[DeviceState::Running]).await?;

    let device = options.device.as_ref().unwrap();
    if let Err(e) = check_commissioned_board(device, image_board, force).await {
        return restart(options, stopped, Err(e)).await;
    }
    // After the board check, since a refused image isn't written.
    println!("Programming device - DO NOT DISCONNECT");
    if options.verbose {
        println!("Flashing {} bytes...", data.len());
    }
    flash_program(device, data).await
}

/// Reboots the device into the bootloader if its state is one of `states` and
/// selects it again by its chip ID. Returns whether it rebooted it.
pub(crate) async fn reboot_to_stopped(
    options: &mut Options,
    states: &[DeviceState],
) -> Result<bool, Error> {
    let device = options.device.as_ref().unwrap();
    if !states.contains(&device.state) {
        return Ok(false);
    }

    if options.verbose {
        let state = if device.state == DeviceState::Limp {
            "in limp mode"
        } else {
            "running"
        };
        println!("Device is {state}, rebooting into stopped mode...");
    }
    let chip_id = device.chip_id;
    reboot(device, &RebootArgs::stopped(false, false)).await?;

    let new_device = select_device_by_chip_id(chip_id, options).await?;
    if states.contains(&new_device.state) {
        return Err(Error::DeviceStillRunning);
    }
    options.device = Some(new_device);
    Ok(true)
}

/// Reboots a device that [`reboot_to_stopped`] stopped back into running
/// mode.
async fn reboot_to_running(options: &Options) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();
    if options.verbose {
        println!("Rebooting device into running mode...");
    }
    reboot(device, &RebootArgs::running(false, false)).await
}

/// Reboots a One ROM this command `stopped` back into running mode and
/// returns `result`. A failed reboot is the error where `result` is `Ok`.
pub(crate) async fn restart(
    options: &Options,
    stopped: bool,
    result: Result<(), Error>,
) -> Result<(), Error> {
    if !stopped {
        return result;
    }
    match (result, reboot_to_running(options).await) {
        (Ok(()), rebooted) => rebooted,
        (Err(e), Ok(())) => Err(e),
        (Err(e), Err(reboot)) => {
            eprintln!("Warning: couldn't reboot the One ROM into running mode.\n  {reboot}");
            Err(e)
        }
    }
}

/// Reboots a stopped device into the bootloader again.
pub(crate) async fn reboot_stopped(options: &Options) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();
    if options.verbose {
        println!("Rebooting device into stopped mode...");
    }
    reboot(device, &RebootArgs::stopped(false, false)).await
}

/// Refuses to program a commissioned board with an image for another board
/// unless `force`.
///
/// An image without a board isn't checked. Where OTP can't be read it warns and
/// goes ahead.
async fn check_commissioned_board(
    device: &Device,
    image_board: Option<Board>,
    force: bool,
) -> Result<(), Error> {
    let Some(image_board) = image_board else {
        return Ok(());
    };
    let area = match PicobootOtp::open(device).await {
        Ok(mut otp) => read_commissioning(&mut otp).await.map_err(Error::from),
        Err(e) => Err(e),
    };
    match area {
        Ok(area) => check_board(area.current().and_then(|i| i.board()), image_board, force),
        Err(e) => {
            eprintln!(
                "Warning: Commissioning information couldn't be read so unable to check whether the board type matches version being programmed\n  {e}"
            );
            Ok(())
        }
    }
}

/// Refuses an image for `image` on a board commissioned as `commissioned`.
/// With `force` it warns instead.
fn check_board(commissioned: Option<&str>, image: Board, force: bool) -> Result<(), Error> {
    let Some(commissioned) = commissioned else {
        return Ok(());
    };
    if Board::try_from_str(commissioned) == Some(image) {
        return Ok(());
    }
    let commissioned = escape_controls(commissioned);
    if force {
        eprintln!(
            "Warning: image board type '{}' does not match the commissioned board type '{commissioned}' (continuing due to --force)",
            image.name()
        );
        Ok(())
    } else {
        Err(Error::CommissionedBoardMismatch {
            commissioned,
            image: image.name().to_string(),
        })
    }
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

    // onerom program sets a board up as One ROM.
    if let Some(file) = &args.firmware
        && matches!(image, ParsedDevice::Lab)
    {
        return Err(Error::LabFirmware(file.clone()));
    }

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

        flash_device(options, &data, image.get_board(), args.force).await?;

        if args.verify {
            verify_flash(options, &data).await?;
        }

        reboot_and_rescan(options, &args.into()).await?;
        println!("Programming complete");

        if args.scan_slots {
            if let Some(device) = options.device.as_ref() {
                println!("Reading device after programming...");
                crate::inspect::output_slot_info(device, options, "", &[])
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

#[cfg(test)]
mod tests {
    use super::*;

    fn board(name: &str) -> Board {
        Board::try_from_str(name).unwrap()
    }

    #[test]
    fn a_board_without_commissioning_takes_any_image() {
        assert!(check_board(None, board("fire-24-f"), false).is_ok());
    }

    #[test]
    fn a_commissioned_board_takes_an_image_for_its_board() {
        assert!(check_board(Some("fire-24-f"), board("fire-24-f"), false).is_ok());
    }

    #[test]
    fn an_image_for_another_board_needs_force() {
        let error = check_board(Some("fire-24-f"), board("fire-24-e"), false).unwrap_err();
        assert!(
            matches!(&error, Error::CommissionedBoardMismatch { commissioned, image }
                if commissioned == "fire-24-f" && image == "fire-24-e"),
            "{error}"
        );
        assert!(check_board(Some("fire-24-f"), board("fire-24-e"), true).is_ok());
    }

    /// A board name this build doesn't know can't be the image's board.
    #[test]
    fn an_unknown_commissioned_board_needs_force() {
        assert!(check_board(Some("fire-99-z"), board("fire-24-f"), false).is_err());
        assert!(check_board(Some("fire-99-z"), board("fire-24-f"), true).is_ok());
    }

    #[test]
    fn a_commissioned_board_is_shown_with_its_control_characters_escaped() {
        let error = check_board(Some("fire\u{1b}[2J"), board("fire-24-f"), false).unwrap_err();
        assert!(
            matches!(&error, Error::CommissionedBoardMismatch { commissioned, .. }
                if commissioned == "fire\\u{1b}[2J"),
            "{error}"
        );
    }
}
