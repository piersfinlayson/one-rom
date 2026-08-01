// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Create view methods

use iced::widget::{Column, Container, Row, Space, column, row};
use iced::{Element, Length};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use onerom_config::hw::MODELS;

use crate::config::Config;
use crate::create::{Create, Message};
use crate::device::Device;
use crate::studio::RuntimeInfo;
use crate::style::Style;
use crate::{AppLink, AppMessage};

use super::build::{ACTIVE_STATES, Active};

/// Create tab view
#[allow(clippy::wildcard_enum_match_arm)]
pub fn view<'a>(
    create: &'a Create,
    runtime_info: &'a RuntimeInfo,
    device: &Device,
    style: &'a Style,
) -> Element<'a, AppMessage> {
    if !create.state.is_user_building() {
        // Create the "Select Hardware" section
        let mut columns = select_hardware_element(create);

        // Add firmware row if hardware selected
        if create.hardware_selected() {
            columns = columns
                .push(firmware_row(create, runtime_info))
                .push(Style::horiz_line());
        }

        // Add config row if hardware selected and release selected and configs exist
        if create.hardware_selected() && runtime_info.firmware_selected() {
            columns = columns
                .push(rom_config_row(create, runtime_info, style))
                .push(Style::horiz_line());
        }

        // Add build section if ready to build
        if create.ready_to_build(runtime_info) {
            let (button_row, window_container) = build_section(create, runtime_info, device);

            columns = columns.push(button_row);
            columns = columns.push(window_container);
        }

        columns.spacing(20).into()
    } else {
        // User building custom config - build custom view
        let (rom_types, rom_type, cs, data) = match &create.state {
            crate::create::State::UserBuilding {
                valid_rom_types,
                rom_type,
                cs,
                data,
            } => (valid_rom_types, rom_type, cs, data),
            _ => unreachable!(),
        };

        // Add a ROM type picklist row
        let pick_list = Style::pick_list_small(rom_types.as_slice(), *rom_type, |rt| {
            Message::BuildingSelectChipType(rt).into()
        });

        // Build the CS picker(s)
        let cs_pickers = if let Some(rom_type) = rom_type {
            let control_lines = rom_type.control_lines();
            let cs_lines = control_lines.iter().filter(|cl| cl.name.starts_with("cs"));
            let mut pickers = Vec::new();
            for (i, _cs_line) in cs_lines.enumerate() {
                let msg = move |active: Active| Message::BuildingSelectCsActive(i, active).into();
                let current = cs.get(i).cloned().flatten();
                let picker = Style::pick_list_small(ACTIVE_STATES, current, msg);
                pickers.push(picker);
            }
            pickers
        } else {
            Vec::new()
        };

        // Build the data vector input area
        let data_input = Style::box_scrollable_text(data.as_deref().unwrap_or(""), 100.0, false);

        // Now construct the view
        let mut column = column![
            row![Style::text_h3("ROM Type:"), pick_list,]
                .spacing(20)
                .align_y(iced::alignment::Vertical::Center),
        ];
        if !cs_pickers.is_empty() {
            let mut cs_row = row![Style::text_h3("Chip Selects:")]
                .spacing(20)
                .align_y(iced::alignment::Vertical::Center);
            for picker in cs_pickers {
                cs_row = cs_row.push(picker);
            }
            column = column.push(cs_row);
        }
        column = column.push(
            row![Style::text_h3("Data Vector:"), data_input,]
                .spacing(20)
                .align_y(iced::alignment::Vertical::Center),
        );

        // Add done and cancel buttons
        let done_button =
            Style::text_button_small("Done", Some(Message::BuildingComplete.into()), true);
        let cancel_button =
            Style::text_button_small("Cancel", Some(Message::BuildingCancelled.into()), true);

        column = column.push(
            row![done_button, cancel_button,]
                .spacing(20)
                .align_y(iced::alignment::Vertical::Center),
        );

        column.spacing(20).into()
    }
}

