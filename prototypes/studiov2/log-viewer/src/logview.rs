//! A fixed-size window onto an unbounded log.
//!
//! # The shape of it
//!
//! [`Store`] holds every line the device ever sent.  The `text_editor` holds
//! [`LogView::window_len`] lines and no more, and those lines are read out of
//! the store each time the window moves.  So the widget's cost is set by the
//! window and the log's cost is set by the store's index — neither grows with
//! the other.
//!
//! The scrollbar is a real `scrollable`, wrapped around a column of three
//! things:
//!
//! ```text
//!   Space   window_start lines tall
//!   editor  window_len lines tall
//!   Space   the rest of the log tall
//! ```
//!
//! The two spacers make the scrollable's content exactly as tall as the whole
//! log, so its thumb is sized and placed from the real line count rather than
//! from what the editor happens to hold.  Moving the window changes the two
//! spacers by equal and opposite amounts, so the total never changes and the
//! scroll offset stays valid across a window move — which is what stops the
//! view jumping when the window slides.
//!
//! # The four things that had to be worked around
//!
//! 1. **The editor eats the wheel.** `text_editor` captures every
//!    `WheelScrolled` whose cursor is over it, whether or not it has anything
//!    to scroll, so the `scrollable` never sees one.  Its `Action::Scroll`
//!    comes to us instead and [`LogView::on_action`] answers with
//!    [`ScrollRequest::By`], which the caller turns into
//!    `iced::widget::operation::scroll_by`.  Wheel and trackpad both work,
//!    and the editor's own fractional-line accumulator is what makes trackpad
//!    scrolling smooth.
//! 2. **The editor's selection cannot leave the window.** `Content` only
//!    knows the lines it holds, so its cursor and anchor are window-local.
//!    This module keeps the caret and the anchor in whole-log coordinates
//!    ([`Pos`]) and re-applies a clamped copy to the widget after every window
//!    move.  What the user sees highlighted is the part of their selection
//!    inside the window, which is the part they can see.  What
//!    [`LogView::selected_text`] returns is the whole thing, read from the
//!    store.
//! 3. **`Content::move_to` cannot clear a selection.** It sets one when given
//!    `Some` and leaves the old one alone when given `None`, so clearing is
//!    done by setting a zero-length selection at the caret.
//! 4. **`Content` has no append and no trim**, so a window that slides forward
//!    over lines the widget already holds pastes the arriving lines at the end
//!    and deletes the departing ones from the top.  Handing the widget a fresh
//!    buffer instead would re-shape every line that had not moved, twice over,
//!    on every batch of lines the device sent.
//!
//! # What is not worked around
//!
//! An `Action::Scroll` moves the window but the editor's own scroll offset
//! stays at zero, because the editor's height is its content's height.  That
//! is deliberate: cosmic-text shapes as far as the editor's *bounds* reach, so
//! an editor told it is a thousand lines tall shapes a thousand lines.  The
//! window size is therefore the shaping cost, and [`WINDOW_DEFAULT`] is chosen
//! against that rather than against how much scrolling it buys.

use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use iced::widget::text_editor::{Action, Content, Cursor, Edit, Motion, Position};

use crate::store::{Pos, Store, StoreError};

/// The line height the pane renders at, in pixels.
///
/// Absolute rather than relative to the text size, because the spacer heights
/// either side of the editor have to agree with the editor's own line
/// spacing exactly.  A relative height would be `1.3 * size`, which is only a
/// round number by luck.
pub const LINE_H: f32 = 16.0;

/// How many lines the window holds unless told otherwise.
///
/// Putting a line into the widget costs about 35 microseconds, so this is the
/// size of the stall a jump puts on the update thread — a jump rebuilds the
/// whole window, and it is chosen to fit inside a frame.  A larger window
/// moves less often but stalls harder each time, and measuring 60, 120, 400
/// and 1200 over a million lines found the total work rising with the window
/// as well.  A live tail is not what sets it: that slides, and pays for the
/// lines arriving alone.
pub const WINDOW_DEFAULT: usize = 120;

/// The smallest window, as a multiple of what is on screen.
///
/// The window has to cover the visible span with room either side, so a tall
/// pane raises the floor whatever the switch asked for.
const WINDOW_SCREENS: f32 = 3.0;

/// How many visible lines to assume before the pane has been measured.
const ASSUMED_VISIBLE: f32 = 40.0;

/// Why the pane is not showing new lines as they arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldReason {
    /// The user scrolled up.
    ScrolledUp,
    /// The user has text selected, and sliding the window would move it under
    /// their hand.
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

