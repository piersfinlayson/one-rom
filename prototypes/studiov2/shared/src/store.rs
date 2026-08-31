//! The whole log: a file, plus an in-memory index of where each line starts.
//!
//! # Why a file and not a `Vec` of lines
//!
//! Three reasons, in order of weight.
//!
//! 1. **A copy across an arbitrary range is one read.** A user selecting from
//!    line 12 to line 900,000 wants those bytes, and the file holds them
//!    contiguously: one `read_at` of the byte range between the two positions.
//!    A `Vec<Arc<str>>` would have to walk and join 900,000 allocations.
//! 2. **Memory is the index alone.** Eight bytes a line, so a million lines
//!    cost 8 MB and the text sits in the page cache, which the kernel reclaims
//!    when it needs to.  A `Vec<Arc<str>>` of the same log costs about 112 MB
//!    that it never gives back: 16 bytes of fat pointer, 16 bytes of `Arc`
//!    header and the line itself, each in its own allocation.  A single
//!    `String` arena with the same offset index would cost about 88 MB, which
//!    is better but still every byte resident for the whole session.
//! 3. **It is what the CLI already does.** `onerom monitor log --output`
//!    writes a transcript, so a GUI whose log store *is* that transcript needs
//!    no second format and can be pointed at a file from a previous session.
//!
//! Searching gets the same benefit: [`Store::searcher`] hands out a second
//! file handle and a list of line-aligned byte ranges, and that handle scans
//! off the update thread while the log keeps growing.  Nothing is shared and
//! nothing is locked.
//!
//! # What is uncapped
//!
//! Everything.  A line written here is never dropped, and no method removes
//! one.  The bound on the process is the index, and the bound on the disk is
//! the disk.

use std::fs::{File, OpenOptions};
use std::io;
use std::ops::Range;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Anything that can go wrong reaching the log store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The backing file could not be created or opened.
    #[error("could not open the log store at {path}: {source}")]
    Open {
        /// The path that failed.
        path: PathBuf,
        /// Why it failed.
        #[source]
        source: io::Error,
    },

    /// A write to the backing file failed.
    #[error("could not write to the log store: {0}")]
    Write(#[source] io::Error),

    /// A read from the backing file failed.
    #[error("could not read the log store: {0}")]
    Read(#[source] io::Error),

    /// A line index was past the end of the log.
    #[error("line {line} is past the end of a {len}-line log")]
    OutOfRange {
        /// The line asked for.
        line: usize,
        /// How many lines the log holds.
        len: usize,
    },

    /// The bytes read back were not UTF-8, which means the index is wrong.
    #[error("the log store holds bytes that are not UTF-8 at offset {offset}")]
    NotUtf8 {
        /// Where the bad bytes start.
        offset: u64,
    },
}

/// A position in the whole log.
///
/// `column` is a **byte** index into the line, which is what iced's
/// `text_editor::Position::column` also holds — see `iced_graphics`'s editor,
/// which passes it straight to cosmic-text's `Cursor::index`.  That means a
/// widget position converts to a file offset with an addition and no scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    /// The line, counted from the start of the log.
    pub line: usize,
    /// The byte offset within that line.
    pub column: usize,
}

impl Pos {
    /// The very start of the log.
    pub const START: Self = Self { line: 0, column: 0 };

    /// A position at the start of `line`.
    pub fn line(line: usize) -> Self {
        Self { line, column: 0 }
    }
}

/// The whole log.
pub struct Store {
    /// Where the backing file lives.
    path: PathBuf,
    /// The backing file, opened for reading and writing.
    ///
    /// Every read and write is positional (`read_at` / `write_all_at`), so
    /// there is no seek position to keep straight and a read never has to
    /// wait for a buffer to be flushed.
    file: File,
    /// Whether the file survives the process.
    keep: bool,
    /// Byte offset of the start of each line.
    starts: Vec<u64>,
    /// Bytes written, including the newline that terminates every line.
    bytes: u64,
    /// Wall time spent in `append`, in microseconds.
    append_us: u128,
    /// Bumped by every `append`.
    ///
    /// A screen showing the log holds its own window over it in a widget, and
    /// a widget cannot be refreshed from `view`.  Comparing this against the
    /// number the screen last rebuilt at is how it learns that some other
    /// screen appended.
    revision: u64,
}

