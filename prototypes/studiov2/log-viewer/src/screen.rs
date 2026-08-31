//! The log and console screen.
//!
//! Two panes, both of which a hardware-management desktop app needs:
//!
//! * a **live log** that streams device output, sticks to the tail, lets the
//!   user scroll back, select with the mouse and copy — and keeps every line
//!   the device ever sent, for as long as the session lasts
//! * an **interactive console** — a device command shell, output plus a typed
//!   input line
//!
//! The log itself is not here.  It lives in [`Shared::log`], because the
//! builder writes to it too, and this screen holds a window of a few hundred
//! lines of it in a widget ([`crate::logview`]).  The earlier design, which
//! capped the widget's buffer and dropped the oldest lines, is still here
//! behind `--mode capped` so the two can be measured against each other.

use crate::{device, error, logpane, logsrc, logview, metrics, options};

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::Id;
use iced::widget::operation::{AbsoluteOffset, scroll_by, scroll_to, snap_to_end};
use iced::widget::{
    Space, button, column, container, pick_list, row, rule, scrollable, sensor, slider, text,
    text_editor, text_input,
};
use iced::{Alignment, Element, Font, Length, Pixels, Subscription, Task};

use studiov2_shared::store::{Scan, Store, StoreError};
use studiov2_shared::{Shared, style};

use device::{Effect, Reply, Session};
use error::Error;
use logpane::LogPane;
use logsrc::{Generator, LogBatch};
use logview::{HoldReason, LogView, ScrollRequest};
pub use options::Options;

use options::Mode;

/// The typeface both panes use.  A log is columnar and needs a fixed pitch.
const MONO: Font = Font::MONOSPACE;

/// The point size both panes use.
const TEXT_SIZE: f32 = 12.0;

/// The scrollable that carries the whole log's height.
const LOG_SCROLL: Id = Id::new("log-scroll");

/// The find bar's input, so Cmd+F can put the caret in it.
const FIND_INPUT: &str = "find-input";

/// How often the streaming source wakes up.  Lines per wake-up come from the
/// configured rate, so a high rate is one big batch rather than many small
/// ones — which is also how a real device's USB reads arrive.
const STREAM_INTERVAL: Duration = Duration::from_millis(50);

/// How often a drag held outside the pane scrolls it.
const DRAG_SCROLL_INTERVAL: Duration = Duration::from_millis(16);

/// How many lines a `--fill` writes per step.
const FILL_STEP: usize = 50_000;

/// Which pane is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// The live log.
    Log,
    /// The device console.
    Console,
}

/// A retention cap, as offered in the picker.
///
/// Only `--mode capped` has one.  A windowed pane keeps everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention(Option<usize>);

/// The caps the picker offers.
const RETENTIONS: [Retention; 6] = [
    Retention(Some(1_000)),
    Retention(Some(10_000)),
    Retention(Some(50_000)),
    Retention(Some(100_000)),
    Retention(Some(250_000)),
    Retention(None),
];

impl std::fmt::Display for Retention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(n) if n >= 1_000 => write!(f, "keep {}k lines", n / 1_000),
            Some(n) => write!(f, "keep {n} lines"),
            None => write!(f, "keep everything"),
        }
    }
}

/// The log pane, in whichever of the two designs is under test.
enum Log {
    /// The whole log in a file, a window of it in the widget.
    Windowed(Box<LogView>),
    /// The whole widget is the log, and the oldest lines are dropped.
    Capped(Box<LogPane>),
}

/// Everything the app can be told.
///
/// Nothing here carries a copy of a log buffer.  Batches travel as
/// `Vec<Arc<str>>`, so a hundred thousand lines move as a hundred thousand
/// pointers and the text itself is never duplicated.
#[derive(Debug, Clone)]
pub enum Message {
    /// Show a different pane.
    TabSelected(Tab),

    /// The log editor reported an interaction.
    LogAction(text_editor::Action),
    /// The log's scrollable moved.
    LogScrolled(scrollable::Viewport),
    /// The log pane was measured.
    LogResized(iced::Size),
    /// The mouse button came up somewhere.
    MouseReleased,
    /// A drag is being held outside the pane.
    DragScroll,
    /// Copy the whole selection, which is served from the store.
    CopySelection,
    /// Select the whole log.
    SelectAll,
    /// Go to the first line.
    JumpTop,
    /// Go to the last line and pin there.
    JumpTail,
    /// The go-to-line box changed.
    GotoInput(String),
    /// Go to the line the box names.
    GotoSubmit,

    /// The find box changed.
    FindInput(String),
    /// Put the caret in the find box.
    FindFocus,
    /// Search forwards or backwards from the caret.
    Find(bool),
    /// A scan came back, with the direction that asked for it.
    Found(bool, Result<Scan, Arc<StoreError>>),

    /// The streaming timer fired.
    StreamTick,
    /// Turn the streaming source on or off.
    StreamToggled,
    /// Change the streaming rate, in lines per second.
    RateChanged(u32),
    /// Inject a fixed number of lines at once.
    Inject(usize),
    /// A generator task finished.  Carries the generator back so its sequence
    /// numbers keep running.
    Generated(Generator, LogBatch),
    /// Take the generator back after a chunked injection.
    GeneratorBack(Generator),
    /// Append one chunk of a chunked injection.
    AppendChunk(Vec<Arc<str>>),
    /// Change the retention cap.  Capped mode only.
    RetentionChanged(Retention),
    /// Empty the log.
    ClearLog,

    /// The console scrollback reported an interaction.
    ConsoleAction(text_editor::Action),
    /// The command line changed.
    ConsoleInput(String),
    /// The command line was submitted.
    ConsoleSubmit,
    /// Step back or forward through command history.
    ConsoleHistory(isize),
    /// The fake device answered.
    ConsoleReply(Result<Reply, Arc<Error>>),
    /// Reconnect a disconnected console.
    ConsoleConnect,

    /// Somebody outside this screen appended to the shared log.
    LogGrew,
    /// The selected device changed.
    DeviceChanged,

    /// A frame was presented.
    Frame(Instant),
    /// Time to re-read the resident set size.
    PollRss,
    /// A resident-set-size reading came back.
    Rss(Result<u64, Arc<error::RssError>>),
    /// Forget the recorded frame timings.
    ResetFrames,

    /// Write the next `FILL_STEP` lines of a `--fill`.
    FillStep(usize),
    /// Run measurement step `index`.
    ProbeStep(usize),
    /// Run benchmark step `index`.
    BenchStep(usize),
    /// Report the figures for benchmark step `index`.
    BenchReport(usize),
    /// Put known lines in the log, ready for the self test.
    SelfTestArm,
    /// Select some of those lines with the same actions a mouse drag makes,
    /// and copy the result to the real system clipboard.
    SelfTestSelect,
    /// Print the console scrollback, so a scripted session can be checked.
    ConsoleDump,
    /// Take a picture of the window.
    Shot,
    /// A picture came back.
    Shotted(iced::window::Screenshot),
    /// Everything asked for on the command line has finished.
    Done,
}

/// The console pane's state.
struct Console {
    /// Scrollback.  Uncapped, and small: a command transcript is not a device
    /// firehose, so it stays on the simple widget-is-the-buffer design.
    scrollback: LogPane,
    /// What the user has typed but not submitted.
    input: String,
    /// Previously submitted commands, oldest first.
    history: Vec<String>,
    /// Where the user is in `history`, when stepping through it.
    history_cursor: Option<usize>,
    /// Whether a session can be opened.
    connected: bool,
    /// Whether a command is in flight.
    busy: bool,
    /// Commands submitted while another was in flight, oldest first.
    ///
    /// Two `Task`s returned from separate `update` calls have no ordering
    /// between them, so submitting a second command before the first replies
    /// makes the answers arrive out of order.  A device shell answers one
    /// command at a time, so only one is ever in flight.
    queue: VecDeque<String>,
}

