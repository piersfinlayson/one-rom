
// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - stateful line editor
//!
//! Provides [`LineEditor`]: a single long-lived struct that handles all
//! byte-level CDC input across every prompt in a session, accumulating
//! command history as it goes.
//!
//! Replaces the old `read_raw_line` / `prompt_raw` pair in `parser.rs`.

use alloc::string::{String, ToString};

use embassy_futures::yield_now;
use embassy_time::Timer;

use crate::error::Error;
use crate::usb;

use super::send;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum line length in bytes.  Sufficient for any colon-syntax command.
pub const LINE_BUF: usize = 80;

/// Number of command-history entries retained across calls.
const HISTORY_CAP: usize = 16;

// ---------------------------------------------------------------------------
// Escape-sequence state machine
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq)]
enum EscState {
    Normal,
    GotEsc,
    GotBracket,
    /// `ESC [ 3` seen — waiting for `~` to confirm the Delete key.
    GotBracket3,
}

// ---------------------------------------------------------------------------
// History ring buffer
// ---------------------------------------------------------------------------

struct History {
    /// Slots, stored in insertion order.  `head` is the next write slot.
    entries: [Option<String>; HISTORY_CAP],
    /// Next write index (wraps at `HISTORY_CAP`).
    head: usize,
    /// Number of valid entries (saturates at `HISTORY_CAP`).
    count: usize,
}

impl History {
    fn new() -> Self {
        Self {
            entries: core::array::from_fn(|_| None),
            head: 0,
            count: 0,
        }
    }

    /// Append `line` to history.
    ///
    /// Empty lines and exact duplicates of the most-recent entry are dropped.
    fn push(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        if self.count > 0 && self.get(0) == Some(line) {
            return;
        }
        self.entries[self.head] = Some(line.to_string());
        self.head = (self.head + 1) % HISTORY_CAP;
        if self.count < HISTORY_CAP {
            self.count += 1;
        }
    }

    /// Return the `idx`th most-recent entry (`idx == 0` is the newest).
    fn get(&self, idx: usize) -> Option<&str> {
        if idx >= self.count {
            return None;
        }
        let slot = (self.head + HISTORY_CAP - 1 - idx) % HISTORY_CAP;
        self.entries[slot].as_deref()
    }

    fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// Line editor
// ---------------------------------------------------------------------------

/// Stateful line editor with history, cursor movement, and ANSI/VT100 support.
///
/// Create once per session (e.g. as a field of `SessionState`) so history
/// survives across individual `read_line` calls.
///
/// ## ANSI sequences handled
///
/// | Key         | Sequence          |
/// |-------------|-------------------|
/// | Left        | `ESC [ D`         |
/// | Right       | `ESC [ C`         |
/// | Up          | `ESC [ A`         |
/// | Down        | `ESC [ B`         |
/// | Home        | `ESC [ H`         |
/// | End         | `ESC [ F`         |
/// | Delete      | `ESC [ 3 ~`       |
/// | Backspace   | `0x08` or `0x7F`  |
/// | Ctrl-C      | `0x03`            |
pub struct LineEditor {
    /// Edit buffer — always valid UTF-8 (printable ASCII only).
    buf: [u8; LINE_BUF],
    /// Number of valid bytes in `buf`.
    len: usize,
    /// Cursor position within `buf` (0 ≤ `pos` ≤ `len`).
    pos: usize,
    /// Escape-sequence parser state; reset at the start of each `read_line`.
    esc: EscState,
    /// Command history.
    history: History,
    /// `None`    = cursor is on the live (in-progress) edit.
    /// `Some(n)` = cursor is on the nth-most-recent history entry.
    history_idx: Option<usize>,
    /// Snapshot of the live buffer taken the first time Up is pressed.
    /// Restored when Down brings the user back past the most-recent entry.
    saved_buf: [u8; LINE_BUF],
    saved_len: usize,
    /// Number of content bytes currently rendered on the terminal line.
    /// Used to clear only stale tail characters on redraw without ANSI CSI.
    display_len: usize,
}

impl LineEditor {
    pub fn new() -> Self {
        Self {
            buf: [0u8; LINE_BUF],
            len: 0,
            pos: 0,
            esc: EscState::Normal,
            history: History::new(),
            history_idx: None,
            saved_buf: [0u8; LINE_BUF],
            saved_len: 0,
            display_len: 0,
        }
    }

