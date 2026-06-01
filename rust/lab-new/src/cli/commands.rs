// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - CLI command dispatch and implementations

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use alloc::{format, string::ToString, vec::Vec};

use embassy_futures::select::{Either, select};
use embassy_time::Timer;

use onerom_config::chip::{ChipType, ControlLineType, chip_type_names_for_pins};
use onerom_config::hw::{BOARDS, Board};
use onerom_config::pin_map::BoardPinMap;
use onerom_config::mcu::Family;

use sha1::{Digest, Sha1};

use crate::error::Error;
use crate::rom::RomReader;
use crate::usb;

use super::super::serial_id;
use super::parser::{self, Args};
use super::{CsPolaritySetting, CsSettings, OutputFormat, ReadRange, SessionState};
use super::{default_chip_for_board, send_line};

const CS_KEY: &str = "(0=active-low 1=active-high ?=auto)";

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatch a trimmed, non-empty command line to the appropriate handler.
///
/// The first character is the command; everything after the first colon
/// (if present) is the colon-separated argument list.
pub async fn dispatch(line: &str, state: &mut SessionState) -> Result<(), Error> {
    // Command token must be a single character.  Anything longer (e.g.
    // "hello" or "read:...") is treated as invalid syntax.
    let cmd_token = line.split(':').next().unwrap_or("").trim();
    if cmd_token.chars().count() != 1 {
        send_line("\r\nInvalid command syntax. Use a single-letter command, optionally followed by :args.  ? or h for help.").await?;
        return Ok(());
    }

    let (cmd, mut args) = match parser::split_command(line) {
        Some(x) => x,
        None => return Ok(()), // guard: session_loop never calls us with an empty line
    };

    send_line("").await?;

    match cmd {
        'r' => cmd_read(&mut args, state).await,
        'b' => cmd_batch(&mut args, state).await,
        'i' => cmd_chip_info(&mut args, state).await,
        'c' => cmd_set_chip_type(&mut args, state).await,
        'f' => cmd_set_format(&mut args, state).await,
        't' => cmd_toggle_tristate(state).await,
        'q' => cmd_quick_read(state).await,
        'l' => cmd_list_chips(state).await,
        'v' => cmd_version_info(state).await,
        's' => cmd_default_info(state).await,
        'B' => cmd_set_board(&mut args, state).await,
        'T' => cmd_list_board_types(state).await,
        'z' => cmd_reset_to_bootloader().await,
        '?' | 'h' => super::show_help(state).await,
        _ => {
            send_line(&format!(
                "Unknown command '{}'. Use single-letter commands (optionally :args). Use ? or h for help.",
                cmd
            ))
            .await
        }
    }
}

async fn cmd_read(args: &mut Args<'_>, state: &mut SessionState) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;

    let chip = parser::require_chip(args.next_token(), state.chip, &mut state.editor).await?;
    let start = parser::get_addr(args.next_token(), state.range.start, "Start address", &mut state.editor).await?;
    let len = parser::get_addr(args.next_token(), state.range.len, "Length (0=full)", &mut state.editor).await?;
    let fmt = parser::get_format(args.next_token(), state.format, &mut state.editor).await?;
    let cs1 = parser::get_cs_polarity(
        args.next_token(),
        state.cs.cs1,
        &format!("CS1 polarity {}", CS_KEY),
        chip_has_configurable_cs(chip, "cs1"),
        &mut state.editor,
    )
    .await?;
    let cs2 = parser::get_cs_polarity(
        args.next_token(),
        state.cs.cs2,
        &format!("CS2 polarity {}", CS_KEY),
        chip_has_configurable_cs(chip, "cs2"),
        &mut state.editor,
    )
    .await?;
    let cs3 = parser::get_cs_polarity(
        args.next_token(),
        state.cs.cs3,
        &format!("CS3 polarity {}", CS_KEY),
        chip_has_configurable_cs(chip, "cs3"),
        &mut state.editor,
    )
    .await?;

    // Update session defaults before executing so they persist even if the
    // read fails (e.g. board not set).
    state.chip = Some(chip);
    state.range = ReadRange { start, len };
    state.format = fmt;
    state.cs = CsSettings { cs1, cs2, cs3 };

    send_line(&format!(
        "Reading {chip}, range {}, output {fmt}",
        state.range
    ))
    .await?;
    do_read(board, chip, state.range, fmt, state.cs, state.tri_state).await
}