/// The whole application.
pub struct Screen {
    /// What the command line asked for.
    options: Options,
    /// Which pane is showing.
    tab: Tab,
    /// The live log.
    log: Log,
    /// The synthetic log source.
    generator: Generator,
    /// Whether the streaming source is running.
    streaming: bool,
    /// Streaming rate, in lines per second.
    rate: u32,
    /// The console.
    console: Console,
    /// What is typed in the find box.
    find: String,
    /// What the last scan found.
    found: Option<Scan>,
    /// Where the last scan put the caret.
    found_line: Option<usize>,
    /// What is typed in the go-to-line box.
    goto: String,
    /// Rolling frame timings.
    frames: metrics::Frames,
    /// The last resident-set-size reading, in kilobytes.
    rss_kb: Option<u64>,
    /// Microseconds the last off-thread generation took.
    last_generate_us: u128,
    /// Microseconds spent appending across the whole of the last injection.
    inject_append_us: u128,
    /// Chunks the last injection was split into.
    inject_chunks: usize,
    /// Microseconds the last jump took, end to end inside `update`.
    last_jump_us: u128,
    /// The longest single `update` call since the last reset, in
    /// milliseconds.  This is the number that says whether the window froze:
    /// nothing can be drawn or clicked while `update` is running.
    slowest_update_ms: f32,
    /// The longest single `view` call since the last reset, in milliseconds.
    slowest_view_ms: std::cell::Cell<f32>,
    /// The last thing that went wrong, for the status bar.
    last_error: Option<String>,
    /// When the current measurement step started.
    probe_marker: Instant,
    /// Window rebuilds when it started.
    probe_rebuilds: u64,
    /// Microseconds spent rebuilding when it started.
    probe_rebuild_us: u128,
}

impl Screen {
    /// Builds the screen, and whatever the command line asked it to run.
    ///
    /// `shared` is taken mutably because the log lives there and the capped
    /// mode has to be told about it at boot.
    pub fn boot(options: Options, shared: &mut Shared) -> (Self, Task<Message>) {
        let mut console = Console {
            scrollback: LogPane::new(None),
            input: String::new(),
            history: Vec::new(),
            history_cursor: None,
            connected: true,
            busy: false,
            queue: VecDeque::new(),
        };
        console
            .scrollback
            .append(&device::banner(&Session::for_device(
                shared.device.as_ref(),
            )));

        let log = Self::open_log(&options);
        let boot_error = None;

        let app = Self {
            tab: Tab::Log,
            log,
            generator: Generator::new(),
            streaming: options.streaming,
            rate: options.rate,
            console,
            find: String::new(),
            found: None,
            found_line: None,
            goto: String::new(),
            frames: metrics::Frames::default(),
            rss_kb: None,
            last_generate_us: 0,
            inject_append_us: 0,
            inject_chunks: 0,
            last_jump_us: 0,
            slowest_update_ms: 0.0,
            slowest_view_ms: std::cell::Cell::new(0.0),
            last_error: boot_error,
            probe_marker: Instant::now(),
            probe_rebuilds: 0,
            probe_rebuild_us: 0,
            options: options.clone(),
        };

        // Bring the window to the front on launch, the way any app does.
        let mut start = iced::window::latest()
            .and_then(iced::window::gain_focus)
            .chain(Task::done(Message::PollRss));

        if options.shot.is_some() && options.fill == 0 {
            start = start.chain(Task::done(Message::Shot));
        } else if options.fill > 0 {
            start = start.chain(Task::done(Message::FillStep(0)));
        } else if !options.bench.is_empty() {
            start = start.chain(Task::done(Message::BenchStep(0)));
        } else if options.probe {
            start = start.chain(Task::done(Message::ProbeStep(0)));
        } else if options.selftest {
            start = start.chain(Task::done(Message::SelfTestArm));
        } else if options.console_demo {
            start = start.chain(Self::console_demo());
        } else if options.quit_when_done {
            start = start.chain(Task::done(Message::Done));
        }

        (app, start)
    }

    /// Builds whichever log pane the command line asked for.
    fn open_log(options: &Options) -> Log {
        match options.mode {
            Mode::Windowed => Log::Windowed(Box::new(LogView::new(options.window))),
            Mode::Capped => {
                let mut pane = LogPane::with_trim_mode(options.retention, options.trim_mode);
                pane.set_trim_slack(options.trim_slack);
                Log::Capped(Box::new(pane))
            }
        }
    }

    /// The windowed pane, when that is the one under test.
    fn view_mut(&mut self) -> Option<&mut LogView> {
        match &mut self.log {
            Log::Windowed(view) => Some(view),
            Log::Capped(_) => None,
        }
    }

