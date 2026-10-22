// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Implementation of `onerom update` and `onerom clone`.
//!
//! Each reads a One ROM or an image file back into its config, changes the
//! config and programs it with `program`'s build and flash steps.

use onerom_app::{
    BoardSize, CheckedImage, PluginType, ReadBack, RecordedPlugin, assemble_image,
    published_plugins, validate_resolved_plugin_types,
};
use onerom_cli::device::{matches_serial, select_device};
use onerom_cli::plugin::{LocalFetch, ResolvedPlugin, parse_plugin, resolve_plugin};
use onerom_cli::read_back::{inline_files, problem_error, read_back, read_back_error};
use onerom_cli::usb::{RebootArgs, enumerate_devices};
use onerom_cli::{Error, Options};
use onerom_config::chip::ChipType;
use onerom_config::fw::FirmwareVersion;
use onerom_config::hw::Board;
use onerom_config::mcu::{Rp235xChipId, Variant};
use onerom_fw_parser::ParsedDevice;
use onerom_gen::{ChipSetConfig, Config};

use crate::args::update::{
    CloneArgs, RebuildArgs, UpdateCommitArgs, UpdateFirmwareArgs, UpdatePluginArgs, UpdateSlotArgs,
};
use crate::firmware::{accept_image, acquire_firmware, build_rom_image, plugin_error};
use crate::program::{flash_device, image_size, reboot_and_rescan};
use crate::save::chip_index;
use crate::utils::{check_device, resolve_board};

pub async fn cmd_firmware(options: &mut Options, args: &UpdateFirmwareArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let read_back = read_device(options).await?;
    let firmware = firmware(options, &args.base_firmware, &args.version, read_back.board).await?;
    let config = inline_files(&read_back);
    rebuild(
        options,
        Command::Update,
        &read_back,
        config,
        read_back.board,
        firmware,
        &args.rebuild,
    )
    .await
}

pub async fn cmd_slot(options: &mut Options, args: &UpdateSlotArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let read_back = read_device(options).await?;
    let config = replace_rom(&read_back, args.slot, args.chip, &args.image)?;
    rebuild(
        options,
        Command::Update,
        &read_back,
        config,
        read_back.board,
        reused_firmware(&read_back),
        &args.rebuild,
    )
    .await
}

pub async fn cmd_plugin(options: &mut Options, args: &UpdatePluginArgs) -> Result<(), Error> {
    check_device(options, args, false)?;
    let read_back = read_device(options).await?;
    let config = plugin_config(
        &read_back,
        &args.plugin,
        &read_back.version,
        &onerom_cli::CliFetch,
    )
    .await?;
    rebuild(
        options,
        Command::Update,
        &read_back,
        config,
        read_back.board,
        reused_firmware(&read_back),
        &args.rebuild,
    )
    .await
}

pub async fn cmd_commit(options: &Options, args: &UpdateCommitArgs) -> Result<(), Error> {
    check_device(options, args, true)?;
    let _device = options.device.as_ref().unwrap();
    Err(Error::Unimplemented("update commit".to_string()))
}

pub async fn cmd_clone(options: &mut Options, args: &CloneArgs) -> Result<(), Error> {
    let connected = async || {
        enumerate_devices(options)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|device| device.serial)
            .collect()
    };
    let (read_back, source_chip_id) = read_source(options, &args.from, connected).await?;
    check_recovered(&read_back)?;

    let target = select_device(Some(&args.to), options).await?;
    if source_chip_id.is_some() && source_chip_id == target.chip_id {
        return Err(Error::CloneOntoItself);
    }
    let config = clone_config(&read_back, target.onerom());
    options.device = Some(target);
    let board = resolve_board(options, &None)?.ok_or(Error::NoDevice)?;
    rebuild(
        options,
        Command::Clone,
        &read_back,
        config,
        board,
        reused_firmware(&read_back),
        &args.rebuild,
    )
    .await
}

/// The command a rebuild is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Update,
    Clone,
}

impl Command {
    /// The One ROM or firmware file the command reads, as the subject of a
    /// sentence.
    fn source(self) -> &'static str {
        match self {
            Command::Update => "The One ROM",
            Command::Clone => "The source",
        }
    }
}

/// Whether `clone --from` value `from` is a serial number rather than a file.
/// `connected` returns the connected One ROMs' serial numbers.
pub(crate) async fn is_serial(from: &str, connected: impl AsyncFnOnce() -> Vec<String>) -> bool {
    if std::path::Path::new(from).exists() {
        return false;
    }
    let chip_id = from.len() == 16 && from.chars().all(|c| c.is_ascii_hexdigit());
    if chip_id || from.contains(['*', '?']) {
        return true;
    }
    connected()
        .await
        .iter()
        .any(|serial| matches_serial(Some(serial), from))
}

/// Reads back `clone --from` value `from`, a One ROM's serial number or a
/// firmware file. Returns the read-back and, for a One ROM, its chip ID.
pub(crate) async fn read_source(
    options: &Options,
    from: &str,
    connected: impl AsyncFnOnce() -> Vec<String>,
) -> Result<(ReadBack, Option<Rp235xChipId>), Error> {
    if is_serial(from, connected).await {
        let device = select_device(Some(from), options).await?;
        let read_back = read_back(&device).await.map_err(source_error)?;
        Ok((read_back, device.chip_id))
    } else {
        let image = std::fs::read(from).map_err(|e| Error::io(from, e))?;
        let read_back = onerom_app::read_back_file(&image)
            .await
            .map_err(|e| read_back_error(from, e))?;
        Ok((read_back, None))
    }
}

/// `error`, from reading `clone`'s source One ROM back, with the source as
/// its subject.
#[allow(clippy::wildcard_enum_match_arm)]
pub(crate) fn source_error(error: Error) -> Error {
    match error {
        Error::FirmwareTooNew { version, .. } => Error::FirmwareTooNew {
            what: Command::Clone.source().to_string(),
            version,
        },
        error => error,
    }
}

/// Reads the selected One ROM back. Fails where part of its config couldn't be
/// read back, as a rebuild would lose it.
async fn read_device(options: &Options) -> Result<ReadBack, Error> {
    let device = options.device.as_ref().ok_or(Error::NoDevice)?;
    let read_back = read_back(device).await?;
    check_recovered(&read_back)?;
    Ok(read_back)
}

