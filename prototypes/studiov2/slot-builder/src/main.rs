// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Runs the slot builder on its own.
//!
//! Everything here is what it takes to host one screen: the shared state it
//! reads, a window, fonts, a theme, and forwarding of the three calls iced
//! makes.  The screen itself is [`studiov2_slot_builder::screen`], and the
//! shell at `../../shell` hosts the same type beside the log pane.

use iced::Font;
use studiov2_shared::{Shared, style};
use studiov2_slot_builder::{FONTS, Message, Screen, dev};

fn main() -> iced::Result {
    iced::application(Standalone::boot, Standalone::update, Standalone::view)
        .title("One ROM Builder - prototype")
        .theme(theme)
        .font(FONTS[0])
        .font(FONTS[1])
        .font(FONTS[2])
        .default_font(Font::with_name("Inter"))
        .window_size(dev::window_size())
        .run()
}

/// The theme the window runs under.
fn theme(_state: &Standalone) -> iced::Theme {
    style::theme()
}

/// The screen, plus the state it reads.
struct Standalone {
    /// What the screen and everything else reads.  The shell holds one of
    /// these too.
    shared: Shared,
    /// The screen under test.
    screen: Screen,
}

impl Standalone {
    /// Opens a log and boots the builder over it.
    fn boot() -> (Self, iced::Task<Message>) {
        let mut shared = match Shared::stub() {
            Ok(shared) => shared,
            Err(error) => {
                eprintln!("could not open a log store: {error}");
                std::process::exit(1);
            }
        };

        let (screen, task) = Screen::boot(&mut shared);
        (Self { shared, screen }, task)
    }

    /// Hands a message to the screen.
    fn update(&mut self, message: Message) -> iced::Task<Message> {
        self.screen.update(message, &mut self.shared)
    }

    /// Draws the screen.
    fn view(&self) -> iced::Element<'_, Message> {
        self.screen.view(&self.shared)
    }
}
