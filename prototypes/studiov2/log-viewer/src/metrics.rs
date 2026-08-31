//! Measuring the thing the prototype exists to measure.
//!
//! Two numbers: how much memory the process holds, and how long frames are
//! taking.  Neither function knows about the UI.

use std::time::Instant;

use crate::error::RssError;

/// Reads the process's resident set size, in kilobytes.
///
/// `ps` rather than a `sysinfo` dependency: one fewer crate in a prototype whose
/// whole point is to judge iced, and the figure is the same one this report
/// quotes from the shell.
pub async fn read_rss() -> Result<u64, RssError> {
    let pid = std::process::id();
    let output = tokio::process::Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .await?;

    let text = String::from_utf8_lossy(&output.stdout);
    text.trim()
        .parse()
        .map_err(|_| RssError::Parse(text.trim().to_owned()))
}

/// How many frame intervals to keep for the rolling figures.
const WINDOW: usize = 240;

/// A rolling window of frame intervals.
#[derive(Debug, Default)]
pub struct Frames {
    /// When the previous frame arrived.
    last: Option<Instant>,
    /// Recent intervals in milliseconds, oldest first.
    intervals: Vec<f32>,
    /// The worst interval seen since the last reset, in milliseconds.
    worst: f32,
}

impl Frames {
    /// Records a frame at `now`.
    pub fn tick(&mut self, now: Instant) {
        if let Some(last) = self.last {
            let ms = now.duration_since(last).as_secs_f32() * 1000.0;
            if self.intervals.len() == WINDOW {
                self.intervals.remove(0);
            }
            self.intervals.push(ms);
            self.worst = self.worst.max(ms);
        }
        self.last = Some(now);
    }

    /// Forgets the recorded intervals.
    pub fn reset(&mut self) {
        self.intervals.clear();
        self.worst = 0.0;
        self.last = None;
    }

    /// The mean frame rate over the window, if there is one.
    pub fn fps(&self) -> Option<f32> {
        if self.intervals.is_empty() {
            return None;
        }
        let mean: f32 = self.intervals.iter().sum::<f32>() / self.intervals.len() as f32;
        (mean > 0.0).then(|| 1000.0 / mean)
    }

    /// The 95th-percentile frame interval over the window, in milliseconds.
    ///
    /// The mean hides a stall.  This is the number that says whether the pane
    /// felt smooth.
    pub fn p95_ms(&self) -> Option<f32> {
        if self.intervals.is_empty() {
            return None;
        }
        let mut sorted = self.intervals.clone();
        sorted.sort_by(f32::total_cmp);
        let index = ((sorted.len() as f32 * 0.95) as usize).min(sorted.len() - 1);
        sorted.get(index).copied()
    }

    /// The worst frame interval since the last reset, in milliseconds.
    pub fn worst_ms(&self) -> f32 {
        self.worst
    }
}