    /// Read one line, displaying `prompt` first.
    ///
    /// Returns:
    /// - `Ok(Some(s))` — Enter pressed; `s` is the trimmed line.
    /// - `Ok(None)`    — Ctrl-C pressed.
    /// - `Err(_)`      — USB host disconnected.
    ///
    /// When `with_history` is `true`:
    /// - The completed line is appended to history.
    /// - Up/Down arrow navigation is enabled.
    ///
    /// Set `with_history` to `false` for interactive sub-prompts (chip type,
    /// address, etc.) where history makes no sense.
    pub async fn read_line(
        &mut self,
        prompt: &str,
        with_history: bool,
    ) -> Result<Option<String>, Error> {
        // Reset per-call mutable state; history survives across calls.
        self.buf = [0u8; LINE_BUF];
        self.len = 0;
        self.pos = 0;
        self.esc = EscState::Normal;
        self.history_idx = None;
        self.display_len = 0;

        self.emit(prompt).await?;

        loop {
            let b = match usb::cdc_recv().await {
                Ok(b) => b,
                Err(_) => return Err(Error::UsbDisconnected),
            };

            match self.esc {
                // -----------------------------------------------------------------
                // Normal (non-escape) input
                // -----------------------------------------------------------------
                EscState::Normal => match b {
                    b'\r' => {
                        let s = core::str::from_utf8(&self.buf[..self.len])
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if with_history {
                            self.history.push(&s);
                        }
                        self.display_len = 0;
                        return Ok(Some(s));
                    }

                    b'\n' => {} // ignore; CR is the line terminator

                    // Ctrl-C: cancel the line.
                    0x03 => {
                        self.emit("^C\r\n").await?;
                        self.display_len = 0;
                        return Ok(None);
                    }

                    // ESC: begin escape sequence.
                    0x1B => {
                        self.esc = EscState::GotEsc;
                    }

                    // Backspace / DEL: destructive delete to the left.
                    0x08 | 0x7F => {
                        if self.delete_before() {
                            if self.pos == self.len {
                                // Deleted at EOL: simple VT100 destructive backspace.
                                self.emit("\x08 \x08").await?;
                                self.display_len = self.len;
                            } else {
                                // Deleted mid-line: repaint.
                                self.redraw(prompt).await?;
                            }
                        }
                    }

                    // Printable ASCII: insert at cursor.
                    b if b >= 0x20 => {
                        let was_eol = self.pos == self.len;
                        if self.insert(b) {
                            if was_eol {
                                // Fast path: simple echo.
                                let ch = [b];
                                if let Ok(s) = core::str::from_utf8(&ch) {
                                    self.emit(s).await?;
                                }
                                self.display_len = self.len;
                            } else {
                                // Inserted mid-line: repaint.
                                self.redraw(prompt).await?;
                            }
                        }
                        // Buffer full: character silently dropped.
                    }

                    _ => {} // other control characters ignored
                },

                // -----------------------------------------------------------------
                // Got ESC — expect `[` for CSI
                // -----------------------------------------------------------------
                EscState::GotEsc => {
                    self.esc = if b == b'[' {
                        EscState::GotBracket
                    } else {
                        EscState::Normal // unrecognised sequence; reset
                    };
                }

                // -----------------------------------------------------------------
                // Got ESC [ — expect the final byte of the CSI sequence
                // -----------------------------------------------------------------
                EscState::GotBracket => {
                    self.esc = EscState::Normal;
                    match b {
                        b'A' => {
                            // Up arrow: go back in history.
                            if with_history {
                                self.history_up();
                                self.redraw(prompt).await?;
                            }
                        }
                        b'B' => {
                            // Down arrow: go forward in history.
                            if with_history {
                                self.history_down();
                                self.redraw(prompt).await?;
                            }
                        }
                        b'C' => {
                            if self.pos < self.len {
                                let ch = [self.buf[self.pos]];
                                if let Ok(s) = core::str::from_utf8(&ch) {
                                    self.emit(s).await?;
                                }
                                self.pos += 1;
                            }
                        }
                        b'D' => {
                            if self.pos > 0 {
                                self.pos -= 1;
                                self.emit("\x08").await?;
                            }
                        }
                        b'H' => {
                            // Home (ESC [ H): jump to start.
                            if self.pos > 0 {
                                self.pos = 0;
                                self.reposition(prompt).await?;
                            }
                        }
                        b'F' => {
                            // End (ESC [ F): jump to end.
                            if self.pos < self.len {
                                self.pos = self.len;
                                self.reposition(prompt).await?;
                            }
                        }
                        b'3' => {
                            // Possible Delete key: ESC [ 3 ~.
                            self.esc = EscState::GotBracket3;
                        }
                        _ => {} // unrecognised CSI; reset (already done above)
                    }
                }

                // -----------------------------------------------------------------
                // Got ESC [ 3 — expect `~` for the Delete key
                // -----------------------------------------------------------------
                EscState::GotBracket3 => {
                    self.esc = EscState::Normal;
                    if b == b'~' {
                        // Delete key: forward-delete at cursor.
                        if self.delete_at() {
                            self.redraw(prompt).await?;
                        }
                    }
                    // Anything else: unknown sequence, silently ignore.
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Buffer operations
    // -------------------------------------------------------------------------

    /// Insert `b` at `pos`, shifting the tail right.  Returns `false` if the
    /// buffer is full.
    fn insert(&mut self, b: u8) -> bool {
        if self.len >= LINE_BUF {
            return false;
        }
        let mut i = self.len;
        while i > self.pos {
            self.buf[i] = self.buf[i - 1];
            i -= 1;
        }
        self.buf[self.pos] = b;
        self.pos += 1;
        self.len += 1;
        true
    }

    /// Delete the byte immediately before `pos` (backspace).  Returns `false`
    /// if the cursor is already at the start.
    fn delete_before(&mut self) -> bool {
        if self.pos == 0 {
            return false;
        }
        for i in self.pos..self.len {
            self.buf[i - 1] = self.buf[i];
        }
        self.pos -= 1;
        self.len -= 1;
        true
    }

    /// Delete the byte at `pos` (forward delete).  Returns `false` if the
    /// cursor is at or beyond the end.
    fn delete_at(&mut self) -> bool {
        if self.pos >= self.len {
            return false;
        }
        for i in self.pos..(self.len - 1) {
            self.buf[i] = self.buf[i + 1];
        }
        self.len -= 1;
        true
    }

    // -------------------------------------------------------------------------
    // History navigation
    // -------------------------------------------------------------------------

    fn history_up(&mut self) {
        let next = match self.history_idx {
            None => {
                if self.history.len() == 0 {
                    return;
                }
                // Save the live buffer before navigating away from it.
                self.saved_buf = self.buf;
                self.saved_len = self.len;
                0
            }
            Some(idx) => {
                if idx + 1 >= self.history.len() {
                    return; // already at the oldest entry
                }
                idx + 1
            }
        };
        self.history_idx = Some(next);
        self.load_history_entry(next);
    }

    fn history_down(&mut self) {
        match self.history_idx {
            None => {} // already on the live buffer; nothing to do
            Some(0) => {
                // Back to the live buffer.
                self.history_idx = None;
                self.buf = self.saved_buf;
                self.len = self.saved_len;
                self.pos = self.len;
            }
            Some(idx) => {
                let next = idx - 1;
                self.history_idx = Some(next);
                self.load_history_entry(next);
            }
        }
    }

    fn load_history_entry(&mut self, idx: usize) {
        if let Some(s) = self.history.get(idx) {
            let bytes = s.as_bytes();
            let n = bytes.len().min(LINE_BUF);
            self.buf[..n].copy_from_slice(&bytes[..n]);
            self.len = n;
            self.pos = n; // cursor always jumps to EOL on history recall
        }
    }

    // -------------------------------------------------------------------------
    // Display helpers
    // -------------------------------------------------------------------------

    /// Full line repaint.
    async fn redraw(&mut self, prompt: &str) -> Result<(), Error> {
        self.redraw_with_prev_len(prompt, self.display_len).await?;
        self.display_len = self.len;
        Ok(())
    }

    /// Full line repaint, clearing any stale tail characters left from a
    /// previously longer line using only CR, spaces and backspace.
    async fn redraw_with_prev_len(&self, prompt: &str, prev_len: usize) -> Result<(), Error> {
        let content = core::str::from_utf8(&self.buf[..self.len]).unwrap_or("");
        let clear_tail = prev_len.saturating_sub(self.len);
        let move_back = (self.len - self.pos) + clear_tail;
        let mut s = alloc::string::String::with_capacity(
            1 + prompt.len() + self.len + clear_tail + move_back,
        );
        s.push('\r');
        s.push_str(prompt);
        s.push_str(content);
        for _ in 0..clear_tail {
            s.push(' ');
        }
        for _ in 0..move_back {
            s.push('\x08');
        }
        self.emit(&s).await
    }

    /// Reposition the cursor to `self.pos` without repainting the buffer.
    ///
    /// Used for Home / End where only cursor placement changes.
    async fn reposition(&self, prompt: &str) -> Result<(), Error> {
        let partial = core::str::from_utf8(&self.buf[..self.pos]).unwrap_or("");
        self.emit(&alloc::format!("\r{}{}", prompt, partial)).await
    }

    /// Send `s` over CDC, retrying on `UsbFull`.
    ///
    /// A bounded retry count means we degrade gracefully if the host is slow,
    /// rather than blocking indefinitely.
    async fn emit(&self, s: &str) -> Result<(), Error> {
        let mut retries = 20u32;
        loop {
            match send(s) {
                Ok(()) => {
                    yield_now().await;
                    return Ok(());
                }
                Err(Error::UsbFull) => {
                    if retries == 0 {
                        return Ok(()); // best-effort; a display glitch is preferable to hanging
                    }
                    retries -= 1;
                    Timer::after_millis(10).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}