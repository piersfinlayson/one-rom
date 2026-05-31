// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - CLI argument parsing and interactive prompting
//!
//! This module owns three concerns:
//!
//! 1. **Line reading** (`read_raw_line`): the single implementation of
//!    byte-by-byte CDC input with echo and backspace handling.
//!    `cli/mod.rs` delegates to this rather than duplicating it.
//!
//! 2. **Colon-syntax splitting** (`split_command`, `Args`): decompose a
//!    raw command line into a command character and an ordered argument
//!    iterator.
//!
//! 3. **Typed argument resolution** (`require_chip`, `get_addr`, …): for
//!    each argument position, either parse the inline token (colon syntax)
//!    or issue an interactive prompt with the current default.

use alloc::format;
use alloc::string::{String, ToString};

use embassy_time::Timer;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;

use super::OutputFormat;
use crate::error::Error;
use crate::usb;
use super::CsPolaritySetting;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum input line length in bytes.  Sufficient for any colon-syntax
/// command with all arguments.
pub const LINE_BUF: usize = 80;

// ---------------------------------------------------------------------------
// Colon-syntax splitting
// ---------------------------------------------------------------------------

/// Iterator over the colon-separated argument tokens that follow the command
/// character in a command line.
///
/// Empty slots (consecutive or trailing colons) yield `None` from
/// `next_token`, so the caller falls back to its interactive prompt for
/// that position.  This lets the user write `r:27512::0:cs` to skip the
/// start-address prompt and accept its default while still supplying the
/// remaining arguments inline.
pub struct Args<'a> {
    inner: core::str::Split<'a, char>,
}

impl<'a> Args<'a> {
    /// Return the next argument token, whitespace-trimmed, or `None` if the
    /// slot is absent or empty.
    pub fn next_token(&mut self) -> Option<&'a str> {
        self.inner.next().map(str::trim).filter(|s| !s.is_empty())
    }
}

/// Split a trimmed command line into its command character and an argument
/// iterator.  Returns `None` if `line` is empty.
///
/// ```text
/// "r:27512:0:0:cs"  →  ('r', ["27512", "0", "0", "cs"])
/// "r"               →  ('r', [])
/// "r:27512"         →  ('r', ["27512"])
/// ```
///
/// Commands are case-sensitive (`B` sets the board; `b` runs a batch read).
pub fn split_command(line: &str) -> Option<(char, Args<'_>)> {
    let cmd = line.chars().next()?;
    let mut split = line.split(':');
    split.next(); // consume the command-character token
    Some((cmd, Args { inner: split }))
}

// ---------------------------------------------------------------------------
// Address parsing
// ---------------------------------------------------------------------------

/// Parse an address string into a `usize`.
///
/// | Prefix        | Base        |
/// |---------------|-------------|
/// | `0x` or `0X`  | hexadecimal |
/// | `$`           | hexadecimal |
/// | (none)        | decimal     |
///
/// Hex digits are accepted in either case.
pub fn parse_addr(s: &str) -> Result<usize, Error> {
    let s = s.trim();

    let (hex, digits) = if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix('$') {
        (true, rest)
    } else {
        (false, s)
    };

    if digits.is_empty() {
        return Err(Error::Address);
    }

    if hex {
        usize::from_str_radix(digits, 16).map_err(|_| Error::Address)
    } else {
        digits.parse::<usize>().map_err(|_| Error::Address)
    }
}

// ---------------------------------------------------------------------------
// Core line reader
// ---------------------------------------------------------------------------

