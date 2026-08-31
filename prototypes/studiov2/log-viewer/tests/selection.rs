//! Does mouse selection and copy actually work in the log pane?
//!
//! macOS will not grant this machine permission to post synthetic mouse
//! events, so the test drives a real, headless iced `UserInterface` instead:
//! the same `text_editor` widget, the same `Widget::update`, the same
//! `Binding::from_key_press`, the same `Content`.  The only substitution is
//! the clipboard, which is a recording stand-in so the test can read back what
//! the widget wrote.
//!
//! Run with `cargo test --release -- --nocapture`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use iced::advanced::clipboard::{Clipboard, Kind};
use iced::advanced::renderer::Headless;
use iced::widget::{text, text_editor};
use iced::{Element, Event, Font, Length, Pixels, Point, Size, keyboard, mouse};
use iced_runtime::user_interface::{self, UserInterface};

use studiov2_log_viewer::logpane::LogPane;

/// The size of the simulated window.
const SIZE: Size = Size::new(1200.0, 400.0);

/// The editor's padding, which the widget subtracts from cursor positions.
const PADDING: f32 = 8.0;

/// The point size the pane renders at.
const TEXT_SIZE: f32 = 12.0;

/// A clipboard that remembers what was written to it.
#[derive(Clone, Default)]
struct Recorder(Rc<RefCell<Option<String>>>);

impl Recorder {
    /// What was last written, if anything.
    fn written(&self) -> Option<String> {
        self.0.borrow().clone()
    }
}

impl Clipboard for Recorder {
    fn read(&self, _kind: Kind) -> Option<String> {
        self.0.borrow().clone()
    }

    fn write(&mut self, _kind: Kind, contents: String) {
        *self.0.borrow_mut() = Some(contents);
    }
}

/// A harness holding a log pane and the interface that renders it.
struct Harness {
    /// The pane under test.
    pane: LogPane,
    /// The headless renderer.
    renderer: iced::Renderer,
    /// The interface's retained state between updates.
    cache: user_interface::Cache,
    /// Where the mouse is.
    cursor: mouse::Cursor,
    /// What the widget copied.
    clipboard: Recorder,
}

impl Harness {
    /// Builds a harness whose pane holds `lines` numbered lines.
    fn new(lines: &[&str]) -> Self {
        let renderer = futures_lite_block_on(<iced::Renderer as Headless>::new(
            Font::MONOSPACE,
            Pixels(TEXT_SIZE),
            None,
        ))
        .expect("a headless renderer");

        let mut pane = LogPane::new(None);
        let owned: Vec<Arc<str>> = lines.iter().map(|l| Arc::from(*l)).collect();
        pane.append(&owned);

        Self {
            pane,
            renderer,
            cache: user_interface::Cache::default(),
            cursor: mouse::Cursor::Unavailable,
            clipboard: Recorder::default(),
        }
    }

    /// Points the mouse at a window position.
    fn point_at(&mut self, x: f32, y: f32) {
        self.cursor = mouse::Cursor::Available(Point::new(x, y));
    }

    /// The y of the middle of the given line, in window coordinates.
    ///
    /// The default line height is 1.3 times the text size.
    fn line_y(line: usize) -> f32 {
        PADDING + (line as f32 + 0.5) * TEXT_SIZE * 1.3
    }

    /// Feeds events to the interface and applies the actions they produce.
    fn feed(&mut self, events: &[Event]) {
        // The element borrows the pane's content, so the pane cannot be
        // mutated while the interface exists.  Collect the actions, drop the
        // interface, then apply them — which is exactly what the real
        // application's message loop does.
        let mut actions = Vec::new();

        {
            let element: Element<'_, text_editor::Action> = text_editor(self.pane.content())
                .on_action(|action| action)
                .font(Font::MONOSPACE)
                .size(TEXT_SIZE)
                .wrapping(text::Wrapping::None)
                .height(Length::Fill)
                .padding(PADDING)
                .into();

            let mut interface = UserInterface::build(
                element,
                SIZE,
                std::mem::take(&mut self.cache),
                &mut self.renderer,
            );

            let _ = interface.update(
                events,
                self.cursor,
                &mut self.renderer,
                &mut self.clipboard,
                &mut actions,
            );

            self.cache = interface.into_cache();
        }

        for action in actions {
            self.pane.on_action(action);
        }
    }