/// What the pane needs the surrounding `scrollable` to do.
///
/// The pane cannot reach the scrollable itself, and it must not name a UI
/// message type, so it answers with this and the caller turns it into the
/// matching `iced::widget::operation`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollRequest {
    /// Scroll by this many pixels, positive being towards the end.
    By(f32),
    /// Scroll so this pixel offset is at the top.
    To(f32),
    /// Scroll to the very end.
    End,
}

/// What the last window move cost.
#[derive(Debug, Clone, Copy, Default)]
pub struct Timings {
    /// Microseconds the last window move took, reading included.
    pub rebuild_us: u128,
    /// Microseconds of that spent reading the store.
    pub read_us: u128,
    /// Microseconds of that spent putting the new lines into the widget.
    ///
    /// This is where a window move's time goes, because putting a line into
    /// the widget means shaping it at `Shaping::Advanced`.  A slide shapes the
    /// lines arriving and no others.  A rebuild shapes the whole window twice
    /// over, once at the placeholder size a fresh buffer carries and once at
    /// the real size, so a move that can slide costs a fraction of one that
    /// cannot.
    pub shape_us: u128,
    /// Microseconds of that spent getting rid of the lines leaving the window.
    ///
    /// Letting go of the replaced buffer, for a rebuild, or deleting the lines
    /// that fell off the top, for a slide.
    pub drop_us: u128,
    /// Lines the last move read out of the store.
    pub rebuild_lines: usize,
    /// Window moves since the pane was created.
    pub rebuilds: u64,
    /// Whether the last window move was a rebuild rather than a slide.
    pub rebuilt: bool,
    /// Microseconds spent moving the window since the pane was created.
    pub total_rebuild_us: u128,
    /// Microseconds the last copy took, and how many bytes it produced.
    pub copy_us: u128,
    /// Bytes the last copy produced.
    pub copy_bytes: usize,
}

/// A windowed view of a whole log.
pub struct LogView {
    /// The lines the widget holds.
    content: Content,
    /// The log revision the window was last built from.
    ///
    /// The log is shared, so it can grow without this view's `append` being
    /// the thing that grew it.  See [`LogView::refresh`].
    revision: u64,
    /// The first store line the window holds.
    window_start: usize,
    /// How many store lines the window holds.
    window_len: usize,
    /// How many lines the window aims to hold.
    ///
    /// The larger of what the caller asked for and what the pane's height
    /// needs.
    window_target: usize,
    /// How many lines the caller asked the window to hold.
    window_wanted: usize,
    /// The scroll offset, in pixels from the top of the whole log.
    ///
    /// Tracked rather than read, because `scroll_by` and `scroll_to` are
    /// widget operations and do not report back through `on_scroll`.  A real
    /// `on_scroll` — a thumb drag — overwrites it.
    offset_px: f32,
    /// The visible height of the pane, in pixels.
    viewport_px: f32,
    /// The caret, in whole-log coordinates.
    caret: Pos,
    /// Where the current selection started, in whole-log coordinates.
    anchor: Option<Pos>,
    /// Whether the left button is down and dragging a selection.
    dragging: bool,
    /// Where the last drag put the caret relative to the visible span:
    /// negative above it, positive below it, zero inside.
    drag_edge: f32,
    /// Whether the pane is pinned to the tail.
    follow: bool,
    /// Total lines ever appended.
    received: u64,
    /// What the last window move cost.
    timings: Timings,
}

impl LogView {
    /// Creates a view holding `window` lines at a time.
    ///
    /// The log itself is not held here.  Every method that reads it takes it,
    /// because the log is shared with the rest of the app and a copy of it in
    /// here would be a second version of the truth.
    pub fn new(window: usize) -> Self {
        Self {
            content: Content::new(),
            revision: 0,
            window_start: 0,
            window_len: 0,
            window_target: window.max(8),
            window_wanted: window.max(8),
            offset_px: 0.0,
            viewport_px: ASSUMED_VISIBLE * LINE_H,
            caret: Pos::START,
            anchor: None,
            dragging: false,
            drag_edge: 0.0,
            follow: true,
            received: 0,
            timings: Timings::default(),
        }
    }

    /// The buffer the widget renders.
    pub fn content(&self) -> &Content {
        &self.content
    }

    /// Lines the whole log holds.
    pub fn total_lines(&self, store: &Store) -> usize {
        store.len()
    }

    /// The first log line the window holds.
    pub fn window_start(&self) -> usize {
        self.window_start
    }

    /// How many log lines the window holds.
    pub fn window_len(&self) -> usize {
        self.window_len
    }

    /// Total lines ever appended.
    pub fn received(&self) -> u64 {
        self.received
    }

    /// What the last window move cost.
    pub fn timings(&self) -> Timings {
        self.timings
    }

