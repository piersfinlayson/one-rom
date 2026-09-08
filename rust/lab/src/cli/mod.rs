// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - CDC CLI session management

pub mod commands;
pub mod editor;
pub mod parser;

use core::fmt::Display;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use alloc::format;
use alloc::string::{String, ToString};

use embassy_futures::select::{Either, select};
use embassy_time::Timer;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;

use crate::CsPolarities;
use crate::error::Error;
use crate::usb;

use editor::LineEditor;

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
    /// Motorola S-records (16-byte data records; one record type throughout,
    /// the narrowest that addresses the whole dump)
    Srec,
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Checksum => write!(f, "checksum"),
            Self::HexDump => write!(f, "hexdump"),
            Self::IntelHex => write!(f, "intelhex"),
            Self::Srec => write!(f, "srec"),
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
    /// - `srec` or `s19`
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cs" | "checksum" => Some(Self::Checksum),
            "hex" | "hexdump" => Some(Self::HexDump),
            "ihx" | "ihex" | "intelhex" => Some(Self::IntelHex),
            "srec" | "s19" | "srecord" => Some(Self::Srec),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Checksum => "cs",
            Self::HexDump => "hex",
            Self::IntelHex => "ihex",
            Self::Srec => "srec",
        }
    }

    /// Every format, in the order the help and the prompts list them.
    ///
    /// The help, the format prompt and its error message are all built from
    /// this, so a new variant reaches all three or fails to compile.  Each of
    /// them was a separate hand-written list, and `srec` reached none of them.
    pub const ALL: [Self; 4] = [Self::Checksum, Self::HexDump, Self::IntelHex, Self::Srec];

    /// What the format produces, for the help.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Checksum => "checksum+SHA1 (default)",
            Self::HexDump => "hex dump",
            Self::IntelHex => "Intel HEX",
            Self::Srec => "Motorola S-record",
        }
    }

    /// The accepted tokens, separated by `sep`, for a prompt or an error.
    pub fn token_list(sep: &str) -> String {
        let mut out = String::new();
        for (i, f) in Self::ALL.iter().enumerate() {
            if i > 0 {
                out.push_str(sep);
            }
            out.push_str(f.as_str());
        }
        out
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
    /// Line editor state, including history.
    pub editor: LineEditor,
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
    /// Tri-state testing flag for checksum mode.
    pub tri_state: bool,
}

