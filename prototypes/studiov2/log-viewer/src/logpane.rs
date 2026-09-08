//! The live log pane.
//!
//! # Why a `text_editor` and not a `column` of `text`
//!
//! `iced::widget::text` cannot be selected with the mouse — issue #36 has been
//! open since 2019 and PR #3315 is unmerged — so a `column` of `text` inside a
//! `scrollable` gives up copy/paste, and lays out every line every frame
//! besides.  `text_editor` has drag-select, double- and triple-click,
//! `Ctrl+A` and `Ctrl+C`, and it shapes only the lines its viewport shows.
//! Held read-only by dropping edit actions, it is the only stock iced widget
//! that meets the requirement.
//!
//! # Why the editor is not inside a `scrollable`
//!
//! `text_editor` scrolls itself.  Given `Length::Fill` it takes the height it
//! is offered and asks cosmic-text to shape only as far as its viewport
//! reaches, which is what makes a large buffer affordable at all.
//!
//! A `scrollable` needs its child to report the height of the whole content,
//! so the editor would have to be `Length::Shrink`.  That path reports
//! `Editor::min_bounds`, which `iced_graphics` computes with `text::measure`
//! over the buffer's *shaped* layout runs — the lines shaped so far, not the
//! buffer.  The reported height would describe a fraction of the log, and the
//! scrollable would size itself to that.  So the editor keeps `Length::Fill`
//! and its own scrolling.
//!
//! The cost is that `scrollable`'s `anchor_bottom` and `on_scroll(Viewport)`
//! are not available to us, so following the tail is tracked by hand — see
//! [`LogPane::on_action`].
//!
//! # What it costs, measured
//!
//! * A single `Edit::Paste` is quadratic in the lines it inserts: 1,000 lines
//!   take 4 ms, 9,000 take 247 ms and 90,000 take 27 seconds, all on the
//!   update thread.  Split every append into chunks and the same 90,000 lines
//!   take 72 ms.  `LogPane::write` therefore never pastes more than it is
//!   handed, and the caller keeps its batches small.
//! * A trim costs roughly the lines dropped times the lines left behind,
//!   because cosmic-text's `delete_range` removes each line from a `Vec` and
//!   pushes it onto the front of a change record — two O(n) moves per line.
//!   Dropping 200 lines costs 3 ms at a 2,000-line cap, 13 ms at 10,000 and
//!   53 ms at 50,000.
//! * A line that has been on screen holds about 11 KB of resident memory for
//!   as long as it stays in the buffer, because iced asks cosmic-text to shape
//!   with `prune: false` and never releases a shaped layout.  Measured
//!   steady-state: 105 MB at a 1,000-line cap, 150 MB at 5,000, 207 MB at
//!   10,000, 316 MB at 20,000, against a 94 MB empty baseline.  The retention
//!   cap is what bounds this, and it is the reason the default is 10,000.
//!
//! # The three things iced does not give us
//!
//! 1. **No append.** `Content` has `new`, `with_text` and `perform`, and
//!    nothing else that mutates.  A line is appended by moving the cursor to
//!    the end and pasting.  Batching matters: one paste of a thousand joined
//!    lines costs far less than a thousand pastes.
//! 2. **No trim.** Dropping the oldest lines is done by selecting them with
//!    [`Content::move_to`], which takes a whole `Cursor` including its
//!    selection anchor, and then performing `Edit::Delete`, which deletes a
//!    selection when there is one.  Two calls, no per-line loop.
//! 3. **No scroll position.** `Content` exposes the cursor but not the
//!    viewport, so "is the user at the bottom?" is tracked from the
//!    `Action::Scroll` events the widget hands us.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use iced::widget::text_editor::{Action, Content, Edit, Motion};
use iced::widget::text_editor::{Cursor, Position};

/// How far, in lines, the tracked scroll offset may sit from the bottom and
/// still count as "at the bottom".  A wheel notch is a fraction of a line, so
/// this needs to be forgiving of rounding without swallowing a real scroll.
const AT_BOTTOM_EPSILON: f32 = 0.75;