    /// Whether the pane is pinned to the tail.
    pub fn following(&self) -> bool {
        self.follow && self.anchor.is_none()
    }

    /// Why the pane is not following the tail, if it is not.
    pub fn hold(&self) -> Option<HoldReason> {
        if self.anchor.is_some() {
            Some(HoldReason::Selecting)
        } else if !self.follow {
            Some(HoldReason::ScrolledUp)
        } else {
            None
        }
    }

    /// The caret, in whole-log coordinates.
    pub fn caret(&self) -> Pos {
        self.caret
    }

    /// The selection's extent in whole-log coordinates, ordered.
    pub fn selection_span(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor?;
        if anchor == self.caret {
            return None;
        }
        Some(if anchor <= self.caret {
            (anchor, self.caret)
        } else {
            (self.caret, anchor)
        })
    }

    /// How many whole lines the selection covers.
    pub fn selected_lines(&self) -> usize {
        self.selection_span()
            .map_or(0, |(from, to)| to.line - from.line + 1)
    }

    /// The whole selection, read from the store rather than the widget.
    ///
    /// This is what makes a selection larger than the window copyable: the
    /// widget never held those bytes, and the store hands them over in one
    /// read of the range between the two positions.
    pub fn selected_text(&mut self, store: &Store) -> Result<Option<String>, StoreError> {
        let Some((from, to)) = self.selection_span() else {
            return Ok(None);
        };

        let started = Instant::now();
        let text = store.span(from, to)?;
        self.timings.copy_us = started.elapsed().as_micros();
        self.timings.copy_bytes = text.len();

        Ok(Some(text))
    }

    // -- geometry ---------------------------------------------------------

    /// The height of the whole log, in pixels.
    pub fn total_px(&self, store: &Store) -> f32 {
        store.len() as f32 * LINE_H
    }

    /// The height of the spacer above the editor.
    pub fn above_px(&self) -> f32 {
        self.window_start as f32 * LINE_H
    }

    /// The height of the editor.
    pub fn window_px(&self) -> f32 {
        self.window_len as f32 * LINE_H
    }

    /// The height of the spacer below the editor.
    pub fn below_px(&self, store: &Store) -> f32 {
        (self.total_px(store) - self.above_px() - self.window_px()).max(0.0)
    }

    /// The first visible line, fractional.
    pub fn top_line(&self) -> f32 {
        self.offset_px / LINE_H
    }

    /// How many lines fit on screen.
    fn visible_lines(&self) -> f32 {
        (self.viewport_px / LINE_H).max(1.0)
    }

    /// The largest scroll offset the scrollable will accept.
    fn max_offset(&self, store: &Store) -> f32 {
        (self.total_px(store) - self.viewport_px).max(0.0)
    }

    /// Records the pane's measured height.
    pub fn set_viewport(&mut self, store: &Store, height: f32) -> Result<(), StoreError> {
        if (self.viewport_px - height).abs() < 0.5 {
            return Ok(());
        }
        self.viewport_px = height.max(LINE_H);
        self.window_target = self
            .window_wanted
            .max((self.visible_lines() * WINDOW_SCREENS) as usize);
        self.ensure_window(store)
    }

    // -- scrolling --------------------------------------------------------

    /// Takes the scroll offset the `scrollable` reported.
    pub fn scrolled_to(&mut self, store: &Store, offset_px: f32) -> Result<(), StoreError> {
        self.offset_px = offset_px.clamp(0.0, self.max_offset(store));
        self.follow = self.offset_px >= self.max_offset(store) - LINE_H * 0.5;
        self.ensure_window(store)
    }

    /// Applies a scroll this pane asked for, which reports nothing back.
    fn advance(&mut self, store: &Store, request: ScrollRequest) -> Result<(), StoreError> {
        let offset = match request {
            ScrollRequest::By(dy) => self.offset_px + dy,
            ScrollRequest::To(y) => y,
            ScrollRequest::End => self.max_offset(store),
        };

        self.scrolled_to(store, offset)
    }

    /// Jumps to the top of the log.
    pub fn jump_to_top(&mut self, store: &Store) -> Result<ScrollRequest, StoreError> {
        let request = ScrollRequest::To(0.0);
        self.advance(store, request)?;
        Ok(request)
    }

    /// Jumps to the tail and pins there.
    pub fn jump_to_tail(&mut self, store: &Store) -> Result<ScrollRequest, StoreError> {
        self.advance(store, ScrollRequest::End)?;
        self.follow = true;
        Ok(ScrollRequest::End)
    }

    /// Jumps so `line` sits a third of the way down the pane.
    pub fn jump_to_line(
        &mut self,
        store: &Store,
        line: usize,
    ) -> Result<ScrollRequest, StoreError> {
        let target = (line as f32 - self.visible_lines() / 3.0).max(0.0) * LINE_H;
        let request = ScrollRequest::To(target.min(self.max_offset(store)));
        self.advance(store, request)?;
        Ok(request)
    }

