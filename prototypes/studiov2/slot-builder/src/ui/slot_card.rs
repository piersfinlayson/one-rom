// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One slot card.
//!
//! Title row, then the two groups the design fixes — SOURCE FILE and EMULATED
//! CHIP — then the label. The Intel HEX load address appears only for that
//! format, and the chip-select pickers only for the lines the chosen chip
//! actually has.

use iced::widget::{Space, button, canvas, column, container, pick_list, row, rule, text};
use iced::{Alignment, Center, Element, Fill, Shrink};

use crate::catalog::{self, POLARITIES};
use crate::slot::Slot;
use crate::ui::{jumper, style};
use crate::{Message, Screen};

/// The label column inside a card, from `.sb-field-grid`.
const LABEL_WIDTH: f32 = 108.0;
/// The width every in-card dropdown shares.
const PICKER_WIDTH: f32 = 140.0;

/// The card for the slot at `index`.
pub fn view<'a>(app: &'a Screen, index: usize, slot: &'a Slot) -> Element<'a, Message> {
    let body = column![
        head(app, index, slot),
        group("Source file"),
        source(app, index, slot),
        group("Emulated chip"),
        chip(app, index, slot),
        field("Label:", label_field(index, slot)),
    ]
    .spacing(6);

    container(body)
        .style(style::card)
        .padding([10, 14])
        .width(Fill)
        .into()
}

// ------------------------------------------------------------------- head ---

/// Slot number, jumper setting, and the reorder and remove buttons.
fn head<'a>(app: &'a Screen, index: usize, _slot: &'a Slot) -> Element<'a, Message> {
    let mut left = row![
        text(format!("Slot {index}"))
            .size(style::SLOT_TITLE)
            .font(style::SEMIBOLD)
            .style(style::gold),
    ]
    .spacing(9)
    .align_y(Center);

    match app.board {
        Some(board) => {
            if let Some(positions) = catalog::header_positions(board.0) {
                let block = jumper::SlotBlock {
                    positions,
                    slot: index,
                };
                left = left.push(
                    canvas(block)
                        .width(mini_width(board.0))
                        .height(jumper::BLOCK_HEIGHT),
                );
            }
            left = left.push(
                text(catalog::jumper_hint(board.0, index))
                    .size(12.5)
                    .style(style::dim),
            );
        }
        None => {
            left = left.push(text("Select a One ROM type").size(12.5).style(style::dim));
        }
    }

    let icon = |glyph: &'static str, enabled: bool, message: Message, danger: bool| {
        let mut control = button(
            container(text(glyph).size(13.0))
                .center_x(Fill)
                .center_y(Fill),
        )
        .style(if danger {
            style::remove_button
        } else {
            style::icon_button
        })
        .width(30)
        .height(30)
        .padding(0);
        if enabled {
            control = control.on_press(message);
        }
        control
    };

    row![
        left,
        Space::new().width(Fill).height(Shrink),
        icon(
            "\u{2191}",
            index > 0,
            Message::MoveSlot(index, index as isize - 1),
            false
        ),
        icon(
            "\u{2193}",
            index + 1 < app.slots.len(),
            Message::MoveSlot(index, index as isize + 1),
            false
        ),
        icon(
            "\u{2715}",
            app.slots.len() > 1,
            Message::RemoveSlot(index),
            true
        ),
    ]
    .spacing(6)
    .align_y(Center)
    .into()
}

/// How wide a board's mini jumper block draws.
fn mini_width(board: onerom_config::hw::Board) -> f32 {
    catalog::header_positions(board).map_or(0.0, |positions| {
        jumper::SlotBlock { positions, slot: 0 }.width()
    })
}

// -------------------------------------------------------------- the groups --

/// A group title with its underline, from `.sb-group-title`.
fn group(title: &str) -> Element<'_, Message> {
    column![
        text(title.to_uppercase())
            .size(style::GROUP)
            .style(style::dim),
        rule::horizontal(1).style(spacer),
    ]
    .spacing(3)
    .into()
}

/// The hairline under a group title.
fn spacer(_theme: &iced::Theme) -> rule::Style {
    style::divider(_theme)
}