// Add the ROM config selection row
fn rom_config_row<'a>(
    create: &'a Create,
    runtime_info: &'a RuntimeInfo,
    style: &'a Style,
) -> iced::Element<'a, AppMessage> {
    if runtime_info.config_manifest().is_none() {
        return row![Style::text_h3("No configurations available")]
            .spacing(20)
            .align_y(iced::alignment::Vertical::Center)
            .into();
    }
    let manifest = runtime_info.config_manifest().unwrap();

    // Create the ROM config pick list
    let configs = manifest.internal_configs.as_slice();
    let selected: Option<Config> = runtime_info.selected_config().map(|c| c.clone().into());
    let msg = if create.is_busy() {
        |_| AppMessage::Nop // Ignore picklist selection while busy
    } else {
        |name| Message::ConfigSelected(name).into()
    };
    let pick_list = Style::pick_list_small(configs, selected.clone(), msg);

    let mut row = row![Style::text_h3("ROM Config:"), pick_list,];

    if let Some(config) = selected
        && !config.is_special()
    {
        // Show if config has been loaded/downloaded
        if let Some(config_len) = runtime_info.selected_config_len() {
            let reload = style.reload_icon(Message::ReloadConfig.into());
            row = row.push(reload);

            // split into three rows, with number of bytes gold
            let downloaded_row = row![
                Style::text_small("(loaded: "),
                Style::text_small(format!("{}", config_len)).color(Style::COLOUR_DARK_GOLD),
                Style::text_small(" bytes)"),
            ]
            .spacing(0);
            row = row.push(downloaded_row);
        }
    }

    let help = style.help_link(AppLink::RomConfigs, "ROM Config Help");
    row = row.push(Space::with_width(Length::Fill)).push(help);

    row.spacing(20)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

fn firmware_row<'a>(
    create: &'a Create,
    runtime_info: &'a RuntimeInfo,
) -> iced::Element<'a, AppMessage> {
    // Create release selection row
    if let Some(releases) = &runtime_info.releases() {
        let latest = releases.latest();

        let msg = if create.is_busy() {
            |_| AppMessage::Nop // Ignore picklist selection while busy
        } else {
            |r| Message::ReleaseSelected(r).into()
        };

        // We don't display releases unless model and board selected
        let board = create
            .selected_hw_info
            .board
            .expect("Board should be selected");
        let variant = create
            .selected_hw_info
            .mcu_variant
            .expect("MCU variant should be selected");

        // Choose releases to show based on hardware
        let pick_releases = releases.hw_releases(&board, &variant);
        let no_releases = pick_releases.is_empty();

        let mut row = row![Style::text_h3("Firmware Release")];

        if !no_releases {
            // Get selected release
            let selected_release = if pick_releases.is_empty() {
                None
            } else if let Some(r) = runtime_info.selected_firmware() {
                Some(r)
            } else {
                // Only display latest if it supports the selected hardware
                releases
                    .release_from_string(latest)
                    .filter(|&r| r.supports_hw(&board, &variant))
            };

            // Create release pick list
            let release_pick_list = Style::pick_list_small(pick_releases, selected_release, msg);

            row = row.push(release_pick_list);
        } else {
            let no_releases = Style::text_body("No releases found").color(Style::COLOUR_DARK_GOLD);
            row = row.push(no_releases);
        }

        // Show if release has been downloaded
        if let Some(fw_len) = runtime_info.firmware_len() {
            // split into three rows, with number of bytes gold
            let downloaded_row = row![
                Style::text_small("(loaded: "),
                Style::text_small(format!("{}", fw_len)).color(Style::COLOUR_DARK_GOLD),
                Style::text_small(" bytes)"),
            ]
            .spacing(0);
            row = row.push(downloaded_row);
        }

        // Return the row
        row.spacing(20).align_y(iced::alignment::Vertical::Center)
    } else {
        row![Style::text_h3("No firmware releases available")]
    }
    .spacing(20)
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

fn select_hw_heading_row() -> Element<'static, AppMessage> {
    row![Style::text_h3("Select Hardware")].into()
}

/*
fn detect_button() -> iced::Element<'static, AppMessage> {
    let button = Style::text_button_small(
        "Detect Hardware",
        Some(Message::DetectHardware.into()),
        true,
    );
    row![button].into()
}
*/