    // -- the window -------------------------------------------------------

    /// Where the window would sit for the current scroll offset.
    fn wanted_start(&self, store: &Store) -> usize {
        let total = store.len();
        if total <= self.window_target {
            return 0;
        }

        let lead = self.lead();
        let top = self.top_line().floor() as usize;
        top.saturating_sub(lead).min(total - self.window_target)
    }

    /// How many lines of the window sit above the visible span.
    fn lead(&self) -> usize {
        let spare = (self.window_target as f32 - self.visible_lines()).max(0.0);
        (spare / 2.0) as usize
    }

    /// Moves the window if the visible span has drifted too near its edge.
    ///
    /// The guard is half the lead, so a scroll of a few lines does not rebuild
    /// and a scroll of a screenful does.
    fn ensure_window(&mut self, store: &Store) -> Result<(), StoreError> {
        let total = store.len();
        let wanted_len = self.window_target.min(total);
        let guard = (self.lead() / 2).max(1) as f32;

        let top = self.top_line();
        let bottom = top + self.visible_lines();
        let start = self.window_start as f32;
        let end = start + self.window_len as f32;

        let drifted = (top < start + guard && self.window_start > 0)
            || (bottom > end - guard && self.window_start + self.window_len < total)
            || self.window_len != wanted_len;

        if !drifted {
            return Ok(());
        }

        self.set_window(store, self.wanted_start(store), wanted_len)
    }

    /// Moves the window to `start..start + len` and puts the selection back.
    ///
    /// A move forward that keeps some of what the widget already holds slides
    /// — the lines arriving are pasted on and the lines leaving are deleted —
    /// and every line that stays keeps the shaped layout it already has.  That
    /// is the difference between a live tail costing a fifth of a core and
    /// costing half of one, because a rebuild pays for the whole window twice
    /// on every batch of lines however few arrived.  A jump rebuilds, and so
    /// does a move backwards: reading history is a moment's work and a live
    /// tail is every tick of the session.
    fn set_window(&mut self, store: &Store, start: usize, len: usize) -> Result<(), StoreError> {
        let total = store.len();
        let len = len.min(total);
        let start = start.min(total - len);

        let started = Instant::now();
        let old_start = self.window_start;
        let old_end = old_start + self.window_len;

        let slides =
            self.window_len > 0 && start >= old_start && start <= old_end && start + len >= old_end;

        if slides {
            self.slide_window(store, start, len)?;
        } else {
            self.rebuild_window(store, start, len)?;
        }

        self.timings.rebuilt = !slides;
        self.window_start = start;
        self.window_len = len;
        self.apply_selection();

        self.timings.rebuild_us = started.elapsed().as_micros();
        self.timings.rebuilds += 1;
        self.timings.total_rebuild_us += self.timings.rebuild_us;

        Ok(())
    }

    /// Replaces the whole window with a fresh buffer.
    ///
    /// The expensive path, and unavoidably so.  `Content::with_text` builds a
    /// cosmic-text buffer and, because a fresh buffer has no size set, shapes
    /// every line of it — at `Shaping::Advanced` and at a placeholder font
    /// size of 1.0, which the widget then throws away and re-shapes at the
    /// real size on its next layout.  iced offers no way to hand an existing
    /// `Content` a new body, so a jump pays for the window twice.
    fn rebuild_window(
        &mut self,
        store: &Store,
        start: usize,
        len: usize,
    ) -> Result<(), StoreError> {
        let reading = Instant::now();
        let text = store.text(start..start + len)?;
        self.timings.read_us = reading.elapsed().as_micros();
        self.timings.rebuild_lines = len;

        let shaping = Instant::now();
        let previous = std::mem::replace(&mut self.content, Content::with_text(&text));
        self.timings.shape_us = shaping.elapsed().as_micros();

        // Measured separately because it is not small: a buffer that has been
        // drawn holds a shaped layout for every line, and letting go of one is
        // a third of the cost of building the next.
        let dropping = Instant::now();
        drop(previous);
        self.timings.drop_us = dropping.elapsed().as_micros();

        Ok(())
    }

