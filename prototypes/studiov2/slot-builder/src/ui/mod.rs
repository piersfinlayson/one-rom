// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The builder page.
//!
//! Laid out section by section against the shipped markup in
//! `one-rom-site/web/index.html`: help and jumper legend, device
//! configuration, the flash tally, the slot cards, plugins, and the build
//! row.

pub mod capacity;
pub mod jumper;
pub mod slot_card;

/// The palette and widget shapes, which live in `studiov2-shared` because
/// every screen in the app has to look the same.  Re-exported so this
/// module's own children reach it where they always did.
pub use studiov2_shared::style;

use iced::widget::{
    Space, button, canvas, column, container, pick_list, row, rule, scrollable, text, text_editor,
};
use iced::{Alignment, Center, Element, Fill, Length, Shrink};

use crate::catalog::{self, PluginChoice, PluginSlot};
use crate::{Message, Screen};
use studiov2_shared::Shared;

/// The page's scroll region, so a screenshot run can be told to show the
/// bottom of it.
pub fn scroll_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("page")
}

/// The whole page.
///
/// Two borrows: the screen's own state, and the state it shares with the
/// rest of the app.  Only the Programming row reads the second one, so it is
/// the only place the second borrow travels to.
pub fn page<'a>(app: &'a Screen, shared: &'a Shared) -> Element<'a, Message> {
    let body = column![
        help(app),
        divider(),
        heading("Device Configuration"),
        device(app),
        divider(),
        heading("ROM Slots"),
        capacity::view(app),
        slots(app),
        add_row(app),
        divider(),
        heading("Plugins"),
        plugins(app),
        divider(),
        heading("Programming"),
        programming(app, shared),
        divider(),
        heading("Log"),
        log(app),
    ]
    .spacing(14)
    .padding([24, 32]);

    container(scrollable(body).id(scroll_id()).height(Fill))
        .style(style::panel)
        .width(Fill)
        .height(Fill)
        .into()
}

// ------------------------------------------------------------------ parts ---

/// A section heading, the site's `h3`.
fn heading(title: &str) -> Element<'_, Message> {
    text(title).size(style::HEADING).font(style::MEDIUM).into()
}

/// The `<hr>` between sections.
fn divider<'a>() -> Element<'a, Message> {
    rule::horizontal(1).style(style::divider).into()
}

/// A dim label above or beside a control.
fn label<'a>(content: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(content).size(style::BODY).style(style::dim)
}

/// A note line under a section.
fn note<'a>(content: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(content).size(style::NOTE).style(style::dim)
}

/// One row of a label grid: a right-aligned label of fixed width, then its
/// control.
fn field<'a>(
    caption: &'a str,
    width: f32,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![
        container(label(caption))
            .width(width)
            .align_x(Alignment::End),
        control.into(),
    ]
    .spacing(14)
    .align_y(Center)
    .into()
}

// ------------------------------------------------------------------- help ---

/// The help block, holding the jumper legend.
fn help(app: &Screen) -> Element<'_, Message> {
    let summary = button(
        row![
            text(if app.help_open {
                "\u{25BE}"
            } else {
                "\u{25B8}"
            })
            .size(14),
            text("Help").size(style::HEADING).font(style::MEDIUM),
        ]
        .spacing(8)
        .align_y(Center),
    )
    .style(|_theme, _status| button::Style {
        background: None,
        text_color: style::TEXT,
        ..button::Style::default()
    })
    .padding(0)
    .on_press(Message::ToggleHelp);

    if !app.help_open {
        return summary.into();
    }

    column![
        summary,
        note(
            "Program One ROM for one or more ROM images, with the board's slot select \
             jumpers choosing which image One ROM serves at power on."
        ),
        note(
            "One image per slot, of any supported ROM type. The slot select jumpers encode \
             the slot number in binary: all open is Slot 0, and the right-most jumper is the \
             least significant bit."
        ),
        legend(app),
    ]
    .spacing(10)
    .into()
}