/// The SOURCE FILE group: the file picker, the format, and — for Intel HEX —
/// the load address.
fn source<'a>(app: &'a Screen, index: usize, slot: &'a Slot) -> Element<'a, Message> {
    let name = slot.file_name();
    let file = row![
        button(text("Upload Image").size(13.5))
            .style(style::file_button)
            .padding([6, 11])
            .on_press(Message::PickFile(index)),
        match &name {
            Some(name) => text(name.clone()).size(13.5).style(style::gold),
            None => text("No file selected").size(13.5).style(style::dim),
        },
    ]
    .spacing(10)
    .align_y(Center);

    let mut format = row![
        pick_list(
            app.formats.as_slice(),
            Some(catalog::FormatChoice(slot.format)),
            move |choice| Message::FormatSelected(index, choice),
        )
        .text_size(style::BODY)
        .padding([7, 10])
        .width(PICKER_WIDTH)
        .style(style::picker),
    ]
    .spacing(10)
    .align_y(Center);

    if slot.format == onerom_gen::FileFormat::IntelHex {
        format = format
            .push(text("Load address:").size(13.5).style(style::dim))
            .push(
                iced::widget::text_input("0x0000", &slot.load_address)
                    .on_input(move |value| Message::LoadAddressEdited(index, value))
                    .size(style::BODY)
                    .padding([7, 10])
                    .width(PICKER_WIDTH)
                    .style(style::field),
            );
    }

    column![field("Image:", file), field("Format:", format)]
        .spacing(6)
        .into()
}

/// The EMULATED CHIP group: type, size handling, and the chip selects the type
/// gives it.
fn chip<'a>(app: &'a Screen, index: usize, slot: &'a Slot) -> Element<'a, Message> {
    let mut type_row = row![
        pick_list(app.chips.as_slice(), slot.chip, move |choice| {
            Message::ChipSelected(index, choice)
        })
        .placeholder("\u{2014} select \u{2014}")
        .text_size(style::BODY)
        .padding([7, 10])
        .width(PICKER_WIDTH)
        .style(style::picker),
    ]
    .spacing(10)
    .align_y(Center);

    if let Some(chip) = slot.chip {
        type_row = type_row.push(
            text(format!("Size: {}", catalog::kb(u64::from(chip.rom_bytes))))
                .size(13.5)
                .style(style::dim),
        );
    }

    let sizing = pick_list(
        app.sizings.as_slice(),
        Some(catalog::SizingChoice(slot.sizing.clone())),
        move |choice| Message::SizingSelected(index, choice),
    )
    .text_size(style::BODY)
    .padding([7, 10])
    .width(PICKER_WIDTH)
    .style(style::picker);

    let mut content = column![
        field("ROM Type:", type_row),
        field("Size handling:", sizing),
    ]
    .spacing(6);

    let lines = slot.chip_selects();
    if lines > 0 {
        let mut selects = row![].spacing(9).align_y(Center);
        for line in 0..lines {
            selects = selects.push(
                row![
                    pick_list(
                        &POLARITIES[..],
                        Some(slot.polarities[line]),
                        move |choice| Message::PolaritySelected(index, line, choice),
                    )
                    .text_size(style::BODY)
                    .padding([7, 10])
                    .width(PICKER_WIDTH)
                    .style(style::picker),
                    text(format!("CS{}", line + 1)).size(12.5).style(style::dim),
                ]
                .spacing(5)
                .align_y(Center),
            );
        }
        content = content.push(field("Chip selects:", selects));
    }

    content.into()
}

/// The metadata label for this image.
fn label_field(index: usize, slot: &Slot) -> Element<'_, Message> {
    iced::widget::text_input("optional, e.g. C64 KERNAL 901227-03", &slot.label)
        .on_input(move |value| Message::LabelEdited(index, value))
        .size(style::BODY)
        .padding([7, 10])
        .width(Fill)
        .style(style::field)
        .into()
}

/// A labelled row inside a card.
fn field<'a>(caption: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![
        container(text(caption).size(style::BODY).style(style::dim))
            .width(LABEL_WIDTH)
            .align_x(Alignment::End),
        control.into(),
    ]
    .spacing(13)
    .align_y(Center)
    .into()
}