/// Read one raw line from the CDC interface, echoing each character to the
/// host as it arrives.
///
/// | Input           | Behaviour                                      |
/// |-----------------|------------------------------------------------|
/// | CR or LF        | End of line; returns `Ok(Some(trimmed_line))`  |
/// | Ctrl-C (0x03)   | Echoes `^C\r\n`; returns `Ok(None)`            |
/// | BS (0x08) / DEL | Destructive backspace with VT100 erasure       |
/// | Printable ASCII | Echoed and buffered; dropped silently if full  |
/// | USB disconnect  | Returns `Err(UsbDisconnected)`                 |
pub async fn read_raw_line() -> Result<Option<String>, Error> {
    let mut buf: [u8; 80] = [0u8; LINE_BUF];
    let mut pos = 0usize;

    loop {
        match usb::cdc_recv().await {
            Err(_) => return Err(Error::UsbDisconnected),

            Ok(b'\r') => {
                //super::send("\r").ok();
                let s = core::str::from_utf8(&buf[..pos])
                    .unwrap_or("")
                    .trim()
                    .to_string();
                return Ok(Some(s));
            }

            Ok(b'\n') => {
                // Echo but ignore
                //super::send("\n").ok();
            }

            // Destructive backspace: VT100 sequence move-back / space / move-back.
            Ok(0x08) | Ok(0x7F) => {
                if pos > 0 {
                    pos -= 1;
                    super::send("\x08 \x08").ok();
                }
            }

            // Ctrl-C: cancel the current line.
            Ok(0x03) => {
                super::send("^C\r\n").ok();
                return Ok(None);
            }

            // Printable ASCII.
            Ok(b) if b >= 0x20 => {
                if pos < LINE_BUF {
                    buf[pos] = b;
                    pos += 1;
                    // Echo without a heap allocation.
                    let ch = [b];
                    if let Ok(s) = core::str::from_utf8(&ch) {
                        super::send(s).ok();
                    }
                }
                // Buffer full: character silently dropped.
            }

            Ok(_) => {} // other control characters ignored
        }
    }
}

// ---------------------------------------------------------------------------
// Interactive prompt helper
// ---------------------------------------------------------------------------

/// Display `msg` and read one raw line.
async fn prompt_raw(msg: &str) -> Result<Option<String>, Error> {
    loop {
        match super::send(msg) {
            Ok(()) => break,
            Err(Error::UsbFull) => Timer::after_millis(10).await,
            Err(_) => return Err(Error::UsbDisconnected),
        }
    }
    read_raw_line().await
}

// ---------------------------------------------------------------------------
// Typed argument resolution
// ---------------------------------------------------------------------------

/// Resolve a chip type from an inline token or an interactive prompt.
///
/// - If `token` is `Some`, it is parsed directly and no prompt is shown.
/// - Blank input or Ctrl-C accepts `default` if one is set.
/// - Ctrl-C with no default returns `Err(Cancelled)`.
/// - An unrecognised name prints a message and re-prompts.
pub async fn require_chip(
    token: Option<&str>,
    default: Option<ChipType>,
) -> Result<ChipType, Error> {
    if let Some(t) = token {
        return ChipType::try_from_str(t).ok_or(Error::InvalidChip);
    }

    loop {
        let input = if let Some(d) = default {
            let prompt = format!("Chip type [{}]: ", d.name());
            prompt_raw(&prompt).await?
        } else {
            prompt_raw("Chip type: ").await?
        };

        let s = match input {
            // Ctrl-C
            None => match default {
                Some(d) => return Ok(d),
                None => return Err(Error::Cancelled),
            },
            // Blank
            Some(s) if s.is_empty() => match default {
                Some(d) => return Ok(d),
                None => {
                    super::send_line("Chip type is required. Use 'l' to list supported chips.")
                        .await?;
                    continue;
                }
            },
            Some(s) => s,
        };

        match ChipType::try_from_str(&s) {
            Some(c) => return Ok(c),
            None => {
                super::send_line(&format!(
                    "Unknown chip '{}'. Use 'l' to list supported chips.",
                    s
                ))
                .await?;
            }
        }
    }
}

/// Resolve a board from an inline token or an interactive prompt.
///
/// Same defaulting rules as `require_chip`.
pub async fn require_board(token: Option<&str>, default: Option<Board>) -> Result<Board, Error> {
    if let Some(t) = token {
        return Board::try_from_str(t).ok_or(Error::InvalidBoard);
    }

    loop {
        let input = if let Some(d) = default {
            let prompt = format!("Board [{}]: ", d.name());
            prompt_raw(&prompt).await?
        } else {
            prompt_raw("Board: ").await?
        };

        let s = match input {
            None => match default {
                Some(d) => return Ok(d),
                None => return Err(Error::Cancelled),
            },
            Some(s) if s.is_empty() => match default {
                Some(d) => return Ok(d),
                None => {
                    super::send_line("Board name is required.").await?;
                    continue;
                }
            },
            Some(s) => s,
        };

        match Board::try_from_str(&s) {
            Some(b) => return Ok(b),
            None => {
                super::send_line(&format!("Unknown board '{}'.", s)).await?;
            }
        }
    }
}