/// The jumper legend: the board wireframe beside the copy for this board.
fn legend(app: &Screen) -> Element<'_, Message> {
    let Some(board) = app.board else {
        return container(
            text("Select a One ROM type to see its jumper positions.")
                .size(style::NOTE)
                .style(style::dim),
        )
        .style(style::card)
        .padding([14, 16])
        .width(Fill)
        .into();
    };

    let letters = catalog::select_letters(board.0);
    let list = letters
        .iter()
        .map(char::to_string)
        .collect::<Vec<_>>()
        .join("/");
    let plural = if letters.len() > 1 { "s" } else { "" };

    let positions = catalog::header_positions(board.0);
    let has_power = positions.as_ref().is_some_and(|positions| {
        positions
            .iter()
            .any(|position| matches!(position, catalog::HeaderPosition::Power))
    });

    let mut copy = column![note(format!(
        "This One ROM's slot select jumper{plural}: {list}{}.",
        if letters.len() > 1 {
            " (A is the least significant bit)"
        } else {
            ""
        }
    ))]
    .spacing(6);

    if has_power {
        copy = copy.push(note("The left-most pair is 5V/GND - never jumper."));
    }

    let mut content = row![].spacing(19).align_y(Center);
    if let Some(positions) = positions {
        content = content.push(
            canvas(jumper::Wireframe {
                positions,
                code: catalog::board_short_code(board.0),
            })
            .width(jumper::BOARD.width)
            .height(jumper::BOARD.height),
        );
    } else {
        copy = copy.push(
            text(format!(
                "This One ROM's jumper header isn't drawn yet - check the silkscreen for \
                 jumper{plural} {list}."
            ))
            .size(11.5)
            .style(style::dim),
        );
    }

    container(content.push(copy.width(Fill)))
        .style(style::card)
        .padding([14, 16])
        .width(Fill)
        .into()
}

// ----------------------------------------------------------------- device ---

/// The One ROM type and firmware version pickers.
fn device(app: &Screen) -> Element<'_, Message> {
    let board = pick_list(app.boards.as_slice(), app.board, Message::BoardSelected)
        .placeholder("Select...")
        .text_size(style::BODY)
        .padding([8, 10])
        .width(200)
        .style(style::picker);

    let version = pick_list(
        app.versions.as_slice(),
        app.version,
        Message::VersionSelected,
    )
    .placeholder("Select...")
    .text_size(style::BODY)
    .padding([8, 10])
    .width(200)
    .style(style::picker);

    column![
        field("One ROM Type:", 130.0, board),
        field("Firmware Version:", 130.0, version),
    ]
    .spacing(10)
    .into()
}

// ------------------------------------------------------------------ slots ---

/// Every slot card, in boot-select order.
fn slots(app: &Screen) -> Element<'_, Message> {
    column(
        app.slots
            .iter()
            .enumerate()
            .map(|(index, slot)| slot_card::view(app, index, slot)),
    )
    .spacing(14)
    .into()
}

/// The Add ROM Slot button and whatever is stopping it.
fn add_row(app: &Screen) -> Element<'_, Message> {
    let limit = app.board.map_or(0, |board| catalog::max_slots(board.0));
    let at_cap = app.board.is_some() && app.slots.len() >= limit;

    let mut add = button(text("+ Add ROM Slot").size(style::BODY))
        .style(style::gold_button)
        .padding([9, 16]);
    if !at_cap && app.board.is_some() {
        add = add.on_press(Message::AddSlot);
    }

    let hint: Element<'_, Message> = match app.board {
        None => note("Select a One ROM type to add slots.").into(),
        Some(board) if at_cap => {
            let jumpers = board.0.sel_pins().len();
            let plural = if jumpers > 1 { "s" } else { "" };
            text(format!(
                "This board's {jumpers} jumper{plural} select up to {limit} slots. \
                 For more, use the One ROM CLI."
            ))
            .size(style::NOTE)
            .style(style::danger)
            .into()
        }
        Some(_) => Space::new().width(Shrink).height(Shrink).into(),
    };

    row![add, hint].spacing(14).align_y(Center).into()
}