impl Store {
    /// Creates a store backed by `path`, truncating anything already there.
    ///
    /// The file is left behind when the process exits, which is the point of
    /// naming one.
    pub fn at(path: &Path) -> Result<Self, StoreError> {
        Self::open(path.to_owned(), true)
    }

    /// Creates a store backed by a temporary file, removed on drop.
    pub fn temporary() -> Result<Self, StoreError> {
        let name = format!(
            "onerom-log-viewer-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );

        Self::open(std::env::temp_dir().join(name), false)
    }

    /// Opens the backing file.
    fn open(path: PathBuf, keep: bool) -> Result<Self, StoreError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|source| StoreError::Open {
                path: path.clone(),
                source,
            })?;

        Ok(Self {
            path,
            file,
            keep,
            starts: Vec::new(),
            bytes: 0,
            append_us: 0,
            revision: 0,
        })
    }

    /// Where the log is written.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many lines the log holds.
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    /// Whether the log holds no lines.
    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    /// How many bytes the log holds, newlines included.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// How much memory the line index holds, in bytes.
    ///
    /// This is the whole resident cost of the store, and the number that says
    /// what a session of a given length costs the process.
    pub fn index_bytes(&self) -> usize {
        self.starts.capacity() * size_of::<u64>()
    }

    /// Wall time spent appending since the store was created, in microseconds.
    pub fn append_us(&self) -> u128 {
        self.append_us
    }

    /// How many times the log has grown.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Appends lines to the log.
    ///
    /// One `write_all_at` for the whole batch, so a batch costs one syscall
    /// whatever it holds.  Nothing here can drop a line.
    pub fn append(&mut self, lines: &[Arc<str>]) -> Result<(), StoreError> {
        if lines.is_empty() {
            return Ok(());
        }

        let started = Instant::now();

        let mut buffer = Vec::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
        let mut offset = self.bytes;

        self.starts.reserve(lines.len());
        for line in lines {
            self.starts.push(offset);
            buffer.extend_from_slice(line.as_bytes());
            buffer.push(b'\n');
            offset += line.len() as u64 + 1;
        }

        self.file
            .write_all_at(&buffer, self.bytes)
            .map_err(StoreError::Write)?;
        self.bytes = offset;
        self.revision += 1;
        self.append_us += started.elapsed().as_micros();

        Ok(())
    }

    /// The byte offset of the start of `line`.
    ///
    /// A line one past the end answers with the end of the log, which is what
    /// a selection running to the very bottom needs.
    fn start_of(&self, line: usize) -> Result<u64, StoreError> {
        match self.starts.get(line) {
            Some(&offset) => Ok(offset),
            None if line == self.starts.len() => Ok(self.bytes),
            None => Err(StoreError::OutOfRange {
                line,
                len: self.starts.len(),
            }),
        }
    }

    /// How many bytes `line` holds, its newline excluded.
    pub fn line_len(&self, line: usize) -> Result<usize, StoreError> {
        let start = self.start_of(line)?;
        let end = self.start_of(line + 1)?;
        Ok((end.saturating_sub(start).saturating_sub(1)) as usize)
    }

    /// The file offset a position names.
    pub fn offset_of(&self, at: Pos) -> Result<u64, StoreError> {
        let start = self.start_of(at.line)?;
        let len = if at.line < self.starts.len() {
            self.line_len(at.line)?
        } else {
            0
        };

        Ok(start + at.column.min(len) as u64)
    }

    /// The position a file offset falls in.
    ///
    /// A binary search over the index, so it costs a handful of comparisons
    /// whatever the log holds.  This is how a search hit's byte offset becomes
    /// a line and a column.
    pub fn position_of(&self, offset: u64) -> Pos {
        let line = match self.starts.binary_search(&offset) {
            Ok(exact) => exact,
            Err(0) => 0,
            Err(after) => after - 1,
        };

        Pos {
            line,
            column: (offset - self.starts.get(line).copied().unwrap_or(0)) as usize,
        }
    }

    /// The position of the very end of the log.
    pub fn end(&self) -> Pos {
        match self.starts.len() {
            0 => Pos::START,
            n => Pos {
                line: n - 1,
                column: self.line_len(n - 1).unwrap_or(0),
            },
        }
    }

    /// Reads a byte range out of the log.
    fn read(&self, range: Range<u64>) -> Result<String, StoreError> {
        if range.end <= range.start {
            return Ok(String::new());
        }

        let mut buffer = vec![0u8; (range.end - range.start) as usize];
        self.file
            .read_exact_at(&mut buffer, range.start)
            .map_err(StoreError::Read)?;

        String::from_utf8(buffer).map_err(|_| StoreError::NotUtf8 {
            offset: range.start,
        })
    }

    /// The text of a range of lines, joined by newlines and with no trailing
    /// one.
    ///
    /// This is what fills the widget's window, so it is on the path of every
    /// scroll that moves the window.  It costs one read of the bytes asked
    /// for, which for a few hundred lines is a few tens of kilobytes out of
    /// the page cache.
    pub fn text(&self, lines: Range<usize>) -> Result<String, StoreError> {
        let end = lines.end.min(self.starts.len());
        if lines.start >= end {
            return Ok(String::new());
        }

        let from = self.start_of(lines.start)?;
        // Up to the start of the line after the last one wanted, less its
        // newline, so the result has no trailing blank line.
        let to = self.start_of(end)?.saturating_sub(1);

        self.read(from..to)
    }

    /// The text between two positions, in the order given.
    ///
    /// One read whatever the span, which is the whole reason the log lives in
    /// a file: a selection covering a million lines is a single `read_at`.
    pub fn span(&self, from: Pos, to: Pos) -> Result<String, StoreError> {
        let (from, to) = if from <= to { (from, to) } else { (to, from) };
        let start = self.offset_of(from)?;
        let end = self.offset_of(to)?;

        self.read(start..end.max(start))
    }

    /// The whole log as one string.
    pub fn all(&self) -> Result<String, StoreError> {
        self.span(Pos::START, self.end())
    }

    /// Hands out a handle that can scan the log from another thread.
    ///
    /// The handle carries its own file descriptor and a snapshot of the
    /// line-aligned chunk boundaries, so it shares nothing with the store and
    /// the log can keep growing underneath it.  It sees the log as it was when
    /// the handle was made.
    pub fn searcher(&self) -> Result<Searcher, StoreError> {
        /// Roughly how many bytes a chunk covers.  Chunks land on line
        /// boundaries, so a chunk is always valid UTF-8 and no match can
        /// straddle two.
        const TARGET: u64 = 4 * 1024 * 1024;

        let file = self.file.try_clone().map_err(|source| StoreError::Open {
            path: self.path.clone(),
            source,
        })?;

        let mut chunks = Vec::new();
        let mut start = 0u64;
        let mut line = 0usize;

        while line < self.starts.len() {
            // Walk forward a line at a time only when the log is tiny; for a
            // real one, jump by a guess and then land on the next line start.
            let guess = start + TARGET;
            let next = match self.starts[line..].binary_search(&guess) {
                Ok(hit) => line + hit,
                Err(after) => line + after,
            };

            let next = next.max(line + 1).min(self.starts.len());
            let end = self.start_of(next)?;
            chunks.push(start..end);
            start = end;
            line = next;
        }

        Ok(Searcher { file, chunks })
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// A read-only handle that scans the log away from the update thread.
pub struct Searcher {
    /// A file descriptor of its own.
    file: File,
    /// Line-aligned byte ranges covering the log as it was.
    chunks: Vec<Range<u64>>,
}

/// What one scan of the whole log found.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scan {
    /// How many times the needle occurs in the whole log.
    pub hits: u64,
    /// The first occurrence at or after the offset asked for.
    pub next: Option<u64>,
    /// The last occurrence before the offset asked for.
    pub previous: Option<u64>,
    /// The first occurrence anywhere, for wrapping forwards.
    pub first: Option<u64>,
    /// The last occurrence anywhere, for wrapping backwards.
    pub last: Option<u64>,
    /// How long the scan took, in microseconds.
    pub micros: u128,
}

