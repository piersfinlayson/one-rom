//! Command-line options.
//!
//! The prototype needs to be measurable without a hand on the mouse — this machine
//! will not grant permission to post synthetic input events — so every button
//! the benchmark would press has a switch here.

use std::path::PathBuf;

use crate::error::Error;
use crate::logview::WINDOW_DEFAULT;

/// Which of the two log designs is under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// The whole log in a file, a fixed-size window of it in the widget.
    #[default]
    Windowed,

    /// The whole log in the widget, with the oldest lines dropped to hold it
    /// to a cap.  Kept only so the two can be measured against each other.
    Capped,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Windowed => write!(f, "windowed, whole log kept"),
            Self::Capped => write!(f, "capped, oldest lines dropped"),
        }
    }
}

/// How the app should start.
#[derive(Debug, Clone)]
pub struct Options {
    /// Which log design to run.
    pub mode: Mode,
    /// How many lines the widget's window holds, in windowed mode.
    pub window: usize,
    /// Where the log store lives.  `None` is a temporary file, removed on
    /// exit.
    pub store_path: Option<PathBuf>,
    /// Lines to write into the store before anything else runs.
    pub fill: usize,
    /// Whether to run the scripted measurement afterwards.
    pub probe: bool,
    /// Where to write a PNG of the window, if anywhere.
    pub shot: Option<PathBuf>,
    /// Scroll to this line before the shot.
    pub shot_line: Option<usize>,
    /// Search for this before the shot, so the hit is highlighted in it.
    pub shot_find: Option<String>,
    /// Select the whole log before the shot.
    pub shot_select: bool,
    /// Line counts to inject in turn, measuring after each.
    pub bench: Vec<usize>,
    /// Whether to run the selection-and-clipboard self test.
    pub selftest: bool,
    /// Whether to drive the console pane through a scripted session.
    pub console_demo: bool,
    /// Streaming rate, in lines per second.
    pub rate: u32,
    /// Whether the streaming source starts running.
    pub streaming: bool,
    /// The retention cap.  Capped mode only.
    pub retention: Option<usize>,
    /// Whether to exit once everything asked for has finished.
    pub quit_when_done: bool,
    /// Split each benchmark injection into chunks of this many lines.  Zero
    /// means one paste for the whole injection.
    pub chunk: usize,
    /// How the retention cap drops the oldest lines.  Capped mode only.
    pub trim_mode: crate::logpane::TrimMode,
    /// How many lines over the cap to run before trimming.  `None` is a tenth
    /// of the cap.
    pub trim_slack: Option<usize>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            window: WINDOW_DEFAULT,
            store_path: None,
            fill: 0,
            probe: false,
            shot: None,
            shot_line: None,
            shot_find: None,
            shot_select: false,
            bench: Vec::new(),
            selftest: false,
            console_demo: false,
            rate: 200,
            streaming: true,
            retention: Some(10_000),
            quit_when_done: false,
            chunk: 0,
            trim_mode: crate::logpane::TrimMode::default(),
            trim_slack: Some(200),
        }
    }
}

/// What `--help` prints.
pub const USAGE: &str = "\
studiov2-log-viewer — an iced 0.14 log and console prototype

  --mode MODE        `windowed` (default) keeps the whole log in a file and
                     shows a window of it, or `capped` keeps the whole log in
                     the widget and drops the oldest lines
  --window N         lines the widget's window holds (default 120, and never
                     fewer than three screenfuls).  Every line in the window
                     is shaped on every window move, so this is the cost
  --store PATH       write the log here and leave it behind.  Without this the
                     store is a temporary file, removed on exit
  --fill N           write N lines into the store before anything else
  --probe            after --fill, run the scripted measurement: jumps, a
                     scroll, a search of the whole log and a select-all copy
  --shot PATH        write a PNG of the window and exit
  --shot-line N      scroll to line N before the shot
  --shot-find TEXT   search for TEXT before the shot, so the hit is in it
  --shot-select      select the whole log before the shot
  --bench N[,N...]   inject each line count in turn through the pane's own
                     append path, reporting timings and RSS
  --selftest         select text and copy it to the system clipboard, then
                     print what was selected
  --console-demo     run a scripted session in the console pane
  --rate N           streaming rate in lines per second (default 200)
  --no-stream        start with the streaming source paused
  --retention N      capped mode only: cap in lines, or `none` (default 10000)
  --chunk N          append each --bench injection in chunks of N lines,
                     the way a streaming source would, rather than one paste
  --trim MODE        capped mode only: `select-delete` (default) or `rebuild`
  --trim-slack N     capped mode only: lines to let the buffer run over the
                     cap before trimming (default 200)
  --quit-when-done   exit once everything asked for has finished
  --help             this text