async fn cmd_batch(args: &mut Args<'_>, state: &mut SessionState) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;

    let chip = parser::require_chip(args.next_token(), state.chip, &mut state.editor).await?;
    let start = parser::get_addr(args.next_token(), state.range.start, "Start address", &mut state.editor).await?;
    let len = parser::get_addr(args.next_token(), state.range.len, "Length (0=full)", &mut state.editor).await?;
    let fmt = parser::get_format(args.next_token(), state.format, &mut state.editor).await?;
    let interval = parser::get_interval(args.next_token(), state.interval_secs, &mut state.editor).await?;
    let cs1 = parser::get_cs_polarity(
        args.next_token(),
        state.cs.cs1,
        &format!("CS1 polarity {}", CS_KEY),
        chip_has_configurable_cs(chip, "cs1"),
        &mut state.editor,
    )
    .await?;
    let cs2 = parser::get_cs_polarity(
        args.next_token(),
        state.cs.cs2,
        &format!("CS2 polarity {}", CS_KEY),
        chip_has_configurable_cs(chip, "cs2"),
        &mut state.editor,
    )
    .await?;
    let cs3 = parser::get_cs_polarity(
        args.next_token(),
        state.cs.cs3,
        &format!("CS3 polarity {}", CS_KEY),
        chip_has_configurable_cs(chip, "cs3"),
        &mut state.editor,
    )
    .await?;

    state.chip = Some(chip);
    state.range = ReadRange { start, len };
    state.format = fmt;
    state.interval_secs = interval;
    state.cs = CsSettings { cs1, cs2, cs3 };

    send_line("Batch read started. Press any key to stop.").await?;

    // Drain any bytes buffered during command entry (e.g. trailing LF from CR+LF).
    while let Ok(Some(_)) = usb::cdc_try_recv().await {}

    let mut pass = 0u32;
    loop {
        pass += 1;
        send_line(&format!("--- Pass {} ---", pass)).await?;

        do_read(board, chip, state.range, fmt, state.cs, state.tri_state).await?;

        // Wait for the interval to expire OR a keypress, whichever comes first.
        // A key pressed during a read is buffered and will be returned here
        // immediately, stopping after the current pass.  That is the intended
        // behaviour.
        match select(Timer::after_secs(interval as u64), usb::cdc_recv()).await {
            Either::First(_) => {} // timeout: run next pass
            Either::Second(Ok(_)) => {
                // keypress: stop
                send_line("Stopped.").await?;
                return Ok(());
            }
            Either::Second(Err(_)) => return Err(Error::UsbDisconnected),
        }
    }
}

async fn cmd_chip_info(args: &mut Args<'_>, state: &mut SessionState) -> Result<(), Error> {
    let chip = parser::require_chip(args.next_token(), state.chip, &mut state.editor).await?;
    state.chip = Some(chip);

    let n_addr = chip.address_pins().len();
    let n_data = chip.data_pins().len();
    let size_bytes = chip.size_bytes();
    let size_kb = size_bytes >> 10;
    let size_kb_str = if size_kb > 0 {
        format!("{} KB", size_kb)
    } else {
        if matches![chip, ChipType::Chip2704] {
            format!("0.5 KB")
        } else {
            format!("0 KB")
        }
    };

    send_line("").await?;
    send_line(&format!("Chip:       {}", chip.name())).await?;

    send_line(&format!(
        "Addr lines: {}  ({} bytes / {})",
        n_addr, size_bytes, size_kb_str
    ))
    .await?;
    send_line(&format!("Data lines: {}", n_data)).await?;

    match chip.bit_modes() {
        [8] => send_line("Bit modes:  8").await?,
        [16] => send_line("Bit modes:  16").await?,
        [8, 16] => send_line("Bit modes:  8 and 16").await?,
        modes => send_line(&format!("Bit modes:  {:?}", modes)).await?,
    }

    send_line("Control lines:").await?;
    for ctrl in chip.control_lines() {
        send_line(&format!(
            "  pin {:2}  {:<6}  {:?}",
            ctrl.pin, ctrl.name, ctrl.line_type
        ))
        .await?;
    }

    send_line("").await?;
    Ok(())
}

