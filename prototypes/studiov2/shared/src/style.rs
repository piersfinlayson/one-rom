// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The site's palette and widget shapes.
//!
//! Colours are the `:root` custom properties from `one-rom-site/style.css`, and
//! the sizes are the `#slotBuilder .sb-*` rules, so the two can be compared
//! side by side.

use iced::widget::{button, container, pick_list, rule, text, text_input};
use iced::{Background, Border, Color, Font, Theme, color};

// ------------------------------------------------------------- palette ------

/// `--text-color`.
pub const TEXT: Color = color!(0xe2e8f0);
/// `--text-secondary`.
pub const DIM: Color = color!(0x9a9aa8);
/// `--bg-color`: the page ground and the inside of a card.
pub const BG: Color = color!(0x181820);
/// `--bg-secondary`: the panel the builder sits on.
pub const PANEL: Color = color!(0x28282f);
/// `--input-bg-color`.
pub const INPUT: Color = color!(0x3a3a42);
/// `--button-bg-color`, also `--border-color`.
pub const BORDER: Color = color!(0x4a4a52);
/// `--button-hover-color`.
pub const HOVER: Color = color!(0x5a5a62);
/// `--one-rom-gold`.
pub const GOLD: Color = color!(0xffb700);
/// `--one-rom-gold-darker`.
pub const GOLD_DARK: Color = color!(0xcc9200);
/// `--wire`: every structural line of the board drawing.
pub const WIRE: Color = color!(0x7a7a86);
/// `--cyan`: the firmware segment.
pub const CYAN: Color = color!(0x2fb6cf);
/// `--power`: the 5V/GND cross.
pub const POWER: Color = color!(0xb0563a);
/// `--bronze`: a plugin segment.
pub const BRONZE: Color = color!(0x8a6a2f);
/// `--danger`.
pub const DANGER: Color = color!(0xe5484d);
/// `--danger-dim`: the delete button's hover.
pub const DANGER_DIM: Color = color!(0x7a2a2d);

// --------------------------------------------------------------- type -------

/// Body text, `0.9rem`.
pub const BODY: f32 = 14.0;
/// A section heading, `1.25rem`.
pub const HEADING: f32 = 20.0;
/// A slot's own title, `1.05rem`.
pub const SLOT_TITLE: f32 = 17.0;
/// A note or hint under a control, `0.8rem`.
pub const NOTE: f32 = 12.5;
/// The uppercase group titles inside a slot card, `0.68rem`.
pub const GROUP: f32 = 11.0;

/// The weight the site's `h3` section headings carry.
pub const MEDIUM: Font = Font {
    weight: iced::font::Weight::Medium,
    ..Font::DEFAULT
};
/// The weight a slot title carries.
pub const SEMIBOLD: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..Font::DEFAULT
};

/// The board wireframe's brand mark.
pub const MICHROMA: Font = Font::with_name("Michroma");
/// `--mono`, for the jumper letters and the USB label.
pub const MONO: Font = Font::MONOSPACE;

/// The theme the app runs under.
///
/// Only the runtime's own defaults come from here — every widget in the
/// builder carries an explicit style below — but the palette still has to be
/// right, because it is what a scrollbar and a text cursor pick up.
pub fn theme() -> Theme {
    Theme::custom(
        "One ROM".to_owned(),
        iced::theme::Palette {
            background: BG,
            text: TEXT,
            primary: GOLD,
            success: CYAN,
            warning: BRONZE,
            danger: DANGER,
        },
    )
}

// ------------------------------------------------------------ containers ----

/// The panel the whole builder sits on: `--bg-secondary`.
pub fn panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL)),
        ..container::Style::default()
    }
}

/// A slot card, and the jumper legend box: `--bg-color` inside a 1px border,
/// 8px radius.
pub fn card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

/// The flash bar's trough: `--input-bg-color` behind the segments.
pub fn trough(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(INPUT)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 5.0.into(),
        },
        ..container::Style::default()
    }
}

/// A solid block of colour: one flash segment, or a legend swatch.
pub fn swatch(fill: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(fill)),
        ..container::Style::default()
    }
}

/// A horizontal rule between sections.
pub fn divider(_theme: &Theme) -> rule::Style {
    rule::Style {
        color: BORDER,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

// ----------------------------------------------------------------- text -----

/// Secondary text: labels, hints, figures.
pub fn dim(_theme: &Theme) -> text::Style {
    text::Style { color: Some(DIM) }
}

/// Gold text: a slot title, a chosen filename.
pub fn gold(_theme: &Theme) -> text::Style {
    text::Style { color: Some(GOLD) }
}

/// An error line, or an over-capacity figure.
pub fn danger(_theme: &Theme) -> text::Style {
    text::Style {
        color: Some(DANGER),
    }
}

// -------------------------------------------------------------- buttons -----

/// The site's `.gold-button`: gold, with `--bg-color` text, dimming to grey
/// when it has nothing to do.
pub fn gold_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Disabled => (BORDER, DIM),
        button::Status::Hovered | button::Status::Pressed => (GOLD_DARK, BG),
        button::Status::Active => (GOLD, BG),
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// The site's `.file-button`: a gold pill with a border, used to pick an image.
pub fn file_button(theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..gold_button(theme, status)
    }
}

/// A square `.sb-icon-btn`: move up, move down.
pub fn icon_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Disabled => (BORDER, Color { a: 0.3, ..TEXT }),
        button::Status::Hovered | button::Status::Pressed => (HOVER, TEXT),
        button::Status::Active => (BORDER, TEXT),
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

/// The remove button: the same square, turning red under the pointer.
pub fn remove_button(theme: &Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(DANGER_DIM)),
            border: Border {
                color: DANGER,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..icon_button(theme, status)
        },
        _ => icon_button(theme, status),
    }
}

// ------------------------------------------------------------- controls -----

/// A dropdown: `--input-bg-color` in a 1px border, gold on focus.
pub fn picker(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let border_color = match status {
        pick_list::Status::Hovered | pick_list::Status::Opened { .. } => GOLD,
        pick_list::Status::Active => BORDER,
    };

    pick_list::Style {
        text_color: TEXT,
        placeholder_color: DIM,
        handle_color: DIM,
        background: Background::Color(INPUT),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 6.0.into(),
        },
    }
}

/// A text field, matching the dropdowns.
pub fn field(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused { .. } => GOLD,
        _ => BORDER,
    };

    text_input::Style {
        background: Background::Color(INPUT),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 6.0.into(),
        },
        icon: DIM,
        placeholder: DIM,
        value: TEXT,
        selection: Color { a: 0.35, ..GOLD },
    }
}
