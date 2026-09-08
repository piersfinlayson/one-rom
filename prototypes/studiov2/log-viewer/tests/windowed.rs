//! Does a selection larger than the widget's window actually work?
//!
//! The unit tests in `logview` drive the view directly.  These drive the real
//! `text_editor` through a headless iced `UserInterface` — the same widget,
//! the same `Widget::update`, the same key bindings the app installs — so a
//! pass here says the mouse and the keyboard reach the store, not just that
//! the arithmetic is right.
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

use studiov2_log_viewer::logview::{LINE_H, LogView, ScrollRequest};
use studiov2_log_viewer::store::Store;

/// How many lines the widget's window holds in these tests.
const WINDOW: usize = 200;

/// How tall the pane is on screen, in pixels.  The editor itself is taller —
/// it is the whole window — and the `scrollable` shows this much of it.
const VIEWPORT: f32 = 400.0;

/// The point size the pane renders at.
const TEXT_SIZE: f32 = 12.0;

/// What the harness's widget tree can produce.
///
/// The same three things the application's tree produces: an editor action,
/// and the two bindings taken away from the widget because it would answer
/// them from the window rather than from the log.
#[derive(Debug, Clone)]
enum Msg {
    /// The editor reported an interaction.
    Action(text_editor::Action),
    /// Cmd+C.
    Copy,
    /// Cmd+A.
    SelectAll,
}

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

/// A harness holding a windowed log view and the interface that renders it.
struct Harness {
    /// The view under test.
    view: LogView,
    /// The log the view is a window onto.
    ///
    /// In the app this lives in `Shared` and the shell owns it.  Here the
    /// harness stands in for the shell.
    store: Store,
    /// The headless renderer.
    renderer: iced::Renderer,
    /// The interface's retained state between updates.
    cache: user_interface::Cache,
    /// Where the mouse is.
    cursor: mouse::Cursor,
    /// What the app copied.
    clipboard: Recorder,
    /// The last scroll the view asked for.
    last_request: Option<ScrollRequest>,
}

impl Harness {
    /// Builds a harness whose log holds `count` numbered lines.
    fn new(count: usize) -> Self {
        let renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            Font::MONOSPACE,
            Pixels(TEXT_SIZE),
            None,
        ))
        .expect("a headless renderer");

        let mut store = Store::temporary().expect("a temporary store");
        let mut view = LogView::new(WINDOW);
        view.set_viewport(&store, VIEWPORT)
            .expect("the pane to be measured");

        let lines: Vec<Arc<str>> = (0..count)
            .map(|i| Arc::from(format!("{i:07} the quick brown fox")))
            .collect();
        view.append(&mut store, &lines).expect("the append to land");

        Self {
            view,
            store,
            renderer,
            cache: user_interface::Cache::default(),
            cursor: mouse::Cursor::Unavailable,
            clipboard: Recorder::default(),
            last_request: None,
        }
    }

    /// The size the interface lays out in.
    ///
    /// As tall as the whole window, because that is the height the editor is
    /// given inside the `scrollable`: the two spacers, not the editor, carry
    /// the rest of the log.
    fn size() -> Size {
        Size::new(1200.0, WINDOW as f32 * LINE_H)
    }

    /// The y of the middle of a window-local line, in editor coordinates.
    fn line_y(line: usize) -> f32 {
        (line as f32 + 0.5) * LINE_H
    }

    /// The y of the middle of a whole-log line, if it is on screen.
    fn global_y(&self, line: usize) -> f32 {
        Self::line_y(line - self.view.window_start())
    }

    /// Feeds events to the interface and applies what they produce.
    fn feed(&mut self, events: &[Event]) {
        // The element borrows the view's content, so the view cannot be
        // mutated while the interface exists.  Collect the messages, drop the
        // interface, then apply them — which is exactly what the real
        // application's message loop does.
        let mut messages = Vec::new();

        {
            let element: Element<'_, Msg> = text_editor(self.view.content())
                .on_action(Msg::Action)
                .key_binding(key_binding)
                .font(Font::MONOSPACE)
                .size(TEXT_SIZE)
                .line_height(Pixels(LINE_H))
                .wrapping(text::Wrapping::None)
                .height(Length::Fixed(self.view.window_px()))
                .padding(0)
                .into();

            let mut interface = UserInterface::build(
                element,
                Self::size(),
                std::mem::take(&mut self.cache),
                &mut self.renderer,
            );

            let _ = interface.update(
                events,
                self.cursor,
                &mut self.renderer,
                &mut self.clipboard,
                &mut messages,
            );

            self.cache = interface.into_cache();
        }

        for message in messages {
            self.apply(message);
        }
    }

    /// Handles one message the way the application does.
    fn apply(&mut self, message: Msg) {
        match message {
            Msg::Action(action) => {
                self.last_request = self
                    .view
                    .on_action(&self.store, action)
                    .expect("the store to serve the window");
            }
            Msg::SelectAll => {
                self.view.select_all(&self.store).expect("select all");
            }
            Msg::Copy => {
                if let Some(text) = self
                    .view
                    .selected_text(&self.store)
                    .expect("the store to serve the span")
                {
                    self.clipboard.write(Kind::Standard, text);
                }
            }
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

    /// Clicks twice where the cursor is, as one burst of events.
    ///
    /// Iced decides a double click from the wall clock: two presses more than
    /// 300 ms apart are two single clicks, whatever the events say.  Every
    /// `feed` builds the interface and lays out the whole window afresh, which
    /// on a busy machine costs more than that, so the pair has to reach the
    /// widget inside a single update — which is also how a real double click
    /// arrives, as several events in one frame's batch.
    fn double_click(&mut self) {
        self.feed(&[
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        ]);
    }

    /// Moves the mouse to an editor position, reporting the move.
    fn move_to(&mut self, x: f32, y: f32) {
        self.cursor = mouse::Cursor::Available(Point::new(x, y));
        self.feed(&[Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(x, y),
        })]);
    }

    /// Points the mouse at an editor position without reporting a move.
    fn point_at(&mut self, x: f32, y: f32) {
        self.cursor = mouse::Cursor::Available(Point::new(x, y));
    }

    /// Taps a character key with the platform's command modifier held.
    fn command(&mut self, character: &str) {
        let key = keyboard::Key::Character(character.into());
        self.feed(&[Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::COMMAND,
            repeat: false,
            text: Some(character.into()),
        })]);
    }
}