async fn cmd_set_chip_type(args: &mut Args<'_>, state: &mut SessionState) -> Result<(), Error> {
    let chip = parser::require_chip(args.next_token(), state.chip, &mut state.editor).await?;
    let cs1 = parser::get_cs_polarity(
        args.next_token(),
        state.cs.cs1,
        &format!("CS1 polarity {}", CS_KEY),
        chip_has_configurable_cs(chip, "cs1"),
        &mut state.editor,
    )
    .await?;
    let cs2 = parser::get_cs_polarity(
        args.next_token(),
        state.cs.cs2,
        &format!("CS2 polarity {}", CS_KEY),
        chip_has_configurable_cs(chip, "cs2"),
        &mut state.editor,
    )
    .await?;
    let cs3 = parser::get_cs_polarity(
        args.next_token(),
        state.cs.cs3,
        &format!("CS3 polarity {}", CS_KEY),
        chip_has_configurable_cs(chip, "cs3"),
        &mut state.editor,
    )
    .await?;

    state.chip = Some(chip);
    state.cs = CsSettings { cs1, cs2, cs3 };

    send_line(&format!("Chip set to '{}'.", chip.name())).await?;
    Ok(())
}

async fn cmd_set_format(args: &mut Args<'_>, state: &mut SessionState) -> Result<(), Error> {
    let fmt = parser::get_format(args.next_token(), state.format, &mut state.editor).await?;
    state.format = fmt;
    send_line(&format!("Format set to '{}'.", fmt.as_str())).await?;
    Ok(())
}

async fn cmd_toggle_tristate(state: &mut SessionState) -> Result<(), Error> {
    state.tri_state = !state.tri_state;
    send_line(&format!("Tri-state testing {}", if state.tri_state { "on" } else { "off" })).await?;
    Ok(())
}

async fn cmd_quick_read(state: &mut SessionState) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;
    let chip = state.chip.ok_or(Error::ChipNotSet)?;
    send_line(&format!(
        "Reading {chip}, range {}, output {}",
        state.range, state.format
    ))
    .await?;
    do_read(board, chip, state.range, state.format, state.cs, state.tri_state).await
}

async fn cmd_list_chips(state: &SessionState) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;
    let pins = board.chip_pins();

    let mut names: Vec<&'static str> = match chip_type_names_for_pins(pins) {
        Some(n) => n.to_vec(),
        None => {
            send_line(&format!(
                "No chips known for {}-pin board '{}'.",
                pins,
                board.name()
            ))
            .await?;
            return Ok(());
        }
    };
    let extra_chip_types = board.extra_chip_types();
    if !extra_chip_types.is_empty() {
        names.extend(extra_chip_types.iter().map(|c| c.name()));
        names.sort_unstable();
        names.dedup();
    }

    send_line("").await?;
    send_line(&format!(
        "Chips supported on '{}' ({} pins):",
        board.name(),
        pins
    ))
    .await?;
    send_line("").await?;

    for name in names {
        if let Some(chip) = ChipType::try_from_str(name) {
            let n_addr = chip.address_pins().len();
            let size_bytes = chip.size_bytes();
            let size_kb = size_bytes >> 10;
            let size_kb_str = if size_kb > 0 {
                format!("{} KB", size_kb)
            } else {
                if matches![chip, ChipType::Chip2704] {
                    format!("0.5 KB")
                } else {
                    format!("0 KB")
                }
            };
            let mode_str = match chip.bit_modes() {
                [8] => "8-bit",
                [16] => "16-bit",
                [8, 16] => "8/16-bit",
                _ => "?",
            };
            send_line(&format!(
                "  {:12}  {:>2} addr lines  {:>9}  ({})",
                name, n_addr, size_kb_str, mode_str,
            ))
            .await?;
        }
    }

    send_line("").await?;
    Ok(())
}

pub async fn cmd_version_info(_state: &SessionState) -> Result<(), Error> {
    send_line("").await?;
    send_line(&format!("One ROM Lab v{}", crate::PKG_VERSION)).await?;
    send_line(&format!("Serial:   {}", serial_id())).await?;
    send_line("").await?;
    Ok(())
}

async fn cmd_default_info(state: &SessionState) -> Result<(), Error> {
    send_line("").await?;
    send_line(&format!(
        "Board:     {}",
        state.board.map(|b| b.name()).unwrap_or("(not set)")
    ))
    .await?;
    send_line(&format!(
        "Chip:      {}",
        state.chip.map(|c| c.name()).unwrap_or("(not set)")
    ))
    .await?;
    send_line(&format!(
        "CS:        cs1={} cs2={} cs3={}",
        state.cs.cs1, state.cs.cs2, state.cs.cs3,
    ))
    .await?;
    send_line(&format!(
        "Range:     {:#x}..{}",
        state.range.start,
        if state.range.len == 0 {
            "end".to_string()
        } else {
            format!("{:#x}", state.range.len + state.range.start)
        }
    ))
    .await?;
    send_line(&format!("Format:    {}", state.format)).await?;
    send_line(&format!("Interval:  {}s", state.interval_secs)).await?;
    send_line(&format!("Tri-state: {}", if state.tri_state { "on" } else { "off" })).await?;
    send_line("").await?;
    Ok(())
}