/// How the retention cap drops the oldest lines.
///
/// Neither route is given by iced.  `Content` has no trim, so both are worked
/// around it, and they cost very different amounts — see the prototype's report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrimMode {
    /// Select the doomed lines with `Content::move_to` and delete them.
    ///
    /// Two calls whatever the count, and no second copy of the text.  It is
    /// also slow: cosmic-text's `delete_range` copies every deleted line into
    /// a change record for undo, and the cursor moves it makes force shaping
    /// at both ends of the buffer.
    #[default]
    SelectDelete,

    /// Keep a mirror of the lines and rebuild the `Content` from the tail.
    ///
    /// Costs a second copy of the text — around 9 MB at 100,000 lines — and
    /// throws away the shaped layout of every retained line.  `Content` does
    /// no shaping until something asks it to, so that turns out to be the
    /// cheap direction.
    Rebuild,
}

/// Where a trim spent its time.
#[derive(Debug, Clone, Copy, Default)]
pub struct TrimBreakdown {
    /// Microseconds setting the selection, or rebuilding the buffer.
    pub select_us: u128,
    /// Microseconds deleting, or zero when rebuilding.
    pub delete_us: u128,
    /// Microseconds returning the cursor to the tail.
    pub restore_us: u128,
}

/// Why the pane is not currently showing new lines as they arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldReason {
    /// The user scrolled up.
    ScrolledUp,
    /// The user has text selected, and appending would destroy the selection.
    Selecting,
}

impl HoldReason {
    /// A short label for the status bar.
    pub fn label(self) -> &'static str {
        match self {
            Self::ScrolledUp => "held: scrolled up",
            Self::Selecting => "held: selection",
        }
    }
}

/// Timings from the last append, for the status bar.
#[derive(Debug, Clone, Copy, Default)]
pub struct Timings {
    /// Lines in the last append.
    pub last_append_lines: usize,
    /// Microseconds the last append's paste took.
    pub last_append_us: u128,
    /// Lines dropped by the last trim.
    pub last_trim_lines: usize,
    /// Microseconds the last trim took.
    pub last_trim_us: u128,
    /// Where the last trim spent its time.
    pub last_trim_breakdown: TrimBreakdown,
    /// Trims performed since the pane was created.
    pub trims: u64,
    /// Microseconds spent trimming since the pane was created.
    pub total_trim_us: u128,
}

/// The log pane's state.
///
/// `Content` is deliberately not cloned anywhere: iced implements
/// `Clone for Content` as `Content::with_text(&self.text())`, which serialises
/// and re-shapes the entire buffer.  At 100,000 lines that is a multi-second
/// operation, so the whole model is mutated in place and messages carry
/// `Arc<str>` line batches rather than copies of the buffer.
pub struct LogPane {
    /// The selectable, scrollable buffer the widget renders.
    content: Content,
    /// How many lines `content` holds.  Tracked rather than asked for, because
    /// `Content::line_count` borrows a `RefCell` and walks nothing useful for
    /// us that we do not already know.
    live_lines: usize,
    /// Lines received while the pane is held, oldest first.
    pending: Vec<Arc<str>>,
    /// Why the pane is held, or `None` when it is following the tail.
    hold: Option<HoldReason>,
    /// The most lines to keep.  `None` means keep everything.
    retention: Option<usize>,
    /// Tracked distance from the bottom, in lines.  Zero means at the bottom.
    scroll_backlog: f32,
    /// Total lines ever handed to the pane.
    received: u64,
    /// Total lines dropped by the retention cap.
    dropped: u64,
    /// Timings from the last append and trim.
    timings: Timings,
    /// How the retention cap drops lines.
    trim_mode: TrimMode,
    /// A copy of the live lines, kept only for [`TrimMode::Rebuild`].
    mirror: VecDeque<Arc<str>>,
    /// How far over the cap the buffer is allowed to run before a trim.
    ///
    /// A trim's cost grows with the buffer it deletes from *and* with how
    /// many lines it deletes, so a small slack means frequent cheap trims and
    /// a large one means rare expensive trims.  `None` means a tenth of the
    /// cap.
    trim_slack: Option<usize>,
}

impl LogPane {
    /// Creates an empty log pane with the given retention cap.
    pub fn new(retention: Option<usize>) -> Self {
        Self::with_trim_mode(retention, TrimMode::default())
    }

    /// Creates an empty log pane with the given retention cap and trim mode.
    pub fn with_trim_mode(retention: Option<usize>, trim_mode: TrimMode) -> Self {
        Self {
            content: Content::new(),
            live_lines: 0,
            pending: Vec::new(),
            hold: None,
            retention,
            scroll_backlog: 0.0,
            received: 0,
            dropped: 0,
            timings: Timings::default(),
            trim_mode,
            mirror: VecDeque::new(),
            trim_slack: None,
        }
    }

