// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The board wireframe and the per-slot jumper block.
//!
//! Both are drawn from [`crate::catalog::header_positions`], so a board
//! revision that moves its 5V/GND pair, or has none, draws correctly with no
//! change here. Sizes are the CSS ones: the wireframe is the `.sb-board` rule
//! and the mini block is `.sb-jhint .sb-pin`.

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, alignment};

use crate::catalog::HeaderPosition;
use crate::ui::style;

/// The wireframe's overall size, from `.sb-board`.
pub const BOARD: Size = Size::new(124.0, 210.0);
/// The mini jumper block drawn in a slot card's title row.
pub const BLOCK_HEIGHT: f32 = 22.0;

/// One jumper cell in the wireframe: a 16x32 pad pair under an 18px column.
const CELL_WIDTH: f32 = 18.0;
const PAD_WIDTH: f32 = 16.0;
const PAD_HEIGHT: f32 = 32.0;
const CELL_GAP: f32 = 3.0;

/// One pin of a slot card's mini block.
const PIN_WIDTH: f32 = 11.0;
const PIN_GAP: f32 = 3.0;

/// The board drawn top-down, component side up, with its jumper header along
/// the top edge.
#[derive(Debug)]
pub struct Wireframe {
    /// The header columns, left to right.
    pub positions: Vec<HeaderPosition>,
    /// The board's short code, printed under the brand.
    pub code: String,
}

impl<Message> canvas::Program<Message> for Wireframe {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        outline(&mut frame);
        pin_one(&mut frame);
        self.header(&mut frame);
        self.brand(&mut frame);
        usb_port(&mut frame);

        vec![frame.into_geometry()]
    }
}

impl Wireframe {
    /// The jumper header along the top edge, centred in the board's width.
    fn header(&self, frame: &mut Frame) {
        let span = self.positions.len() as f32 * CELL_WIDTH
            + (self.positions.len().saturating_sub(1)) as f32 * CELL_GAP;
        let mut x = (BOARD.width - span) / 2.0;
        let top = 22.5;

        for position in &self.positions {
            let pad = Rectangle {
                x: x + (CELL_WIDTH - PAD_WIDTH) / 2.0,
                y: top,
                width: PAD_WIDTH,
                height: PAD_HEIGHT,
            };
            draw_position(frame, *position, pad, 3.0, true);
            x += CELL_WIDTH + CELL_GAP;
        }
    }