    /// Presses the left mouse button where the cursor is.
    fn press(&mut self) {
        self.feed(&[Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))]);
    }

    /// Releases the left mouse button.
    fn release(&mut self) {
        self.feed(&[Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))]);
    }

    /// Moves the mouse to a window position, reporting the move.
    fn move_to(&mut self, x: f32, y: f32) {
        self.point_at(x, y);
        self.feed(&[Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(x, y),
        })]);
    }

    /// Taps a key with the given modifiers.
    fn tap(&mut self, key: keyboard::Key, modifiers: keyboard::Modifiers) {
        let text = match &key {
            keyboard::Key::Character(c) => Some(c.clone()),
            _ => None,
        };

        self.feed(&[Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
            repeat: false,
            text,
        })]);

        self.feed(&[Event::Keyboard(keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
        })]);
    }

    /// Taps a character key with the platform's command modifier held.
    fn command(&mut self, character: &str) {
        self.tap(
            keyboard::Key::Character(character.into()),
            keyboard::Modifiers::COMMAND,
        );
    }
}

/// Blocks on a future without pulling in a runtime.
fn futures_lite_block_on<F: std::future::Future>(future: F) -> F::Output {
    iced::futures::executor::block_on(future)
}

/// The lines every test selects from.
const LINES: [&str; 6] = [
    "00:00:00.000 [      0] INFO  serve: slot 0 armed",
    "00:00:00.007 [      1] DEBUG pio:   sm1 stalled 3 cycles",
    "00:00:00.014 [      2] TRACE dma:   channel 4 wrap",
    "00:00:00.021 [      3] WARN  flash: XIP cache miss 0.4%",
    "00:00:00.028 [      4] ERROR usb:   heartbeat missed",
    "00:00:00.035 [      5] INFO  led:   solid green",
];

#[test]
fn mouse_drag_selects_and_command_c_copies() {
    let mut harness = Harness::new(&LINES);

    // Press at the very start of line 1 and drag to the start of line 3.
    harness.point_at(PADDING + 1.0, Harness::line_y(1));
    harness.press();
    harness.move_to(PADDING + 200.0, Harness::line_y(2));
    harness.move_to(PADDING + 1.0, Harness::line_y(3));
    harness.release();

    let selection = harness
        .pane
        .selection()
        .expect("a mouse drag across three lines selects something");

    println!("drag selection:\n{selection}\n---");

    assert_eq!(
        selection,
        format!("{}\n{}\n", LINES[1], LINES[2]),
        "the drag should cover lines 1 and 2 whole"
    );

    // Now copy it the way a user would.
    harness.command("c");

    assert_eq!(
        harness.clipboard.written().as_deref(),
        Some(selection.as_str()),
        "Cmd+C should put exactly the selection on the clipboard"
    );

    println!(
        "clipboard after Cmd+C:\n{}\n---",
        harness.clipboard.written().unwrap_or_default()
    );
}

#[test]
fn double_click_selects_a_word() {
    let mut harness = Harness::new(&LINES);

    // The glyph advance of the default monospace face is not a constant this
    // test can assume, so measure it: click a known distance in and ask the
    // content which column the caret landed on.
    let probe = 200.0;
    harness.point_at(PADDING + probe, Harness::line_y(0));
    harness.press();
    harness.release();

    let column = harness.pane.content().cursor().position.column;
    assert!(column > 0, "the probe click should land past column zero");
    let advance = probe / column as f32;
    println!("measured glyph advance: {advance:.2} px");

    // Aim at the middle of `serve`, which is unambiguously one word.
    let target = LINES[0].find("serve").expect("line 0 contains `serve`");
    let x = PADDING + (target as f32 + 2.5) * advance;

    harness.point_at(x, Harness::line_y(0));
    harness.press();
    harness.release();
    harness.press();
    harness.release();

    let selection = harness
        .pane
        .selection()
        .expect("a double click selects a word");

    println!("double-click selection: {selection:?}");
    assert_eq!(selection, "serve");
}

#[test]
fn command_a_then_command_c_copies_the_whole_buffer() {
    let mut harness = Harness::new(&LINES);

    // Click once to focus, then select all and copy.
    harness.point_at(PADDING + 10.0, Harness::line_y(0));
    harness.press();
    harness.release();
    harness.command("a");
    harness.command("c");

    let copied = harness
        .clipboard
        .written()
        .expect("Cmd+A then Cmd+C should copy");

    assert_eq!(copied, LINES.join("\n"));
    println!("Cmd+A, Cmd+C copied {} bytes", copied.len());
}

