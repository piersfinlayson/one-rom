// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - CDC CLI session management

pub mod commands;
pub mod parser;

use core::fmt::Display;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use alloc::format;
use alloc::string::{String, ToString};

use embassy_time::Timer;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;

use crate::CsPolarities;
use crate::error::Error;
use crate::usb;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Output format for a ROM read operation.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum OutputFormat {
    /// 32-bit wrapping checksum + SHA-1 digest (default)
    #[default]
    Checksum,
    /// Hex dump: address | 16 bytes hex | ASCII sidebar
    HexDump,
    /// Intel HEX records (16-byte data records; extended linear address
    /// records emitted as needed for ROMs larger than 64 KB)
    IntelHex,
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Checksum => write!(f, "checksum"),
            Self::HexDump => write!(f, "hexdump"),
            Self::IntelHex => write!(f, "intelhex"),
        }
    }
}

impl OutputFormat {
    /// Parse the short token used in colon-syntax commands.
    ///
    /// Accepted tokens (case-insensitive):
    /// - `cs` or `checksum`
    /// - `hex` or `hexdump`
    /// - `ihex` or `intelhex`
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cs" | "checksum" => Some(Self::Checksum),
            "hex" | "hexdump" => Some(Self::HexDump),
            "ihex" | "intelhex" => Some(Self::IntelHex),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Checksum => "cs",
            Self::HexDump => "hex",
            Self::IntelHex => "ihex",
        }
    }
}

/// Address range for a read operation.
/// `len == 0` is the sentinel meaning "to the end of the ROM".
#[derive(Debug, Copy, Clone, Default)]
pub struct ReadRange {
    pub start: usize,
    /// 0 means "full ROM from `start`".
    pub len: usize,
}

impl Display for ReadRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.len == 0 {
            write!(f, "{:#x}..end", self.start)
        } else {
            write!(f, "{:#x}..{:#x}", self.start, self.start + self.len)
        }
    }
}

/// State that survives USB connect/disconnect cycles (but not power loss).
///
/// Constructed once in `main` and passed by mutable reference into `run`.
pub struct SessionState {
    /// Board variant.  Populated from the `BOARD` build-time env var if set,
    /// or via the `B` command at runtime.
    pub board: Option<Board>,
    /// Most recently used chip type; becomes the default for subsequent
    /// commands that require one.
    pub chip: Option<ChipType>,
    /// Most recently used address range; defaults to full ROM.
    pub range: ReadRange,
    /// Most recently used output format; defaults to `Checksum`.
    pub format: OutputFormat,
    /// Batch read interval in seconds; defaults to 5.
    pub interval_secs: u32,
    /// Control line polarities for the current chip.
    pub cs: CsSettings,
}