pub(crate) fn check_recovered(read_back: &ReadBack) -> Result<(), Error> {
    match read_back.problems.first() {
        Some(problem) => Err(problem_error(read_back, problem)),
        None => Ok(()),
    }
}

/// [`inline_slots`]'s config with chip `chip` of ROM slot `slot` replaced by
/// `image`. Without `chip`, the slot's only chip.
///
/// An `image` with the name of one of `read_back`'s files is that file as
/// `save --path` saved it, possibly edited. Its label is copied from that
/// file's chip so the next save writes the same name and label. Any other
/// `image` is labelled with its path.
pub(crate) fn replace_rom(
    read_back: &ReadBack,
    slot: u8,
    chip: Option<u8>,
    image: &str,
) -> Result<Config, Error> {
    let set = read_back
        .chip_set_of_slot(slot.into())
        .ok_or_else(|| no_slot(read_back, slot.into()))?;
    let name = std::path::Path::new(image)
        .file_name()
        .and_then(|name| name.to_str());
    let label = read_back
        .config
        .chip_sets
        .iter()
        .flat_map(|set| &set.chips)
        .find(|chip| Some(chip.file.as_str()) == name && read_back.file(&chip.file).is_some())
        .and_then(|chip| chip.label.clone());
    let mut config = inline_slots(read_back);
    let chips = &mut config.chip_sets[set].chips;
    let index = chip_index(slot, chips.len(), chip)?;
    let chip = &mut chips[index];
    chip.file = image.to_string();
    chip.label = label;
    Ok(config)
}

/// Puts `plugin` in `config`, replacing any plugin of the same type.
pub(crate) fn place_plugin(config: &mut Config, plugin: &ResolvedPlugin) -> Result<(), Error> {
    let chip_set = plugin.to_chip_set_config()?;
    let slot = plugin.plugin_type.slot_index();
    if config
        .chip_sets
        .get(slot)
        .is_some_and(|set| set_plugin_type(set) == Some(plugin.plugin_type))
    {
        config.chip_sets[slot] = chip_set;
    } else {
        config
            .chip_sets
            .insert(slot.min(plugin_count(config)), chip_set);
    }
    let types: Vec<_> = config
        .chip_sets
        .iter()
        .filter_map(set_plugin_type)
        .collect();
    validate_resolved_plugin_types(&types)?;
    Ok(())
}

/// Resolves `spec` for firmware `fw` and puts the plugin in `config`. `spec`
/// has the syntax of `program`'s `--plugin`.
pub(crate) async fn add_plugin<F: LocalFetch<Error = onerom_fw::Error>>(
    config: &mut Config,
    spec: &str,
    fw: &FirmwareVersion,
    fetch: &F,
) -> Result<(), Error> {
    let spec = parse_plugin(spec)?;
    let plugin = resolve_plugin(&spec, fw, fetch)
        .await
        .map_err(|e| plugin_error(std::slice::from_ref(&spec), e))?;
    place_plugin(config, &plugin)
}

/// [`inline_slots`]'s config with the plugin `spec` identifies added.
pub(crate) async fn plugin_config<F: LocalFetch<Error = onerom_fw::Error>>(
    read_back: &ReadBack,
    spec: &str,
    fw: &FirmwareVersion,
    fetch: &F,
) -> Result<Config, Error> {
    let mut config = inline_slots(read_back);
    add_plugin(&mut config, spec, fw, fetch).await?;
    Ok(config)
}

/// [`inline_slots`]'s config with the instance name and USB serial override
/// of the One ROM `target` was parsed from.
pub(crate) fn clone_config(read_back: &ReadBack, target: Option<&ParsedDevice>) -> Config {
    let fw = target
        .and_then(|onerom| onerom.as_schema())
        .and_then(|onerom| onerom.metadata())
        .map(|metadata| &metadata.fw);
    let mut config = inline_slots(read_back);
    config.instance_name = fw.and_then(|fw| fw.name.clone());
    config.serial_override = fw.and_then(|fw| fw.serial_number.clone());
    config
}

/// [`inline_files`]'s config with every plugin slot inlined as read,
/// published or not, so the data each plugin stores in its slot is kept.
/// `update firmware` builds from [`inline_files`]'s instead so a published
/// plugin comes from its release for the new firmware.
fn inline_slots(read_back: &ReadBack) -> Config {
    let mut read_back = read_back.clone();
    read_back.reuse_plugin_slots();
    inline_files(&read_back)
}

fn no_slot(read_back: &ReadBack, slot: usize) -> Error {
    let slots = (0..read_back.config.chip_sets.len())
        .take_while(|&slot| read_back.chip_set_of_slot(slot).is_some())
        .count();
    Error::NoSlot { slot, slots }
}

fn set_plugin_type(set: &ChipSetConfig) -> Option<PluginType> {
    let chip_type = set.chips.first()?.chip_type.resolved();
    if chip_type == ChipType::SystemPlugin {
        Some(PluginType::System)
    } else if chip_type == ChipType::UserPlugin {
        Some(PluginType::User)
    } else {
        None
    }
}

fn plugin_count(config: &Config) -> usize {
    config
        .chip_sets
        .iter()
        .take_while(|set| set_plugin_type(set).is_some())
        .count()
}

/// The base firmware for `board` and its version.
async fn firmware(
    options: &Options,
    base_firmware: &Option<String>,
    version: &Option<String>,
    board: Board,
) -> Result<(Vec<u8>, FirmwareVersion), Error> {
    let (firmware, version, _) =
        acquire_firmware(options, base_firmware, version, &board, &Variant::RP2350).await?;
    Ok((firmware, version))
}

pub(crate) fn reused_firmware(read_back: &ReadBack) -> (Vec<u8>, FirmwareVersion) {
    (read_back.base_firmware.clone(), read_back.version)
}