    /// "One ROM" with a gold O, and the board's short code beneath it.
    ///
    /// The gold O has to be its own text run, which means laying the three
    /// runs out by hand — canvas text carries no measurement, so the advances
    /// come from [`michroma_advance`].
    fn brand(&self, frame: &mut Frame) {
        let size = 15.0;
        let centre = BOARD.height / 2.0;

        let (left, middle, right) = ("One R", "O", "M");
        let total: f32 = [left, middle, right]
            .iter()
            .map(|run| michroma_width(run, size))
            .sum();
        let mut x = (BOARD.width - total) / 2.0;

        for (run, color) in [
            (left, style::WIRE),
            (middle, style::GOLD),
            (right, style::WIRE),
        ] {
            frame.fill_text(Text {
                content: run.to_owned(),
                position: Point::new(x, centre - size - 2.0),
                color,
                size: size.into(),
                font: style::MICHROMA,
                align_y: alignment::Vertical::Top,
                ..Text::default()
            });
            x += michroma_width(run, size);
        }

        frame.fill_text(Text {
            content: self.code.clone(),
            position: Point::new(BOARD.width / 2.0, centre + 1.0),
            color: style::WIRE,
            size: size.into(),
            font: style::MICHROMA,
            align_x: iced::Alignment::Center.into(),
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }
}

/// How wide a run of Michroma is at `size`.
fn michroma_width(run: &str, size: f32) -> f32 {
    run.chars().map(michroma_advance).sum::<f32>() * size
}

/// One character's advance in Michroma, as a fraction of the em.
///
/// Read out of `Michroma-Regular.ttf`'s `hmtx` table. Only the characters the
/// wireframe prints are listed — the brand, and the digits and capitals a
/// board short code is made of.
fn michroma_advance(character: char) -> f32 {
    match character {
        'n' => 0.7817,
        'e' => 0.7651,
        ' ' => 0.2842,
        '0' | '1' | '2' | '3' | '5' | '6' | '8' => 0.9512,
        '4' | 'V' => 1.0000,
        '7' | '9' => 0.9688,
        'A' | 'X' | 'Y' => 1.0625,
        'B' => 1.0312,
        'C' | 'O' | 'Q' => 1.0474,
        'D' => 1.0776,
        'E' => 0.8750,
        'F' => 0.8438,
        'G' => 1.0630,
        'H' => 1.0664,
        'I' => 0.2812,
        'J' => 0.8374,
        'K' => 0.9844,
        'L' => 0.8125,
        'M' => 1.3711,
        'N' => 1.1333,
        'P' => 0.9722,
        'R' => 1.0190,
        'S' => 1.0459,
        'T' => 0.9355,
        'U' => 1.0352,
        'W' => 1.6250,
        'Z' => 0.9990,
        _ => 1.0,
    }
}

/// The board edge: a rounded rectangle in `--wire`.
fn outline(frame: &mut Frame) {
    let path = Path::rounded_rectangle(
        Point::new(0.75, 0.75),
        Size::new(BOARD.width - 1.5, BOARD.height - 1.5),
        9.0.into(),
    );
    frame.stroke(
        &path,
        Stroke::default().with_color(style::WIRE).with_width(1.5),
    );
}

/// The pin-1 marker: a filled triangle in the top-left corner.
fn pin_one(frame: &mut Frame) {
    let triangle = Path::new(|builder| {
        builder.move_to(Point::new(1.0, 1.0));
        builder.line_to(Point::new(19.0, 1.0));
        builder.line_to(Point::new(1.0, 19.0));
        builder.close();
    });
    frame.fill(&triangle, style::WIRE);
}

/// The USB connector: a box butting the bottom edge, open at the bottom.
fn usb_port(frame: &mut Frame) {
    let width = 50.0;
    let height = 34.0;
    let left = (BOARD.width - width) / 2.0;
    let top = BOARD.height - height;

    let bottom = BOARD.height - 1.5;
    let path = Path::new(|builder| {
        builder.move_to(Point::new(left, bottom));
        builder.line_to(Point::new(left, top + 5.0));
        builder.quadratic_curve_to(Point::new(left, top), Point::new(left + 5.0, top));
        builder.line_to(Point::new(left + width - 5.0, top));
        builder.quadratic_curve_to(
            Point::new(left + width, top),
            Point::new(left + width, top + 5.0),
        );
        builder.line_to(Point::new(left + width, bottom));
    });
    frame.stroke(
        &path,
        Stroke::default().with_color(style::WIRE).with_width(1.5),
    );

    frame.fill_text(Text {
        content: "USB".to_owned(),
        position: Point::new(BOARD.width / 2.0, top + height / 2.0),
        color: style::WIRE,
        size: 12.0.into(),
        font: style::MONO,
        align_x: iced::Alignment::Center.into(),
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
}

/// One header position, as a pad rectangle plus whatever marks it.
///
/// `lettered` distinguishes the wireframe, where a select column carries its
/// silkscreen letter below the pad, from a slot card's block, where the letter
/// would repeat what the text beside it already says.
fn draw_position(
    frame: &mut Frame,
    position: HeaderPosition,
    pad: Rectangle,
    radius: f32,
    lettered: bool,
) {
    let outline = |frame: &mut Frame, color: Color| outline_pad(frame, pad, radius, color);

    match position {
        HeaderPosition::Gap => {}
        HeaderPosition::Power => {
            outline(frame, style::POWER);
            cross(frame, pad, style::POWER);
            if lettered {
                label(frame, pad, "5V", style::POWER, 9.0, 0.0);
                label(frame, pad, "GND", style::POWER, 9.0, 10.0);
            }
        }
        HeaderPosition::Other => {
            outline(frame, style::BORDER);
            cross(frame, pad, style::BORDER);
        }
        HeaderPosition::Select { letter, .. } => {
            outline(frame, style::GOLD);
            centre_line(frame, pad, style::GOLD);
            if lettered {
                label(frame, pad, &letter.to_string(), style::GOLD, 11.0, 0.0);
            }
        }
    }
}

/// The 1px outline of one pad position.
fn outline_pad(frame: &mut Frame, pad: Rectangle, radius: f32, color: Color) {
    let path = Path::rounded_rectangle(
        Point::new(pad.x + 0.5, pad.y + 0.5),
        Size::new(pad.width - 1.0, pad.height - 1.0),
        radius.into(),
    );
    frame.stroke(&path, Stroke::default().with_color(color).with_width(1.0));
}

/// The corner-to-corner cross marking a pad that is not a slot select.
fn cross(frame: &mut Frame, pad: Rectangle, color: Color) {
    let stroke = Stroke::default().with_color(color).with_width(1.5);
    let inset = 1.5;
    for (from, to) in [
        (
            Point::new(pad.x + inset, pad.y + inset),
            Point::new(pad.x + pad.width - inset, pad.y + pad.height - inset),
        ),
        (
            Point::new(pad.x + pad.width - inset, pad.y + inset),
            Point::new(pad.x + inset, pad.y + pad.height - inset),
        ),
    ] {
        frame.stroke(&Path::line(from, to), stroke);
    }
}

/// The line splitting a two-pin position into its top and bottom pad.
fn centre_line(frame: &mut Frame, pad: Rectangle, color: Color) {
    let y = pad.y + pad.height / 2.0;
    frame.stroke(
        &Path::line(Point::new(pad.x, y), Point::new(pad.x + pad.width, y)),
        Stroke::default().with_color(color).with_width(1.0),
    );
}

/// A silkscreen label centred under a pad.
fn label(frame: &mut Frame, pad: Rectangle, content: &str, color: Color, size: f32, offset: f32) {
    frame.fill_text(Text {
        content: content.to_owned(),
        position: Point::new(pad.x + pad.width / 2.0, pad.y + pad.height + 3.0 + offset),
        color,
        size: size.into(),
        font: style::MONO,
        align_x: iced::Alignment::Center.into(),
        align_y: alignment::Vertical::Top,
        ..Text::default()
    });
}

/// The header as it should look for one slot: the selects this slot closes are
/// filled gold, the rest are drawn as they are on the board.
#[derive(Debug)]
pub struct SlotBlock {
    /// The header columns, left to right.
    pub positions: Vec<HeaderPosition>,
    /// The slot number, whose bits say which selects are closed.
    pub slot: usize,
}

impl SlotBlock {
    /// How wide the block draws, so the caller can size its canvas.
    pub fn width(&self) -> f32 {
        self.positions.len() as f32 * PIN_WIDTH
            + (self.positions.len().saturating_sub(1)) as f32 * PIN_GAP
    }
}

impl<Message> canvas::Program<Message> for SlotBlock {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let mut x = 0.0;

        for position in &self.positions {
            let pad = Rectangle {
                x,
                y: 0.0,
                width: PIN_WIDTH,
                height: BLOCK_HEIGHT,
            };

            match position {
                HeaderPosition::Select { bit, .. } if (self.slot >> bit) & 1 == 1 => {
                    let path = Path::rounded_rectangle(
                        Point::new(pad.x, pad.y),
                        Size::new(pad.width, pad.height),
                        2.0.into(),
                    );
                    frame.fill(&path, style::GOLD);
                }
                HeaderPosition::Select { .. } => {
                    outline_pad(&mut frame, pad, 2.0, style::BORDER);
                    centre_line(&mut frame, pad, style::BORDER);
                }
                other => draw_position(&mut frame, *other, pad, 2.0, false),
            }

            x += PIN_WIDTH + PIN_GAP;
        }

        vec![frame.into_geometry()]
    }
}