    /// Slides the window forward over lines the widget already holds.
    ///
    /// iced gives a `Content` no append and no trim, so both are worked around
    /// the way [`crate::logpane`] does: the arriving lines go in as a paste at
    /// the end, and the departing ones are selected from the top and deleted.
    /// The caller has already checked that the two ranges overlap.
    fn slide_window(&mut self, store: &Store, start: usize, len: usize) -> Result<(), StoreError> {
        let old_start = self.window_start;
        let old_end = old_start + self.window_len;

        let reading = Instant::now();
        let arriving = store.text(old_end..start + len)?;
        self.timings.read_us = reading.elapsed().as_micros();
        self.timings.rebuild_lines = start + len - old_end;

        let shaping = Instant::now();
        if !arriving.is_empty() {
            self.content.perform(Action::Move(Motion::DocumentEnd));
            self.content
                .perform(Action::Edit(Edit::Paste(Arc::new(format!("\n{arriving}")))));
        }
        self.timings.shape_us = shaping.elapsed().as_micros();

        let leaving = start - old_start;
        let dropping = Instant::now();
        if leaving > 0 {
            self.content.move_to(Cursor {
                position: Position { line: 0, column: 0 },
                selection: Some(Position {
                    line: leaving,
                    column: 0,
                }),
            });
            self.content.perform(Action::Edit(Edit::Delete));
        }
        self.timings.drop_us = dropping.elapsed().as_micros();

        Ok(())
    }

    /// The window's line range.
    pub fn window(&self) -> Range<usize> {
        self.window_start..self.window_start + self.window_len
    }

    // -- coordinates ------------------------------------------------------

    /// Turns a widget position into a whole-log one.
    fn to_global(&self, at: Position) -> Pos {
        Pos {
            line: self.window_start + at.line,
            column: at.column,
        }
    }

    /// Turns a whole-log position into a widget one, clamped to the window.
    ///
    /// A position above the window becomes the very start of it and one below
    /// becomes the very end, so a selection reaching past the window is drawn
    /// as covering everything the user can see of it.
    fn to_local(&self, at: Pos) -> Position {
        if self.window_len == 0 {
            return Position { line: 0, column: 0 };
        }

        let last = self.window_len - 1;
        if at.line < self.window_start {
            return Position { line: 0, column: 0 };
        }

        let line = at.line - self.window_start;
        if line > last {
            return Position {
                line: last,
                column: self.local_line_len(last),
            };
        }

        Position {
            line,
            column: at.column.min(self.local_line_len(line)),
        }
    }

    /// How many bytes the window's `line` holds.
    fn local_line_len(&self, line: usize) -> usize {
        self.content.line(line).map_or(0, |line| line.text.len())
    }

    /// Puts the tracked caret and anchor back into the widget.
    fn apply_selection(&mut self) {
        let position = self.to_local(self.caret);
        let selection = Some(match self.anchor {
            Some(anchor) => self.to_local(anchor),
            // `move_to` leaves an old selection alone when given `None`, so a
            // zero-length one at the caret is how a selection is cleared.
            None => position,
        });

        self.content.move_to(Cursor {
            position,
            selection,
        });
    }

    /// Takes the caret from the widget, leaving the anchor alone.
    fn take_caret(&mut self) {
        self.caret = self.to_global(self.content.cursor().position);
    }

    /// Takes both ends of the selection from the widget.
    fn take_selection(&mut self) {
        let cursor = self.content.cursor();
        self.caret = self.to_global(cursor.position);
        self.anchor = match cursor.selection {
            Some(anchor) if anchor != cursor.position => Some(self.to_global(anchor)),
            _ => None,
        };
    }

    /// Takes a word or line selection from the widget.
    ///
    /// A double or triple click needs its own path, because cosmic-text
    /// records those as `Selection::Word` and `Selection::Line` holding the
    /// *click* position at both ends and expands them only when the selection
    /// is drawn or copied.  Reading the cursor the way a drag does would
    /// therefore see an empty selection.  What is selected is always inside
    /// the window, so the text the widget reports is enough to place it.
    fn take_expanded(&mut self) {
        let cursor = self.content.cursor();
        let Some(selected) = self.content.selection() else {
            self.take_selection();
            return;
        };

        let line = cursor.position.line;
        let text = self
            .content
            .line(line)
            .map(|line| line.text.into_owned())
            .unwrap_or_default();

        let start = locate(&text, &selected, cursor.position.column);
        self.anchor = Some(self.to_global(Position {
            line,
            column: start,
        }));
        self.caret = self.to_global(Position {
            line,
            column: (start + selected.len()).min(text.len()),
        });
    }

    // -- input ------------------------------------------------------------

