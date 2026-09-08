// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Runs the generated screen on its own.
//!
//! Everything here is what it takes to host one screen: the shared state it
//! reads, a window, fonts, a theme, and forwarding of the three calls iced
//! makes.  The screen itself is [`studiov2_generated::screen`], and the shell
//! hosts the same type beside the other screens.
//!
//! `--list` prints every command path, one per line, which is what a review
//! run of all of the panes loops over.

use iced::Font;
use studiov2_commands::COMMANDS;
use studiov2_generated::{FONTS, Message, Screen, dev};
use studiov2_shared::{Shared, style};

fn main() -> iced::Result {
    if std::env::args().skip(1).any(|arg| arg == "--list") {
        for command in COMMANDS {
            println!("{}", command.path.join(" "));
        }
        return Ok(());
    }

    iced::application(Standalone::boot, Standalone::update, Standalone::view)
        .title("One ROM generated panes - prototype")
        .theme(theme)
        .font(FONTS[0])
        .font(FONTS[1])
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
    /// Opens a log and boots the screen over it.
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
