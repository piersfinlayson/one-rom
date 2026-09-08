//! A synthetic source of device log lines.
//!
//! This module knows nothing about the UI.  It produces [`LogBatch`] values
//! and the UI layer decides what message to wrap them in.

use std::sync::Arc;

/// A batch of generated log lines, plus how long generating them took.
///
/// Lines are `Arc<str>` so that moving a batch from the generator task into
/// the update loop copies pointers rather than text.
#[derive(Debug, Clone)]
pub struct LogBatch {
    /// The generated lines, oldest first.  No trailing newlines.
    pub lines: Vec<Arc<str>>,
    /// Wall time spent generating, in microseconds.
    pub generate_us: u128,
}

/// Generates realistic-looking device log lines.
///
/// Deterministic given the same starting sequence number, so two runs of the
/// same benchmark produce byte-identical text and therefore identical shaping
/// work.
#[derive(Debug, Clone)]
pub struct Generator {
    /// The next sequence number to emit.
    seq: u64,
    /// A small xorshift state, so the module needs no `rand` dependency.
    rng: u64,
}

/// The severities the generator emits.
const LEVELS: [&str; 5] = ["TRACE", "DEBUG", "INFO ", "WARN ", "ERROR"];

/// The subsystems the generator attributes lines to.
const SUBSYSTEMS: [&str; 8] = [
    "usb", "pio", "dma", "flash", "serve", "plugin", "led", "rbcp",
];

/// Message bodies, chosen to give a realistic spread of line lengths.
const BODIES: [&str; 12] = [
    "slot 0 armed, cs1=active-low cs2=active-low cs3=active-high",
    "read 0x3f40 -> 0xa7",
    "bank switch requested: 2 -> 3",
    "DMA channel 4 wrap, refilled 512 bytes from 0x10004000",
    "PIO sm1 stalled 3 cycles waiting on CS deassert",
    "host GET_INFO, replying 148 bytes",
    "plugin `usb` heartbeat, stack high-water 412/1024 bytes",
    "chip 23128 selected, address mask 0x3fff",
    "flash XIP cache miss ratio 0.4% over last 10000 reads",
    "serving window programmed for 5 cycles after CS assertion",
    "status LED set to solid green by ora_set_status_led",
    "warning: CS-to-data latency 7 cycles, expected 5 — check cpu-freq",
];

impl Generator {
    /// Creates a generator starting from sequence number zero.
    pub fn new() -> Self {
        Self {
            seq: 0,
            rng: 0x2545_f491_4f6c_dd1d,
        }
    }

    /// Draws the next pseudo-random value.
    fn next_u64(&mut self) -> u64 {
        // xorshift64*, adequate for picking strings out of a table.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Produces a single log line.
    fn line(&mut self) -> Arc<str> {
        let r = self.next_u64();
        let level = LEVELS[(r % LEVELS.len() as u64) as usize];
        let subsystem = SUBSYSTEMS[((r >> 8) % SUBSYSTEMS.len() as u64) as usize];
        let body = BODIES[((r >> 16) % BODIES.len() as u64) as usize];

        let seq = self.seq;
        self.seq += 1;

        // A monotonic fake timestamp, so lines sort and look like a real log.
        let ms = seq * 7 + (r >> 32) % 5;
        let (h, m, s, milli) = (
            ms / 3_600_000 % 24,
            ms / 60_000 % 60,
            ms / 1_000 % 60,
            ms % 1_000,
        );

        Arc::from(format!(
            "{h:02}:{m:02}:{s:02}.{milli:03} [{seq:>7}] {level} {subsystem:>6}: {body}"
        ))
    }

    /// Produces `count` log lines, timing the work.
    pub fn batch(&mut self, count: usize) -> LogBatch {
        let started = std::time::Instant::now();
        let mut lines = Vec::with_capacity(count);
        for _ in 0..count {
            lines.push(self.line());
        }
        LogBatch {
            lines,
            generate_us: started.elapsed().as_micros(),
        }
    }
}

impl Default for Generator {
    fn default() -> Self {
        Self::new()
    }
}