impl SessionState {
    pub fn new(board: Option<Board>) -> Self {
        Self {
            board,
            chip: None,
            range: ReadRange::default(),
            format: OutputFormat::default(),
            interval_secs: 5,
            cs: CsSettings::unset(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum CsPolaritySetting {
    #[default]
    Unset,
    Auto,
    Low,
    High,
}

impl core::fmt::Display for CsPolaritySetting {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unset => write!(f, "_"),
            Self::Auto => write!(f, "?"),
            Self::Low => write!(f, "0"),
            Self::High => write!(f, "1"),
        }
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct CsSettings {
    pub cs1: CsPolaritySetting,
    pub cs2: CsPolaritySetting,
    pub cs3: CsPolaritySetting,
}

impl CsSettings {
    pub fn unset() -> Self {
        Self {
            cs1: CsPolaritySetting::Unset,
            cs2: CsPolaritySetting::Unset,
            cs3: CsPolaritySetting::Unset,
        }
    }

    pub fn to_polarities(&self) -> CsPolarities {
        CsPolarities {
            cs1: match self.cs1 {
                CsPolaritySetting::Low  => Some(false),
                CsPolaritySetting::High => Some(true),
                _                       => None,
            },
            cs2: match self.cs2 {
                CsPolaritySetting::Low  => Some(false),
                CsPolaritySetting::High => Some(true),
                _                       => None,
            },
            cs3: match self.cs3 {
                CsPolaritySetting::Low  => Some(false),
                CsPolaritySetting::High => Some(true),
                _                       => None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Internal line-read result
// ---------------------------------------------------------------------------

/// Outcome of reading one command line from the CDC interface.
enum LineRead {
    /// A complete line was entered (may be empty, which triggers help).
    Line(String),
    /// Ctrl-C was pressed; the line was cancelled.
    Cancelled,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Top-level CLI driver.  Call from `main` with a `SessionState` that lives
/// for the full application lifetime.  Never returns.
pub async fn run(state: &mut SessionState) -> ! {
    loop {
        // Wait silently for a USB host connection — no banner.
        usb::cdc_wait_connection().await;
        debug!("CDC host connected");

        // Enter-to-wake: discard everything until the first CR or LF.
        // The user opens their terminal and presses Enter to get a prompt
        // without seeing unsolicited output on connect.
        if !wait_for_enter().await {
            continue; // disconnected before the wake keystroke arrived
        }

        session_loop(state).await;
        debug!("CDC session ended");
    }
}

// ---------------------------------------------------------------------------
// Session loop
// ---------------------------------------------------------------------------

/// Run one interactive session until the USB host disconnects.
async fn session_loop(state: &mut SessionState) {
    let mut last_blank = false;
    loop {
        loop {
            match send("> ") {
                Ok(()) => break,
                Err(Error::UsbFull) => Timer::after_millis(10).await,
                Err(_) => return, // genuinely disconnected
            }
        }
        let mut was_blank = false;
        match read_line().await {
            Err(_) => return,

            Ok(LineRead::Cancelled) => {
                // Ctrl-C at the bare prompt: just re-display it.
            }

            Ok(LineRead::Line(line)) if line.is_empty() => {
                was_blank = true;
                if send_line("").await.is_err() {
                    return;
                }
                if last_blank {
                    if send_line(&format!("No command entered - use ?/h for help"))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }

            Ok(LineRead::Line(line)) => {
                match commands::dispatch(&line, state).await {
                    Ok(()) => {}
                    Err(Error::Cancelled) => {} // command cancelled; re-show prompt
                    Err(Error::UsbDisconnected) => return,
                    Err(e) => {
                        if send_line(&format!("Error: {}", e)).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }

        last_blank = was_blank;
    }
}

// ---------------------------------------------------------------------------
// Input helpers
// ---------------------------------------------------------------------------

/// Discard incoming bytes until a CR or LF arrives.
///
/// Returns `true` on the wake keypress, `false` if the host disconnects first.
async fn wait_for_enter() -> bool {
    loop {
        match usb::cdc_recv().await {
            Ok(b'\r') => return true,
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
}

/// Read one command line, delegating the byte-level work to `parser::read_raw_line`.
async fn read_line() -> Result<LineRead, Error> {
    match parser::read_raw_line().await? {
        None => Ok(LineRead::Cancelled),
        Some(s) => Ok(LineRead::Line(s)),
    }
}

// ---------------------------------------------------------------------------
// Help / firmware info
// ---------------------------------------------------------------------------

pub async fn show_help(state: &SessionState) -> Result<(), Error> {
    send_line("").await?;
    send_line("One ROM Lab").await?;
    send_line(&format!("  Version:  {}", crate::PKG_VERSION)).await?;
    send_line(&format!(
        "  Board:    {}",
        state.board.map(|b| b.name()).unwrap_or("(not set)")
    ))
    .await?;
    send_line(&format!(
        "  Chip:     {}",
        state.chip.map(|c| c.name()).unwrap_or("(not set)")
    ))
    .await?;
    send_line(&format!(
        "  Defaults: range {:#010x}+{}  fmt {}  interval {}s",
        state.range.start,
        if state.range.len == 0 {
            "full".to_string()
        } else {
            format!("{:#010x}", state.range.len)
        },
        state.format.as_str(),
        state.interval_secs,
    ))
    .await?;
    send_line(&format!(
        "  CS:       cs1={}  cs2={}  cs3={}",
        state.cs.cs1,
        state.cs.cs2,
        state.cs.cs3,
    )).await?;
    send_line("").await?;
    send_line("Commands:  type alone for interactive prompts,").await?;
    send_line("           or with colon-separated args to skip prompts:").await?;
    send_line("").await?;
    send_line("  B   Set One ROM Lab board type").await?;
    send_line("        B:<board>").await?;
    send_line("  r   Read ROM").await?;
    send_line("        r[:<chip>[:<start>[:<len>[:<fmt>[:<cs1>[:<cs2>[:<cs3>]]]]]]]]").await?;
    send_line("  b   Batch ROM read").await?;
    send_line("        b[:<chip>[:<start>[:<len>[:<fmt>[:<secs>[:<cs1>[:<cs2>[:<cs3>]]]]]]]]]").await?;
    send_line("  i   Chip type information").await?;
    send_line("        i[:<chip>]").await?;
    send_line("  l   List chips supported by this board type").await?;
    send_line("  v   Firmware info").await?;
    send_line("  z   Reset to bootloader").await?;
    send_line("  ?/h This help").await?;
    send_line("").await?;
    send_line("Formats:   cs   - checksum+SHA1 (default)").await?;
    send_line("           hex  - hex dump").await?;
    send_line("           ihex - Intel HEX").await?;
    send_line("").await?;
    send_line("Addresses: decimal by default.").await?;
    send_line("           Prefix with 0x, 0X, or $ for hexadecimal.").await?;
    send_line("           len=0 means to end of ROM.").await?;
    send_line("").await?;
    send_line("CS polarity: 0=active-low  1=active-high  ?=auto-detect").await?;
    send_line("").await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// CDC output helpers — pub so parser.rs, commands.rs and output modules
// can use them via super::
// ---------------------------------------------------------------------------

/// Send a string slice over CDC.
///
/// Maps `Disconnected` → `Err(UsbDisconnected)`.
/// `Full` is silently swallowed here (best-effort display).  Output modules
/// that need reliable delivery should yield between sends.
pub fn send(s: &str) -> Result<(), Error> {
    usb::cdc_send(s.to_string()).map_err(|e| e.into())
}

/// Send a string followed by `\r\n`.
pub async fn send_line(s: &str) -> Result<(), Error> {
    let mut retries = 20;
    loop {
        match send(&format!("{}\r\n", s)) {
            Ok(()) => {
                embassy_futures::yield_now().await;
                return Ok(());
            }
            Err(Error::UsbFull) => {
                if retries == 0 {
                    return Err(Error::UsbFull);
                }
                retries -= 1;
            }
            Err(e) => return Err(e),
        }
        Timer::after_millis(10).await;
    }
}
