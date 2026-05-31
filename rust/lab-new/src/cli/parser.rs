// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - CLI argument parsing and interactive prompting
//!
//! This module owns two concerns:
//!
//! 1. **Colon-syntax splitting** (`split_command`, `Args`): decompose a
//!    raw command line into a command character and an ordered argument
//!    iterator.
//!
//! 2. **Typed argument resolution** (`require_chip`, `get_addr`, …): for
//!    each argument position, either parse the inline token (colon syntax)
//!    or issue an interactive prompt via the shared [`LineEditor`].

use alloc::format;
use alloc::string::String;

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;
use onerom_config::mcu::Family;

use super::CsPolaritySetting;
use super::OutputFormat;
use super::editor::LineEditor;
use crate::error::Error;

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
// Interactive prompt helper
// ---------------------------------------------------------------------------

/// Send `msg` as a prompt and read one line via the shared editor.
///
/// On a successful line read, emits `\r\n` to advance the terminal to the
/// next line before any follow-up output.  On Ctrl-C the editor has already
/// emitted `^C\r\n`, so no extra newline is needed.
async fn prompt(editor: &mut LineEditor, msg: &str) -> Result<Option<String>, Error> {
    let result = editor.read_line(msg, false).await?;
    if result.is_some() {
        super::send_line("").await?;
    }
    Ok(result)
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
    editor: &mut LineEditor,
) -> Result<ChipType, Error> {
    if let Some(t) = token {
        return ChipType::try_from_str(t).ok_or(Error::InvalidChip);
    }

    loop {
        let input = if let Some(d) = default {
            prompt(editor, &format!("Chip type [{}]: ", d.name())).await?
        } else {
            prompt(editor, "Chip type: ").await?
        };

        let s = match input {
            None => return Err(Error::Cancelled),
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
pub async fn require_board(
    token: Option<&str>,
    default: Option<Board>,
    editor: &mut LineEditor,
) -> Result<Board, Error> {
    if let Some(t) = token {
        let b = Board::try_from_str(t).ok_or(Error::InvalidBoard)?;
        if !matches!(b.mcu_family(), Family::Rp2350) {
            return Err(Error::NonFireBoard);
        }
        return Ok(b);
    }

    loop {
        let input = if let Some(d) = default {
            prompt(editor, &format!("Board [{}]: ", d.name())).await?
        } else {
            prompt(editor, "Board: ").await?
        };

        let s = match input {
            None => return Err(Error::Cancelled),
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
            Some(b) => {
                if !matches!(b.mcu_family(), Family::Rp2350) {
                    super::send_line("Only Fire boards are supported.").await?;
                    continue;
                }
                return Ok(b);
            }
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
pub async fn get_addr(
    token: Option<&str>,
    default: usize,
    label: &str,
    editor: &mut LineEditor,
) -> Result<usize, Error> {
    if let Some(t) = token {
        return parse_addr(t);
    }

    loop {
        let input = prompt(editor, &format!("{} [{:#x}]: ", label, default)).await?;

        let s = match input {
            None => return Err(Error::Cancelled),
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
pub async fn get_format(
    token: Option<&str>,
    default: OutputFormat,
    editor: &mut LineEditor,
) -> Result<OutputFormat, Error> {
    if let Some(t) = token {
        return OutputFormat::from_str(t).ok_or(Error::InvalidFormat);
    }

    loop {
        let input =
            prompt(editor, &format!("Format (cs/hex/ihex) [{}]: ", default.as_str())).await?;

        let s = match input {
            None => return Err(Error::Cancelled),
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
pub async fn get_interval(
    token: Option<&str>,
    default: u32,
    editor: &mut LineEditor,
) -> Result<u32, Error> {
    if let Some(t) = token {
        let n = t.trim().parse::<u32>().map_err(|_| Error::Syntax)?;
        if n == 0 {
            return Err(Error::Syntax);
        }
        return Ok(n);
    }

    loop {
        let input = prompt(editor, &format!("Interval (seconds) [{}]: ", default)).await?;

        let s = match input {
            None => return Err(Error::Cancelled),
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
        _ => Err(Error::InvalidCsPolarity),
    }
}

/// Resolve a CS polarity from an inline token or interactive prompt.
///
/// `needed` is true when the chip actually has this line as configurable —
/// if false the token is still consumed from the arg stream but no prompt
/// is issued and the session default is returned unchanged.
///
/// When `default` is `Unset` the prompt shows `[?]` and blank input returns
/// `Auto`, giving the user a sensible starting point without requiring them
/// to have previously configured a polarity.
pub async fn get_cs_polarity(
    token: Option<&str>,
    default: CsPolaritySetting,
    label: &str,
    needed: bool,
    editor: &mut LineEditor,
) -> Result<CsPolaritySetting, Error> {
    if let Some(t) = token {
        return if needed {
            parse_cs_polarity(t)
        } else {
            Ok(default)
        };
    }

    if !needed {
        return Ok(default);
    }

    loop {
        let prompt_str = match default {
            CsPolaritySetting::Unset => format!("{} [?]: ", label),
            CsPolaritySetting::Auto => format!("{} [?]: ", label),
            CsPolaritySetting::Low => format!("{} [0]: ", label),
            CsPolaritySetting::High => format!("{} [1]: ", label),
        };
        let input = prompt(editor, &prompt_str).await?;

        let s = match input {
            None => return Err(Error::Cancelled),
            Some(s) if s.is_empty() => {
                // Blank input: return the default, treating Unset as Auto.
                return Ok(match default {
                    CsPolaritySetting::Unset => CsPolaritySetting::Auto,
                    d => d,
                });
            }
            Some(s) => s,
        };

        match parse_cs_polarity(&s) {
            Ok(v) => return Ok(v),
            Err(_) => {
                super::send_line("Enter 0 (active-low), 1 (active-high), or ? (auto-detect).")
                    .await?;
            }
        }
    }
}