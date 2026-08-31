// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Driving the shell without a screen-recording grant.
//!
//! `ONEROM_SHELL_SHOT` is a path to write a PNG to and `ONEROM_SHELL_SETUP` is
//! a comma-separated script: `logs` shows the log screen, `device:SERIAL`
//! picks a device, `stream` lets the log source run for a second first.

use std::path::PathBuf;

use iced::window;

use crate::{DeviceChoice, Message, Showing};

/// Where a screenshot run writes its picture.
pub fn shot_path() -> Option<PathBuf> {
    std::env::var("ONEROM_SHELL_SHOT").ok().map(PathBuf::from)
}

/// The script the run was asked to perform before the picture.
fn setup() -> Vec<String> {
    std::env::var("ONEROM_SHELL_SETUP")
        .unwrap_or_default()
        .split(',')
        .filter(|step| !step.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The whole scripted run: set the shell up, wait, capture, exit.
pub fn capture() -> iced::Task<Message> {
    let mut task = iced::Task::none();

    for step in setup() {
        let parts: Vec<&str> = step.split(':').collect();
        task = match parts.as_slice() {
            ["logs"] => task.chain(iced::Task::done(Message::Show(Showing::Logs))),
            ["device", serial] => {
                let device = studiov2_shared::device::attached()
                    .into_iter()
                    .find(|device| device.serial == *serial);
                task.chain(iced::Task::done(Message::DeviceSelected(DeviceChoice(
                    device,
                ))))
            }
            ["stream"] => task.chain(settle(1_200)),
            _ => {
                eprintln!("unrecognised setup step: {step}");
                task
            }
        };
    }

    // Two captures, and the first is thrown away: a screenshot renders its
    // own frame, and the first one after a period of quiet comes back with
    // glyphs missing while the text atlas is still filling.
    task.chain(settle(1_500))
        .chain(window::oldest().and_then(window::screenshot).discard())
        .chain(settle(600))
        .chain(
            window::oldest()
                .and_then(window::screenshot)
                .map(Message::Shot),
        )
}

/// Waits, without blocking the update loop.
fn settle(millis: u64) -> iced::Task<Message> {
    iced::Task::future(tokio::time::sleep(std::time::Duration::from_millis(millis))).discard()
}

/// Writes a captured window to `path` as a PNG.
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
    writer.write_image_data(&shot.rgba)?;
    println!("--- wrote {} ---", path.display());
    Ok(())
}
