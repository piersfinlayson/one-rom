// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Reads the analysed file or device back into the config that builds it and
//! passes the config to Create with its ROMs inlined.

use iced::Task;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use onerom_app::{ReadBack, read_back_file};
use onerom_cli::read_back::{inline_files, problem_error, read_back_error};
use onerom_config::mcu::Variant as McuVariant;

use crate::analyse::{Analyse, AnalyseState, Message, Source};
use crate::app::AppMessage;
use crate::config::Config;
use crate::create::Message as CreateMessage;
use crate::device::{Client, Message as DeviceMessage, load_error};
use crate::hw::HardwareInfo;
use crate::studio::{Message as StudioMessage, RuntimeInfo, StudioTab};

pub fn can_read_back(analyse: &Analyse) -> bool {
    analyse
        .fw_info
        .as_ref()
        .is_some_and(|device| device.as_schema().is_some())
}

/// Reads the analysed file or device back
pub fn use_in_create(analyse: &mut Analyse) -> Task<AppMessage> {
    if analyse.state.is_busy() || !can_read_back(analyse) {
        warn!("Ignoring UseInCreate while busy or without firmware to read back");
        return Task::none();
    }

    let task = match (analyse.selected_source_tab, &analyse.file) {
        (Source::File, Some(file)) => {
            analyse.read_back_source = file
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let data = file.data.clone();
            let source = analyse.read_back_source.clone();
            Task::perform(
                async move {
                    read_back_file(&data)
                        .await
                        .map_err(|e| load_error(read_back_error(&source, e)))
                },
                |result| Message::ReadBackResult(result).into(),
            )
        }
        (Source::Device, _) => {
            analyse.read_back_source = "the device".to_string();
            let hw_info = analyse
                .fw_info
                .as_ref()
                .map(HardwareInfo::from_parsed)
                .unwrap_or_default();
            Task::done(
                DeviceMessage::ReadBack {
                    client: Client::Analyse,
                    hw_info,
                }
                .into(),
            )
        }
        (Source::File, None) => return Task::none(),
    };

    analyse.state = AnalyseState::ReadingBack;
    analyse.analysis_content += &format!("\n{}", analyse.state.content());
    task
}