    /// Sets how many lines over the cap the buffer may run before a trim.
    pub fn set_trim_slack(&mut self, slack: Option<usize>) {
        self.trim_slack = slack;
    }

    /// The buffer the widget renders.
    pub fn content(&self) -> &Content {
        &self.content
    }

    /// Lines currently held in the buffer.
    pub fn live_lines(&self) -> usize {
        self.live_lines
    }

    /// Lines waiting for the pane to resume following.
    pub fn pending_lines(&self) -> usize {
        self.pending.len()
    }

    /// Total lines ever handed to the pane.
    pub fn received(&self) -> u64 {
        self.received
    }

    /// Total lines dropped by the retention cap.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Timings from the last append and trim.
    pub fn timings(&self) -> Timings {
        self.timings
    }

    /// Why the pane is held, or `None` when it is following the tail.
    pub fn hold(&self) -> Option<HoldReason> {
        self.hold
    }

    /// The current retention cap.
    pub fn retention(&self) -> Option<usize> {
        self.retention
    }

    /// Sets the retention cap, trimming at once if the buffer already exceeds
    /// it.
    pub fn set_retention(&mut self, retention: Option<usize>) {
        self.retention = retention;
        self.trim();
    }

    /// The text currently selected, if any.
    pub fn selection(&self) -> Option<String> {
        self.content.selection()
    }

    /// Empties the pane.
    ///
    /// `Content::new` rather than a select-all-and-delete, because throwing the
    /// buffer away is the one case where rebuilding is the cheap option.
    pub fn clear(&mut self) {
        self.content = Content::new();
        self.live_lines = 0;
        self.mirror.clear();
        self.pending.clear();
        self.scroll_backlog = 0.0;
        self.hold = None;
        self.timings = Timings::default();
    }

    /// Handles an action from the widget.
    ///
    /// Edits are dropped, which is what makes the editor read-only.  Everything
    /// else — clicks, drags, motions, selection, scrolling — is passed through,
    /// and that is what keeps selection and `Ctrl+C` working.
    pub fn on_action(&mut self, action: Action) {
        if action.is_edit() {
            return;
        }

        // Track the viewport before performing, so the scroll bookkeeping and
        // the action agree about which line the user is looking at.
        match action {
            Action::Scroll { lines } => {
                // A negative `lines` moves the view up, away from the tail.
                let ceiling = self.live_lines as f32;
                self.scroll_backlog = (self.scroll_backlog - lines as f32).clamp(0.0, ceiling);
            }
            Action::Move(Motion::DocumentEnd) | Action::Select(Motion::DocumentEnd) => {
                self.scroll_backlog = 0.0;
            }
            Action::Move(Motion::DocumentStart)
            | Action::Select(Motion::DocumentStart)
            | Action::SelectAll => {
                self.scroll_backlog = self.live_lines as f32;
            }
            Action::Move(_)
            | Action::Select(_)
            | Action::SelectWord
            | Action::SelectLine
            | Action::Edit(_)
            | Action::Click(_)
            | Action::Drag(_) => {}
        }

        self.content.perform(action);
        self.reassess_hold();
    }

    /// Decides whether the pane should be following the tail, and flushes
    /// anything held if it should.
    fn reassess_hold(&mut self) {
        let scrolled_up = self.scroll_backlog > AT_BOTTOM_EPSILON;
        let selecting = self.content.selection().is_some();

        self.hold = if selecting {
            // Checked first: appending would collapse the selection, and a
            // user mid-drag cares about that more than about live output.
            Some(HoldReason::Selecting)
        } else if scrolled_up {
            Some(HoldReason::ScrolledUp)
        } else {
            None
        };

        if self.hold.is_none() && !self.pending.is_empty() {
            let flush = std::mem::take(&mut self.pending);
            self.write(&flush);
        }
    }

    /// Returns to the tail, flushing anything held.
    pub fn follow_tail(&mut self) {
        self.content.perform(Action::Move(Motion::DocumentEnd));
        self.scroll_backlog = 0.0;
        self.reassess_hold();
    }

    /// Adds lines to the pane.
    ///
    /// While the pane is held they queue up instead, capped at the retention
    /// limit so that a held pane cannot grow without bound either.
    pub fn append(&mut self, lines: &[Arc<str>]) {
        self.received += lines.len() as u64;

        if self.hold.is_some() {
            self.pending.extend_from_slice(lines);
            if let Some(cap) = self.retention
                && self.pending.len() > cap
            {
                let excess = self.pending.len() - cap;
                self.pending.drain(..excess);
                self.dropped += excess as u64;
            }
            return;
        }

        self.write(lines);
    }