/// The application's own key bindings, so the test proves what ships.
fn key_binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Msg>> {
    use text_editor::{Binding, Status};

    if !matches!(press.status, Status::Focused { .. }) {
        return None;
    }

    if press.modifiers.command() {
        match press.key.to_latin(press.physical_key) {
            Some('c') | Some('x') => return Some(Binding::Custom(Msg::Copy)),
            Some('a') => return Some(Binding::Custom(Msg::SelectAll)),
            _ => {}
        }
    }

    Binding::from_key_press(press)
}

#[test]
fn a_drag_held_past_the_edge_selects_more_than_the_window() {
    let mut harness = Harness::new(50_000);
    harness
        .view
        .jump_to_line(&harness.store, 10_000)
        .expect("a jump");

    let start = harness.view.window_start();
    let first_visible = harness.view.top_line().floor() as usize;

    // Press at the top of what is on screen and drag to the bottom of it.
    // The widget publishes no drag once the pointer leaves the pane, so this
    // is as far as the mouse alone gets.
    harness.point_at(1.0, harness.global_y(first_visible));
    harness.press();
    let last_visible = first_visible + (VIEWPORT / LINE_H) as usize - 1;
    harness.move_to(400.0, harness.global_y(last_visible));

    assert!(
        harness.view.selected_lines() <= (VIEWPORT / LINE_H) as usize + 1,
        "the mouse alone can only reach the bottom of the pane"
    );

    // Holding the drag below the pane is what the app's 16 ms ticker does.
    assert!(
        harness.view.drag_overrun().is_none(),
        "a drag inside the pane is not an overrun"
    );
    harness.move_to(400.0, harness.global_y(last_visible + 4));
    assert!(
        harness.view.drag_overrun().is_some(),
        "a drag past the visible span is an overrun"
    );

    for _ in 0..400 {
        harness
            .view
            .drag_scroll(&harness.store)
            .expect("a drag scroll");
    }
    harness.view.release();

    let (from, to) = harness
        .view
        .selection_span()
        .expect("the drag to have selected something");

    assert_eq!(from.line, first_visible);
    assert!(
        to.line > first_visible + WINDOW,
        "the selection must be longer than the window: {} lines",
        harness.view.selected_lines()
    );
    assert!(
        harness.view.window_start() > start,
        "the window must have moved under the drag"
    );

    // The widget only ever held the window, and only ever highlighted the
    // part of the selection inside it.
    assert_eq!(harness.view.content().line_count(), WINDOW);
    let in_widget = harness
        .view
        .content()
        .selection()
        .expect("the window to show part of the selection");

    // Cmd+C goes to the store, so what reaches the clipboard is the whole
    // range rather than what the widget could see.
    harness.command("c");
    let copied = harness.clipboard.written().expect("Cmd+C to copy");

    assert!(
        copied.len() > in_widget.len(),
        "the copy ({} bytes) must be larger than the window's own \
         selection ({} bytes)",
        copied.len(),
        in_widget.len()
    );
    assert_eq!(copied.lines().count(), harness.view.selected_lines());
    assert!(copied.starts_with(&format!("{first_visible:07} ")));
    assert!(copied.contains(&format!("{:07} ", first_visible + WINDOW)));

    println!(
        "drag selected {} lines ({} bytes) while the widget held {}",
        harness.view.selected_lines(),
        copied.len(),
        WINDOW
    );
}