/// Passes a read back's hardware, firmware version and config to Create and
/// selects Create
pub fn read_back_result(
    analyse: &mut Analyse,
    runtime_info: &RuntimeInfo,
    result: Result<ReadBack, String>,
) -> Task<AppMessage> {
    analyse.state = AnalyseState::Idle;
    let read_back = match result {
        Ok(read_back) => read_back,
        Err(e) => {
            analyse.analysis_content += &format!("\n{e}\n");
            return Task::none();
        }
    };

    for problem in &read_back.problems {
        analyse.analysis_content += &format!("\nWarning: {}", problem_error(&read_back, problem));
    }
    let json = match serde_json::to_string(&inline_files(&read_back)) {
        Ok(json) => json,
        Err(e) => {
            analyse.analysis_content += &format!("\nFailed to serialize the config: {e}\n");
            return Task::none();
        }
    };

    // Firmware that can be read back runs on an RP2350.  A device's board
    // size is read from it.  A file's is the smallest its image fits.
    let from_device = analyse.selected_source_tab == Source::Device;
    let (board, version) = (read_back.board, read_back.version);
    let hw_info = HardwareInfo {
        board: Some(board),
        model: Some(board.model()),
        mcu_variant: Some(McuVariant::RP2350),
        board_size: read_back.board_size.filter(|_| from_device),
        min_board_size: read_back.board_size.filter(|_| !from_device),
    };
    let offered = runtime_info
        .releases()
        .and_then(|releases| hw_info.offered_release(releases, version));
    if offered.is_none() {
        analyse.analysis_content +=
            &format!("\nWarning: Firmware v{version} couldn't be selected in Create.");
    }

    let config = Config::ReadBack {
        source: analyse.read_back_source.clone(),
        json: json.into_bytes(),
    };
    Task::done(CreateMessage::ReadBack { hw_info, version }.into())
        .chain(Task::done(CreateMessage::ConfigSelected(config).into()))
        .chain(Task::done(
            StudioMessage::TabSelected(StudioTab::Create).into(),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::Create;
    use crate::device::Device;
    use futures::StreamExt;
    use iced_runtime::Action;
    use onerom_cli::image::parse_firmware;
    use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
    use onerom_config::hw::{Board, Model};
    use onerom_config::mcu::{Family, Variant};
    use onerom_fw::net::Releases;
    use onerom_fw_parser::SDRR_INFO_FW_OFFSET;
    use onerom_gen::{Builder, FileData};
    use onerom_metadata::{
        FLASH_CS0_BASE_ADDR, FirmwareType, METADATA_BASE, ONEROM_FAMILY_MAGIC,
        ONEROM_INFO_BUILD_DATE_OFFSET, ONEROM_INFO_FIRMWARE_TYPE_OFFSET,
        ONEROM_INFO_METADATA_OFFSET, ONEROM_INFO_MINOR_VERSION_OFFSET, ONEROM_INFO_VERSION,
        ONEROM_INFO_VERSION_OFFSET,
    };
    use std::path::PathBuf;

    // Handles `msg` and returns the messages its Task sends
    fn update(analyse: &mut Analyse, msg: Message) -> Vec<AppMessage> {
        update_with(analyse, &RuntimeInfo::default(), msg)
    }

    fn update_with(
        analyse: &mut Analyse,
        runtime_info: &RuntimeInfo,
        msg: Message,
    ) -> Vec<AppMessage> {
        sent(analyse.update(runtime_info, msg))
    }

    // The messages `task` sends
    fn sent(task: Task<AppMessage>) -> Vec<AppMessage> {
        let Some(stream) = iced_runtime::task::into_stream(task) else {
            return Vec::new();
        };
        let outputs = stream.filter_map(|action| async move {
            if let Action::Output(msg) = action {
                Some(msg)
            } else {
                None
            }
        });
        futures::executor::block_on(outputs.collect())
    }

    // Base firmware that is a bare onerom_info_t pointing at the metadata
    fn base_firmware() -> Vec<u8> {
        let mut image = vec![0u8; 0x1000];
        let base = SDRR_INFO_FW_OFFSET as usize;
        let mut put = |offset: usize, bytes: &[u8]| {
            image[base + offset..base + offset + bytes.len()].copy_from_slice(bytes);
        };
        put(0, ONEROM_FAMILY_MAGIC.as_bytes());
        put(ONEROM_INFO_MINOR_VERSION_OFFSET, &8u16.to_le_bytes());
        put(
            ONEROM_INFO_BUILD_DATE_OFFSET,
            &(FLASH_CS0_BASE_ADDR + 0x800).to_le_bytes(),
        );
        put(ONEROM_INFO_METADATA_OFFSET, &METADATA_BASE.to_le_bytes());
        put(
            ONEROM_INFO_VERSION_OFFSET,
            &ONEROM_INFO_VERSION.to_le_bytes(),
        );
        put(
            ONEROM_INFO_FIRMWARE_TYPE_OFFSET,
            &(FirmwareType::FirmwareTypeOneRom as u16).to_le_bytes(),
        );
        image
    }

    fn image() -> Vec<u8> {
        let version = FirmwareVersion::new(0, 8, 0, 0);
        let json = r#"{ "version": 1, "description": "Test", "chip_sets": [
            { "type": "single", "chips": [{ "file": "kernal.bin", "type": "2364", "cs1": "active_low" }] }
        ] }"#;
        let mut builder = Builder::from_json(version, Family::Rp2350, json).unwrap();
        builder
            .add_file(FileData::new(0, vec![0x5A; 8192]))
            .unwrap();
        let props = FirmwareProperties::new(
            version,
            Board::Fire24F,
            Variant::RP2350,
            ServeAlg::Default,
            false,
        )
        .unwrap();
        let (metadata, roms) = builder.build(props).unwrap();
        futures::executor::block_on(onerom_app::assemble_image(
            &props,
            base_firmware(),
            metadata,
            roms,
        ))
        .unwrap()
        .image
    }

    /// A loaded file is read back, its config passed to Create and Create
    /// selected.
    #[test]
    fn a_file_is_read_back_into_create() {
        let mut analyse = Analyse::new();
        let image = image();
        let device = futures::executor::block_on(parse_firmware(&image));
        let path = PathBuf::from("c64.bin");
        update(&mut analyse, Message::FileLoaded(path, Ok((device, image))));

        let sent = update(&mut analyse, Message::UseInCreate);
        let result = sent
            .into_iter()
            .find_map(|msg| {
                if let AppMessage::Analyse(Message::ReadBackResult(result)) = msg {
                    Some(result)
                } else {
                    None
                }
            })
            .unwrap();
        let sent = update(&mut analyse, Message::ReadBackResult(result));
        assert!(analyse.state.is_idle());

        let json = sent
            .iter()
            .find_map(|msg| {
                if let AppMessage::Create(CreateMessage::ConfigSelected(Config::ReadBack {
                    source,
                    json,
                })) = msg
                    && source == "c64.bin"
                {
                    Some(json.clone())
                } else {
                    None
                }
            })
            .unwrap();
        let config: onerom_gen::Config = serde_json::from_slice(&json).unwrap();
        let chip = &config.chip_sets[0].chips[0];
        assert!(chip.file.starts_with("base64:"));
        assert_eq!(chip.label.as_deref(), Some("kernal.bin"));
        assert!(sent.iter().any(|msg| matches!(
            msg,
            AppMessage::Studio(StudioMessage::TabSelected(StudioTab::Create))
        )));
    }

    /// A device is read back by the device module, and the read back it
    /// returns is passed to Create.
    #[test]
    fn a_device_is_read_back_into_create() {
        let mut analyse = Analyse::new();
        let image = image();
        analyse.selected_source_tab = Source::Device;
        analyse.fw_info = Some(futures::executor::block_on(parse_firmware(&image)));

        let sent = update(&mut analyse, Message::UseInCreate);
        assert!(sent.iter().any(|msg| matches!(
            msg,
            AppMessage::Device(DeviceMessage::ReadBack {
                client: Client::Analyse,
                ..
            })
        )));
        assert!(!analyse.state.is_idle());

        let read_back = futures::executor::block_on(read_back_file(&image)).unwrap();
        let sent = update(&mut analyse, Message::ReadBackResult(Ok(read_back)));
        assert!(analyse.state.is_idle());
        let json = sent
            .iter()
            .find_map(|msg| {
                if let AppMessage::Create(CreateMessage::ConfigSelected(Config::ReadBack {
                    json,
                    ..
                })) = msg
                {
                    Some(json.clone())
                } else {
                    None
                }
            })
            .unwrap();
        let config: onerom_gen::Config = serde_json::from_slice(&json).unwrap();
        assert!(config.chip_sets[0].chips[0].file.starts_with("base64:"));
    }

    /// A file with firmware newer than this build supports fails to load with
    /// Studio's error text, not the CLI's.
    #[test]
    fn newer_firmware_isnt_read_back() {
        let mut analyse = Analyse::new();
        let mut image = base_firmware();
        let at = SDRR_INFO_FW_OFFSET as usize + ONEROM_INFO_MINOR_VERSION_OFFSET;
        image[at..at + 2].copy_from_slice(&9u16.to_le_bytes());
        let device = futures::executor::block_on(parse_firmware(&image));
        let path = PathBuf::from("new.bin");
        update(&mut analyse, Message::FileLoaded(path, Ok((device, image))));

        let sent = update(&mut analyse, Message::UseInCreate);
        let error = sent
            .into_iter()
            .find_map(|msg| {
                if let AppMessage::Analyse(Message::ReadBackResult(Err(error))) = msg {
                    Some(error)
                } else {
                    None
                }
            })
            .unwrap();
        for value in ["new.bin", "v0.9.0", "Studio"] {
            assert!(error.contains(value), "{error}");
        }
    }

    /// A setting that isn't recognised is a warning, and the config is still
    /// passed to Create.
    #[test]
    fn a_setting_that_isnt_recognised_is_a_warning() {
        let mut analyse = Analyse::new();
        let mut read_back = futures::executor::block_on(read_back_file(&image())).unwrap();
        read_back
            .problems
            .push(onerom_app::RecoveryProblem::ChipSelect { set: 0 });
        let warning = problem_error(&read_back, &read_back.problems[0]).to_string();

        let sent = update(&mut analyse, Message::ReadBackResult(Ok(read_back)));
        assert!(analyse.analysis_content.contains(&warning));
        assert!(
            sent.iter()
                .any(|msg| matches!(msg, AppMessage::Create(CreateMessage::ConfigSelected(_))))
        );
    }

    /// A failed read back is reported and isn't passed to Create.
    #[test]
    fn a_failed_read_back_is_reported() {
        let mut analyse = Analyse::new();
        analyse.state = AnalyseState::ReadingBack;

        let error = "Slot 0's data at 0x10010000 couldn't be read";
        let sent = update(
            &mut analyse,
            Message::ReadBackResult(Err(error.to_string())),
        );
        assert!(analyse.state.is_idle());
        assert!(analyse.analysis_content.contains(error));
        assert!(!sent.iter().any(|msg| matches!(msg, AppMessage::Create(_))));
    }

    fn runtime_info(v0_8_0_board: &str) -> RuntimeInfo {
        let mcus = r#"[{ "name": "rp2350" }]"#;
        let json = format!(
            r#"{{ "version": 1, "latest": "0.8.1", "releases": [
                {{ "version": "0.8.1", "boards": [
                    {{ "name": "fire-24-f", "mcus": {mcus} }},
                    {{ "name": "fire-40-a", "mcus": {mcus} }}
                ] }},
                {{ "version": "0.8.0", "boards": [{{ "name": "{v0_8_0_board}", "mcus": {mcus} }}] }}
            ] }}"#
        );
        let mut runtime_info = RuntimeInfo::default();
        runtime_info.set_releases(Releases::from_json(&json).unwrap());
        runtime_info
    }

    /// Open in Create selects the read back's board, then its version's
    /// release where offered for the board. Otherwise the latest is selected
    /// and a warning added.
    #[test]
    fn use_in_create_selects_the_read_back_board_and_version() {
        let image = image();
        let device = Device::new();
        for (v0_8_0_board, selected, warned) in
            [("fire-24-f", "0.8.0", false), ("fire-40-a", "0.8.1", true)]
        {
            let runtime_info = runtime_info(v0_8_0_board);
            let mut create = Create::new();
            let _ = create.update(
                &runtime_info,
                &device,
                CreateMessage::ModelSelected(Model::Fire),
            );
            let _ = create.update(
                &runtime_info,
                &device,
                CreateMessage::BoardSelected(Board::Fire40A),
            );

            let mut analyse = Analyse::new();
            let read_back = futures::executor::block_on(read_back_file(&image)).unwrap();
            let version = read_back.version;
            let to_create = update_with(
                &mut analyse,
                &runtime_info,
                Message::ReadBackResult(Ok(read_back)),
            );
            let has_version = analyse.analysis_content.contains(&format!("v{version}"));
            assert_eq!(has_version, warned);

            let read_back = to_create
                .into_iter()
                .find_map(|msg| {
                    if let AppMessage::Create(msg @ CreateMessage::ReadBack { .. }) = msg {
                        Some(msg)
                    } else {
                        None
                    }
                })
                .unwrap();
            let downloads = sent(create.update(&runtime_info, &device, read_back));
            assert!(downloads.iter().any(|msg| matches!(
                msg,
                AppMessage::Studio(StudioMessage::DownloadRelease(release, Board::Fire24F, _))
                    if release.version == selected
            )));
        }
    }

    /// A device's board size is passed to Create as read from it, and a
    /// file's as the smallest its image fits.
    #[test]
    fn use_in_create_passes_the_read_back_board_size() {
        let image = image();
        for source in [Source::Device, Source::File] {
            let mut analyse = Analyse::new();
            analyse.selected_source_tab = source;
            let read_back = futures::executor::block_on(read_back_file(&image)).unwrap();
            let size = read_back.board_size;
            assert!(size.is_some());

            let to_create = update(&mut analyse, Message::ReadBackResult(Ok(read_back)));
            let hw_info = to_create
                .iter()
                .find_map(|msg| {
                    if let AppMessage::Create(CreateMessage::ReadBack { hw_info, .. }) = msg {
                        Some(*hw_info)
                    } else {
                        None
                    }
                })
                .unwrap();
            let from_device = source == Source::Device;
            assert_eq!(hw_info.board_size, size.filter(|_| from_device));
            assert_eq!(hw_info.min_board_size, size.filter(|_| !from_device));
        }
    }
}
