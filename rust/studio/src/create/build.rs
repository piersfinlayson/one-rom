// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Builds firmware images

use iced::Task;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::chip::ChipType;

use crate::app::AppMessage;
use crate::config::Config;
use crate::create::{Create, State};
use crate::studio::{Message as StudioMessage, RuntimeInfo};
use crate::{internal_error, task_from_msg};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Active {
    #[default]
    Low,
    High,
}

pub const ACTIVE_STATES: [Active; 2] = [Active::Low, Active::High];

impl std::fmt::Display for Active {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Active::Low => write!(f, "Low"),
            Active::High => write!(f, "High"),
        }
    }
}

/// Kick off image build
pub fn build_image(create: &mut Create, runtime_info: &RuntimeInfo) -> Task<AppMessage> {
    debug!("Build image requested");

    if create.is_busy() {
        warn!("Busy - skipping build image");
        return Task::none();
    }

    if runtime_info.selected_config().is_none() {
        internal_error!("Build image requested with no selected config.");
        create.set_display_content("Unable to build image: no configuration selected.");
        return Task::none();
    }
    let selected = runtime_info.selected_config().unwrap();

    // Set state and content
    create.state = State::Building;
    create.set_display_content(format!("Building image: {} ...", selected.name()));

    // Send build image message to Studio
    Task::done(StudioMessage::BuildImage(create.selected_hw_info).into())
}

/// Handle the result of a build image operation
pub fn build_image_result(
    create: &mut Create,
    result: Result<String, String>,
    runtime_info: &RuntimeInfo,
) -> Task<AppMessage> {
    // Log the result
    debug!(
        "Build image result received: {}",
        if result.is_ok() { "OK" } else { "Error" }
    );

    // Ensure we were building
    if !create.is_building() {
        internal_error!("BuildImageResult received while not building.");
    }

    // Update state to idle
    create.state = State::Idle;

    match result {
        Ok(desc) => {
            create.display_content = format!(
                "Image built successfully, total: {} bytes ({}/{}/{} plus padding)\n\n{}\n ",
                runtime_info.built_full_image_len().unwrap_or(0),
                runtime_info.built_firmware_len().unwrap_or(0),
                runtime_info.built_metadata_len().unwrap_or(0),
                runtime_info.built_roms_len().unwrap_or(0),
                desc,
            );
        }
        Err(e) => {
            warn!("Error building : {e}");
            create.display_content = format!("Error building image:\n  {e}");
        }
    }

    // Nothing to do
    Task::none()
}

#[allow(clippy::wildcard_enum_match_arm)]
pub fn select_rom_type(create: &mut Create, rom_type: ChipType) -> Task<AppMessage> {
    debug!("User selected ROM type: {}", rom_type.name());
    if !create.is_building() && !create.is_flashing() && !create.is_saving() {
        create.state = State::UserBuilding {
            valid_rom_types: match &create.state {
                State::UserBuilding {
                    valid_rom_types, ..
                } => valid_rom_types.clone(),
                _ => vec![],
            },
            rom_type: Some(rom_type),
            cs: vec![],
            data: None,
        };
        create.set_display_content(format!(
            "Building custom configuration: ROM type {}",
            rom_type.name()
        ));
    } else {
        warn!("Ignoring BuildingSelectChipType while busy");
    }
    Task::none()
}

pub fn select_cs_active(create: &mut Create, index: usize, active: Active) -> Task<AppMessage> {
    debug!("User selected CS{} active state: {}", index, active);
    if let State::UserBuilding {
        valid_rom_types,
        rom_type,
        cs,
        data,
    } = &create.state
    {
        let mut cs = cs.clone();
        // Ensure cs vector is large enough
        while cs.len() <= index {
            cs.push(None);
        }
        cs[index] = Some(active);
        create.state = State::UserBuilding {
            valid_rom_types: valid_rom_types.clone(),
            rom_type: *rom_type,
            cs,
            data: data.clone(),
        };
    } else {
        warn!("Ignoring BuildingSelectCsActive while not user building");
    }
    Task::none()
}

#[allow(clippy::wildcard_enum_match_arm)]
pub fn select_data_vec(create: &mut Create, data: Vec<u8>) -> Task<AppMessage> {
    debug!("User provided data vector of length {}", data.len());
    if let State::UserBuilding { rom_type, cs, .. } = &create.state {
        create.state = State::UserBuilding {
            valid_rom_types: match &create.state {
                State::UserBuilding {
                    valid_rom_types, ..
                } => valid_rom_types.clone(),
                _ => vec![],
            },
            rom_type: *rom_type,
            cs: cs.clone(),
            data: Some(hex::encode(&data)),
        };
    } else {
        warn!("Ignoring BuildingSelectDataVec while not user building");
    }
    Task::none()
}

pub fn build_json_config_from_state(create: &mut Create) -> Task<AppMessage> {
    if let State::UserBuilding {
        rom_type, cs, data, ..
    } = &create.state
    {
        if rom_type.is_none() {
            create.set_display_content("Error: ROM type not selected.");
            return Task::none();
        }
        let rom_type = rom_type.as_ref().unwrap();

        // Set CS actives as a local Vec<u8>
        let mut cs_states = Vec::new();
        cs.iter().for_each(|active_opt| {
            if let Some(active) = active_opt {
                match active {
                    Active::Low => cs_states.push(0),
                    Active::High => cs_states.push(1),
                }
            } else {
                cs_states.push(0); // Default to Low if not selected
            }
        });

        // Set data
        let data_vec = if let Some(data_hex) = data {
            match hex::decode(data_hex) {
                Ok(vec) => vec,
                Err(e) => {
                    create.set_display_content(format!("Error decoding data hex: {}", e));
                    return Task::none();
                }
            }
        } else {
            create.set_display_content("ROM data not provided");
            return Task::none();
        };

        task_from_msg!(StudioMessage::LoadConfig(Config::Built {
            rom_type: *rom_type,
            chip_select: cs_states,
            data: data_vec,
        }))
    } else {
        create.set_display_content("Not in user building state");
        Task::none()
    }
}