fn select_hw_row(create: &Create) -> iced::Element<'_, AppMessage> {
    // Set up model picker
    let msg = if create.is_busy() {
        |_| AppMessage::Nop // Ignore picklist selection while busy
    } else {
        |model| Message::ModelSelected(model).into()
    };
    let model_picker =
        Style::pick_list_small(MODELS.as_slice(), create.selected_hw_info.model, msg);
    let model_picker = row![Style::text_body("Model:"), model_picker,]
        .spacing(10)
        .align_y(iced::alignment::Vertical::Center);

    // Set up board picker
    let board_values = if let Some(model) = create.selected_hw_info.model {
        model.boards()
    } else {
        &[]
    };
    let msg = if create.is_busy() {
        |_| AppMessage::Nop // Ignore picklist selection while busy
    } else {
        |board| Message::BoardSelected(board).into()
    };
    let board_picker = Style::pick_list_small(board_values, create.selected_hw_info.board, msg);
    let board_picker = row![Style::text_body("Board:"), board_picker,]
        .spacing(10)
        .align_y(iced::alignment::Vertical::Center);

    // Set up MCU picker
    let mcu_values = if let Some(vars) = &create.mcu_variants {
        vars.as_slice()
    } else {
        &[]
    };
    let msg = if create.is_busy() {
        |_| AppMessage::Nop // Ignore picklist selection while busy
    } else {
        |mcu| Message::McuSelected(mcu).into()
    };
    let mcu_picker = Style::pick_list_small(mcu_values, create.selected_hw_info.mcu_variant, msg);
    let mcu_picker = row![Style::text_body("MCU:"), mcu_picker,]
        .spacing(10)
        .align_y(iced::alignment::Vertical::Center);

    row![model_picker, board_picker, mcu_picker]
        .spacing(20)
        .into()
}

// Create the "Select Hardware" section.  Return as a column for the rest of the
// view to add to.
fn select_hardware_element(create: &Create) -> Column<'_, AppMessage> {
    column![
        row![
            select_hw_heading_row(),
            //Style::text_h3("or"),
            //detect_button(),
        ]
        .spacing(20)
        .align_y(iced::alignment::Vertical::Center),
        select_hw_row(create),
        board_description_row(create),
        Style::horiz_line()
    ]
}

// Create the hardware description row - based on selected hardware
fn board_description_row(create: &Create) -> iced::Element<'_, AppMessage> {
    if create.hardware_selected() {
        let hw_info_row = Style::hw_info_row(
            None,
            None,
            create.selected_hw_info.model,
            create.selected_hw_info.board,
            create.selected_hw_info.mcu_variant,
            true,
        );

        row![
            Style::text_body("Selected:").color(Style::COLOUR_GOLD),
            hw_info_row,
        ]
        .spacing(20)
        .align_y(iced::alignment::Vertical::Center)
    } else {
        row![Style::text_body("Hardware not selected")]
    }
    .into()
}

fn build_section<'a>(
    create: &'a Create,
    runtime_info: &'a RuntimeInfo,
    device: &Device,
) -> (Row<'a, AppMessage>, Container<'a, AppMessage>) {
    let content = if create.is_building() {
        "Building...".to_string()
    } else {
        "Build Image".to_string()
    };
    let (on_press, highlighted) = if create.is_busy() {
        (None, false)
    } else {
        (Some(Message::BuildImage.into()), true)
    };
    let build_button = Style::text_button_small(content, on_press, highlighted);

    let button_row = row![build_button].spacing(20);

    // Only show Save and Flash buttons if images exist _and_ not in
    // the process of building one.
    let button_row = if runtime_info.image().is_some() && !create.is_building() {
        let (on_press, highlighted) = if create.is_busy() {
            (None, false)
        } else {
            (Some(Message::SaveFirmware.into()), true)
        };
        let save_content = if create.is_saving() {
            "Saving...".to_string()
        } else {
            "Save Firmware".to_string()
        };
        let save_button = Style::text_button_small(save_content, on_press, highlighted);

        let flash_content = if create.is_flashing() {
            "Flashing...".to_string()
        } else {
            "Flash Firmware".to_string()
        };
        let (on_press, highlighted) =
            if create.is_busy() || !device.is_ready() || device.is_running() {
                (None, false)
            } else {
                (Some(Message::FlashFirmware.into()), true)
            };
        let flash_button = Style::text_button_small(flash_content, on_press, highlighted);

        let mut row = button_row.push(Space::with_width(Length::Fill));

        if device.is_usb_run_capable() {
            let (content, message) = if device.is_running() {
                (
                    "Stop",
                    if !create.is_busy() {
                        Some(Message::StopDevice.into())
                    } else {
                        None
                    },
                )
            } else {
                (
                    "Run",
                    if !create.is_busy() {
                        Some(Message::RunDevice.into())
                    } else {
                        None
                    },
                )
            };
            row = row.push(Style::text_button_small(
                content,
                message,
                !create.is_busy(),
            ));
        }

        row.push(save_button).push(flash_button)
    } else {
        button_row
    };

    let window = Style::box_scrollable_text(create.display_content.clone(), 166.0, true);
    let window_container = Style::container(window);

    (button_row, window_container)
}