#[test]
fn command_a_then_command_c_copies_the_whole_log() {
    let mut harness = Harness::new(120_000);

    // Click once to focus, the way a user would before a keystroke.
    harness.point_at(1.0, Harness::line_y(2));
    harness.press();

    harness.command("a");
    harness.command("c");

    let copied = harness.clipboard.written().expect("Cmd+A, Cmd+C to copy");

    assert_eq!(copied.lines().count(), 120_000);
    assert!(copied.starts_with("0000000 the quick"));
    assert!(copied.ends_with("0119999 the quick brown fox"));
    assert_eq!(
        harness.view.content().line_count(),
        WINDOW,
        "the widget never held more than the window"
    );

    println!(
        "Cmd+A, Cmd+C copied {:.1} MB from a {}-line widget",
        copied.len() as f64 / (1024.0 * 1024.0),
        WINDOW
    );
}

#[test]
fn the_wheel_moves_the_scrollable_rather_than_the_editor() {
    let mut harness = Harness::new(50_000);
    harness
        .view
        .jump_to_line(&harness.store, 20_000)
        .expect("a jump");
    let before = harness.view.top_line();

    harness.point_at(400.0, 100.0);
    harness.feed(&[Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
    })]);

    // The editor captured the wheel and published a scroll rather than
    // scrolling itself, and the view answered with what the surrounding
    // `scrollable` has to be told.
    assert!(
        matches!(harness.last_request, Some(ScrollRequest::By(dy)) if dy > 0.0),
        "a wheel notch must become a scroll request: {:?}",
        harness.last_request
    );
    assert!(harness.view.top_line() > before);
}

#[test]
fn the_scrollbar_measures_the_log_and_not_the_window() {
    let mut harness = Harness::new(1_000_000);

    for line in [0usize, 1, 250_000, 999_999] {
        harness
            .view
            .jump_to_line(&harness.store, line)
            .expect("a jump");

        let view = &harness.view;
        let total = view.above_px() + view.window_px() + view.below_px(&harness.store);

        assert_eq!(
            total,
            view.total_px(&harness.store),
            "the scrollable's content must stay exactly as tall as the log"
        );
        assert_eq!(view.window_len(), WINDOW);
    }

    // A million lines at this line height is 16 million pixels, which is
    // still inside the range an f32 counts in whole numbers.  Twice that
    // would not be, and this is where the design's ceiling sits.
    assert_eq!(harness.view.total_px(&harness.store), 16_000_000.0);
    assert_eq!(
        harness.view.total_px(&harness.store) as u64 as f32,
        16_000_000.0
    );
}

#[test]
fn double_click_still_selects_a_word() {
    let mut harness = Harness::new(5_000);
    harness
        .view
        .jump_to_line(&harness.store, 2_000)
        .expect("a jump");

    // Measure the glyph advance rather than assuming it.
    let line = harness.view.window_start() + 3;
    harness.point_at(200.0, harness.global_y(line));
    harness.press();
    harness.release();

    let column = harness.view.caret().column;
    assert!(column > 0, "the probe click should land past column zero");
    let advance = 200.0 / column as f32;

    // Aim at the middle of `brown`.  The probe click above is far enough away
    // in x that it cannot be read as the first half of this double click.
    let x = ("0000000 the quick ".len() as f32 + 2.5) * advance;
    harness.point_at(x, harness.global_y(line));
    harness.double_click();

    harness.command("c");
    assert_eq!(harness.clipboard.written().as_deref(), Some("brown"));
}

#[test]
fn an_edit_cannot_reach_the_store() {
    let mut harness = Harness::new(2_000);
    let before = harness.view.total_lines(&harness.store);

    harness.point_at(1.0, Harness::line_y(1));
    harness.press();
    harness.command("a");

    harness.feed(&[Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(keyboard::key::Named::Backspace),
        modified_key: keyboard::Key::Named(keyboard::key::Named::Backspace),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::default(),
        repeat: false,
        text: None,
    })]);

    harness.command("c");
    let copied = harness.clipboard.written().expect("a copy");

    assert_eq!(harness.view.total_lines(&harness.store), before);
    assert_eq!(copied.lines().count(), before);
}