impl SessionState {
    pub fn new(board: Option<Board>) -> Self {
        let chip = board.and_then(default_chip_for_board);
        Self {
            editor: LineEditor::new(),
            board,
            chip,
            range: ReadRange::default(),
            format: OutputFormat::default(),
            interval_secs: 5,
            cs: CsSettings::unset(),
            tri_state: true,
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

    #[allow(clippy::wildcard_enum_match_arm)]
    pub fn to_polarities(self) -> CsPolarities {
        CsPolarities {
            cs1: match self.cs1 {
                CsPolaritySetting::Low => Some(false),
                CsPolaritySetting::High => Some(true),
                _ => None,
            },
            cs2: match self.cs2 {
                CsPolaritySetting::Low => Some(false),
                CsPolaritySetting::High => Some(true),
                _ => None,
            },
            cs3: match self.cs3 {
                CsPolaritySetting::Low => Some(false),
                CsPolaritySetting::High => Some(true),
                _ => None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Top-level CLI driver.  Call from `main` with a `SessionState` that lives
/// for the full application lifetime.  Never returns.
pub async fn run(state: &mut SessionState) -> ! {
    loop {
        // Nothing can be written until the host has enumerated the device,
        // whatever the terminal on the other end does about DTR.
        usb::cdc_wait_connection().await;
        debug!("CDC host connected");

        // A terminal opening the port raises DTR, and that is what normally
        // starts a session.  Enter starts one too, for a terminal configured to
        // leave DTR alone, which would otherwise never see anything at all.
        match select(usb::cdc_wait_dtr(), wait_for_enter()).await {
            Either::First(()) => debug!("Terminal opened the port"),
            Either::Second(true) => debug!("Woken by Enter"),
            Either::Second(false) => continue, // disconnected before either
        }

        // Whatever arrived while the terminal was opening is not input.
        usb::cdc_drain_rx();

        // A host raises DTR partway through opening the port and discards
        // whatever arrives before it has finished, so a greeting sent the
        // instant DTR rises is thrown away by the terminal rather than by us -
        // pyserial, and so miniterm, does exactly this.  It is a heuristic: a
        // host slower than this still misses the greeting.
        Timer::after_millis(OPEN_SETTLE_MS).await;

        if send_banner(state).await.is_err() {
            continue;
        }

        session_loop(state).await;
        debug!("CDC session ended");
    }
}

/// How long to let a host finish opening the port before greeting it.
///
/// The same 250ms the USB system plugin waits before draining the log to a
/// terminal, for the same reason and honed there - see LOG_DRAIN_SETTLE_MS in
/// plugins/system/usb/src/usb_log.c.  Lab shares no build with that plugin, so
/// the two cannot share a constant, but they should not disagree either.
const OPEN_SETTLE_MS: u64 = 250;

/// Greet a terminal that has just opened the port.
///
/// The same shape as the USB system plugin's log banner - a titled rule, what
/// the device is, what it is called, then a plain rule of the same width, with
/// anything that is not identity following it.  One product, so one banner.
///
/// A token with no value is left out rather than filled with a placeholder the
/// reader would have to know to discount, which is why the board appears only
/// once one is set.
async fn send_banner(state: &SessionState) -> Result<(), Error> {
    const TITLE: &str = "----- One ROM Lab -----";
    const RULE: &str = "-----------------------";
    const _: () = assert!(TITLE.len() == RULE.len());

    send_line(TITLE).await?;
    match state.board {
        Some(board) => {
            send_line(&format!(
                "One ROM Lab {} v{}",
                board.name(),
                crate::PKG_VERSION
            ))
            .await?;
        }
        None => send_line(&format!("One ROM Lab v{}", crate::PKG_VERSION)).await?,
    }
    send_line(&format!("Serial: {}", crate::serial_id())).await?;
    send_line(RULE).await?;
    send_line("Type ? for help.").await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Session loop
// ---------------------------------------------------------------------------

/// Run one interactive session until the USB host disconnects.
async fn session_loop(state: &mut SessionState) {
    let mut last_blank = false;
    loop {
        let mut was_blank = false;

        // The editor sends the prompt and returns the completed line.
        match state.editor.read_line("> ", true).await {
            Err(_) => return, // USB disconnected

            Ok(None) => {
                // Ctrl-C at the bare prompt: the editor already echoed ^C\r\n;
                // just loop back to re-display the prompt.
            }

            Ok(Some(line)) if line.is_empty() => {
                was_blank = true;
                if send_line("").await.is_err() {
                    return;
                }
                if last_blank
                    && send_line("No command entered - use ?/h for help")
                        .await
                        .is_err()
                {
                    return;
                }
            }

            Ok(Some(line)) => {
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
            Ok(b'\r' | b'\n') => return true,
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
}

// ---------------------------------------------------------------------------
// Help / firmware info
// ---------------------------------------------------------------------------

pub async fn show_help(_state: &SessionState) -> Result<(), Error> {
    send_line("").await?;
    send_line("Commands:  single-letter command token, optionally followed by :args").await?;
    send_line("           Type command alone for interactive prompts.").await?;
    send_line("           Use colon-separated args to skip prompts.").await?;
    send_line("").await?;
    send_line("  B   Set One ROM Lab board type").await?;
    send_line("        B:<board>").await?;
    send_line("  r   Read ROM").await?;
    send_line("        r[:<chip>[:<start>[:<len>[:<fmt>[:<cs1>[:<cs2>[:<cs3>]]]]]]]]").await?;
    send_line("  b   Batch ROM read").await?;
    send_line("        b[:<chip>[:<start>[:<len>[:<fmt>[:<secs>[:<cs1>[:<cs2>[:<cs3>]]]]]]]]]")
        .await?;
    send_line("  i   Chip type information").await?;
    send_line("        i[:<chip>]").await?;
    send_line("  c   Set or change chip type").await?;
    send_line("        c:<chip>[:<cs1>[:<cs2>[:<cs3>]]]").await?;
    send_line("  f   Set or change output format").await?;
    send_line("  t   Toggle tri-state testing during checksum mode on and off").await?;
    send_line("  q   Quick read (uses default chip, range and format)").await?;
    send_line("  l   List chips supported by this board type").await?;
    send_line("  v   Display One ROM Lab version and hardware information").await?;
    send_line("  s   Display settings").await?;
    send_line("  p   Show board pin map (socket pin -> GPIO)").await?;
    send_line("        p[:<chip>]").await?;
    send_line("  T   List supported board types").await?;
    send_line("  z   Reset to bootloader").await?;
    send_line("  ?/h This help").await?;
    send_line("").await?;
    for (i, f) in OutputFormat::ALL.iter().enumerate() {
        let label = if i == 0 { "Formats:  " } else { "          " };
        send_line(&format!("{label} {:<4} - {}", f.as_str(), f.describe())).await?;
    }
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

fn default_chip_for_board(board: Board) -> Option<ChipType> {
    match board.chip_pins() {
        24 => Some(ChipType::Chip2732),
        28 => Some(ChipType::Chip27512),
        32 => Some(ChipType::Chip27C040),
        40 => Some(ChipType::Chip27C400),
        _ => None,
    }
}
