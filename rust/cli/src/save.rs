// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Implementation of `onerom save`.

use std::path::Path;

use onerom_app::ReadBack;
use onerom_cli::read_back::{problem_error, read_back, read_back_error, read_slot_roms};
use onerom_cli::{Error, Options};

use crate::args::save::SaveArgs;
use crate::utils::check_device;

pub async fn cmd_save(options: &Options, args: &SaveArgs) -> Result<(), Error> {
    for line in save(options, args).await? {
        println!("{line}");
    }
    Ok(())
}

/// Runs `onerom save` with `args`. Returns the lines reporting what was saved.
pub(crate) async fn save(options: &Options, args: &SaveArgs) -> Result<Vec<String>, Error> {
    check_device(options, args, false)?;
    if let (Some(slot), Some(output)) = (args.slot, &args.output) {
        if !args.force && Path::new(output).exists() {
            return Err(Error::OutputExists(output.clone()));
        }
        let roms = match &args.firmware {
            Some(file) => onerom_app::read_slot_roms_file(&read_file(file)?, slot.into())
                .await
                .map_err(|e| read_back_error(file, e))?,
            None => {
                let device = options.device.as_ref().ok_or(Error::NoDevice)?;
                read_slot_roms(device, slot.into()).await?
            }
        };
        let rom = chip(&roms, slot, args.chip)?;
        std::fs::write(output, rom).map_err(|e| Error::io(output, e))?;
        Ok(vec![match args.chip {
            Some(chip) => format!("Saved slot {slot} chip {chip} to {output}"),
            None => format!("Saved slot {slot} to {output}"),
        }])
    } else if let Some(path) = &args.path {
        let read_back = match &args.firmware {
            Some(file) => onerom_app::read_back_file(&read_file(file)?)
                .await
                .map_err(|e| read_back_error(file, e))?,
            None => read_back(options.device.as_ref().ok_or(Error::NoDevice)?).await?,
        };
        save_read_back(read_back, path, args.force)
    } else {
        Ok(Vec::new())
    }
}

fn read_file(file: &str) -> Result<Vec<u8>, Error> {
    std::fs::read(file).map_err(|e| Error::io(file, e))
}

fn chip(roms: &[Vec<u8>], slot: u8, chip: Option<u8>) -> Result<&[u8], Error> {
    Ok(&roms[chip_index(slot, roms.len(), chip)?])
}

/// The index of chip `chip` of slot `slot`, which holds `chips` chips.
/// Without `chip`, the slot's only chip.
pub(crate) fn chip_index(slot: u8, chips: usize, chip: Option<u8>) -> Result<usize, Error> {
    match chip {
        None if chips == 1 => Ok(0),
        None => Err(Error::ChipNeeded { slot, chips }),
        Some(chip) if usize::from(chip) < chips => Ok(usize::from(chip)),
        Some(chip) => Err(Error::NoChip { slot, chip, chips }),
    }
}