";

impl Options {
    /// Parses the process arguments.
    pub fn parse<I>(args: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = String>,
    {
        let mut options = Self::default();
        let mut args = args.into_iter();

        while let Some(argument) = args.next() {
            let mut value = || {
                args.next().ok_or_else(|| Error::BadOption {
                    option: argument.clone(),
                    detail: "expected a value".to_owned(),
                })
            };

            match argument.as_str() {
                "--help" | "-h" => {
                    println!("{USAGE}");
                    std::process::exit(0);
                }
                "--mode" => {
                    let raw = value()?;
                    options.mode = match raw.as_str() {
                        "windowed" => Mode::Windowed,
                        "capped" => Mode::Capped,
                        other => {
                            return Err(Error::BadOption {
                                option: "--mode".to_owned(),
                                detail: format!(
                                    "expected `windowed` or `capped`, \
                                     got {other:?}"
                                ),
                            });
                        }
                    };
                }
                "--window" => {
                    options.window = parse_count(&mut value, "--window")?;
                }
                "--store" => {
                    options.store_path = Some(PathBuf::from(value()?));
                }
                "--fill" => {
                    options.fill = parse_count(&mut value, "--fill")?;
                }
                "--probe" => options.probe = true,
                "--shot" => options.shot = Some(PathBuf::from(value()?)),
                "--shot-line" => {
                    options.shot_line = Some(parse_count(&mut value, "--shot-line")?);
                }
                "--shot-find" => options.shot_find = Some(value()?),
                "--shot-select" => options.shot_select = true,
                "--bench" => {
                    let raw = value()?;
                    options.bench = raw
                        .split(',')
                        .map(|part| {
                            part.trim().parse::<usize>().map_err(|_| Error::BadOption {
                                option: "--bench".to_owned(),
                                detail: format!("expected line counts, got {part:?}"),
                            })
                        })
                        .collect::<Result<_, _>>()?;
                }
                "--selftest" => options.selftest = true,
                "--console-demo" => options.console_demo = true,
                "--rate" => {
                    let raw = value()?;
                    options.rate = raw.parse().map_err(|_| Error::BadOption {
                        option: "--rate".to_owned(),
                        detail: format!("expected a number, got {raw:?}"),
                    })?;
                }
                "--no-stream" => options.streaming = false,
                "--retention" => {
                    let raw = value()?;
                    options.retention = if raw == "none" {
                        None
                    } else {
                        Some(raw.parse().map_err(|_| Error::BadOption {
                            option: "--retention".to_owned(),
                            detail: format!("expected a number or `none`, got {raw:?}"),
                        })?)
                    };
                }
                "--chunk" => {
                    options.chunk = parse_count(&mut value, "--chunk")?;
                }
                "--trim" => {
                    let raw = value()?;
                    options.trim_mode = match raw.as_str() {
                        "select-delete" => crate::logpane::TrimMode::SelectDelete,
                        "rebuild" => crate::logpane::TrimMode::Rebuild,
                        other => {
                            return Err(Error::BadOption {
                                option: "--trim".to_owned(),
                                detail: format!(
                                    "expected `select-delete` or `rebuild`, \
                                     got {other:?}"
                                ),
                            });
                        }
                    };
                }
                "--trim-slack" => {
                    options.trim_slack = Some(parse_count(&mut value, "--trim-slack")?);
                }
                "--quit-when-done" => options.quit_when_done = true,
                other => {
                    return Err(Error::BadOption {
                        option: other.to_owned(),
                        detail: "unknown option".to_owned(),
                    });
                }
            }
        }

        Ok(options)
    }
}

/// Reads a count, allowing `1_000_000` and `1000000` to mean the same thing.
fn parse_count<F>(value: &mut F, option: &str) -> Result<usize, Error>
where
    F: FnMut() -> Result<String, Error>,
{
    let raw = value()?;
    raw.replace('_', "").parse().map_err(|_| Error::BadOption {
        option: option.to_owned(),
        detail: format!("expected a number, got {raw:?}"),
    })
}
