// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The widget shapes this screen needs that no other screen does.
//!
//! Everything else comes from [`studiov2_shared::style`], re-exported here so
//! the rest of the crate reaches one palette rather than two.  A tab and a
//! checkbox are here because the builder has neither, and Iced has no
//! stylesheet — a widget without an explicit style picks up the runtime's
//! defaults, which are not this palette.

pub use studiov2_shared::style::*;

use iced::widget::{button, checkbox, container};
use iced::{Background, Border, Color, Theme};

/// A navigation tab.
///
/// The selected one is gold on the page ground and the rest are flat, so a row
/// of them reads as one choice rather than a row of buttons.
pub fn tab(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let (background, text_color) = match (selected, status) {
            (true, _) => (GOLD, BG),
            (false, button::Status::Hovered | button::Status::Pressed) => (HOVER, TEXT),
            (false, _) => (BORDER, TEXT),
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color,
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..button::Style::default()
        }
    }
}

/// A disclosure heading: text and a marker, with no button around them.
pub fn disclosure(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: TEXT,
        ..button::Style::default()
    }
}

/// A small square button beside a value: clear, add, remove.
pub fn small(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Disabled => (BORDER, Color { a: 0.3, ..TEXT }),
        button::Status::Hovered | button::Status::Pressed => (HOVER, TEXT),
        button::Status::Active => (BORDER, DIM),
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 5.0.into(),
        },
        ..button::Style::default()
    }
}

/// A checkbox, matching the text fields it sits among.
pub fn tick(_theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let checked = matches!(
        status,
        checkbox::Status::Active { is_checked: true }
            | checkbox::Status::Hovered { is_checked: true }
            | checkbox::Status::Disabled { is_checked: true }
    );

    let border_color = match status {
        checkbox::Status::Hovered { .. } => GOLD,
        _ if checked => GOLD,
        _ => BORDER,
    };

    checkbox::Style {
        background: Background::Color(if checked { GOLD } else { INPUT }),
        icon_color: BG,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(TEXT),
    }
}

/// The box the built command line sits in: fixed pitch, on the page ground.
pub fn terminal(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG)),
        border: Border {
            color: GOLD_DARK,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..container::Style::default()
    }
}

/// One square of the copy button's two-square mark.
///
/// The mark is drawn rather than taken from a font: Inter carries no copy
/// glyph, and a missing one renders as a hollow box, which reads as a bug.
pub fn icon_square(_theme: &Theme) -> container::Style {
    container::Style {
        border: iced::Border {
            color: studiov2_shared::style::DIM,
            width: 1.0,
            radius: 1.0.into(),
        },
        background: Some(studiov2_shared::style::BG.into()),
        ..container::Style::default()
    }
}