    /// Handles an action from the widget.
    ///
    /// Edits are dropped, which is what makes the editor read-only.  A scroll
    /// is answered rather than performed, because the editor has nothing to
    /// scroll — its height is its content's height, and the surrounding
    /// `scrollable` is what moves.
    pub fn on_action(
        &mut self,
        store: &Store,
        action: Action,
    ) -> Result<Option<ScrollRequest>, StoreError> {
        if action.is_edit() {
            return Ok(None);
        }

        match action {
            Action::Scroll { lines } => {
                let request = ScrollRequest::By(lines as f32 * LINE_H);
                self.advance(store, request)?;
                return Ok(Some(request));
            }

            Action::Click(_) => {
                self.content.perform(action);
                self.take_selection();
                self.anchor = Some(self.caret);
                self.dragging = true;
                self.drag_edge = 0.0;
            }

            Action::Drag(point) => {
                self.content.perform(action);
                self.take_caret();
                self.drag_edge = self.edge_of(point);
            }

            Action::SelectWord | Action::SelectLine => {
                self.content.perform(action);
                self.take_expanded();
            }

            // Cmd+Up and Cmd+Down mean the whole log, not the window.
            Action::Move(Motion::DocumentStart) => {
                self.caret = Pos::START;
                self.anchor = None;
                return self.jump_to_top(store).map(Some);
            }
            Action::Move(Motion::DocumentEnd) => {
                self.caret = store.end();
                self.anchor = None;
                return self.jump_to_tail(store).map(Some);
            }
            Action::Select(Motion::DocumentStart) => {
                self.anchor.get_or_insert(self.caret);
                self.caret = Pos::START;
                return self.jump_to_top(store).map(Some);
            }
            Action::Select(Motion::DocumentEnd) => {
                self.anchor.get_or_insert(self.caret);
                self.caret = store.end();
                return self.jump_to_tail(store).map(Some);
            }

            Action::Move(_) | Action::Select(_) => {
                self.content.perform(action);
                self.take_selection();
                return self.follow_caret(store).map(Some);
            }

            Action::SelectAll => {
                return self.select_all(store).map(|()| None);
            }

            Action::Edit(_) => {}
        }

        Ok(None)
    }

    /// Where a drag point sits relative to the visible span, in lines.
    ///
    /// Zero inside it, negative above, positive below.  The caller uses this
    /// to keep scrolling while the button is held outside the pane, which the
    /// widget does not do for itself.
    fn edge_of(&self, point: iced::Point) -> f32 {
        let line = self.window_start as f32 + point.y / LINE_H;
        let top = self.top_line();
        let bottom = top + self.visible_lines();

        if line < top {
            line - top
        } else if line > bottom {
            line - bottom
        } else {
            0.0
        }
    }

    /// How far past the visible span the drag is, in lines.
    pub fn drag_overrun(&self) -> Option<f32> {
        (self.dragging && self.drag_edge != 0.0).then_some(self.drag_edge)
    }

    /// Ends a drag.
    pub fn release(&mut self) {
        self.dragging = false;
        self.drag_edge = 0.0;
    }

    /// Scrolls one step while a drag is held outside the pane, extending the
    /// selection to the edge it is heading for.
    pub fn drag_scroll(&mut self, store: &Store) -> Result<ScrollRequest, StoreError> {
        let overrun = self.drag_edge;
        let step = overrun.signum() * overrun.abs().clamp(1.0, 12.0);
        let request = ScrollRequest::By(step * LINE_H);
        self.advance(store, request)?;

        let line = if step < 0.0 {
            self.top_line().floor() as usize
        } else {
            (self.top_line() + self.visible_lines()).floor() as usize
        };

        let line = line.min(store.len().saturating_sub(1));
        self.caret = Pos {
            line,
            column: if step < 0.0 { 0 } else { store.line_len(line)? },
        };
        self.apply_selection();

        Ok(request)
    }

    /// Brings the caret back into view if a motion took it out.
    fn follow_caret(&mut self, store: &Store) -> Result<ScrollRequest, StoreError> {
        let top = self.top_line();
        let bottom = top + self.visible_lines() - 1.0;
        let caret = self.caret.line as f32;

        let target = if caret < top {
            caret
        } else if caret > bottom {
            caret - self.visible_lines() + 1.0
        } else {
            top
        };

        let request = ScrollRequest::To((target * LINE_H).max(0.0));
        self.advance(store, request)?;
        Ok(request)
    }

    /// Selects the whole log.
    ///
    /// The widget only ever holds the window, so what this does is set the
    /// whole-log anchor and caret and then paint the window from end to end.
    pub fn select_all(&mut self, store: &Store) -> Result<(), StoreError> {
        if store.is_empty() {
            return Ok(());
        }

        self.anchor = Some(Pos::START);
        self.caret = store.end();
        self.apply_selection();
        Ok(())
    }

    /// Selects a byte range of the log and scrolls it into view.
    pub fn reveal(
        &mut self,
        store: &Store,
        from: u64,
        len: usize,
    ) -> Result<ScrollRequest, StoreError> {
        let start = store.position_of(from);
        let end = store.position_of(from + len as u64);

        self.anchor = Some(start);
        self.caret = end;
        self.follow = false;

        let request = self.jump_to_line(store, start.line)?;
        self.apply_selection();
        Ok(request)
    }