    /// Records an error in the status bar and gives back nothing.
    fn note<T>(&mut self, result: Result<T, StoreError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.last_error = Some(error.to_string());
                None
            }
        }
    }

    /// Turns a pane's scroll request into the widget operation that carries
    /// it out.
    fn scroll(request: Option<ScrollRequest>) -> Task<Message> {
        match request {
            Some(ScrollRequest::By(dy)) => scroll_by(LOG_SCROLL, AbsoluteOffset { x: 0.0, y: dy }),
            Some(ScrollRequest::To(y)) => scroll_to(LOG_SCROLL, AbsoluteOffset { x: 0.0, y }),
            Some(ScrollRequest::End) => snap_to_end(LOG_SCROLL),
            None => Task::none(),
        }
    }

    /// Waits, without blocking the update loop.
    fn settle(millis: u64) -> Task<Message> {
        Task::future(tokio::time::sleep(Duration::from_millis(millis))).discard()
    }

    /// The window title.
    pub fn title(&self, shared: &Shared) -> String {
        match &self.log {
            Log::Windowed(view) => format!(
                "Log viewer - prototype — {} lines stored, {} in the widget",
                view.total_lines(&shared.log),
                view.window_len()
            ),
            Log::Capped(pane) => format!(
                "Log viewer - prototype (capped) — {} lines live, {} received",
                pane.live_lines(),
                pane.received()
            ),
        }
    }

    /// What the app listens to.
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::window::frames().map(Message::Frame),
            iced::time::every(Duration::from_secs(1)).map(|_| Message::PollRss),
        ];

        if self.streaming {
            subscriptions.push(iced::time::every(STREAM_INTERVAL).map(|_| Message::StreamTick));
        }

        // The widget publishes a drag only while the pointer is inside it, so
        // holding a drag past the edge of the pane has to be driven from here.
        if matches!(&self.log, Log::Windowed(view) if view.drag_overrun().is_some()) {
            subscriptions
                .push(iced::time::every(DRAG_SCROLL_INTERVAL).map(|_| Message::DragScroll));
        }

        subscriptions.push(iced::event::listen_raw(
            |event, _status, _window| match event {
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::MouseReleased),
                _ => None,
            },
        ));

        if self.tab == Tab::Console {
            subscriptions.push(iced::keyboard::listen().filter_map(|event| {
                use iced::keyboard::{Event, Key, key::Named};
                match event {
                    Event::KeyPressed {
                        key: Key::Named(Named::ArrowUp),
                        ..
                    } => Some(Message::ConsoleHistory(-1)),
                    Event::KeyPressed {
                        key: Key::Named(Named::ArrowDown),
                        ..
                    } => Some(Message::ConsoleHistory(1)),
                    _ => None,
                }
            }));
        }

        Subscription::batch(subscriptions)
    }

    /// Generates `count` lines away from the update loop, then reports memory.
    ///
    /// The generator is moved into the future and handed back with the batch,
    /// which keeps its sequence numbers running without a shared lock.  The
    /// `then` is the sequencing: generate, deliver, and only then re-measure,
    /// because a reading taken before the append lands measures nothing.
    fn generate(generator: Generator, count: usize) -> Task<Message> {
        Task::future(async move {
            let mut generator = generator;
            let batch = generator.batch(count);
            (generator, batch)
        })
        .then(|(generator, batch)| {
            Task::done(Message::Generated(generator, batch)).chain(Task::done(Message::PollRss))
        })
    }

    /// The commands the scripted console session runs.
    const DEMO_COMMANDS: [&'static str; 7] = [
        "help",
        "version",
        "status",
        "scan",
        "echo hello device",
        "sprocket",
        "flood 5",
    ];

    /// Drives the console pane through a scripted session.
    ///
    /// The whole chain is built before any of it runs, so this exercises the
    /// same `and_then` path a typed command takes without anything driving
    /// itself through the message loop.
    fn console_demo() -> Task<Message> {
        let mut task = Task::done(Message::TabSelected(Tab::Console));

        for command in Self::DEMO_COMMANDS {
            task = task
                .chain(Task::done(Message::ConsoleInput(command.to_owned())))
                .chain(Task::done(Message::ConsoleSubmit))
                .chain(Self::settle(400));
        }

        // The device replies arrive on their own tasks, which the event loop
        // polls when it next wakes.  Wait for them before dumping.
        task.chain(Self::settle(2_500))
            .chain(Task::done(Message::ConsoleDump))
    }

    /// Generates `count` lines and appends them in chunks of `chunk`.
    ///
    /// The whole chain is built before any of it runs — one generate, then one
    /// append message per chunk — so nothing here drives itself through the
    /// message loop.
    fn generate_chunked(generator: Generator, count: usize, chunk: usize) -> Task<Message> {
        Task::future(async move {
            let mut generator = generator;
            let batch = generator.batch(count);
            (generator, batch)
        })
        .then(move |(generator, batch)| {
            let mut task = Task::done(Message::GeneratorBack(generator));
            for slice in batch.lines.chunks(chunk.max(1)) {
                task = task.chain(Task::done(Message::AppendChunk(slice.to_vec())));
            }
            task
        })
    }

    /// Handles a message, timing the work.
    ///
    /// The window cannot draw or respond while this runs, so the slowest call
    /// is the honest measure of a freeze.
    pub fn update(&mut self, message: Message, shared: &mut Shared) -> Task<Message> {
        let started = Instant::now();
        let task = self.dispatch(message, shared);
        let elapsed = started.elapsed().as_secs_f32() * 1000.0;
        self.slowest_update_ms = self.slowest_update_ms.max(elapsed);
        task
    }

    /// Handles a message.
    fn dispatch(&mut self, message: Message, shared: &mut Shared) -> Task<Message> {
        match message {
            Message::TabSelected(tab) => {
                self.tab = tab;
                Task::none()
            }

            Message::LogAction(action) => match &mut self.log {
                Log::Windowed(view) => {
                    let result = view.on_action(&shared.log, action);
                    let request = self.note(result).flatten();
                    Self::scroll(request)
                }
                Log::Capped(pane) => {
                    pane.on_action(action);
                    Task::none()
                }
            },

            Message::LogScrolled(viewport) => {
                let Some(view) = self.view_mut() else {
                    return Task::none();
                };
                let offset = viewport.absolute_offset().y;
                let result = view.scrolled_to(&shared.log, offset);
                let _ = self.note(result);
                Task::none()
            }

            Message::LogResized(size) => {
                let Some(view) = self.view_mut() else {
                    return Task::none();
                };
                let result = view.set_viewport(&shared.log, size.height);
                let _ = self.note(result);
                Task::none()
            }

            Message::MouseReleased => {
                if let Some(view) = self.view_mut() {
                    view.release();
                }
                Task::none()
            }

            Message::DragScroll => {
                let Some(view) = self.view_mut() else {
                    return Task::none();
                };
                let result = view.drag_scroll(&shared.log);
                let request = self.note(result);
                Self::scroll(request)
            }

            Message::CopySelection => {
                let selection = match &mut self.log {
                    Log::Windowed(view) => {
                        let result = view.selected_text(&shared.log);
                        self.note(result).flatten()
                    }
                    Log::Capped(pane) => pane.selection(),
                };

                match selection {
                    Some(text) => iced::clipboard::write(text),
                    None => {
                        self.last_error = Some("nothing selected".to_owned());
                        Task::none()
                    }
                }
            }

            Message::SelectAll => match &mut self.log {
                Log::Windowed(view) => {
                    let result = view.select_all(&shared.log);
                    let _ = self.note(result);
                    Task::none()
                }
                Log::Capped(pane) => {
                    pane.on_action(text_editor::Action::SelectAll);
                    Task::none()
                }
            },

            Message::JumpTop => self.timed_jump(&shared.log, LogView::jump_to_top),
            Message::JumpTail => self.timed_jump(&shared.log, LogView::jump_to_tail),

            Message::GotoInput(input) => {
                self.goto = input;
                Task::none()
            }

            Message::GotoSubmit => {
                let Ok(line) = self.goto.trim().parse::<usize>() else {
                    self.last_error = Some(format!("not a line number: {:?}", self.goto));
                    return Task::none();
                };
                self.timed_jump(&shared.log, move |view, store| {
                    view.jump_to_line(store, line)
                })
            }

            Message::FindInput(input) => {
                self.find = input;
                self.found = None;
                Task::none()
            }

            Message::FindFocus => iced::widget::operation::focus(FIND_INPUT),

            Message::Find(forwards) => {
                let needle = self.find.clone();
                if needle.is_empty() {
                    return Task::none();
                }

                let Some(view) = self.view_mut() else {
                    self.last_error = Some("find needs --mode windowed".to_owned());
                    return Task::none();
                };

                let searcher = match view.searcher(&shared.log) {
                    Ok(searcher) => searcher,
                    Err(error) => {
                        self.last_error = Some(error.to_string());
                        return Task::none();
                    }
                };

                let from = match view.caret_offset(&shared.log) {
                    Ok(from) => from,
                    Err(error) => {
                        self.last_error = Some(error.to_string());
                        return Task::none();
                    }
                };

                // The scan reads the whole log, so it runs on the executor
                // rather than in `update`, and the pane stays live while it
                // does.  The handle carries its own descriptor, so appends
                // continue underneath it.
                Task::future(async move {
                    tokio::task::spawn_blocking(move || searcher.scan(&needle, from))
                        .await
                        .map_err(|error| StoreError::Read(std::io::Error::other(error.to_string())))
                        .and_then(|result| result)
                })
                .map(move |result| Message::Found(forwards, result.map_err(Arc::new)))
            }

            Message::Found(forwards, result) => {
                let scan = match result {
                    Ok(scan) => scan,
                    Err(error) => {
                        self.last_error = Some(error.to_string());
                        return Task::none();
                    }
                };

                self.found = Some(scan);
                let needle_len = self.find.len();

                let target = if forwards {
                    scan.next.or(scan.first)
                } else {
                    scan.previous.or(scan.last)
                };

                let Some(offset) = target else {
                    self.found_line = None;
                    self.last_error = Some(format!("no match for {:?}", self.find));
                    return Task::none();
                };

                let Some(view) = self.view_mut() else {
                    return Task::none();
                };

                let result = view.reveal(&shared.log, offset, needle_len);
                let line = view.line_of(&shared.log, offset);
                self.found_line = Some(line);
                let request = self.note(result);
                Self::scroll(request)
            }

            Message::StreamTick => {
                let per_tick = (self.rate as f64 * STREAM_INTERVAL.as_secs_f64()).round() as usize;
                if per_tick == 0 {
                    return Task::none();
                }
                // Small batches are generated inline: moving a task through
                // the executor twenty times a second costs more than making
                // ten lines.
                let batch = self.generator.batch(per_tick);
                self.append(shared, &batch.lines)
            }

            Message::StreamToggled => {
                self.streaming = !self.streaming;
                Task::none()
            }

            Message::RateChanged(rate) => {
                self.rate = rate;
                Task::none()
            }

            Message::Inject(count) => {
                self.frames.reset();
                Self::generate(self.generator.clone(), count)
            }

            Message::Generated(generator, batch) => {
                self.generator = generator;
                self.last_generate_us = batch.generate_us;
                let task = self.append(shared, &batch.lines);
                self.inject_append_us = self.append_us(shared);
                self.inject_chunks = 1;
                task
            }

            Message::GeneratorBack(generator) => {
                self.generator = generator;
                Task::none()
            }

            Message::AppendChunk(lines) => {
                let task = self.append(shared, &lines);
                self.inject_append_us += self.append_us(shared);
                self.inject_chunks += 1;
                task
            }

            Message::RetentionChanged(retention) => {
                if let Log::Capped(pane) = &mut self.log {
                    pane.set_retention(retention.0);
                }
                Task::done(Message::PollRss)
            }

            Message::ClearLog => {
                match &mut self.log {
                    Log::Windowed(_) => {
                        // Throwing the log away means a new store, which is
                        // the one place a windowed pane loses lines — and it
                        // is the user asking.  The store is shared, so this
                        // clears it for every other screen too.
                        if let Err(error) = shared.clear_log() {
                            self.last_error = Some(error.to_string());
                        }
                        self.log = Self::open_log(&self.options);
                    }
                    Log::Capped(pane) => pane.clear(),
                }
                self.frames.reset();
                Task::done(Message::PollRss)
            }

            Message::ConsoleAction(action) => {
                self.console.scrollback.on_action(action);
                Task::none()
            }

            Message::ConsoleInput(input) => {
                self.console.input = input;
                self.console.history_cursor = None;
                Task::none()
            }

            Message::ConsoleSubmit => self.submit_command(shared),

            Message::ConsoleHistory(step) => {
                self.step_history(step);
                Task::none()
            }

            Message::ConsoleReply(result) => {
                self.console.busy = false;
                let effect = match result {
                    Ok(reply) => {
                        self.console.scrollback.append(&reply.lines);
                        self.apply_effect(reply.effect)
                    }
                    Err(error) => {
                        let line: Arc<str> = Arc::from(format!("error: {error}"));
                        self.console.scrollback.append(&[line]);
                        self.last_error = Some(error.to_string());
                        Task::none()
                    }
                };

                effect.chain(self.run_next_command(shared))
            }

            Message::ConsoleConnect => {
                self.console.connected = true;
                let session = Session::for_device(shared.device.as_ref());
                self.console.scrollback.append(&device::banner(&session));
                Task::none()
            }

            Message::LogGrew => match &mut self.log {
                // Somebody else appended.  The widget holds a window over
                // bytes that have moved, and nothing but this tells it so.
                Log::Windowed(view) => {
                    let result = view.refresh(&shared.log);
                    let request = self.note(result).flatten();
                    Self::scroll(request)
                }
                Log::Capped(_) => Task::none(),
            },

            Message::DeviceChanged => {
                let session = Session::for_device(shared.device.as_ref());
                self.console.scrollback.append(&device::banner(&session));
                Task::none()
            }

            Message::Frame(now) => {
                self.frames.tick(now);
                Task::none()
            }

            Message::ResetFrames => {
                self.frames.reset();
                self.slowest_update_ms = 0.0;
                self.slowest_view_ms.set(0.0);
                Task::none()
            }

            Message::FillStep(done) => self.fill_step(shared, done),

            Message::ProbeStep(index) => self.probe_step(shared, index),

            Message::BenchStep(index) => {
                let Some(&count) = self.options.bench.get(index) else {
                    return Task::done(if self.options.selftest {
                        Message::SelfTestArm
                    } else if self.options.probe {
                        Message::ProbeStep(0)
                    } else {
                        Message::Done
                    });
                };

                let chunk = self.options.chunk;
                println!(
                    "--- injecting {count} lines{} ---",
                    if chunk == 0 {
                        String::new()
                    } else {
                        format!(" in chunks of {chunk}")
                    }
                );
                self.inject_append_us = 0;
                self.inject_chunks = 0;

                let inject = if chunk == 0 {
                    Self::generate(self.generator.clone(), count)
                } else {
                    Self::generate_chunked(self.generator.clone(), count, chunk)
                };

                inject
                    .chain(Self::settle(2_000))
                    .chain(Task::done(Message::ResetFrames))
                    .chain(Self::settle(3_000))
                    .chain(Task::done(Message::PollRss))
                    .chain(Self::settle(400))
                    .chain(Task::done(Message::BenchReport(index)))
            }

            Message::BenchReport(index) => {
                self.report_bench(shared, index);
                Task::done(Message::BenchStep(index + 1))
            }

            Message::ConsoleDump => {
                println!("--- console scrollback ---");
                for line in self.console.scrollback.content().lines() {
                    println!("{}", line.text);
                }
                println!("--- end ---");
                Task::done(Message::Done)
            }

            Message::SelfTestArm => {
                self.streaming = false;
                let lines: Vec<Arc<str>> =
                    SELFTEST_LINES.iter().map(|line| Arc::from(*line)).collect();
                let task = self.append(shared, &lines);

                // The editor has to lay itself out before a click can land on
                // a line, so let a frame go by.
                task.chain(Self::settle(700))
                    .chain(Task::done(Message::SelfTestSelect))
            }

            Message::SelfTestSelect => self.selftest_select(shared),

            Message::Shot => {
                self.streaming = false;

                let mut task = match self.options.shot_line {
                    Some(line) => self.timed_jump(&shared.log, move |view, store| {
                        view.jump_to_line(store, line)
                    }),
                    None => Task::none(),
                };

                if self.options.shot_select {
                    task = task.chain(Task::done(Message::SelectAll));
                }

                if let Some(needle) = self.options.shot_find.clone() {
                    self.find = needle;
                    task = task.chain(Task::done(Message::Find(true)));
                }

                // Two captures, and the first is thrown away.  A screenshot
                // renders its own frame, and the first one after a period of
                // quiet comes back with glyphs missing — the log body and
                // whichever button was drawn last — because the text atlas is
                // still being filled when it is read back.
                task.chain(Self::settle(1_500))
                    .chain(
                        iced::window::oldest()
                            .and_then(iced::window::screenshot)
                            .discard(),
                    )
                    .chain(Self::settle(600))
                    .chain(
                        iced::window::oldest()
                            .and_then(iced::window::screenshot)
                            .map(Message::Shotted),
                    )
                    .chain(Self::settle(200))
            }

            Message::Shotted(shot) => {
                if let Some(path) = self.options.shot.clone()
                    && let Err(error) = write_png(&path, &shot)
                {
                    self.last_error = Some(error.to_string());
                    println!("--- shot failed: {error} ---");
                }
                Task::done(Message::Done)
            }

            Message::Done => {
                if self.options.quit_when_done {
                    println!("--- done ---");
                    return iced::exit();
                }
                Task::none()
            }

            Message::PollRss => Task::future(metrics::read_rss())
                .map(|result| Message::Rss(result.map_err(Arc::new))),

            Message::Rss(result) => {
                match result {
                    Ok(kb) => self.rss_kb = Some(kb),
                    Err(error) => {
                        self.last_error =
                            Some(Error::Rss(error::RssError::Parse(error.to_string())).to_string());
                    }
                }
                Task::none()
            }
        }
    }

    /// Adds lines to whichever pane is under test.
    fn append(&mut self, shared: &mut Shared, lines: &[Arc<str>]) -> Task<Message> {
        match &mut self.log {
            Log::Windowed(view) => {
                let result = view.append(&mut shared.log, lines);
                let request = self.note(result).flatten();
                Self::scroll(request)
            }
            Log::Capped(pane) => {
                pane.append(lines);
                Task::none()
            }
        }
    }

    /// Microseconds the last append took.
    fn append_us(&self, shared: &Shared) -> u128 {
        match &self.log {
            Log::Windowed(_) => shared.log.append_us(),
            Log::Capped(pane) => pane.timings().last_append_us,
        }
    }

    /// Runs a jump and records how long it took.
    fn timed_jump(
        &mut self,
        store: &Store,
        jump: impl FnOnce(&mut LogView, &Store) -> Result<ScrollRequest, StoreError>,
    ) -> Task<Message> {
        let Some(view) = self.view_mut() else {
            return Task::none();
        };

        let started = Instant::now();
        let result = jump(view, store);
        let request = self.note(result);
        self.last_jump_us = started.elapsed().as_micros();

        Self::scroll(request)
    }

    /// Writes the next slice of a `--fill`, until the whole of it is written.
    ///
    /// One step per message so the window keeps drawing, and the lines go
    /// straight into whichever pane is under test rather than through the
    /// widget's own controls.
    fn fill_step(&mut self, shared: &mut Shared, done: usize) -> Task<Message> {
        let wanted = self.options.fill;
        if done >= wanted {
            if self.options.shot.is_some() {
                println!("--- filled {wanted} lines ---");
                return Task::done(Message::PollRss)
                    .chain(Self::settle(500))
                    .chain(Task::done(Message::Shot));
            }

            println!(
                "--- filled {wanted} lines in {:.1} ms of appends ---",
                self.append_us(shared) as f64 / 1000.0
            );
            return Task::done(Message::PollRss)
                .chain(Self::settle(500))
                .chain(Task::done(if self.options.probe {
                    Message::ProbeStep(0)
                } else if !self.options.bench.is_empty() {
                    Message::BenchStep(0)
                } else if self.options.selftest {
                    Message::SelfTestArm
                } else {
                    Message::Done
                }));
        }

        let count = FILL_STEP.min(wanted - done);
        let batch = self.generator.batch(count);
        let task = self.append(shared, &batch.lines);

        task.chain(Task::done(Message::FillStep(done + count)))
    }

    /// The scripted measurement run.
    ///
    /// Each step is a message, so the window draws between them and the frame
    /// figures mean something.
    fn probe_step(&mut self, shared: &mut Shared, index: usize) -> Task<Message> {
        /// How many scroll steps the scrolling probe makes.
        const SCROLL_STEPS: usize = 240;

        match index {
            0 => {
                self.report_store(shared);
                Task::done(Message::ResetFrames)
                    .chain(Self::settle(1_500))
                    .chain(Task::done(Message::ProbeStep(1)))
            }

            1 => {
                let before = self.rebuilds();
                let task = self.timed_jump(&shared.log, LogView::jump_to_top);
                println!(
                    "jump to top        {:.2} ms, {} window rebuild(s), \
                     read {:.2} / build {:.2} / drop {:.2} / total {:.2} ms",
                    self.jump_ms(),
                    self.rebuilds() - before,
                    self.split().0,
                    self.split().1,
                    self.split().2,
                    self.split().3,
                );
                task.chain(Self::settle(400))
                    .chain(Task::done(Message::ProbeStep(2)))
            }

            2 => {
                let before = self.rebuilds();
                let middle = self.total_lines(shared) / 2;
                let task = self.timed_jump(&shared.log, move |view, store| {
                    view.jump_to_line(store, middle)
                });
                println!(
                    "jump to line {middle:<9}{:.2} ms, {} window rebuild(s)",
                    self.jump_ms(),
                    self.rebuilds() - before
                );
                task.chain(Self::settle(400))
                    .chain(Task::done(Message::ProbeStep(3)))
            }

            3 => {
                let before = self.rebuilds();
                let task = self.timed_jump(&shared.log, LogView::jump_to_tail);
                println!(
                    "jump to tail       {:.2} ms, {} window rebuild(s)",
                    self.jump_ms(),
                    self.rebuilds() - before
                );
                task.chain(Self::settle(400))
                    .chain(Task::done(Message::ResetFrames))
                    .chain(Self::settle(200))
                    .chain(Task::done(Message::ProbeStep(4)))
            }

            4 => {
                // Scroll the way a hard trackpad flick does — about 1,500
                // lines a second — with a real gap between steps.  The
                // stream runs throughout, both because that is what a device
                // is doing while the user scrolls and because a window only
                // redraws when something asks it to, so a still app produces
                // no frames to measure.
                self.streaming = true;
                self.rate = 200;
                self.probe_marker = Instant::now();
                self.probe_rebuilds = self.rebuilds();
                self.probe_rebuild_us = self.rebuild_us();

                let mut task = Task::done(Message::ResetFrames);
                for _ in 0..SCROLL_STEPS {
                    task = task
                        .chain(Task::done(Message::LogAction(
                            text_editor::Action::Scroll { lines: -12 },
                        )))
                        .chain(Self::settle(8));
                }
                task.chain(Task::done(Message::ProbeStep(5)))
            }

            5 => {
                let seconds = self.probe_marker.elapsed().as_secs_f64();
                let rebuilds = self.rebuilds() - self.probe_rebuilds;
                let spent = (self.rebuild_us() - self.probe_rebuild_us) as f64 / 1000.0;

                self.report_frames("scrolling, stream running");
                println!(
                    "  {rebuilds} window rebuilds in {seconds:.1} s, \
                     {:.2} ms each, {:.0}% of the update thread",
                    spent / rebuilds.max(1) as f64,
                    100.0 * spent / (seconds * 1000.0),
                );

                // Now the live tail: pinned to the bottom with lines arriving
                // at whatever `--rate` asked for, which is the other thing a
                // device log does.
                self.rate = self.options.rate;
                self.probe_marker = Instant::now();
                self.probe_rebuilds = self.rebuilds();
                self.probe_rebuild_us = self.rebuild_us();

                self.timed_jump(&shared.log, LogView::jump_to_tail)
                    .chain(Task::done(Message::ResetFrames))
                    .chain(Self::settle(4_000))
                    .chain(Task::done(Message::ProbeStep(6)))
            }

            6 => {
                let seconds = self.probe_marker.elapsed().as_secs_f64();
                let rebuilds = self.rebuilds() - self.probe_rebuilds;
                let spent = (self.rebuild_us() - self.probe_rebuild_us) as f64 / 1000.0;

                self.report_frames(&format!("live tail at {} lines/s", self.options.rate));
                println!(
                    "  {rebuilds} window rebuilds in {seconds:.1} s, \
                     {:.0}% of the update thread",
                    100.0 * spent / (seconds * 1000.0),
                );
                self.streaming = false;

                self.find = "cache miss ratio".to_owned();
                Task::done(Message::Find(true))
                    .chain(Self::settle(2_000))
                    .chain(Task::done(Message::ProbeStep(7)))
            }

            7 => {
                match self.found {
                    Some(scan) => println!(
                        "search whole log   {:.1} ms, {} hits, first at line {}",
                        scan.micros as f64 / 1000.0,
                        scan.hits,
                        self.found_line
                            .map_or("—".to_owned(), |line| line.to_string()),
                    ),
                    None => println!("search whole log   no result"),
                }

                Task::done(Message::SelectAll).chain(Task::done(Message::ProbeStep(8)))
            }

            8 => {
                if let Log::Windowed(view) = &mut self.log {
                    let started = Instant::now();
                    let text = view.selected_text(&shared.log);
                    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                    match text {
                        Ok(Some(text)) => println!(
                            "select-all + copy  {:.1} ms for {:.1} MB \
                             ({} lines, widget held {})",
                            elapsed,
                            text.len() as f64 / (1024.0 * 1024.0),
                            text.lines().count(),
                            view.window_len(),
                        ),
                        Ok(None) => println!("select-all + copy  nothing"),
                        Err(error) => println!("select-all + copy  {error}"),
                    }
                }

                Task::done(Message::PollRss)
                    .chain(Self::settle(500))
                    .chain(Task::done(Message::ProbeStep(9)))
            }

            _ => {
                self.report_store(shared);
                self.report_frames("after everything");
                println!("slowest update     {:.2} ms", self.slowest_update_ms);
                println!("slowest view       {:.2} ms", self.slowest_view_ms.get());
                Task::done(Message::Done)
            }
        }
    }

    /// The last rebuild's read, build, drop and total times, in
    /// milliseconds.
    fn split(&self) -> (f64, f64, f64, f64) {
        match &self.log {
            Log::Windowed(view) => {
                let timings = view.timings();
                (
                    timings.read_us as f64 / 1000.0,
                    timings.shape_us as f64 / 1000.0,
                    timings.drop_us as f64 / 1000.0,
                    timings.rebuild_us as f64 / 1000.0,
                )
            }
            Log::Capped(_) => (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Microseconds spent rebuilding the window so far.
    fn rebuild_us(&self) -> u128 {
        match &self.log {
            Log::Windowed(view) => view.timings().total_rebuild_us,
            Log::Capped(pane) => pane.timings().total_trim_us,
        }
    }

    /// Window rebuilds so far.
    fn rebuilds(&self) -> u64 {
        match &self.log {
            Log::Windowed(view) => view.timings().rebuilds,
            Log::Capped(_) => 0,
        }
    }

    /// Milliseconds the last jump took.
    fn jump_ms(&self) -> f64 {
        self.last_jump_us as f64 / 1000.0
    }

    /// How many lines the pane under test holds.
    fn total_lines(&self, shared: &Shared) -> usize {
        match &self.log {
            Log::Windowed(view) => view.total_lines(&shared.log),
            Log::Capped(pane) => pane.live_lines(),
        }
    }

    /// Prints what the store and the widget currently hold.
    fn report_store(&self, shared: &Shared) {
        println!("--- {} ---", self.options.mode);
        match &self.log {
            Log::Windowed(view) => println!(
                "lines stored       {}\n\
                 bytes stored       {:.1} MB\n\
                 line index         {:.1} MB resident\n\
                 lines in widget    {}\n\
                 store file         {}\n\
                 rss                {}",
                view.total_lines(&shared.log),
                shared.log.bytes() as f64 / (1024.0 * 1024.0),
                shared.log.index_bytes() as f64 / (1024.0 * 1024.0),
                view.window_len(),
                shared.log.path().display(),
                self.rss(),
            ),
            Log::Capped(pane) => println!(
                "lines live         {}\n\
                 lines received     {}\n\
                 lines dropped      {}\n\
                 rss                {}",
                pane.live_lines(),
                pane.received(),
                pane.dropped(),
                self.rss(),
            ),
        }
    }

    /// Prints the frame figures under a label.
    fn report_frames(&self, what: &str) {
        println!(
            "frames ({what}) {} fps, p95 {} ms, worst {:.1} ms",
            self.frames
                .fps()
                .map_or("—".to_owned(), |fps| format!("{fps:.1}")),
            self.frames
                .p95_ms()
                .map_or("—".to_owned(), |ms| format!("{ms:.1}")),
            self.frames.worst_ms(),
        );
    }

    /// The resident set size, as text.
    fn rss(&self) -> String {
        self.rss_kb.map_or("—".to_owned(), |kb| {
            format!("{:.1} MB", kb as f64 / 1024.0)
        })
    }

    /// Prints the figures for one `--bench` step.
    fn report_bench(&self, shared: &Shared, index: usize) {
        let count = self.options.bench.get(index).copied().unwrap_or(0);
        println!(
            "injected           {count}\n\
             generate           {:.1} ms\n\
             append             {:.1} ms over {} chunk(s)\n\
             slowest update     {:.1} ms\n\
             slowest view       {:.1} ms",
            self.last_generate_us as f64 / 1000.0,
            self.inject_append_us as f64 / 1000.0,
            self.inject_chunks,
            self.slowest_update_ms,
            self.slowest_view_ms.get(),
        );
        self.report_store(shared);
        self.report_frames("after inject");

        if let Log::Capped(pane) = &self.log {
            let timings = pane.timings();
            println!(
                "trim               {} x, {} lines last, {:.1} ms last, \
                 {:.1} ms total",
                timings.trims,
                timings.last_trim_lines,
                timings.last_trim_us as f64 / 1000.0,
                timings.total_trim_us as f64 / 1000.0,
            );
        }
    }

    /// Selects a few lines with the same actions a mouse drag makes, then
    /// copies them to the real system clipboard.
    fn selftest_select(&mut self, shared: &mut Shared) -> Task<Message> {
        let line_height = match self.log {
            Log::Windowed(_) => logview::LINE_H,
            Log::Capped(_) => TEXT_SIZE * 1.3,
        };

        let click = text_editor::Action::Click(iced::Point::new(1.0, line_height * 1.5));
        let drag = text_editor::Action::Drag(iced::Point::new(1.0, line_height * 3.5));

        let _ = self.dispatch(Message::LogAction(click), shared);
        let _ = self.dispatch(Message::LogAction(drag), shared);

        let selection = match &mut self.log {
            Log::Windowed(view) => {
                let result = view.selected_text(&shared.log);
                self.note(result).flatten()
            }
            Log::Capped(pane) => pane.selection(),
        };

        match selection {
            Some(selection) => {
                println!("--- selftest ---");
                println!("selected {} bytes:", selection.len());
                println!("{selection}");
                println!("--- writing to the system clipboard ---");
                iced::clipboard::write::<Message>(selection)
                    .chain(Self::settle(500))
                    .chain(Task::done(Message::Done))
            }
            None => {
                println!("--- selftest FAILED: nothing selected ---");
                Task::done(Message::Done)
            }
        }
    }

    /// Runs whatever the console's command line holds.
    ///
    /// The two device stages are sequenced with `and_then` over the shared
    /// error type: if opening the session fails, the execute stage never runs
    /// and the error arrives at the same place a command error would.
    fn submit_command(&mut self, shared: &Shared) -> Task<Message> {
        let line = std::mem::take(&mut self.console.input);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Task::none();
        }

        let echo: Arc<str> = Arc::from(format!("> {trimmed}"));
        self.console.scrollback.append(&[echo]);

        if self.console.history.last().map(String::as_str) != Some(trimmed) {
            self.console.history.push(trimmed.to_owned());
        }
        self.console.history_cursor = None;

        self.console.queue.push_back(trimmed.to_owned());
        self.run_next_command(shared)
    }

    /// Starts the next queued command, if the device is free.
    fn run_next_command(&mut self, shared: &Shared) -> Task<Message> {
        if self.console.busy {
            return Task::none();
        }

        let Some(command) = self.console.queue.pop_front() else {
            return Task::none();
        };

        self.console.busy = true;
        let connected = self.console.connected;
        let session = Session::for_device(shared.device.as_ref());

        Task::future(device::open_session(connected, session))
            .and_then(move |session| {
                let command = command.clone();
                Task::future(device::execute(session, command))
            })
            .map(|result| Message::ConsoleReply(result.map_err(Arc::new)))
    }

    /// Carries out whatever a reply asked for beyond printing.
    fn apply_effect(&mut self, effect: Effect) -> Task<Message> {
        match effect {
            Effect::None => Task::none(),
            Effect::Clear => {
                self.console.scrollback.clear();
                Task::none()
            }
            Effect::Disconnect => {
                self.console.connected = false;
                Task::none()
            }
            Effect::Flood(count) => {
                let mut generator = Generator::new();
                let batch = generator.batch(count);
                self.console.scrollback.append(&batch.lines);
                Task::none()
            }
        }
    }

    /// Steps through the command history.
    fn step_history(&mut self, step: isize) {
        if self.console.history.is_empty() {
            return;
        }
        let last = self.console.history.len() - 1;
        let next = match (self.console.history_cursor, step) {
            (None, -1) => Some(last),
            (None, _) => None,
            (Some(0), -1) => Some(0),
            (Some(index), -1) => Some(index - 1),
            (Some(index), _) if index >= last => None,
            (Some(index), _) => Some(index + 1),
        };

        self.console.history_cursor = next;
        self.console.input = next
            .and_then(|index| self.console.history.get(index))
            .cloned()
            .unwrap_or_default();
    }

    /// Draws the app.
    pub fn view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
        let started = Instant::now();
        let element = self.build_view(shared);
        let elapsed = started.elapsed().as_secs_f32() * 1000.0;
        self.slowest_view_ms
            .set(self.slowest_view_ms.get().max(elapsed));
        element
    }

    /// Builds the widget tree.
    fn build_view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
        let tabs = row![
            tab_button("Log pane", Tab::Log, self.tab),
            tab_button("Console pane", Tab::Console, self.tab),
            Space::new().width(Length::Fill),
            text(format!("RSS {}", self.rss())).size(TEXT_SIZE),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let body = match self.tab {
            Tab::Log => self.log_view(shared),
            Tab::Console => self.console_view(),
        };

        column![tabs, rule::horizontal(1), body]
            .spacing(8)
            .padding(10)
            .into()
    }

    /// Draws the log pane.
    fn log_view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
        let controls = row![
            button(text(if self.streaming { "Pause" } else { "Stream" }).size(TEXT_SIZE))
                .style(style::gold_button)
                .padding([5, 12])
                .on_press(Message::StreamToggled),
            text(format!("{} lines/s", self.rate)).size(TEXT_SIZE),
            slider(0..=5_000u32, self.rate, Message::RateChanged).width(140),
            plain("+1k", Message::Inject(1_000)),
            plain("+100k", Message::Inject(100_000)),
            plain("Clear", Message::ClearLog),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let controls = match &self.log {
            Log::Capped(pane) => controls.push(
                pick_list(
                    RETENTIONS,
                    Some(Retention(pane.retention())),
                    Message::RetentionChanged,
                )
                .style(style::picker)
                .text_size(TEXT_SIZE),
            ),
            Log::Windowed(_) => controls,
        };

        let navigation = row![
            plain("Top", Message::JumpTop),
            plain("Tail", Message::JumpTail),
            text_input("line", &self.goto)
                .on_input(Message::GotoInput)
                .on_submit(Message::GotoSubmit)
                .style(style::field)
                .size(TEXT_SIZE)
                .width(80),
            text_input("find in the whole log", &self.find)
                .id(FIND_INPUT)
                .on_input(Message::FindInput)
                .on_submit(Message::Find(true))
                .style(style::field)
                .size(TEXT_SIZE)
                .width(240),
            plain("Prev", Message::Find(false)),
            plain("Next", Message::Find(true)),
            text(self.find_status()).size(TEXT_SIZE),
            Space::new().width(Length::Fill),
            plain("Copy selection", Message::CopySelection),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        column![
            controls,
            navigation,
            self.log_status(shared),
            self.log_measurements(),
            self.log_body(shared),
            text(self.last_error.as_deref().unwrap_or(
                "drag to select  ·  drag past the edge to keep going  ·  \
                 Cmd+A selects the whole log  ·  Cmd+C copies it"
            ))
            .size(TEXT_SIZE),
        ]
        .spacing(8)
        .into()
    }

    /// What the find bar reports.
    fn find_status(&self) -> String {
        match (self.found, self.found_line) {
            (Some(scan), Some(line)) => format!(
                "{} hits, at line {line} ({:.0} ms)",
                scan.hits,
                scan.micros as f64 / 1000.0
            ),
            (Some(scan), None) => format!("{} hits", scan.hits),
            (None, _) => String::new(),
        }
    }

    /// The counts line.
    fn log_status(&self, shared: &Shared) -> Element<'_, Message> {
        let (left, hold) = match &self.log {
            Log::Windowed(view) => (
                format!(
                    "stored {}  ({:.1} MB on disk, {:.1} MB of index)  \
                     widget holds {} from line {}",
                    view.total_lines(&shared.log),
                    shared.log.bytes() as f64 / (1024.0 * 1024.0),
                    shared.log.index_bytes() as f64 / (1024.0 * 1024.0),
                    view.window_len(),
                    view.window_start(),
                ),
                view.hold(),
            ),
            Log::Capped(pane) => (
                format!(
                    "live {}  received {}  dropped {}  pending {}",
                    pane.live_lines(),
                    pane.received(),
                    pane.dropped(),
                    pane.pending_lines(),
                ),
                pane.hold().map(|reason| match reason {
                    logpane::HoldReason::ScrolledUp => HoldReason::ScrolledUp,
                    logpane::HoldReason::Selecting => HoldReason::Selecting,
                }),
            ),
        };

        let selected = match &self.log {
            Log::Windowed(view) => match view.selection_span() {
                Some(_) => format!("{} lines selected", view.selected_lines()),
                None => "no selection".to_owned(),
            },
            Log::Capped(pane) => match pane.selection() {
                Some(text) => format!("{} bytes selected", text.len()),
                None => "no selection".to_owned(),
            },
        };

        row![
            text(left).size(TEXT_SIZE),
            Space::new().width(Length::Fill),
            text(selected).size(TEXT_SIZE),
            text(match hold {
                Some(reason) => reason.label().to_owned(),
                None => "following tail".to_owned(),
            })
            .size(TEXT_SIZE),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    /// The timings line.
    fn log_measurements(&self) -> Element<'_, Message> {
        let left = match &self.log {
            Log::Windowed(view) => {
                let timings = view.timings();
                format!(
                    "window rebuild {:.2} ms ({} lines)   \
                     jump {:.2} ms   copy {:.1} ms / {} bytes",
                    timings.rebuild_us as f64 / 1000.0,
                    timings.rebuild_lines,
                    self.jump_ms(),
                    timings.copy_us as f64 / 1000.0,
                    timings.copy_bytes,
                )
            }
            Log::Capped(pane) => {
                let timings = pane.timings();
                format!(
                    "append {} lines in {:.1} ms   trim {} lines in {:.1} ms",
                    timings.last_append_lines,
                    timings.last_append_us as f64 / 1000.0,
                    timings.last_trim_lines,
                    timings.last_trim_us as f64 / 1000.0,
                )
            }
        };

        row![
            text(left).size(TEXT_SIZE),
            Space::new().width(Length::Fill),
            text(match (self.frames.fps(), self.frames.p95_ms()) {
                (Some(fps), Some(p95)) => format!(
                    "{fps:.0} fps   p95 {p95:.1} ms   worst {:.1} ms   \
                     slowest update {:.1} ms",
                    self.frames.worst_ms(),
                    self.slowest_update_ms,
                ),
                _ => "frames —".to_owned(),
            })
            .size(TEXT_SIZE),
            plain("Reset frames", Message::ResetFrames),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    /// The text itself.
    fn log_body<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
        match &self.log {
            Log::Windowed(view) => {
                let editor = text_editor(view.content())
                    .on_action(Message::LogAction)
                    .key_binding(log_key_binding)
                    .font(MONO)
                    .size(TEXT_SIZE)
                    .line_height(Pixels(logview::LINE_H))
                    .wrapping(iced::widget::text::Wrapping::None)
                    .height(Length::Fixed(view.window_px()))
                    .padding(0);

                // The two spacers make the scrollable's content as tall as
                // the whole log, so the thumb is sized and placed from the
                // real line count rather than from what the widget holds.
                let stack = column![
                    Space::new().height(Length::Fixed(view.above_px())),
                    editor,
                    Space::new().height(Length::Fixed(view.below_px(&shared.log))),
                ];

                let scroller = scrollable(stack)
                    .id(LOG_SCROLL)
                    .on_scroll(Message::LogScrolled)
                    .width(Length::Fill)
                    .height(Length::Fill);

                container(
                    sensor(scroller)
                        .on_show(Message::LogResized)
                        .on_resize(Message::LogResized),
                )
                .padding(8)
                .height(Length::Fill)
                .into()
            }

            Log::Capped(pane) => {
                let editor = text_editor(pane.content())
                    .on_action(Message::LogAction)
                    .font(MONO)
                    .size(TEXT_SIZE)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .height(Length::Fill)
                    .padding(8);

                container(editor).height(Length::Fill).into()
            }
        }
    }

    /// Draws the console pane.
    fn console_view(&self) -> Element<'_, Message> {
        let scrollback = text_editor(self.console.scrollback.content())
            .on_action(Message::ConsoleAction)
            .font(MONO)
            .size(TEXT_SIZE)
            .wrapping(iced::widget::text::Wrapping::None)
            .height(Length::Fill)
            .padding(8);

        let prompt = if self.console.connected {
            text_input("type a command, then Enter", &self.console.input)
                .on_input(Message::ConsoleInput)
                .on_submit(Message::ConsoleSubmit)
                .style(style::field)
                .font(MONO)
                .size(TEXT_SIZE)
                .padding(8)
        } else {
            text_input("disconnected", &self.console.input)
                .style(style::field)
                .font(MONO)
                .size(TEXT_SIZE)
                .padding(8)
        };

        let mut actions = row![
            text(if self.console.connected {
                "connected"
            } else {
                "disconnected"
            })
            .size(TEXT_SIZE),
            text(if self.console.busy {
                "· busy"
            } else {
                "· idle"
            })
            .size(TEXT_SIZE),
            text(format!("· {} in history", self.console.history.len())).size(TEXT_SIZE),
            Space::new().width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        if !self.console.connected {
            actions = actions.push(plain("Reconnect", Message::ConsoleConnect));
        }

        column![
            actions,
            container(scrollback).height(Length::Fill),
            row![text("$").size(TEXT_SIZE).font(MONO), prompt]
                .spacing(8)
                .align_y(Alignment::Center),
            text(format!(
                "scrollback {} lines  ·  Up/Down for history  ·  \
                 select and Cmd+C to copy",
                self.console.scrollback.live_lines(),
            ))
            .size(TEXT_SIZE),
        ]
        .spacing(8)
        .into()
    }
}

/// The log editor's key bindings.
///
/// Copy and select-all are taken away from the widget, because the widget
/// would answer them from the window it holds and the user means the whole
/// log.  Everything else keeps iced's own binding.
fn log_key_binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    use text_editor::{Binding, Status};

    if !matches!(press.status, Status::Focused { .. }) {
        return None;
    }

    if press.modifiers.command() {
        match press.key.to_latin(press.physical_key) {
            // Cut is copy here: the pane is read-only.
            Some('c') | Some('x') => {
                return Some(Binding::Custom(Message::CopySelection));
            }
            Some('a') => return Some(Binding::Custom(Message::SelectAll)),
            Some('f') => return Some(Binding::Custom(Message::FindFocus)),
            Some('g') => return Some(Binding::Custom(Message::Find(true))),
            _ => {}
        }
    }

    Binding::from_key_press(press)
}

/// The lines the self test selects from.
const SELFTEST_LINES: [&str; 8] = [
    "00:00:00.000 [      0] INFO  serve: slot 0 armed",
    "00:00:00.007 [      1] DEBUG pio:   sm1 stalled 3 cycles",
    "00:00:00.014 [      2] TRACE dma:   channel 4 wrap",
    "00:00:00.021 [      3] WARN  flash: XIP cache miss 0.4%",
    "00:00:00.028 [      4] ERROR usb:   heartbeat missed",
    "00:00:00.035 [      5] INFO  led:   solid green",
    "00:00:00.042 [      6] DEBUG rbcp:  bank switch 2 -> 3",
    "00:00:00.049 [      7] INFO  plugin: heartbeat 412/1024 bytes",
];

/// Writes a captured window to `path` as a PNG.
///
/// The picture shows the chrome and the scrollbar but **not** the log text:
/// `iced::window::screenshot` renders its own frame and comes back with the
/// editor's box and border and none of its glyphs.  That is true of the
/// capped pane too, so it is iced rather than this design — which is why the
/// evidence that the text is really there is `tests/windowed.rs`, where a
/// click at a pixel resolves to the right column.
fn write_png(path: &std::path::Path, shot: &iced::window::Screenshot) -> std::io::Result<()> {
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

/// A tab-selector button.
fn tab_button(label: &str, tab: Tab, current: Tab) -> Element<'_, Message> {
    let style = if tab == current {
        style::gold_button
    } else {
        style::icon_button
    };

    button(text(label).size(TEXT_SIZE))
        .style(style)
        .padding([6, 12])
        .on_press(Message::TabSelected(tab))
        .into()
}

/// A plain button.
///
/// The palette is the whole app's, so a stock iced button here would come out
/// in the builder's gold.  Gold means "do the main thing", and none of these
/// is that.
fn plain(label: &str, message: Message) -> iced::widget::Button<'_, Message> {
    button(text(label).size(TEXT_SIZE))
        .style(style::icon_button)
        .padding([5, 10])
        .on_press(message)
}