/// Writes `read_back`'s config and a file for each ROM to `dir`, which is
/// created where it doesn't exist. Returns the lines reporting what was
/// written.
///
/// Without `force`, fails before writing anything where one of the files
/// exists.
///
/// The config's file paths are `dir` joined with each file's name, which
/// `program --config` reads relative to the directory it runs in.
pub(crate) fn save_read_back(
    mut read_back: ReadBack,
    dir: &str,
    force: bool,
) -> Result<Vec<String>, Error> {
    if Path::new(dir).exists() && !Path::new(dir).is_dir() {
        return Err(Error::NotADirectory(dir.to_string()));
    }
    let path_of = |name: &str| Path::new(dir).join(name).to_string_lossy().into_owned();
    let config = path_of("config.json");
    if !force
        && let Some(existing) = read_back
            .files
            .iter()
            .map(|file| path_of(&file.name))
            .chain([config.clone()])
            .find(|path| Path::new(path).exists())
    {
        return Err(Error::OutputExists(existing));
    }
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;

    let mut lines = Vec::new();
    for chip_set in &mut read_back.config.chip_sets {
        for chip in &mut chip_set.chips {
            if !chip.file.is_empty() {
                chip.file = path_of(&chip.file);
            }
        }
    }
    for file in &read_back.files {
        let path = path_of(&file.name);
        std::fs::write(&path, &file.data).map_err(|e| Error::io(&path, e))?;
        lines.push(format!("Saved {path}"));
    }

    let json = serde_json::to_string_pretty(&read_back.config)
        .map_err(|e| Error::Other(format!("Failed to serialize the config: {e}")))?;
    std::fs::write(&config, json + "\n").map_err(|e| Error::io(&config, e))?;
    lines.push(format!("Saved {config}"));

    for problem in &read_back.problems {
        lines.push(format!("Warning: {}", problem_error(&read_back, problem)));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use onerom_app::{BoardSize, RecoveryProblem, read_back_file};
    use onerom_cli::LogLevel;
    use onerom_config::fw::FirmwareVersion;
    use onerom_config::hw::Board;
    use onerom_config::mcu::Variant;
    use onerom_gen::Config;

    use super::*;
    use crate::firmware::build_rom_image;
    use crate::test_board::{
        IMAGE_2364, NoFetch, USB_URL, base_firmware, holds, image_2364_sets, image_with_plugins,
        original_image, shows, slot_range,
    };

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

    async fn image() -> Vec<u8> {
        image_2364_sets(Board::Fire24F, &["single", "single", "banked"], &[]).await
    }

    fn image_file(dir: &Path, image: &[u8]) -> String {
        let file = dir.join("image.bin");
        std::fs::write(&file, image).unwrap();
        file.to_string_lossy().into_owned()
    }

    fn rom_args(firmware: &str, slot: u8, chip: Option<u8>, output: &str) -> SaveArgs {
        SaveArgs {
            slot: Some(slot),
            chip,
            output: Some(output.to_string()),
            path: None,
            firmware: Some(firmware.to_string()),
            force: false,
        }
    }

    fn forced_rom_args(firmware: &str, slot: u8, output: &str) -> SaveArgs {
        SaveArgs {
            force: true,
            ..rom_args(firmware, slot, None, output)
        }
    }

    fn path_args(firmware: &str, path: &str, force: bool) -> SaveArgs {
        SaveArgs {
            slot: None,
            chip: None,
            output: None,
            path: Some(path.to_string()),
            firmware: Some(firmware.to_string()),
            force,
        }
    }

    fn contents(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let data = std::fs::read(&path).unwrap();
                (path, data)
            })
            .collect();
        files.sort();
        files
    }

    /// A save over a file that exists fails and leaves the directory as it
    /// was. With --force, it overwrites the files.
    #[tokio::test]
    async fn an_existing_file_is_overwritten_only_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let firmware = image_file(dir.path(), &image().await);
        let path = dir.path().join("c64");
        std::fs::create_dir(&path).unwrap();
        let config_file = path.join("config.json");
        std::fs::write(&config_file, "{}").unwrap();
        let other = path.join("notes.txt");
        std::fs::write(&other, "notes").unwrap();
        let before = contents(&path);
        let path = path.to_string_lossy().into_owned();

        let e = save(&options(), &path_args(&firmware, &path, false))
            .await
            .unwrap_err();
        let config_file = config_file.to_string_lossy().into_owned();
        assert!(
            matches!(&e, Error::OutputExists(file) if *file == config_file),
            "{e:?}"
        );
        assert_eq!(contents(Path::new(&path)), before);

        save(&options(), &path_args(&firmware, &path, true))
            .await
            .unwrap();
        let json = std::fs::read_to_string(&config_file).unwrap();
        let config: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config.chip_sets.len(), 3);
        assert_eq!(std::fs::read(&other).unwrap(), b"notes");
    }

    /// The error identifies the first existing file in the order the files
    /// are written.
    #[tokio::test]
    async fn the_first_existing_file_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let image = image().await;
        let firmware = image_file(dir.path(), &image);
        let path = dir.path().join("c64").to_string_lossy().into_owned();
        save(&options(), &path_args(&firmware, &path, false))
            .await
            .unwrap();
        let read_back = read_back_file(&image).await.unwrap();
        let first = Path::new(&path)
            .join(&read_back.files[0].name)
            .to_string_lossy()
            .into_owned();

        let e = save(&options(), &path_args(&firmware, &path, false))
            .await
            .unwrap_err();
        assert!(
            matches!(&e, Error::OutputExists(file) if *file == first),
            "{e:?}"
        );
    }

    /// A --path that's a file fails and leaves the file as it was.
    #[tokio::test]
    async fn a_path_thats_a_file_isnt_saved_to() {
        let dir = tempfile::tempdir().unwrap();
        let firmware = image_file(dir.path(), &image().await);
        let file = dir.path().join("one.bin");
        std::fs::write(&file, "one").unwrap();
        let file = file.to_string_lossy().into_owned();

        for force in [false, true] {
            let e = save(&options(), &path_args(&firmware, &file, force))
                .await
                .unwrap_err();
            assert!(
                matches!(&e, Error::NotADirectory(path) if *path == file),
                "{e:?}"
            );
            assert_eq!(std::fs::read(&file).unwrap(), b"one");
        }
    }

    /// A save to an --output file that exists fails and leaves the file as it
    /// was. With --force, it overwrites the file.
    #[tokio::test]
    async fn an_output_file_is_overwritten_only_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let firmware = image_file(dir.path(), &image().await);
        let output = dir.path().join("basic.bin").to_string_lossy().into_owned();
        std::fs::write(&output, "old").unwrap();

        let e = save(&options(), &rom_args(&firmware, 1, None, &output))
            .await
            .unwrap_err();
        assert!(
            matches!(&e, Error::OutputExists(file) if *file == output),
            "{e:?}"
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"old");

        save(&options(), &forced_rom_args(&firmware, 1, &output))
            .await
            .unwrap();
        assert_eq!(std::fs::read(&output).unwrap(), [1; IMAGE_2364]);
    }

    /// `program --config` rebuilds the image's ROMs and config from the saved
    /// config and ROM files.
    #[tokio::test]
    async fn a_config_and_its_roms_are_saved() {
        let dir = tempfile::tempdir().unwrap();
        let image = image().await;
        let firmware = image_file(dir.path(), &image);
        let path = dir.path().join("c64").to_string_lossy().into_owned();

        let lines = save(&options(), &path_args(&firmware, &path, false))
            .await
            .unwrap();

        let config_file = Path::new(&path)
            .join("config.json")
            .to_string_lossy()
            .into_owned();
        let json = std::fs::read_to_string(&config_file).unwrap();
        let config: Config = serde_json::from_str(&json).unwrap();
        let files: Vec<&str> = config
            .chip_sets
            .iter()
            .flat_map(|set| set.chips.iter())
            .map(|chip| chip.file.as_str())
            .collect();
        assert_eq!(files.len(), 4);
        for (n, file) in files.iter().enumerate() {
            assert!(file.starts_with(&path), "{file}");
            assert_eq!(
                std::fs::read(file).unwrap(),
                [n as u8; IMAGE_2364],
                "{file}"
            );
            assert!(shows(&lines, &[file]), "{file}: {lines:?}");
        }
        assert!(shows(&lines, &[&config_file]), "{lines:?}");
        assert_eq!(lines.len(), files.len() + 1, "{lines:?}");

        let (props, metadata, roms, _) = build_rom_image(
            &options(),
            &json,
            FirmwareVersion::new(0, 8, 0, 0),
            Board::Fire24F,
            Variant::RP2350,
            BoardSize::M,
            false,
            &NoFetch,
            |_| Ok(()),
        )
        .await
        .unwrap();
        let rebuilt = onerom_app::assemble_image(&props, base_firmware(8), metadata, roms)
            .await
            .unwrap()
            .image;
        let before = read_back_file(&image).await.unwrap();
        let after = read_back_file(&rebuilt).await.unwrap();
        assert_eq!(after.files, before.files);
        assert_eq!(
            serde_json::to_value(&after.config).unwrap(),
            serde_json::to_value(&before.config).unwrap()
        );
    }

    /// A published plugin is listed in config.json by name and its slot isn't
    /// saved. An unpublished plugin's slot is.
    #[tokio::test]
    async fn a_published_plugin_is_listed_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let image = image_with_plugins(&[USB_URL, "blink.bin"]).await;
        let firmware = image_file(dir.path(), &image);
        let path = dir.path().join("c64");

        save(
            &options(),
            &path_args(&firmware, &path.to_string_lossy(), false),
        )
        .await
        .unwrap();

        let json = std::fs::read_to_string(path.join("config.json")).unwrap();
        let config: Config = serde_json::from_str(&json).unwrap();
        let usb = &config.chip_sets[0].chips[0];
        assert_eq!(usb.plugin.as_deref(), Some("usb"));
        assert_eq!(usb.file, "");
        let blink = std::fs::read(&config.chip_sets[1].chips[0].file).unwrap();
        assert!(blink == image[slot_range(&image, 1).await]);
        // config.json, the user plugin and the ROM.
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 3);
    }

    #[tokio::test]
    async fn one_rom_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let firmware = image_file(dir.path(), &image().await);
        for (slot, chip, byte) in [(0, None, 0), (1, None, 1), (2, Some(0), 2), (2, Some(1), 3)] {
            let output = dir
                .path()
                .join(format!("rom{byte}.bin"))
                .to_string_lossy()
                .into_owned();
            let lines = save(&options(), &rom_args(&firmware, slot, chip, &output))
                .await
                .unwrap();
            assert_eq!(std::fs::read(&output).unwrap(), [byte; IMAGE_2364]);
            let [line] = &lines[..] else {
                panic!("{lines:?}");
            };
            let slot = slot.to_string();
            let mut values = vec![slot.as_str(), output.as_str()];
            let chip = chip.map(|chip| chip.to_string());
            values.extend(chip.as_deref());
            assert!(holds(line, &values), "{line}");
        }
    }

    #[tokio::test]
    async fn a_slot_or_chip_it_doesnt_have_isnt_saved() {
        let dir = tempfile::tempdir().unwrap();
        let firmware = image_file(dir.path(), &image().await);
        let output = dir.path().join("rom.bin").to_string_lossy().into_owned();
        let error = async |slot, chip| {
            save(&options(), &rom_args(&firmware, slot, chip, &output))
                .await
                .unwrap_err()
        };

        let e = error(3, None).await;
        assert!(matches!(e, Error::NoSlot { slot: 3, slots: 3 }), "{e:?}");
        let e = error(2, None).await;
        assert!(
            matches!(e, Error::ChipNeeded { slot: 2, chips: 2 }),
            "{e:?}"
        );
        let e = error(0, Some(1)).await;
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
        assert!(!Path::new(&output).exists());
    }

    /// Base firmware doesn't contain a config. Firmware before v0.7.0 isn't
    /// read back.
    #[tokio::test]
    async fn firmware_it_cant_read_back_isnt_saved() {
        for image in [base_firmware(8), original_image()] {
            let dir = tempfile::tempdir().unwrap();
            let firmware = image_file(dir.path(), &image);
            let path = dir.path().join("c64").to_string_lossy().into_owned();

            let e = save(&options(), &path_args(&firmware, &path, false))
                .await
                .unwrap_err();
            assert!(
                matches!(&e, Error::ReadBack { what, .. } if *what == firmware),
                "{e:?}"
            );
            assert!(!Path::new(&path).exists());
        }
    }

    #[tokio::test]
    async fn a_setting_that_isnt_read_back_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c64").to_string_lossy().into_owned();
        let config_file = Path::new(&path)
            .join("config.json")
            .to_string_lossy()
            .into_owned();
        let mut read_back = read_back_file(&image().await).await.unwrap();
        let problems = [
            RecoveryProblem::ChipSelect { set: 1 },
            RecoveryProblem::Override {
                set: 2,
                name: "vreg",
            },
        ];
        read_back.problems.extend(problems.iter().cloned());

        let lines = save_read_back(read_back.clone(), &path, false).unwrap();
        for problem in &problems {
            let subject = read_back.problem_subject(problem);
            assert!(shows(&lines, &[&subject]), "{lines:?}");
        }
        assert!(shows(&lines, &[&config_file]), "{lines:?}");
    }

    /// Firmware newer than this build supports isn't saved, and the error
    /// identifies the file and the firmware's version.
    #[tokio::test]
    async fn newer_firmware_isnt_saved() {
        let dir = tempfile::tempdir().unwrap();
        let firmware = image_file(dir.path(), &base_firmware(9));
        let path = dir.path().join("c64").to_string_lossy().into_owned();
        let output = dir.path().join("rom.bin").to_string_lossy().into_owned();
        let newer = FirmwareVersion::new(0, 9, 0, 0);

        for args in [
            path_args(&firmware, &path, false),
            rom_args(&firmware, 0, None, &output),
        ] {
            let e = save(&options(), &args).await.unwrap_err();
            assert!(
                matches!(&e, Error::FirmwareTooNew { what, version } if *what == firmware && *version == newer),
                "{e:?}"
            );
        }
        assert!(!Path::new(&path).exists());
        assert!(!Path::new(&output).exists());
    }
}