/// `error`, from building the config `command` read for `board`, as `update`
/// and `clone` report it.
#[allow(clippy::wildcard_enum_match_arm)]
fn build_error(command: Command, board: Board, error: Error) -> Error {
    use onerom_fw::Error as FwError;
    use onerom_gen::Error as GenError;

    let Error::Build(error) = error else {
        return error;
    };
    match error {
        FwError::Parse {
            error:
                GenError::FirmwareTooOld {
                    feat,
                    version,
                    minimum,
                },
        }
        | FwError::Build {
            error:
                GenError::FirmwareTooOld {
                    feat,
                    version,
                    minimum,
                },
        } => Error::FirmwareTooOldFor {
            feature: feat.to_string(),
            version,
            minimum,
        },
        // onerom-fw's parse error message is about a supplied config, and
        // this config was read.
        FwError::Parse { error } => Error::Build(FwError::build(error)),
        FwError::Build {
            error:
                GenError::ImageTooSmall {
                    chip_type,
                    filename,
                    expected,
                    actual,
                },
        } => Error::ImageTooSmall {
            file: filename,
            chip_type: chip_type.name().to_string(),
            expected,
            actual,
        },
        FwError::Build {
            error:
                GenError::SlotUnsupported {
                    slot,
                    chip_type,
                    reason,
                    ..
                },
        } if command == Command::Clone => Error::DestinationUnsupported {
            board: board.name().to_string(),
            slot,
            chip_type: chip_type.name().to_string(),
            reason,
        },
        error => Error::Build(error),
    }
}

/// Builds `config` for `board` with `firmware` and programs it onto the
/// selected One ROM. `config` has `read_back`'s files inlined.
async fn rebuild(
    options: &mut Options,
    command: Command,
    read_back: &ReadBack,
    config: Config,
    board: Board,
    (firmware, version): (Vec<u8>, FirmwareVersion),
    args: &RebuildArgs,
) -> Result<(), Error> {
    let device = options.device.as_ref().ok_or(Error::NoDevice)?;
    let size = device.flash_chips().size();
    let installed = device.onerom().and_then(ParsedDevice::version);
    let (checked, desc) = build_image(
        options,
        command,
        &config,
        board,
        (firmware, version),
        size,
        args.force,
    )
    .await?;
    if options.verbose && !desc.is_empty() {
        println!("ROM configuration:\n---\n{desc}\n---");
    }
    if let Some(line) = firmware_change(installed, version) {
        println!("{line}");
    }
    for line in plugin_changes(&read_back.plugins, &published_plugins(&checked.device)) {
        println!("{line}");
    }

    let (data, image) = accept_image(options, checked, args.force, Some(board))?;
    flash_device(options, &data, image.get_board(), args.force, false).await?;
    reboot_and_rescan(options, &RebootArgs::running(false, false)).await?;
    println!("Programming complete");
    Ok(())
}

/// The image built from `config` and the description of its ROMs.
pub(crate) async fn build_image(
    options: &Options,
    command: Command,
    config: &Config,
    board: Board,
    (firmware, version): (Vec<u8>, FirmwareVersion),
    size: BoardSize,
    force: bool,
) -> Result<(CheckedImage, String), Error> {
    let json = config_json(config)?;
    let (fw_props, metadata, roms, desc) = build_rom_image(
        options,
        &json,
        version,
        board,
        Variant::RP2350,
        image_size(version, size),
        force,
        &onerom_cli::CliFetch,
        |_| Ok(()),
    )
    .await
    .map_err(|e| build_error(command, board, e))?;
    let checked = assemble_image(&fw_props, firmware, metadata, roms).await?;
    Ok((checked, desc))
}

fn config_json(config: &Config) -> Result<String, Error> {
    serde_json::to_string(config)
        .map_err(|e| Error::Other(format!("Failed to serialize the config: {e}")))
}

/// The line for a rebuild that moves the One ROM from firmware `before` to
/// another release, `after`.
pub(crate) fn firmware_change(
    before: Option<FirmwareVersion>,
    after: FirmwareVersion,
) -> Option<String> {
    let release = |version: FirmwareVersion| (version.major(), version.minor(), version.patch());
    let before = before.filter(|&before| release(before) != release(after))?;
    Some(format!("Updating firmware from v{before} to v{after}"))
}

/// A line for each published plugin the rebuild moves to another release.
pub(crate) fn plugin_changes(before: &[RecordedPlugin], after: &[RecordedPlugin]) -> Vec<String> {
    after
        .iter()
        .filter_map(|new| {
            let old = before
                .iter()
                .find(|old| old.plugin_type == new.plugin_type && old.name == new.name)?;
            (old.version != new.version).then(|| {
                format!(
                    "Updating plugin {} from v{} to v{}",
                    new.name, old.version, new.version
                )
            })
        })
        .collect()
}