#[test]
fn edits_are_refused_but_selection_survives() {
    let mut harness = Harness::new(&LINES);

    harness.point_at(PADDING + 10.0, Harness::line_y(0));
    harness.press();
    harness.release();
    harness.command("a");

    // Typing, backspace and paste must all leave the buffer alone.
    harness.tap(
        keyboard::Key::Character("x".into()),
        keyboard::Modifiers::default(),
    );
    harness.tap(
        keyboard::Key::Named(keyboard::key::Named::Backspace),
        keyboard::Modifiers::default(),
    );
    harness.command("v");

    harness.command("c");

    assert_eq!(
        harness.clipboard.written().as_deref(),
        Some(LINES.join("\n").as_str()),
        "a read-only pane must still hold every line after an edit attempt"
    );
}

#[test]
fn scrolling_up_holds_the_tail_and_returning_flushes_it() {
    let mut harness = Harness::new(&LINES);

    // A wheel notch up detaches the pane from the tail.
    harness.point_at(400.0, 100.0);
    harness.feed(&[Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
    })]);

    assert_eq!(
        harness.pane.hold(),
        Some(studiov2_log_viewer::logpane::HoldReason::ScrolledUp),
        "scrolling up should stop the pane following the tail"
    );

    let live_before = harness.pane.live_lines();
    let arriving: Vec<Arc<str>> = (0..50)
        .map(|i| Arc::from(format!("late line {i}")))
        .collect();
    harness.pane.append(&arriving);

    assert_eq!(
        harness.pane.live_lines(),
        live_before,
        "a held pane must not move while the user reads it"
    );
    assert_eq!(
        harness.pane.pending_lines(),
        50,
        "the lines should be queued"
    );

    // Scrolling back down to the bottom resumes following and flushes.
    harness.feed(&[Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -4.0 },
    })]);

    assert_eq!(
        harness.pane.hold(),
        None,
        "back at the tail, follow resumes"
    );
    assert_eq!(harness.pane.pending_lines(), 0);
    assert_eq!(harness.pane.live_lines(), live_before + 50);
}

#[test]
fn a_selection_holds_the_tail_so_it_can_be_copied() {
    let mut harness = Harness::new(&LINES);

    harness.point_at(PADDING + 1.0, Harness::line_y(1));
    harness.press();
    harness.move_to(PADDING + 1.0, Harness::line_y(3));
    harness.release();

    assert_eq!(
        harness.pane.hold(),
        Some(studiov2_log_viewer::logpane::HoldReason::Selecting),
        "a live pane must stop appending while text is selected"
    );

    let selection = harness.pane.selection().expect("a selection");
    let arriving: Vec<Arc<str>> = (0..20)
        .map(|i| Arc::from(format!("late line {i}")))
        .collect();
    harness.pane.append(&arriving);

    assert_eq!(
        harness.pane.selection().as_deref(),
        Some(selection.as_str()),
        "arriving lines must not destroy the selection"
    );

    harness.command("c");
    assert_eq!(
        harness.clipboard.written().as_deref(),
        Some(selection.as_str())
    );
}

#[test]
fn the_retention_cap_drops_the_oldest_lines() {
    use studiov2_log_viewer::logpane::TrimMode;

    let mut pane = LogPane::with_trim_mode(Some(100), TrimMode::SelectDelete);
    pane.set_trim_slack(Some(10));

    for batch in 0..20 {
        let lines: Vec<Arc<str>> = (0..50)
            .map(|i| Arc::from(format!("line {}", batch * 50 + i)))
            .collect();
        pane.append(&lines);
    }

    assert_eq!(pane.received(), 1_000);
    assert!(
        pane.live_lines() <= 110,
        "the cap should hold the buffer near 100, got {}",
        pane.live_lines()
    );
    assert_eq!(
        pane.received() - pane.dropped(),
        pane.live_lines() as u64,
        "every received line is either live or dropped"
    );

    // The lines that survived must be the newest ones.
    let text = pane.content().text();
    assert!(text.contains("line 999"), "the newest line must survive");
    assert!(!text.contains("line 0\n"), "the oldest line must be gone");
}
