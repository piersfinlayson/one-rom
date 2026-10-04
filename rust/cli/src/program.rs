// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Implementation of `onerom program`.

use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::{Board, BoardSize};
use onerom_config::mcu::Variant;
use onerom_fw::{assemble_firmware, validate_sizes};

use crate::args;
use crate::firmware::{
    acquire_firmware, build_rom_image, confirm_slot_overrides, resolve_config_json,
    verify_assembled_firmware,
};
use crate::utils::{check_device, check_fire_board_optional, resolve_board};
use onerom_app::{FlashPlan, FlashStep, read_commissioning};
use onerom_cli::device::select_device_by_chip_id;
use onerom_cli::error::plan_error;
use onerom_cli::otp::{PicobootOtp, check_board};
use onerom_cli::pin::ResolvedPin;
use onerom_cli::plugin::{parse_plugins, resolve_plugins};
use onerom_cli::slot::{self, GlobalConfig, check_slot_confirmations, save_config};
use onerom_cli::usb::{FLASH_BASE, RebootArgs, flash_program, flash_read, reboot};
use onerom_cli::{Device, DeviceState, Error, Options};
use onerom_fw_parser::ParsedDevice;
use onerom_gen::supports_board_size;
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
        reserved_pins: args.reserve_pin.clone(),
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

    let device = options.device.as_ref().ok_or(Error::NoDevice)?;
    let size = image_size(version, device.flash_chips().size());
    let (fw_props, metadata, image_data, desc) = build_rom_image(
        options,
        &config_json,
        version,
        *board,
        *mcu,
        size,
        args.force,
        // Runs with the config resolved and not one ROM image fetched, so a
        // request this build cannot honour costs the user nothing to discover.
        |config| refuse_unservable_request(args, slot::has_system_plugin(config)),
    )
    .await?;

    validate_sizes(&fw_props, &firmware_data, &metadata, &image_data)?;

    if options.verbose && !desc.is_empty() {
        println!("ROM configuration:\n---\n{desc}\n---");
    }

    assemble_firmware(firmware_data, metadata, image_data).map_err(Into::into)
}

/// The size to build an image for a `size` One ROM with firmware `version`.
/// It's M where the firmware doesn't support `size`.
fn image_size(version: FirmwareVersion, size: BoardSize) -> BoardSize {
    if supports_board_size(version, size) {
        size
    } else {
        BoardSize::M
    }
}

// ------------------------------- Flash operations -------------------------------

/// Reads back each chip `plan` writes and compares it with what was written.
async fn verify_flash(options: &Options, plan: &FlashPlan<'_>) -> Result<(), Error> {
    let device = options.device.as_ref().unwrap();
    for step in plan.steps() {
        let FlashStep::Write { addr, data } = *step else {
            continue;
        };
        if options.verbose {
            println!("{}", verify_line(addr, data));
        }
        let readback = flash_read(device, addr, data.len() as u32).await?;
        let offset = (addr - FLASH_BASE) as usize;
        for (i, (expected, actual)) in data.iter().zip(readback.iter()).enumerate() {
            if expected != actual {
                return Err(Error::VerifyFailed(offset + i, *expected, *actual));
            }
        }
    }
    println!("Verification passed");
    Ok(())
}

/// The line `--verbose` shows before reading back `data` from `addr`.
pub(crate) fn verify_line(addr: u32, data: &[u8]) -> String {
    format!("Verifying {} bytes at {addr:#010x}...", data.len())
}

/// Flashes `data` and reads it back where `verify`. `image_board` is the
/// board the image is for. `force` programs a board commissioned as another
/// board. A refused image leaves the One ROM as it was.
async fn flash_device(
    options: &mut Options,
    data: &[u8],
    image_board: Option<Board>,
    force: bool,
    verify: bool,
) -> Result<(), Error> {
    let stopped = reboot_to_stopped(options, &[DeviceState::Running]).await?;

    let device = options.device.as_ref().unwrap();
    if let Err(e) = check_commissioned_board(device, image_board, force).await {
        return restart(options, stopped, Err(e)).await;
    }
    // The stopped One ROM's size comes from OTP through the bootloader.
    let chips = device.flash_chips();
    let plan = match FlashPlan::new(data, &chips) {
        Ok(plan) => plan,
        Err(e) => {
            let error = plan_error(e, data.len(), device.board_size());
            return restart(options, stopped, Err(error)).await;
        }
    };
    // After the board and size checks, since a refused image isn't written.
    println!("Programming device - DO NOT DISCONNECT");
    if options.verbose {
        for line in plan_lines(&plan) {
            println!("{line}");
        }
    }
    flash_program(device, &plan).await?;

    if verify {
        verify_flash(options, &plan).await?;
    }
    Ok(())
}