    // -- the log ----------------------------------------------------------

    /// Adds lines to the log.
    ///
    /// The store always takes them.  The window moves only when the pane is
    /// following the tail, so scrolled-back reading holds its place and a
    /// selection is never disturbed — and either way nothing is queued and
    /// nothing is dropped.
    pub fn append(
        &mut self,
        store: &mut Store,
        lines: &[Arc<str>],
    ) -> Result<Option<ScrollRequest>, StoreError> {
        if lines.is_empty() {
            return Ok(None);
        }

        store.append(lines)?;
        self.received += lines.len() as u64;

        if !self.following() {
            // The window is unchanged, but the log below it grew, so the
            // window may now be shorter than its target.
            if self.window_len < self.window_target {
                self.set_window(store, self.window_start, self.window_target)?;
            }
            self.revision = store.revision();
            return Ok(None);
        }

        self.offset_px = self.max_offset(store);
        self.set_window(
            store,
            store.len().saturating_sub(self.window_target),
            self.window_target,
        )?;
        self.caret = store.end();
        self.anchor = None;
        self.revision = store.revision();

        Ok(Some(ScrollRequest::End))
    }

    /// Rebuilds the window if some other screen appended to the log.
    ///
    /// A screen that appends through its own [`LogView::append`] never needs
    /// this.  A screen sharing the log with others does, because nothing else
    /// tells its widget that the bytes underneath it moved.
    pub fn refresh(&mut self, store: &Store) -> Result<Option<ScrollRequest>, StoreError> {
        if self.revision == store.revision() {
            return Ok(None);
        }
        self.revision = store.revision();

        if !self.following() {
            self.ensure_window(store)?;
            return Ok(None);
        }

        self.offset_px = self.max_offset(store);
        self.set_window(
            store,
            store.len().saturating_sub(self.window_target),
            self.window_target,
        )?;
        self.caret = store.end();
        self.anchor = None;

        Ok(Some(ScrollRequest::End))
    }

    /// A handle that can search the whole log off the update thread.
    pub fn searcher(&self, store: &Store) -> Result<crate::store::Searcher, StoreError> {
        store.searcher()
    }

    /// The file offset the caret sits at, for a search to start from.
    pub fn caret_offset(&self, store: &Store) -> Result<u64, StoreError> {
        store.offset_of(self.caret)
    }

    /// Turns a scan's byte offset into a line number.
    pub fn line_of(&self, store: &Store, offset: u64) -> usize {
        store.position_of(offset).line
    }
}