async fn cmd_set_board(args: &mut Args<'_>, state: &mut SessionState) -> Result<(), Error> {
    let board = parser::require_board(args.next_token(), state.board, &mut state.editor).await?;
    state.board = Some(board);
    send_line(&format!("Board set to '{}'.", board.name())).await?;
    if state.chip.is_none() {
        if let Some(chip) = default_chip_for_board(board) {
            state.chip = Some(chip);
            send_line(&format!("Chip defaulted to '{}'.", chip.name())).await?;
        }
    }
    Ok(())
}

async fn cmd_list_board_types(_state: &SessionState) -> Result<(), Error> {
    send_line("").await?;
    send_line("Supported board types:").await?;
    send_line("").await?;

    for board in BOARDS {
        if matches!(board.mcu_family(), Family::Rp2350) {
            send_line(&format!("  {}", board.name())).await?;
        }
    }

    send_line("").await?;
    Ok(())
}

async fn cmd_reset_to_bootloader() -> Result<(), Error> {
    send_line("Resetting to bootloader...").await?;
    Timer::after_millis(100).await;
    // 0x102 - Reboot to BOOTSEL, don't return
    // 100ms delay before rebooting
    embassy_rp::rom_data::reboot(0x102, 100, 0, 0);
    Ok(())
}

// ---------------------------------------------------------------------------
// Read execution
// ---------------------------------------------------------------------------

/// Compute the effective `(start, byte_count)` for a read, clamped to the
/// chip's actual address space.
fn resolve_range(range: ReadRange, chip: ChipType) -> (usize, usize) {
    let rom_bytes = chip.size_bytes();
    let start = range.start.min(rom_bytes.saturating_sub(1));
    let available = rom_bytes - start;
    let count = if range.len == 0 {
        available
    } else {
        range.len.min(available)
    };
    (start, count)
}

/// Route a read to the correct format handler.
async fn do_read(
    board: Board,
    chip: ChipType,
    range: ReadRange,
    fmt: OutputFormat,
    cs: CsSettings,
    tristate: bool,
) -> Result<(), Error> {
    let needs_scan = chip.control_lines().iter().any(|c| {
        c.line_type == ControlLineType::Configurable
            && match c.name {
                "cs1" => matches!(cs.cs1, CsPolaritySetting::Auto),
                "cs2" => matches!(cs.cs2, CsPolaritySetting::Auto),
                "cs3" => matches!(cs.cs3, CsPolaritySetting::Auto),
                _ => false,
            }
    });

    if needs_scan {
        return scan_cs(board, chip, cs, tristate).await;
    }

    let (start, count) = resolve_range(range, chip);
    let pin_map = BoardPinMap::new(board);
    let mut reader = RomReader::new(&pin_map, chip, cs.to_polarities(), tristate);
    reader.init();

    match fmt {
        OutputFormat::Checksum => output_checksum(&mut reader, chip, start, count).await,
        OutputFormat::HexDump => crate::output::hexdump::dump(&mut reader, start, count).await,
        OutputFormat::IntelHex => crate::output::ihex::dump(&mut reader, start, count).await,
    }
}