/// Resolve an address from an inline token or an interactive prompt.
///
/// `label` appears in the prompt, e.g. `"Start address"`.
/// Ctrl-C or blank input returns `default`.
pub async fn get_addr(token: Option<&str>, default: usize, label: &str) -> Result<usize, Error> {
    if let Some(t) = token {
        return parse_addr(t);
    }

    loop {
        let prompt = format!("{} [{:#010x}]: ", label, default);
        let input = prompt_raw(&prompt).await?;

        let s = match input {
            None => return Ok(default),
            Some(s) if s.is_empty() => return Ok(default),
            Some(s) => s,
        };

        match parse_addr(&s) {
            Ok(a) => return Ok(a),
            Err(_) => {
                super::send_line(&format!(
                    "Invalid address '{}'. Decimal, or prefix 0x/0X/$ for hex.",
                    s
                ))
                .await?;
            }
        }
    }
}

/// Resolve an output format from an inline token or an interactive prompt.
/// Ctrl-C or blank input returns `default`.
pub async fn get_format(token: Option<&str>, default: OutputFormat) -> Result<OutputFormat, Error> {
    if let Some(t) = token {
        return OutputFormat::from_str(t).ok_or(Error::InvalidFormat);
    }

    loop {
        let prompt = format!("Format (cs/hex/ihex) [{}]: ", default.as_str());
        let input = prompt_raw(&prompt).await?;

        let s = match input {
            None => return Ok(default),
            Some(s) if s.is_empty() => return Ok(default),
            Some(s) => s,
        };

        match OutputFormat::from_str(&s) {
            Some(f) => return Ok(f),
            None => {
                super::send_line("Unknown format. Choose: cs, hex, ihex.").await?;
            }
        }
    }
}

/// Resolve a batch interval (whole seconds, minimum 1) from an inline token
/// or an interactive prompt.  Ctrl-C or blank input returns `default`.
pub async fn get_interval(token: Option<&str>, default: u32) -> Result<u32, Error> {
    if let Some(t) = token {
        let n = t.trim().parse::<u32>().map_err(|_| Error::Syntax)?;
        if n == 0 {
            return Err(Error::Syntax);
        }
        return Ok(n);
    }

    loop {
        let prompt = format!("Interval (seconds) [{}]: ", default);
        let input = prompt_raw(&prompt).await?;

        let s = match input {
            None => return Ok(default),
            Some(s) if s.is_empty() => return Ok(default),
            Some(s) => s,
        };

        match s.parse::<u32>() {
            Ok(n) if n > 0 => return Ok(n),
            Ok(_) => {
                super::send_line("Interval must be at least 1 second.").await?;
            }
            Err(_) => {
                super::send_line(&format!("Invalid number '{}'.", s)).await?;
            }
        }
    }
}

/// Parse a single CS polarity token.
/// "0" → active-low, "1" → active-high, "?" → auto-detect.
pub fn parse_cs_polarity(s: &str) -> Result<CsPolaritySetting, Error> {
    match s.trim() {
        "0" => Ok(CsPolaritySetting::Low),
        "1" => Ok(CsPolaritySetting::High),
        "?" => Ok(CsPolaritySetting::Auto),
        _   => Err(Error::InvalidCsPolarity),
    }
}

/// Resolve a CS polarity from an inline token or interactive prompt.
///
/// `needed` is true when the chip actually has this line as configurable —
/// if false the token is still consumed from the arg stream but no prompt
/// is issued and the session default is returned unchanged.
pub async fn get_cs_polarity(
    token: Option<&str>,
    default: CsPolaritySetting,
    label: &str,
    needed: bool,
) -> Result<CsPolaritySetting, Error> {
    if let Some(t) = token {
        return if needed { parse_cs_polarity(t) } else { Ok(default) };
    }

    if !needed {
        return Ok(default);
    }

    loop {
        let prompt = match default {
            CsPolaritySetting::Unset => format!("{}: ", label),
            CsPolaritySetting::Auto  => format!("{} [?]: ", label),
            CsPolaritySetting::Low   => format!("{} [0]: ", label),
            CsPolaritySetting::High  => format!("{} [1]: ", label),
        };
        let input = prompt_raw(&prompt).await?;

        let s = match input {
            None => match default {
                CsPolaritySetting::Unset => return Err(Error::Cancelled),
                d => return Ok(d),
            },
            Some(s) if s.is_empty() => match default {
                CsPolaritySetting::Unset => {
                    super::send_line("Required: 0=active-low  1=active-high  ?=auto-detect").await?;
                    continue;
                }
                d => return Ok(d),
            },
            Some(s) => s,
        };

        match parse_cs_polarity(&s) {
            Ok(v)  => return Ok(v),
            Err(_) => {
                super::send_line("Enter 0 (active-low), 1 (active-high), or ? (auto-detect).").await?;
            }
        }
    }
}