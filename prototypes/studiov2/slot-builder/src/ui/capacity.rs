// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The flash usage bar.
//!
//! One proportional segment per thing that occupies flash, against the MCU's
//! total, with the figures beside it and a key underneath. Over capacity, the
//! segment that spills is drawn in `--danger` and the figures turn red — the
//! `.sb-over` state.

use iced::widget::{Space, column, container, row, text};
use iced::{Center, Color, Element, Fill, Length, Shrink};

use crate::build::{SegmentKind, Usage};
use crate::catalog;
use crate::ui::style;
use crate::{Message, Screen};

/// The bar's height, from `.sb-cap-bar`.
const BAR_HEIGHT: f32 = 16.0;

/// The smallest share a segment is given, so a 1KB piece is still visible.
/// `.sb-seg` does this with `min-width: 3px`.
const MIN_PORTION: u16 = 5;

/// The whole tally: heading, bar and key.
pub fn view(app: &Screen) -> Element<'_, Message> {
    let usage = app.usage();
    let count = app.slots.len();
    let plural = if count == 1 { "slot" } else { "slots" };

    let figures = format!(
        "{} / {} used   -   {}",
        catalog::kb(usage.used),
        catalog::kb(usage.total),
        if usage.over() {
            format!("{} over", catalog::kb(usage.excess()))
        } else {
            format!("{} free", catalog::kb(usage.free()))
        }
    );

    let head = row![
        row![
            text("Flash usage").size(13.5),
            text(format!("({count} {plural})"))
                .size(13.5)
                .style(style::dim),
        ]
        .spacing(6),
        Space::new().width(Fill).height(Shrink),
        text(figures).size(13.5).style(if usage.over() {
            style::danger
        } else {
            style::dim
        }),
    ]
    .align_y(Center);

    column![head, bar(&usage), key()].spacing(6).into()
}

/// The bar itself: the segments over the trough.
fn bar<'a>(usage: &Usage) -> Element<'a, Message> {
    let mut segments = row![];
    let mut placed = 0u64;

    for segment in &usage.segments {
        let room = usage.total.saturating_sub(placed);
        let shown = segment.bytes.min(room);
        placed += segment.bytes;

        // Over-capacity: the segment that runs off the end is the one drawn in
        // red, so the bar says which addition broke it. A segment with no room
        // left at all still shows, as the floor in `portion` keeps it visible.
        let fill = if segment.bytes > shown {
            style::DANGER
        } else {
            colour(segment.kind)
        };

        segments = segments.push(
            container(Space::new().width(Fill).height(Fill))
                .style(style::swatch(fill))
                .width(Length::FillPortion(portion(shown, usage.total)))
                .height(Fill),
        );
    }

    // Whatever is left is the trough showing through.
    let free = usage.free();
    if free > 0 {
        segments = segments.push(
            Space::new()
                .width(Length::FillPortion(portion(free, usage.total)))
                .height(Fill),
        );
    }

    container(segments)
        .style(style::trough)
        .height(BAR_HEIGHT)
        .width(Fill)
        .clip(true)
        .into()
}

/// A segment's share of the bar, floored so a small one still shows.
fn portion(bytes: u64, total: u64) -> u16 {
    let scaled = (bytes * 2048 / total.max(1)) as u16;
    scaled.max(MIN_PORTION)
}

/// The key under the bar.
fn key<'a>() -> Element<'a, Message> {
    row([
        ("Firmware", style::CYAN),
        ("Plugin", style::BRONZE),
        ("ROM image", style::GOLD),
        ("Free", style::INPUT),
    ]
    .into_iter()
    .map(|(name, fill)| {
        row![
            container(Space::new().width(10).height(10)).style(style::swatch(fill)),
            text(name).size(12.0).style(style::dim),
        ]
        .spacing(5)
        .align_y(Center)
        .into()
    }))
    .spacing(16)
    .into()
}

/// The colour a segment kind is drawn in.
fn colour(kind: SegmentKind) -> Color {
    match kind {
        SegmentKind::Firmware => style::CYAN,
        SegmentKind::Plugin => style::BRONZE,
        SegmentKind::Rom => style::GOLD,
    }
}