/// The lines `--verbose` shows for `plan`'s steps, one each.
pub(crate) fn plan_lines(plan: &FlashPlan<'_>) -> Vec<String> {
    plan.steps()
        .iter()
        .filter_map(|step| match *step {
            FlashStep::Erase { addr, len } => {
                Some(format!("Erasing {len} bytes at {addr:#010x}..."))
            }
            FlashStep::Write { addr, data } => {
                Some(format!("Flashing {} bytes to {addr:#010x}...", data.len()))
            }
            // flash_program refuses a step it doesn't know.
            _ => None,
        })
        .collect()
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

/// Reboots a stopped device into the bootloader again and selects it by its
/// chip ID.
pub(crate) async fn reboot_stopped_and_select(options: &mut Options) -> Result<(), Error> {
    reboot_stopped(options).await?;
    let chip_id = options.device.as_ref().unwrap().chip_id;
    options.device = Some(select_device_by_chip_id(chip_id, options).await?);
    Ok(())
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
/// the flash. That is settled by what is being flashed, so it is asked of the
/// build - once from the config, which is known before any ROM image is
/// fetched, and once from a pre-built image, which is all there is to go on
/// when the user supplied one.
fn refuse_unservable_request(
    args: &args::program::ProgramArgs,
    usb_capable: bool,
) -> Result<(), Error> {
    if !usb_capable {
        if args.reset_host.is_some() {
            return Err(without_usb("--reset-host", ", to take the reset."));
        }
        if args.follow {
            return Err(without_usb("--follow", ", and there is no log to follow."));
        }
    }
    Ok(())
}

pub(crate) fn unreserved_reset_pin(image: &ParsedDevice, pin: ResolvedPin) -> Option<String> {
    let reserved = onerom_cli::pin::reserved_pads(image)?;
    let pad = onerom_cli::pin::metadata_pad(image, pin.gpio())?;
    (!reserved.contains(pad)).then(|| {
        format!(
            "--reset-host pin {} is not reserved so One ROM may use it.\n  \
             Reserve it with --reserve-pin {pad}",
            pad.silkscreen()
        )
    })
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
        )?),
        None => None,
    };

    let data = acquire_program_image(options, args, &board, &mcu).await?;
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
        refuse_unservable_request(args, usb_capable)?;
    }

    // The image's metadata holds which pins each slot uses, so this is checked
    // here for a built image and a pre-built one alike.
    if let Some((pin, _)) = reset_host {
        crate::control::refuse_reset_pin_in_use(&image, pin)?;
        if let Some(warning) = unreserved_reset_pin(&image, pin) {
            eprintln!("Warning: {warning}");
        }
    }

    loop {
        if let Some(out) = &args.output {
            write_firmware_file(out, &data)?;
        }

        flash_device(options, &data, image.get_board(), args.force, args.verify).await?;

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
    use onerom_gen::FlashChips;
    use onerom_metadata::{MaybeKnown, OneromBoardSize};

    fn chips(size: BoardSize) -> FlashChips {
        FlashChips::new(Variant::RP2350, size)
    }

    #[test]
    fn a_refused_plan_is_reported_with_the_sizes() {
        let image = vec![0; 5 * 1024 * 1024];
        let error = FlashPlan::new(&image, &chips(BoardSize::M)).unwrap_err();
        let m = Some(MaybeKnown::Known(OneromBoardSize::BoardSizeM));
        let error = plan_error(error, image.len(), m);
        assert!(
            matches!(&error, Error::SecondChipRequired(size) if size == "M"),
            "{error:?}"
        );

        let error = FlashPlan::new(&image, &chips(BoardSize::L)).unwrap_err();
        let l = Some(MaybeKnown::Known(OneromBoardSize::BoardSizeL));
        let error = plan_error(error, image.len(), l);
        assert!(
            matches!(error, Error::ImageTooLarge { image, flash }
                if image == 5 * 1024 * 1024 && flash == 4 * 1024 * 1024),
            "{error:?}"
        );
    }

    /// Firmware before 0.8.0 gets an M image whatever the One ROM's size.
    #[test]
    fn an_image_is_for_m_where_the_firmware_supports_only_m() {
        let old = FirmwareVersion::new(0, 7, 3, 0);
        let new = FirmwareVersion::new(0, 8, 0, 0);
        for &size in BoardSize::supported_values() {
            assert_eq!(image_size(old, size), BoardSize::M, "{size}");
            assert_eq!(image_size(new, size), size, "{size}");
        }
    }

    /// A line for each step, in the order the steps run.
    #[test]
    fn verbose_shows_each_step_where_it_runs() {
        use crate::test_board::holds;
        let image = vec![0; 2 * 1024 * 1024 + 4096];
        let plan = FlashPlan::new(&image, &chips(BoardSize::L)).unwrap();
        let lines = plan_lines(&plan);
        assert_eq!(lines.len(), 4);
        for (line, values) in lines.iter().zip([
            ["2097152", "0x10000000"],
            ["4096", "0x11000000"],
            ["4096", "0x11000000"],
            ["2097152", "0x10000000"],
        ]) {
            assert!(holds(line, &values), "{line}");
        }
    }

    #[tokio::test]
    async fn an_unreserved_reset_pad_is_reported() {
        use crate::test_board::{holds, image_2364};
        use onerom_cli::image::parse_firmware;
        use onerom_cli::pin::parse_pin;

        let pin = |name: &str| {
            parse_pin(name)
                .unwrap()
                .resolve(Some(&Board::Fire24F))
                .unwrap()
        };
        let unreserved = parse_firmware(&image_2364(1, &[])).await;
        let reserved = parse_firmware(&image_2364(1, &["x1", "sel_c"])).await;

        let warning = unreserved_reset_pin(&unreserved, pin("x1")).unwrap();
        assert!(holds(&warning, &["X1", "x1"]), "{warning}");
        let warning = unreserved_reset_pin(&unreserved, pin("gpio25")).unwrap();
        assert!(holds(&warning, &["SEL_C", "sel_c"]), "{warning}");
        assert_eq!(unreserved_reset_pin(&reserved, pin("x1")), None);
        assert_eq!(unreserved_reset_pin(&reserved, pin("gpio25")), None);
        // GPIO23 is an address line.
        assert_eq!(unreserved_reset_pin(&unreserved, pin("gpio23")), None);
    }
}