/// The error `rebuild` returns for `config`.
#[cfg(test)]
pub(crate) async fn build_failure(
    command: Command,
    config: &Config,
    version: FirmwareVersion,
    board: Board,
) -> Error {
    let options = Options {
        verbose: false,
        log_level: onerom_cli::LogLevel::Warn,
        yes: false,
        unrecognised: false,
        device: None,
        vid_pid: Vec::new(),
    };
    let json = config_json(config).unwrap();
    let result = build_rom_image(
        &options,
        &json,
        version,
        board,
        Variant::RP2350,
        onerom_app::BoardSize::M,
        false,
        &onerom_cli::CliFetch,
        |_| Ok(()),
    )
    .await;
    build_error(command, board, result.err().unwrap())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use onerom_app::{BoardSize, read_back_file};
    use onerom_cli::LogLevel;
    use onerom_cli::image::parse_firmware;
    use onerom_cli::plugin::{PluginNote, PluginVersion, ResolvedSource, check_config_plugins};
    use onerom_config::fw::{FirmwareProperties, ServeAlg};
    use onerom_config::mcu::Family;
    use onerom_gen::{Builder, FIRMWARE_SIZE, FileData, INLINE_BASE64};
    use serde_json::json;

    use super::*;
    use crate::args::save::SaveArgs;
    use crate::save::save;
    use crate::test_board::{
        Files, HOST_CONTROL_URL, IMAGE_2364, NoFetch, USB_URL, base_firmware, config_2364,
        holds_in_order, image_2364, image_2364_sets, image_with_plugins, image_with_usb,
        published_plugin, slot_range,
    };

    const VERSION: FirmwareVersion = FirmwareVersion::new(0, 8, 0, 0);

    fn options() -> Options {
        Options {
            verbose: false,
            log_level: LogLevel::Warn,
            yes: false,
            unrecognised: false,
            device: None,
            vid_pid: Vec::new(),
        }
    }

    async fn read_back() -> ReadBack {
        let image = image_2364_sets(Board::Fire24F, &["single", "single", "banked"], &[]).await;
        read_back_file(&image).await.unwrap()
    }

    /// The bytes of chip `chip` of ROM slot `slot`.
    fn rom(read_back: &ReadBack, slot: usize, chip: usize) -> &[u8] {
        let file = read_back
            .files
            .iter()
            .find(|file| file.slot == Some(slot) && file.chip == chip)
            .unwrap();
        &file.data
    }

    /// The read-back of the image `rebuild` builds from `config` for a
    /// fire-24-f.
    async fn rebuilt(config: Config) -> ReadBack {
        read_back_file(&rebuilt_image(config).await).await.unwrap()
    }

    /// The image `rebuild` builds from `config` for a fire-24-f.
    async fn rebuilt_image(config: Config) -> Vec<u8> {
        let json = config_json(&config).unwrap();
        let (props, metadata, roms, _) = build_rom_image(
            &options(),
            &json,
            VERSION,
            Board::Fire24F,
            Variant::RP2350,
            BoardSize::M,
            false,
            &NoFetch,
            |_| Ok(()),
        )
        .await
        .unwrap();
        assemble_image(&props, base_firmware(8), metadata, roms)
            .await
            .unwrap()
            .image
    }

    async fn standby_read_back() -> ReadBack {
        let mut read_back = read_back().await;
        read_back.config.chip_sets[0].firmware_overrides =
            serde_json::from_str(r#"{ "fire": { "standby": true } }"#).unwrap();
        read_back
    }

    #[tokio::test]
    async fn a_setting_older_firmware_doesnt_support_is_named() {
        let config = inline_files(&standby_read_back().await);
        let old = FirmwareVersion::new(0, 7, 3, 0);
        let e = build_failure(Command::Update, &config, old, Board::Fire24F).await;
        assert!(
            matches!(
                &e,
                Error::FirmwareTooOldFor { feature, version, minimum }
                    if feature == "standby" && *version == old && *minimum == VERSION
            ),
            "{e:?}"
        );
    }

    /// An image smaller than the chip it replaces fails with its size and the
    /// chip's.
    #[tokio::test]
    async fn an_undersized_image_fails_with_its_size() {
        let dir = tempfile::tempdir().unwrap();
        let kernal = dir.path().join("kernal.bin");
        std::fs::write(&kernal, [0xa5; IMAGE_2364 / 2]).unwrap();
        let kernal = kernal.to_string_lossy();
        let config = replace_rom(&read_back().await, 1, None, &kernal).unwrap();

        let e = build_failure(Command::Update, &config, VERSION, Board::Fire24F).await;
        assert!(
            matches!(
                &e,
                Error::ImageTooSmall { file, chip_type, expected, actual }
                    if *file == kernal && chip_type == "2364" && *expected == IMAGE_2364 && *actual == IMAGE_2364 / 2
            ),
            "{e:?}"
        );
    }

    /// A fire-32-b doesn't support a 2364. A clone onto one fails identifying
    /// the first slot.
    #[tokio::test]
    async fn a_slot_the_destination_doesnt_support_is_named() {
        let config = inline_files(&read_back().await);
        let e = build_failure(Command::Clone, &config, VERSION, Board::Fire32B).await;
        assert!(
            matches!(
                &e,
                Error::DestinationUnsupported { board, slot: 0, chip_type, reason: None }
                    if board == "fire-32-b" && chip_type == "2364"
            ),
            "{e:?}"
        );
    }

    /// An existing path is a file. A chip ID, a pattern or a connected One
    /// ROM's serial number is a serial number. Anything else is a file.
    #[tokio::test]
    async fn a_source_is_a_file_or_a_serial_number() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("E126C9F97C10ADAC");
        std::fs::write(&existing, "").unwrap();
        let existing = existing.to_string_lossy().into_owned();
        let connected = || async || vec!["ACME0001".to_string()];
        for (from, serial) in [
            (existing.as_str(), false),
            ("E126C9F97C10ADAC", true),
            ("e126c9f97c10adac", true),
            ("E126*", true),
            ("ACME000?", true),
            ("acme0001", true),
            ("nofile.bin", false),
            ("ACME0002", false),
            ("E126C9F97C10ADA", false),
        ] {
            assert_eq!(is_serial(from, connected()).await, serial, "{from}");
        }
    }

    /// A source file that doesn't exist fails with the input/output error, and
    /// one with newer firmware than this build supports fails with its name
    /// and version.
    #[tokio::test]
    async fn a_source_file_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let nofile = dir.path().join("nofile.bin").to_string_lossy().into_owned();
        let e = read_source(&options(), &nofile, async || Vec::new())
            .await
            .unwrap_err();
        assert!(
            matches!(&e, Error::Io(text) if text.starts_with(&nofile)),
            "{e:?}"
        );

        let new = dir.path().join("new.bin");
        std::fs::write(&new, base_firmware(9)).unwrap();
        let new = new.to_string_lossy().into_owned();
        let e = read_source(&options(), &new, async || Vec::new())
            .await
            .unwrap_err();
        let newer = FirmwareVersion::new(0, 9, 0, 0);
        assert!(
            matches!(&e, Error::FirmwareTooNew { what, version } if *what == new && *version == newer),
            "{e:?}"
        );
    }

    /// A clone's source with firmware newer than this build supports is the
    /// subject of the error.
    #[test]
    fn a_newer_source_is_the_subject() {
        let version = FirmwareVersion::new(0, 8, 2, 0);
        let e = source_error(Error::FirmwareTooNew {
            what: "The One ROM".to_string(),
            version,
        });
        assert!(
            matches!(&e, Error::FirmwareTooNew { what, version: v } if what == "The source" && *v == version),
            "{e:?}"
        );
        let e = source_error(Error::NoDevice);
        assert!(matches!(e, Error::NoDevice), "{e:?}");
    }

    /// A setting that isn't recognised fails, identifying the setting.
    #[tokio::test]
    async fn a_setting_that_isnt_recognised_fails() {
        let mut read_back = read_back().await;
        assert!(check_recovered(&read_back).is_ok());
        read_back
            .problems
            .push(onerom_app::RecoveryProblem::ChipSelect { set: 2 });
        let subject = read_back.problem_subject(&read_back.problems[0]);
        let e = check_recovered(&read_back).unwrap_err();
        assert!(
            matches!(&e, Error::NotRecognised(text) if text.starts_with(&subject)),
            "{e:?}"
        );
    }

    #[tokio::test]
    async fn a_typed_plugin_that_isnt_published_is_reported() {
        let mut config = config_of(&["system", "rom"]);
        let e = add_plugin(&mut config, "user/usb", &VERSION, &Files(Vec::new()))
            .await
            .unwrap_err();
        assert!(
            matches!(
                &e,
                Error::PluginNotPublished { plugin_type: PluginType::User, name } if name == "usb"
            ),
            "{e:?}"
        );
    }

    #[tokio::test]
    async fn a_rom_is_replaced_and_the_others_kept() {
        let read_back = read_back().await;
        let dir = tempfile::tempdir().unwrap();
        let kernal = dir.path().join("kernal.bin");
        std::fs::write(&kernal, [0xa5; IMAGE_2364]).unwrap();
        let kernal = kernal.to_string_lossy();

        let config = replace_rom(&read_back, 1, None, &kernal).unwrap();
        let after = rebuilt(config).await;

        assert_eq!(rom(&after, 1, 0), [0xa5; IMAGE_2364]);
        for (slot, chip, byte) in [(0, 0, 0), (2, 0, 2), (2, 1, 3)] {
            assert_eq!(rom(&after, slot, chip), [byte; IMAGE_2364], "{slot} {chip}");
        }
        let (old, new) = (
            &read_back.config.chip_sets[1].chips[0],
            &after.config.chip_sets[1].chips[0],
        );
        assert_eq!(new.chip_type.resolved(), old.chip_type.resolved());
        assert_eq!(new.cs1, old.cs1);
        assert_eq!(new.label.as_deref(), Some(&*kernal));
    }

    /// A saved ROM edited and passed back under its saved name replaces its
    /// slot. That name is also a read-back file's name.
    #[tokio::test]
    async fn an_edited_saved_rom_replaces_its_slot() {
        let image = image_2364_sets(Board::Fire24F, &["single", "single", "banked"], &[]).await;
        let dir = tempfile::tempdir().unwrap();
        let firmware = dir.path().join("image.bin");
        std::fs::write(&firmware, &image).unwrap();
        let path = dir.path().join("c64");
        let args = SaveArgs {
            slot: None,
            chip: None,
            output: None,
            path: Some(path.to_string_lossy().into_owned()),
            firmware: Some(firmware.to_string_lossy().into_owned()),
            force: false,
        };
        save(&options(), &args).await.unwrap();
        let read_back = read_back_file(&image).await.unwrap();
        let name = read_back.files[0].name.clone();
        std::fs::write(path.join(&name), [0xa5; IMAGE_2364]).unwrap();

        // `onerom update slot --slot 0 --image <name>` run in c64. The build
        // reads a relative path from the directory it runs in.
        let mut config = replace_rom(&read_back, 0, None, &name).unwrap();
        for chip in config
            .chip_sets
            .iter_mut()
            .flat_map(|set| set.chips.iter_mut())
        {
            if chip.file == name {
                chip.file = path.join(&name).to_string_lossy().into_owned();
            }
        }
        let after = rebuilt(config).await;

        assert_eq!(rom(&after, 0, 0), [0xa5; IMAGE_2364]);
        assert_eq!(rom(&after, 1, 0), [1; IMAGE_2364]);
    }

    /// A save, an update with an edited saved file and another save write the
    /// same file names and labels.
    #[tokio::test]
    async fn a_saved_rom_keeps_its_name_and_label() {
        let before = read_back().await;
        let names = |read_back: &ReadBack| -> Vec<String> {
            read_back
                .files
                .iter()
                .map(|file| file.name.clone())
                .collect()
        };
        let labels = |read_back: &ReadBack| -> Vec<Option<String>> {
            read_back
                .config
                .chip_sets
                .iter()
                .flat_map(|set| &set.chips)
                .map(|chip| chip.label.clone())
                .collect()
        };
        let dir = tempfile::tempdir().unwrap();
        for (slot, chip, index) in [(1, None, 0), (2, Some(1), 1)] {
            let file = before
                .files
                .iter()
                .find(|file| file.slot == Some(slot.into()) && file.chip == index)
                .unwrap();
            let saved = dir.path().join(&file.name);
            std::fs::write(&saved, [0xa5; IMAGE_2364]).unwrap();

            let config = replace_rom(&before, slot, chip, &saved.to_string_lossy()).unwrap();
            let after = rebuilt(config).await;

            assert_eq!(rom(&after, slot.into(), index), [0xa5; IMAGE_2364]);
            assert_eq!(names(&after), names(&before));
            assert_eq!(labels(&after), labels(&before));
        }
    }

    #[tokio::test]
    async fn a_chip_of_a_slot_holding_several_is_replaced() {
        let read_back = read_back().await;
        let config = replace_rom(&read_back, 2, Some(1), "char.bin").unwrap();

        let inlined = inline_slots(&read_back);
        let (old, new) = (&inlined.chip_sets[2].chips, &config.chip_sets[2].chips);
        assert_eq!(new[1].file, "char.bin");
        assert_eq!(new[1].label, None);
        assert_eq!(new[0].file, old[0].file);
        assert_eq!(new[0].label, old[0].label);
    }

    #[tokio::test]
    async fn a_slot_or_chip_it_doesnt_have_isnt_replaced() {
        let read_back = read_back().await;
        let error = |slot, chip| replace_rom(&read_back, slot, chip, "a.bin").unwrap_err();

        let e = error(3, None);
        assert!(matches!(e, Error::NoSlot { slot: 3, slots: 3 }), "{e:?}");
        let e = error(2, None);
        assert!(
            matches!(e, Error::ChipNeeded { slot: 2, chips: 2 }),
            "{e:?}"
        );
        let e = error(0, Some(1));
        assert!(
            matches!(
                e,
                Error::NoChip {
                    slot: 0,
                    chip: 1,
                    chips: 1
                }
            ),
            "{e:?}"
        );
        let e = error(2, Some(2));
        assert!(
            matches!(
                e,
                Error::NoChip {
                    slot: 2,
                    chip: 2,
                    chips: 2
                }
            ),
            "{e:?}"
        );
    }

    /// A config of a chip set for each of `sets`. `system` and `user` are
    /// plugins from the file of that name and `rom` is a 2364.
    fn config_of(sets: &[&str]) -> Config {
        let sets: Vec<String> = sets
            .iter()
            .map(|&set| {
                let chip = match set {
                    "rom" => {
                        r#"{ "file": "rom.bin", "type": "2364", "cs1": "active_low" }"#.to_string()
                    }
                    plugin => format!(r#"{{ "file": "{plugin}.bin", "type": "{plugin}_plugin" }}"#),
                };
                format!(r#"{{ "type": "single", "chips": [{chip}] }}"#)
            })
            .collect();
        let json = format!(
            r#"{{ "version": 1, "description": "Test", "chip_sets": [{}] }}"#,
            sets.join(", ")
        );
        serde_json::from_str(&json).unwrap()
    }

    /// A plugin of `plugin_type` from the file `file`.
    fn plugin(plugin_type: PluginType, file: &str) -> ResolvedPlugin {
        ResolvedPlugin {
            plugin_type,
            name: file.to_string(),
            version: PluginVersion::new(0, 1, 0, 0),
            size: 1024,
            source: ResolvedSource::File {
                path: file.to_string(),
            },
        }
    }

    /// Each chip set's file.
    fn files(config: &Config) -> Vec<&str> {
        config
            .chip_sets
            .iter()
            .map(|set| set.chips[0].file.as_str())
            .collect()
    }

    #[test]
    fn a_plugin_is_added_ahead_of_the_roms() {
        let mut config = config_of(&["rom", "rom"]);
        place_plugin(&mut config, &plugin(PluginType::System, "usb.bin")).unwrap();
        assert_eq!(files(&config), ["usb.bin", "rom.bin", "rom.bin"]);
        assert_eq!(
            set_plugin_type(&config.chip_sets[0]),
            Some(PluginType::System)
        );

        let mut config = config_of(&["system", "rom"]);
        place_plugin(&mut config, &plugin(PluginType::User, "blink.bin")).unwrap();
        assert_eq!(files(&config), ["system.bin", "blink.bin", "rom.bin"]);
        assert_eq!(
            set_plugin_type(&config.chip_sets[1]),
            Some(PluginType::User)
        );
    }

    #[test]
    fn a_plugin_replaces_the_plugin_of_its_type() {
        let mut config = config_of(&["system", "user", "rom"]);
        place_plugin(&mut config, &plugin(PluginType::System, "usb.bin")).unwrap();
        assert_eq!(files(&config), ["usb.bin", "user.bin", "rom.bin"]);
        place_plugin(&mut config, &plugin(PluginType::User, "blink.bin")).unwrap();
        assert_eq!(files(&config), ["usb.bin", "blink.bin", "rom.bin"]);
    }

    #[test]
    fn a_user_plugin_requires_a_system_plugin() {
        let mut config = config_of(&["rom"]);
        let e = place_plugin(&mut config, &plugin(PluginType::User, "blink.bin")).unwrap_err();
        assert!(matches!(e, Error::UserPluginWithoutSystem), "{e:?}");
    }

    #[tokio::test]
    async fn a_typed_user_plugin_is_added_after_the_system_plugin() {
        let (url, served) = published_plugin(PluginType::User, "host-control");
        let mut config = config_of(&["system", "rom"]);
        add_plugin(&mut config, "user/host-control", &VERSION, &served)
            .await
            .unwrap();
        assert_eq!(files(&config), ["system.bin", url.as_str(), "rom.bin"]);
    }

    /// `update plugin`'s help example. The plugin's type is read from
    /// `plugins.json`.
    #[tokio::test]
    async fn an_untyped_user_plugin_is_added_after_the_system_plugin() {
        let (url, Files(mut served)) = published_plugin(PluginType::User, "host-control");
        let catalogue = json!({
            "version": 1,
            "plugins": [
                { "name": "usb", "type": "system_plugin", "path": "system/usb" },
                { "name": "host-control", "type": "user_plugin", "path": "user/host-control" }
            ]
        });
        served.push((
            "https://images.onerom.org/plugins/plugins.json".to_string(),
            catalogue.to_string().into_bytes(),
        ));
        let mut config = config_of(&["system", "rom"]);
        add_plugin(&mut config, "host-control", &VERSION, &Files(served))
            .await
            .unwrap();
        assert_eq!(files(&config), ["system.bin", url.as_str(), "rom.bin"]);
    }

    /// The `--plugin` help's example of a type.
    #[tokio::test]
    async fn a_typed_system_plugin_replaces_the_system_plugin() {
        let (url, served) = published_plugin(PluginType::System, "usb");
        let mut config = config_of(&["system", "user", "rom"]);
        add_plugin(&mut config, "system/usb", &VERSION, &served)
            .await
            .unwrap();
        assert_eq!(files(&config), [url.as_str(), "user.bin", "rom.bin"]);
    }

    /// A fire-24-f image of one 2364 with `name` as its instance name and
    /// `serial` as its USB serial override.
    async fn named_image(name: &str, serial: Option<&str>) -> Vec<u8> {
        let json = config_2364(&["single"], Path::new("rom"), &[]);
        let mut config: Config = serde_json::from_str(&json).unwrap();
        config.instance_name = Some(name.to_string());
        config.serial_override = serial.map(str::to_string);
        let json = serde_json::to_string(&config).unwrap();
        let mut builder = Builder::from_json(VERSION, Family::Rp2350, &json).unwrap();
        builder
            .add_file(FileData::new(0, vec![0; IMAGE_2364]))
            .unwrap();
        let props = FirmwareProperties::new(
            VERSION,
            Board::Fire24F,
            Variant::RP2350,
            ServeAlg::Default,
            false,
        )
        .unwrap();
        let (metadata, roms) = builder.build(props).unwrap();
        assemble_image(&props, base_firmware(8), metadata, roms)
            .await
            .unwrap()
            .image
    }

    #[tokio::test]
    async fn a_copy_keeps_the_targets_name_and_serial() {
        let source = read_back_file(&named_image("Acme 64", Some("ACME0001")).await)
            .await
            .unwrap();
        assert_eq!(source.config.instance_name.as_deref(), Some("Acme 64"));
        assert_eq!(source.config.serial_override.as_deref(), Some("ACME0001"));
        let target = parse_firmware(&named_image("Spare", None).await).await;

        let config = clone_config(&source, Some(&target));
        assert_eq!(config.instance_name.as_deref(), Some("Spare"));
        assert_eq!(config.serial_override, None);
        let mut expected = inline_slots(&source);
        expected.instance_name = Some("Spare".to_string());
        expected.serial_override = None;
        assert_eq!(
            serde_json::to_value(&config).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );

        // Neither base firmware nor a board without firmware has a name or
        // serial.
        let base = parse_firmware(&base_firmware(8)).await;
        for target in [Some(&base), None] {
            let config = clone_config(&source, target);
            assert_eq!(config.instance_name, None);
            assert_eq!(config.serial_override, None);
        }
    }

    #[test]
    fn a_firmware_change_is_reported() {
        let release = |patch| FirmwareVersion::new(0, 8, patch, 0);
        let line = firmware_change(Some(release(1)), release(0)).unwrap();
        assert!(holds_in_order(&line, &["v0.8.1", "v0.8.0"]), "{line}");
        // Nothing where the release stays the same or the One ROM doesn't
        // have firmware.
        assert_eq!(firmware_change(Some(release(0)), release(0)), None);
        let build = FirmwareVersion::new(0, 8, 0, 3);
        assert_eq!(firmware_change(Some(build), release(0)), None);
        assert_eq!(firmware_change(None, release(0)), None);
    }

    #[tokio::test]
    async fn a_plugin_moving_to_another_release_is_reported() {
        let plugins = async |image: Vec<u8>| published_plugins(&parse_firmware(&image).await);
        let old = plugins(image_with_usb("0.3.1").await).await;
        let new = plugins(image_with_usb("0.3.2").await).await;
        let none = plugins(image_2364(1, &[]).await).await;
        assert_eq!(old.len(), 1);

        let lines = plugin_changes(&old, &new);
        assert_eq!(lines.len(), 1);
        assert!(
            holds_in_order(&lines[0], &["usb", "v0.3.1", "v0.3.2"]),
            "{}",
            lines[0]
        );
        // Nothing for a plugin that stays on its release or is added.
        assert!(plugin_changes(&new, &new).is_empty());
        assert!(plugin_changes(&none, &new).is_empty());
    }

    /// v0.8.0's base firmware header with code that isn't in any release.
    fn unreleased_base_firmware() -> Vec<u8> {
        let mut firmware = base_firmware(8);
        firmware[0x1000..0x1100].fill(0xa5);
        firmware
    }

    /// A fire-24-f image of two single 2364 sets built with
    /// [`unreleased_base_firmware`].
    async fn unreleased_image() -> Vec<u8> {
        let json = config_2364(&["single", "single"], Path::new("rom"), &[]);
        let mut builder = Builder::from_json(VERSION, Family::Rp2350, &json).unwrap();
        for n in 0..2 {
            builder
                .add_file(FileData::new(n, vec![n as u8; IMAGE_2364]))
                .unwrap();
        }
        let props = FirmwareProperties::new(
            VERSION,
            Board::Fire24F,
            Variant::RP2350,
            ServeAlg::Default,
            false,
        )
        .unwrap();
        let (metadata, roms) = builder.build(props).unwrap();
        assemble_image(&props, unreleased_base_firmware(), metadata, roms)
            .await
            .unwrap()
            .image
    }

    /// `update slot` builds with the One ROM's own base firmware, which isn't
    /// a release.
    #[tokio::test]
    async fn a_slot_is_updated_with_the_one_roms_base_firmware() {
        let image = unreleased_image().await;
        let read_back = read_back_file(&image).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let kernal = dir.path().join("kernal.bin");
        std::fs::write(&kernal, [0x5a; IMAGE_2364]).unwrap();
        let config = replace_rom(&read_back, 1, None, &kernal.to_string_lossy()).unwrap();

        let (checked, _) = build_image(
            &options(),
            Command::Update,
            &config,
            read_back.board,
            reused_firmware(&read_back),
            BoardSize::M,
            false,
        )
        .await
        .unwrap();
        assert!(checked.problems.is_empty(), "{:?}", checked.problems);
        assert!(checked.image[..FIRMWARE_SIZE] == image[..FIRMWARE_SIZE]);
        let after = read_back_file(&checked.image).await.unwrap();
        assert_eq!(rom(&after, 0, 0), [0; IMAGE_2364]);
        assert_eq!(rom(&after, 1, 0), [0x5a; IMAGE_2364]);
    }

    /// `clone` from a file builds with the file's base firmware, onto another
    /// board.
    #[tokio::test]
    async fn a_clone_from_a_file_uses_its_base_firmware() {
        let image = unreleased_image().await;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.bin");
        std::fs::write(&source, &image).unwrap();
        let (read_back, chip_id) =
            read_source(&options(), &source.to_string_lossy(), async || Vec::new())
                .await
                .unwrap();
        assert_eq!(chip_id, None);

        let (checked, _) = build_image(
            &options(),
            Command::Clone,
            &clone_config(&read_back, None),
            Board::Fire24E,
            reused_firmware(&read_back),
            BoardSize::M,
            false,
        )
        .await
        .unwrap();
        assert!(checked.problems.is_empty(), "{:?}", checked.problems);
        assert!(checked.image[..FIRMWARE_SIZE] == image[..FIRMWARE_SIZE]);
        let after = read_back_file(&checked.image).await.unwrap();
        assert_eq!(after.board, Board::Fire24E);
        assert_eq!(rom(&after, 1, 0), [1; IMAGE_2364]);
    }

    async fn same_slot(before: &[u8], after: &[u8], slot: usize) -> bool {
        before[slot_range(before, slot).await] == after[slot_range(after, slot).await]
    }

    /// `update slot` leaves each published plugin's slot as read, its stored
    /// data included, without fetching anything. The rebuilt image records the
    /// plugins as the same releases.
    #[tokio::test]
    async fn a_slot_update_keeps_each_plugin_slot() {
        let image = image_with_plugins(&[USB_URL, HOST_CONTROL_URL]).await;
        let read_back = read_back_file(&image).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let kernal = dir.path().join("kernal.bin");
        std::fs::write(&kernal, [0x5a; IMAGE_2364]).unwrap();

        let config = replace_rom(&read_back, 0, None, &kernal.to_string_lossy()).unwrap();
        let after = rebuilt_image(config).await;

        for slot in [0, 1] {
            assert!(same_slot(&image, &after, slot).await, "{slot}");
        }
        let after = read_back_file(&after).await.unwrap();
        assert_eq!(after.plugins, read_back.plugins);
        assert_eq!(rom(&after, 0, 0), [0x5a; IMAGE_2364]);
    }

    /// `update plugin` replaces one plugin and leaves the other's slot as
    /// read.
    #[tokio::test]
    async fn a_plugin_update_keeps_the_other_plugins_slot() {
        let image = image_with_plugins(&[USB_URL, HOST_CONTROL_URL]).await;
        let read_back = read_back_file(&image).await.unwrap();
        for (replaced, kept, plugin_type) in [(0, 1, "system"), (1, 0, "user")] {
            let stub = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../images/test/stub-{plugin_type}-plugin.bin"))
                .to_string_lossy()
                .into_owned();
            let bin = std::fs::read(&stub).unwrap();
            let served = Files(vec![(stub.clone(), bin.clone())]);

            let spec = format!("file={stub}");
            let config = plugin_config(&read_back, &spec, &VERSION, &served)
                .await
                .unwrap();
            let after = rebuilt_image(config).await;

            assert!(same_slot(&image, &after, kept).await, "{plugin_type}");
            assert!(!same_slot(&image, &after, replaced).await, "{plugin_type}");
            assert!(after[slot_range(&after, replaced).await].starts_with(&bin));
        }
    }

    /// `update firmware` fetches a published plugin from its release for the
    /// new firmware and leaves an unpublished plugin's slot as read.
    #[tokio::test]
    async fn a_firmware_update_takes_a_published_plugin_from_its_release() {
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD as BASE64;

        let image = image_with_plugins(&[USB_URL, "blink.bin"]).await;
        let read_back = read_back_file(&image).await.unwrap();
        let (url, Files(mut served)) = published_plugin(PluginType::System, "usb");
        let catalogue = json!({
            "version": 1,
            "plugins": [{ "name": "usb", "type": "system_plugin", "path": "system/usb" }]
        });
        served.push((
            "https://images.onerom.org/plugins/plugins.json".to_string(),
            catalogue.to_string().into_bytes(),
        ));

        // `cmd_firmware` builds from `inline_files`'s config.
        let json = config_json(&inline_files(&read_back)).unwrap();
        let new = FirmwareVersion::new(0, 8, 1, 0);
        let mut builder = Builder::from_json(new, Family::Rp2350, &json).unwrap();
        onerom_cli::plugin::resolve_config_plugins(&mut builder, &new, &Files(served))
            .await
            .unwrap();

        let file = |set: usize| builder.config().chip_sets[set].chips[0].file.clone();
        assert_eq!(file(0), url);
        let blink = BASE64
            .decode(file(1).strip_prefix("base64:").unwrap())
            .unwrap();
        assert!(blink == image[slot_range(&image, 1).await]);
    }

    /// A reused published plugin's slot is official, so it doesn't produce a
    /// note, and nothing is fetched. A reused unpublished plugin's slot is
    /// reported by its label rather than by its inlined bytes.
    #[tokio::test]
    async fn a_reused_plugin_slot_is_reported_by_its_label() {
        let image = image_with_plugins(&[USB_URL, "activity.bin"]).await;
        let read_back = read_back_file(&image).await.unwrap();
        let json = config_json(&inline_slots(&read_back)).unwrap();
        let builder = Builder::from_json(VERSION, Family::Rp2350, &json).unwrap();

        let notes = check_config_plugins(&builder, &VERSION, &NoFetch)
            .await
            .unwrap();
        assert!(
            matches!(&notes[..], [PluginNote::Unofficial { source }] if source == "activity.bin"),
            "{notes:?}"
        );
    }

    /// A clone's ROM configuration contains each reused slot's label, never
    /// its inlined bytes.
    #[tokio::test]
    async fn a_reused_slot_is_described_by_its_label() {
        let image = image_with_plugins(&[USB_URL, "activity.bin"]).await;
        let read_back = read_back_file(&image).await.unwrap();
        let config = clone_config(&read_back, None);
        let json = config_json(&config).unwrap();
        let (_, _, _, desc) = build_rom_image(
            &options(),
            &json,
            VERSION,
            Board::Fire24F,
            Variant::RP2350,
            BoardSize::M,
            false,
            &NoFetch,
            |_| Ok(()),
        )
        .await
        .unwrap();

        for chip in config.chip_sets.iter().flat_map(|set| &set.chips) {
            let data = chip.file.strip_prefix(INLINE_BASE64).unwrap();
            assert!(!desc.contains(&data[..64]), "{desc}");
            assert!(desc.contains(chip.label.as_deref().unwrap()), "{desc}");
        }
    }

    /// `clone` from a file copies every plugin slot as read, published or
    /// not.
    #[tokio::test]
    async fn a_clone_copies_every_plugin_slot() {
        let image = image_with_plugins(&[USB_URL, "blink.bin"]).await;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.bin");
        std::fs::write(&source, &image).unwrap();
        let (read_back, _) =
            read_source(&options(), &source.to_string_lossy(), async || Vec::new())
                .await
                .unwrap();

        let after = rebuilt_image(clone_config(&read_back, None)).await;

        for slot in [0, 1] {
            assert!(same_slot(&image, &after, slot).await, "{slot}");
        }
    }
}