    /// Writes lines into the buffer and trims.
    ///
    /// The whole batch becomes one string and one paste.  Pasting per line
    /// costs a cursor motion, a shaping pass and a `RefCell` borrow each time,
    /// and is the difference between a hundred milliseconds and several
    /// seconds for a ten-thousand-line batch.
    fn write(&mut self, lines: &[Arc<str>]) {
        if lines.is_empty() {
            return;
        }

        // If the cap is smaller than the batch, only the tail of the batch can
        // survive the trim.  Dropping the rest here avoids shaping text that is
        // about to be deleted.
        let (lines, pre_dropped) = match self.retention {
            Some(cap) if cap < lines.len() => {
                let skip = lines.len() - cap;
                (&lines[skip..], skip)
            }
            _ => (lines, 0),
        };
        self.dropped += pre_dropped as u64;

        let mut joined = String::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
        for line in lines {
            if self.live_lines > 0 || !joined.is_empty() {
                joined.push('\n');
            }
            joined.push_str(line);
        }

        if self.trim_mode == TrimMode::Rebuild {
            self.mirror.extend(lines.iter().cloned());
        }

        let started = Instant::now();
        self.content.perform(Action::Move(Motion::DocumentEnd));
        self.content
            .perform(Action::Edit(Edit::Paste(Arc::new(joined))));
        self.live_lines += lines.len();
        self.timings.last_append_lines = lines.len();
        self.timings.last_append_us = started.elapsed().as_micros();

        self.trim();
    }

    /// Drops the oldest lines when the buffer is over its cap.
    ///
    /// There is no trim API, so the oldest lines are *selected* and deleted.
    /// `Content::move_to` takes a `Cursor` whose `selection` field is the
    /// anchor, which sets an arbitrary range in one call, and `Edit::Delete`
    /// deletes a selection when one exists.  Two calls, whatever the count.
    ///
    /// Trimming runs only when the buffer is a tenth over the cap, so a
    /// steady stream pays for one trim per ten per cent of the buffer rather
    /// than one per line.
    fn trim(&mut self) {
        let Some(cap) = self.retention else {
            return;
        };
        let slack = self.trim_slack.unwrap_or(cap / 10).max(1);
        if self.live_lines <= cap.saturating_add(slack) {
            return;
        }

        // Trim all the way back to the cap.  Bounding this to the slack looks
        // attractive — a trim costs roughly the lines deleted times the lines
        // left behind — but then a buffer fed batches larger than the slack
        // never converges and the cap stops holding.  The slack is hysteresis
        // only: it decides how often a trim runs, not how much it removes.
        let drop = self.live_lines - cap;
        let started = Instant::now();
        let mut breakdown = TrimBreakdown::default();

        match self.trim_mode {
            TrimMode::SelectDelete => {
                let step = Instant::now();
                self.content.move_to(Cursor {
                    position: Position { line: 0, column: 0 },
                    selection: Some(Position {
                        line: drop,
                        column: 0,
                    }),
                });
                breakdown.select_us = step.elapsed().as_micros();

                let step = Instant::now();
                self.content.perform(Action::Edit(Edit::Delete));
                breakdown.delete_us = step.elapsed().as_micros();
            }

            TrimMode::Rebuild => {
                let step = Instant::now();
                self.mirror.drain(..drop);
                let mut text = String::with_capacity(self.mirror.iter().map(|l| l.len() + 1).sum());
                for (index, line) in self.mirror.iter().enumerate() {
                    if index > 0 {
                        text.push('\n');
                    }
                    text.push_str(line);
                }
                self.content = Content::with_text(&text);
                breakdown.select_us = step.elapsed().as_micros();
            }
        }

        // `move_to` scrolled the viewport to line zero to place the cursor,
        // and a rebuilt buffer starts at the top.  Put the view back at the
        // tail, which is where a trimming pane is by definition — it only
        // trims while following.
        let step = Instant::now();
        self.content.perform(Action::Move(Motion::DocumentEnd));
        breakdown.restore_us = step.elapsed().as_micros();

        self.live_lines -= drop;
        self.dropped += drop as u64;
        self.timings.last_trim_lines = drop;
        self.timings.last_trim_us = started.elapsed().as_micros();
        self.timings.last_trim_breakdown = breakdown;
        self.timings.trims += 1;
        self.timings.total_trim_us += self.timings.last_trim_us;
    }
}