// ---------------------------------------------------------------- plugins ---

/// The system and user plugin pickers.
fn plugins(app: &Screen) -> Element<'_, Message> {
    let mut content = column![
        field(
            "System Plugin:",
            130.0,
            plugin_picker(
                &app.system_plugins,
                &app.system_plugin,
                Message::SystemPlugin
            )
        ),
        field(
            "User Plugin:",
            130.0,
            plugin_picker(&app.user_plugins, &app.user_plugin, Message::UserPlugin)
        ),
    ]
    .spacing(10);

    if app.user_plugin.is_some() && app.system_plugin.is_none() {
        content = content.push(
            text(
                "A user plugin requires a system plugin. Select a system plugin, or set the \
                 user plugin to None.",
            )
            .size(style::NOTE)
            .style(style::danger),
        );
    }

    content.into()
}

/// One plugin picker, with `None` at the head of the list.
fn plugin_picker<'a>(
    offered: &[PluginChoice],
    selected: &Option<PluginChoice>,
    on_select: fn(PluginSlot) -> Message,
) -> Element<'a, Message> {
    let options: Vec<PluginSlot> = std::iter::once(PluginSlot(None))
        .chain(
            offered
                .iter()
                .cloned()
                .map(|plugin| PluginSlot(Some(plugin))),
        )
        .collect();

    pick_list(options, Some(PluginSlot(selected.clone())), on_select)
        .text_size(style::BODY)
        .padding([8, 10])
        .width(240)
        .style(style::picker)
        .into()
}

// ------------------------------------------------------------ programming ---

/// The Build and Save buttons, and what they have to say.
fn programming<'a>(app: &'a Screen, shared: &'a Shared) -> Element<'a, Message> {
    let mut build = button(
        text(if app.building {
            "Building..."
        } else {
            "Build Firmware"
        })
        .size(style::BODY),
    )
    .style(style::gold_button)
    .padding([11, 22]);
    if app.can_build() {
        build = build.on_press(Message::Build);
    }

    let mut save = button(text("Save Firmware").size(style::BODY))
        .style(style::gold_button)
        .padding([11, 22]);
    if app.build_is_current(shared) {
        save = save.on_press(Message::Save);
    }

    let mut content = column![row![build, save].spacing(10)].spacing(8);

    let incomplete = !app.slots.iter().all(crate::slot::Slot::is_complete);
    let blocked = app.usage().over() || (app.user_plugin.is_some() && app.system_plugin.is_none());
    let message = if app.board.is_none() || app.version.is_none() {
        String::new()
    } else if incomplete {
        "Every slot needs an image and ROM type.".to_owned()
    } else if !app.status.is_empty() {
        app.status.clone()
    } else if blocked {
        // The reason is already spelled out below, or beside the plugin picker.
        String::new()
    } else {
        "Ready to build.".to_owned()
    };

    if !message.is_empty() {
        content = content.push(
            text(message)
                .size(style::BODY - 1.0)
                .style(style::dim)
                .width(Fill),
        );
    }

    if app.usage().over() {
        content = content.push(
            text(
                "These images exceed the flash capacity. Remove a slot, or choose ROM types \
                 with smaller image sizes.",
            )
            .size(style::NOTE)
            .style(style::danger),
        );
    }

    content.into()
}

/// The log pane: read-only, selectable, and scrolling.
fn log(app: &Screen) -> Element<'_, Message> {
    text_editor(&app.log_tail)
        .on_action(Message::Log)
        .height(Length::Fixed(120.0))
        .padding(10)
        .size(style::NOTE)
        .font(style::MONO)
        .style(|_theme, _status| iced::widget::text_editor::Style {
            background: style::BG.into(),
            border: iced::Border {
                color: style::BORDER,
                width: 1.0,
                radius: 6.0.into(),
            },
            placeholder: style::DIM,
            value: style::DIM,
            selection: iced::Color {
                a: 0.35,
                ..style::GOLD
            },
        })
        .into()
}
