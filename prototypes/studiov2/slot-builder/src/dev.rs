// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! A screenshot hook, so the prototype's own window can be compared against the
//! HTML it copies without a screen-recording grant.
//!
//! Set `ONEROM_PROTO_SHOT` to a path and the app applies whatever
//! `ONEROM_PROTO_SETUP` describes, writes a PNG of its window, and quits.
//! Unset, neither variable does anything and the app runs normally.

use std::path::PathBuf;

use iced::widget::text_editor;
use iced::window;
use onerom_config::hw::Board;

use crate::catalog::{self, BoardChoice};
use crate::slot::Slot;
use crate::{Message, Screen};
use studiov2_shared::Shared;

/// The window size a screenshot run asks for, as `WIDTHxHEIGHT`.
pub fn window_size() -> iced::Size {
    let default = iced::Size::new(980.0, 900.0);
    let Ok(text) = std::env::var("ONEROM_PROTO_SIZE") else {
        return default;
    };
    match text.split_once('x') {
        Some((width, height)) => match (width.parse(), height.parse()) {
            (Ok(width), Ok(height)) => iced::Size::new(width, height),
            _ => default,
        },
        None => default,
    }
}

/// Where a screenshot should be written, if anywhere.
pub fn shot_path() -> Option<PathBuf> {
    std::env::var_os("ONEROM_PROTO_SHOT").map(PathBuf::from)
}

/// Apply `ONEROM_PROTO_SETUP` to a fresh app.
///
/// The script is comma-separated: `board:fire-28-d`, `add` for another slot,
/// `chip:0:23128`, `file:1:/tmp/kernal.bin`, `ihex:1`, `nohelp`, `log` for a
/// few lines of log text.
pub fn apply_setup(app: &mut Screen, shared: &mut Shared) {
    let Ok(script) = std::env::var("ONEROM_PROTO_SETUP") else {
        return;
    };

    for step in script.split(',').filter(|step| !step.is_empty()) {
        let parts: Vec<&str> = step.split(':').collect();
        match parts.as_slice() {
            ["nohelp"] => app.help_open = false,
            // Handled by `capture`, not here.
            ["press"] | ["bottom"] => {}
            ["add"] => app.slots.push(Slot::after(app.slots.last())),
            ["select"] => {
                // Selection in the read-only log pane: two words in, three
                // words selected, then what the clipboard would receive.
                app.log_tail.perform(text_editor::Action::Move(
                    text_editor::Motion::DocumentStart,
                ));
                for _ in 0..3 {
                    app.log_tail
                        .perform(text_editor::Action::Move(text_editor::Motion::Down));
                }
                for _ in 0..2 {
                    app.log_tail
                        .perform(text_editor::Action::Move(text_editor::Motion::WordRight));
                }
                for _ in 0..3 {
                    app.log_tail
                        .perform(text_editor::Action::Select(text_editor::Motion::WordRight));
                }
                println!("log selection: {:?}", app.log_tail.selection());
            }
            ["build"] => match app.request() {
                Some(request) => match crate::build::run(request) {
                    Ok(built) => println!(
                        "build ok: {} metadata + {} images ({})",
                        built.metadata.len(),
                        built.images.len(),
                        built.description
                    ),
                    Err(error) => println!("build failed: {}", crate::error::chain(&error)),
                },
                None => println!("build: form incomplete"),
            },
            ["log"] => {
                let lines: Vec<std::sync::Arc<str>> = (1..=8)
                    .map(|line| {
                        std::sync::Arc::from(format!(
                            "log line {line}: the quick brown fox jumps \
                             over the lazy dog"
                        ))
                    })
                    .collect();
                if let Err(error) = shared.log.append(&lines) {
                    eprintln!("log: {error}");
                }
                app.refresh_log(&shared.log);
            }
            ["board", name] => {
                if let Some(board) = Board::try_from_str(name) {
                    app.board = Some(BoardChoice(board));
                    app.chips = catalog::chip_types(board);
                }
            }
            ["device", serial] => {
                shared.device = studiov2_shared::device::attached()
                    .into_iter()
                    .find(|device| device.serial == *serial);
            }
            ["chip", index, alias] => {
                let chip = app.chips.iter().find(|chip| chip.alias == *alias).copied();
                if let Ok(index) = index.parse::<usize>()
                    && let Some(slot) = app.slots.get_mut(index)
                {
                    slot.chip = chip;
                }
            }
            ["file", index, path] => {
                if let Ok(index) = index.parse::<usize>()
                    && let Some(slot) = app.slots.get_mut(index)
                {
                    slot.file = Some(PathBuf::from(path));
                }
            }
            ["ihex", index] => {
                if let Ok(index) = index.parse::<usize>()
                    && let Some(slot) = app.slots.get_mut(index)
                {
                    slot.format = onerom_gen::FileFormat::IntelHex;
                }
            }
            _ => eprintln!("unrecognised setup step: {step}"),
        }
    }
}

/// Wait for the first frame, grab the window, and hand the pixels back.
///
/// A `press` step in the setup script sends [`Message::Build`] first, so the
/// screenshot shows the real update path's result rather than a state poked
/// into place.
pub fn capture() -> iced::Task<Message> {
    let setup = std::env::var("ONEROM_PROTO_SETUP").unwrap_or_default();
    let shot = shot();

    let mut tasks = Vec::new();
    if setup.split(',').any(|step| step == "press") {
        tasks.push(iced::Task::done(Message::Build));
    }
    if setup.split(',').any(|step| step == "bottom") {
        tasks.push(iced::widget::operation::snap_to_end(crate::ui::scroll_id()));
    }
    tasks.push(shot);
    iced::Task::batch(tasks)
}

/// The screenshot itself, once the window has had time to draw.
fn shot() -> iced::Task<Message> {
    iced::Task::future(async {
        std::thread::sleep(std::time::Duration::from_millis(1400));
    })
    .then(|()| window::oldest())
    .and_then(window::screenshot)
    .map(Message::Screenshot)
}

/// Write a captured window to `path` as a PNG.
pub fn write_png(path: &std::path::Path, shot: &window::Screenshot) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        shot.size.width,
        shot.size.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(&shot.rgba)
        .map_err(std::io::Error::other)
}