/// Where in `line` the occurrence of `selected` covering `near` starts.
///
/// A word can appear several times in one line, so the one the caret sits in
/// is the one meant.  Anything that cannot be placed falls back to the start
/// of the line, which is what a triple click selected anyway.
fn locate(line: &str, selected: &str, near: usize) -> usize {
    if selected.is_empty() {
        return near.min(line.len());
    }

    line.match_indices(selected)
        .map(|(start, _)| start)
        .find(|&start| near >= start && near <= start + selected.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view over `count` numbered lines, with a `window`-line window.
    ///
    /// The store comes back beside the view rather than inside it, because
    /// the log is shared state and the view does not own it.
    fn filled(count: usize, window: usize) -> (LogView, Store) {
        let mut store = Store::temporary().expect("a temporary store");
        let mut view = LogView::new(window);
        let lines: Vec<Arc<str>> = (0..count)
            .map(|i| Arc::from(format!("line {i:06}")))
            .collect();
        view.append(&mut store, &lines).expect("the append to land");
        (view, store)
    }

    #[test]
    fn the_window_stays_the_same_size_however_long_the_log_is() {
        for count in [1_000usize, 100_000, 1_000_000] {
            let (view, store) = filled(count, 200);
            assert_eq!(view.window_len(), 200, "{count} lines");
            assert_eq!(view.total_lines(&store), count);
        }
    }

    #[test]
    fn a_selection_larger_than_the_window_copies_whole() {
        let (mut view, store) = filled(50_000, 200);

        view.anchor = Some(Pos {
            line: 10,
            column: 0,
        });
        view.caret = Pos {
            line: 40_000,
            column: 4,
        };

        let text = view
            .selected_text(&store)
            .expect("the store to serve the span")
            .expect("a selection");

        assert_eq!(view.selected_lines(), 39_991);
        assert!(text.starts_with("line 000010"));
        assert!(text.ends_with("line"));
        assert_eq!(text.lines().count(), 39_991);
        assert!(
            text.len() > view.window_len() * 12,
            "the copy must be far larger than the window"
        );
    }

    #[test]
    fn select_all_covers_the_log_not_the_window() {
        let (mut view, store) = filled(20_000, 300);
        view.select_all(&store).expect("select all");

        let text = view
            .selected_text(&store)
            .expect("the store to serve the span")
            .expect("a selection");

        assert_eq!(text.lines().count(), 20_000);
        assert!(text.starts_with("line 000000"));
        assert!(text.ends_with("line 019999"));

        // The widget itself only ever held the window.
        assert_eq!(view.content().line_count(), 300);
    }

    #[test]
    fn scrolling_moves_the_window_without_changing_the_total() {
        let (mut view, store) = filled(500_000, 400);
        let total = view.total_px(&store);

        view.jump_to_top(&store).expect("a jump");
        assert_eq!(view.window_start(), 0);
        assert_eq!(view.total_px(&store), total);

        view.jump_to_line(&store, 250_000).expect("a jump");
        assert!(view.window().contains(&250_000), "{:?}", view.window());
        assert_eq!(view.total_px(&store), total);
        assert_eq!(view.window_len(), 400);

        view.jump_to_tail(&store).expect("a jump");
        assert!(view.window().contains(&499_999));
        assert_eq!(view.total_px(&store), total);
    }

    #[test]
    fn reading_history_holds_its_place_while_the_log_grows() {
        let (mut view, mut store) = filled(10_000, 200);
        view.jump_to_line(&store, 5_000).expect("a jump");

        let start = view.window_start();
        let top = view.top_line();

        for _ in 0..20 {
            let lines: Vec<Arc<str>> = (0..50).map(|i| Arc::from(format!("late {i}"))).collect();
            view.append(&mut store, &lines).expect("an append");
        }

        assert_eq!(view.window_start(), start, "the window must not move");
        assert_eq!(view.top_line(), top, "the view must not move");
        assert_eq!(view.total_lines(&store), 11_000, "the log must still grow");
        assert_eq!(view.hold(), Some(HoldReason::ScrolledUp));
    }

    /// The lines the widget holds, in order.
    fn widget_lines(view: &LogView) -> Vec<String> {
        (0..view.content.line_count())
            .filter_map(|index| view.content.line(index))
            .map(|line| line.text.into_owned())
            .collect()
    }

    /// The lines the store holds over the widget's window.
    fn store_lines(view: &LogView, store: &Store) -> Vec<String> {
        store
            .text(view.window())
            .expect("the store to serve the window")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_slid_window_holds_what_a_rebuilt_one_would() {
        let (mut view, mut store) = filled(400, 120);

        // A tail that slides, a jump that rebuilds, a scroll forward that
        // slides again: after each the widget must hold its window exactly.
        for batch in 1..40usize {
            let lines: Vec<Arc<str>> = (0..batch)
                .map(|i| Arc::from(format!("tail {batch:03}-{i:03}")))
                .collect();
            view.append(&mut store, &lines).expect("an append");
            assert_eq!(widget_lines(&view), store_lines(&view, &store), "{batch}");
        }

        view.jump_to_line(&store, 200).expect("a jump");
        assert_eq!(widget_lines(&view), store_lines(&view, &store));
        assert!(view.timings().rebuilt, "a jump must rebuild");

        view.jump_to_line(&store, 260).expect("a short scroll");
        assert_eq!(widget_lines(&view), store_lines(&view, &store));
        assert!(!view.timings().rebuilt, "an overlapping move must slide");
    }

    #[test]
    fn following_the_tail_slides_the_window() {
        let (mut view, mut store) = filled(10_000, 200);
        view.jump_to_tail(&store).expect("a jump");

        for _ in 0..10 {
            let lines: Vec<Arc<str>> = (0..100).map(|i| Arc::from(format!("late {i}"))).collect();
            view.append(&mut store, &lines).expect("an append");
        }

        assert_eq!(view.total_lines(&store), 11_000);
        assert_eq!(view.window(), 10_800..11_000);
        assert!(view.following());
    }

    #[test]
    fn a_search_hit_is_selected_and_scrolled_to() {
        let (mut view, store) = filled(200_000, 200);
        let searcher = view.searcher(&store).expect("a searcher");
        let scan = searcher.scan("line 123456", 0).expect("a scan");

        assert_eq!(scan.hits, 1);
        let offset = scan.first.expect("a hit");
        view.reveal(&store, offset, "line 123456".len())
            .expect("a reveal");

        assert!(view.window().contains(&123_456), "{:?}", view.window());
        assert_eq!(
            view.selected_text(&store)
                .expect("the store to serve the span")
                .as_deref(),
            Some("line 123456")
        );
    }
}