/// Run a full-ROM (or warned-full) checksum + SHA-1 read.
///
/// Sub-range checksum reads require the streaming API in `rom.rs` which is
/// not yet implemented.  Until then, a non-default range issues a warning
/// and the full ROM is read.
async fn output_checksum(
    reader: &mut RomReader,
    chip: ChipType,
    start: usize,
    count: usize,
) -> Result<(), Error> {
    let rom_bytes = chip.size_bytes();

    if start != 0 || count != rom_bytes {
        // TODO: wire up streaming range reads in rom.rs and remove this.
        send_line(&format!(
            "Warning: range {:#010x}+{:#010x} ignored — reading full ROM.",
            start, count
        ))
        .await?;
    }

    let results = reader.read();

    send_line("").await?;
    for r in &results {
        let mode = if results.len() == 1 {
            // If there's only one mode, omit the redundant "8-bit"/"16-bit" label.
            "".to_string()
        } else {
            format!("{}-bit  ", r.mode)
        };
        send_line(&format!(
            "  {}SHA1: {}  checksum: {:#010X}",
            mode,
            hex::encode(r.sha1),
            r.checksum,
        ))
        .await?;
    }

    if results.len() >= 2 {
        let matched = results
            .windows(2)
            .all(|w| w[0].sha1 == w[1].sha1 && w[0].checksum == w[1].checksum);
        send_line(&format!("  Match: {}", matched)).await?;
    }

    if reader.tristate() {
        for r in &results {
            let mode = if results.len() == 1 {
                "".to_string()
            } else {
                format!("{}-bit ", r.mode)
            };
            send_line(&format!(
                "  {}tristate failures: {}",
                mode, r.failures
            ))
            .await?;
        }
    }

    send_line("").await?;
    Ok(())
}

fn chip_has_configurable_cs(chip: ChipType, name: &str) -> bool {
    chip.control_lines()
        .iter()
        .any(|c| c.name == name && c.line_type == ControlLineType::Configurable)
}

/// SHA-1 of `count` repetitions of `byte` — used to detect trivial reads.
fn trivial_sha1(byte: u8, count: usize) -> [u8; 20] {
    let mut sha = Sha1::new();
    let chunk = [byte; 64];
    let mut remaining = count;
    while remaining > 0 {
        let n = remaining.min(64);
        sha.update(&chunk[..n]);
        remaining -= n;
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&sha.finalize());
    out
}

async fn scan_cs(board: Board, chip: ChipType, cs: CsSettings, tristate: bool) -> Result<(), Error> {
    let auto: alloc::vec::Vec<&'static str> = chip
        .control_lines()
        .iter()
        .filter(|c| {
            c.line_type == ControlLineType::Configurable
                && match c.name {
                    "cs1" => matches!(cs.cs1, CsPolaritySetting::Auto),
                    "cs2" => matches!(cs.cs2, CsPolaritySetting::Auto),
                    "cs3" => matches!(cs.cs3, CsPolaritySetting::Auto),
                    _ => false,
                }
        })
        .map(|c| c.name)
        .collect();

    let rom_bytes = chip.size_bytes();
    let all_zeros = trivial_sha1(0x00, rom_bytes);
    let all_ffs = trivial_sha1(0xFF, rom_bytes);

    send_line(&format!(
        "Scanning {} CS combination(s)...",
        1 << auto.len()
    ))
    .await?;
    send_line("").await?;

    let pin_map = BoardPinMap::new(board);
    let base = cs.to_polarities();

    for combo in 0u32..(1u32 << auto.len()) {
        let mut test = base;
        for (i, &name) in auto.iter().enumerate() {
            let high = (combo >> i) & 1 == 1;
            match name {
                "cs1" => test.cs1 = Some(high),
                "cs2" => test.cs2 = Some(high),
                "cs3" => test.cs3 = Some(high),
                _ => {}
            }
        }

        let mut reader = RomReader::new(&pin_map, chip, test, tristate);
        reader.init();

        let label = auto
            .iter()
            .enumerate()
            .map(|(i, &name)| format!("{}={}", name, if (combo >> i) & 1 == 1 { "1" } else { "0" }))
            .collect::<alloc::vec::Vec<_>>()
            .join(" ");

        for &mode in chip.bit_modes() {
            let mut sha = Sha1::new();
            let mut checksum = core::num::Wrapping(0u32);

            reader.begin_read(mode);
            for addr in 0..rom_bytes {
                let byte = reader.read_byte_at(addr, mode);
                sha.update([byte]);
                checksum += core::num::Wrapping(byte as u32);
            }
            reader.end_read();

            let mut sha1 = [0u8; 20];
            sha1.copy_from_slice(&sha.finalize());

            let trivial = sha1 == all_zeros || sha1 == all_ffs;
            let mode = if chip.bit_modes().len() == 1 {
                // If there's only one mode, omit the redundant "8-bit"/"16-bit" label.
                "".to_string()
            } else {
                format!("{}-bit  ", mode)
            };
            send_line(&format!(
                "  {}  {}SHA1: {}  checksum: {:#010X}  {}",
                label,
                mode,
                hex::encode(sha1),
                checksum.0,
                if trivial {
                    ""
                } else {
                    "*** candidate ***"
                },
            ))
            .await?;
        }
    }

    send_line("").await?;
    Ok(())
}
