//! Runs the log screen on its own.
//!
//! Everything here is what it takes to host one screen: the shared state it
//! reads, a window, a theme, and forwarding of the four calls iced makes.  The
//! screen itself is [`studiov2_log_viewer::screen`], and the shell at
//! `../../shell` hosts the same type beside the builder.
//!
//! Run with `--help` for the switches.

use studiov2_log_viewer::{options, screen};
use studiov2_shared::{Shared, style};

fn main() -> iced::Result {
    let options = match options::Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}\n\n{}", options::USAGE);
            std::process::exit(2);
        }
    };

    iced::application(
        move || Standalone::boot(options.clone()),
        Standalone::update,
        Standalone::view,
    )
    .title(Standalone::title)
    .subscription(Standalone::subscription)
    .theme(theme)
    .window_size((1180.0, 760.0))
    .run()
}

/// The theme the window runs under.
///
/// A named function, not a closure: a closure here fails to infer a
/// higher-ranked lifetime and iced reports it as `FnOnce is not general
/// enough`.
fn theme(_state: &Standalone) -> iced::Theme {
    style::theme()
}

/// The screen, plus the state it reads.
struct Standalone {
    /// What the screen and everything else reads.  The shell holds one of
    /// these too.
    shared: Shared,
    /// The screen under test.
    screen: screen::Screen,
}

impl Standalone {
    /// Opens a log and boots the screen over it.
    fn boot(options: options::Options) -> (Self, iced::Task<screen::Message>) {
        let mut shared = match Shared::stub() {
            Ok(shared) => shared,
            Err(error) => {
                eprintln!("could not open a log store: {error}");
                std::process::exit(1);
            }
        };

        let (screen, task) = screen::Screen::boot(options, &mut shared);

        // Bring the window to the front on launch, the way any app does.
        let start = iced::window::latest()
            .and_then(iced::window::gain_focus)
            .chain(task);

        (Self { shared, screen }, start)
    }

    /// The window title.
    fn title(&self) -> String {
        self.screen.title(&self.shared)
    }

    /// Hands a message to the screen.
    fn update(&mut self, message: screen::Message) -> iced::Task<screen::Message> {
        self.screen.update(message, &mut self.shared)
    }

    /// Draws the screen.
    fn view(&self) -> iced::Element<'_, screen::Message> {
        self.screen.view(&self.shared)
    }

    /// What the screen listens to.
    fn subscription(&self) -> iced::Subscription<screen::Message> {
        self.screen.subscription()
    }
}