impl Searcher {
    /// Scans the whole log for `needle`, once.
    ///
    /// One pass answers every question the find bar asks — the total, the next
    /// hit, the previous hit and both wrap targets — so a search never reads
    /// the log twice.
    pub fn scan(&self, needle: &str, from: u64) -> Result<Scan, StoreError> {
        let started = Instant::now();
        let mut scan = Scan::default();

        if needle.is_empty() {
            return Ok(scan);
        }

        let mut buffer = Vec::new();

        for chunk in &self.chunks {
            let len = (chunk.end - chunk.start) as usize;
            buffer.resize(len, 0);
            self.file
                .read_exact_at(&mut buffer, chunk.start)
                .map_err(StoreError::Read)?;

            let text = std::str::from_utf8(&buffer).map_err(|_| StoreError::NotUtf8 {
                offset: chunk.start,
            })?;

            for (index, _) in text.match_indices(needle) {
                let offset = chunk.start + index as u64;
                scan.hits += 1;
                scan.first.get_or_insert(offset);
                scan.last = Some(offset);

                if offset >= from {
                    scan.next.get_or_insert(offset);
                } else {
                    scan.previous = Some(offset);
                }
            }
        }

        scan.micros = started.elapsed().as_micros();
        Ok(scan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a store holding `count` numbered lines.
    fn filled(count: usize) -> Store {
        let mut store = Store::temporary().expect("a temporary store");
        let lines: Vec<Arc<str>> = (0..count)
            .map(|i| Arc::from(format!("line {i:06} of the log")))
            .collect();
        store.append(&lines).expect("the append to land");
        store
    }

    #[test]
    fn every_line_comes_back() {
        let store = filled(1_000);
        assert_eq!(store.len(), 1_000);
        assert_eq!(store.text(0..1).expect("line 0"), "line 000000 of the log");
        assert_eq!(
            store.text(999..1_000).expect("the last line"),
            "line 000999 of the log"
        );
        assert_eq!(
            store.text(10..13).expect("three lines"),
            "line 000010 of the log\nline 000011 of the log\nline 000012 of the log"
        );
    }

    #[test]
    fn a_span_crosses_any_number_of_lines() {
        let store = filled(5_000);
        let span = store
            .span(
                Pos { line: 7, column: 5 },
                Pos {
                    line: 4_900,
                    column: 4,
                },
            )
            .expect("a long span");

        assert!(span.starts_with("000007"));
        assert!(span.ends_with("line"));
        assert_eq!(span.lines().count(), 4_894);
    }

    #[test]
    fn an_offset_maps_back_to_its_line() {
        let store = filled(10_000);
        for line in [0usize, 1, 4_999, 9_999] {
            let at = Pos { line, column: 3 };
            let offset = store.offset_of(at).expect("an offset");
            assert_eq!(store.position_of(offset), at);
        }
    }

    #[test]
    fn a_scan_finds_every_hit_in_one_pass() {
        let store = filled(20_000);
        let searcher = store.searcher().expect("a searcher");

        let unique = searcher.scan("line 012345", 0).expect("a scan");
        assert_eq!(unique.hits, 1);
        assert_eq!(store.position_of(unique.first.expect("a hit")).line, 12_345);

        let common = searcher.scan("of the log", 0).expect("a scan");
        assert_eq!(common.hits, 20_000);

        // `next` and `previous` are relative to where the caret sits.
        let midpoint = store
            .offset_of(Pos::line(10_000))
            .expect("the midpoint offset");
        let split = searcher.scan("of the log", midpoint).expect("a scan");
        assert_eq!(
            store.position_of(split.next.expect("a next hit")).line,
            10_000
        );
        assert_eq!(
            store
                .position_of(split.previous.expect("a previous hit"))
                .line,
            9_999
        );
    }
}
