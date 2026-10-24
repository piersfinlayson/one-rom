// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT licence

//! One ROM Lab - CLI command dispatch and implementations

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::num::NonZeroU8;

use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};

use onerom_config::chip::{ChipType, ControlLineType};
use onerom_config::hw::{BOARDS, Board};
use onerom_config::mcu::Family;

use onerom_app::{
    SignerTable, Verdict, board_size_text, escape_controls, format_date, verdict_text,
};
use onerom_lab_bus::{C64, HostProfile};
use onerom_metadata::otp::parse_chip_id;
use onerom_metadata::{MaybeKnown, OneromBoardSize};
use sha1::{Digest, Sha1};

use crate::commissioning::Check;
use crate::drive::Driver;
use crate::error::Error;
use crate::fast_read::{self, FastReader};
use crate::host;
use crate::rbcp::Commissioning;
use crate::rom::RomReader;
use crate::usb;

use super::super::serial_id;
use super::parser::{self, Args, HostChoice};
use super::{CsPolaritySetting, CsSettings, OutputFormat, ReadRange, SessionState};
use super::{board_chip_names, default_chip_for_board, send_line};

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
        'b' => cmd_batch(&mut args, state, false).await,
        'a' => cmd_batch(&mut args, state, true).await,
        'x' => cmd_fast_read(&mut args, state).await,
        'd' => cmd_drive(&mut args, state).await,
        'i' => cmd_chip_info(&mut args, state).await,
        'c' => cmd_set_chip_type(&mut args, state).await,
        'f' => cmd_set_format(&mut args, state).await,
        't' => cmd_toggle_tristate(state).await,
        'q' => cmd_quick_read(state).await,
        'l' => cmd_list_chips(state).await,
        'v' => cmd_version_info(state).await,
        's' => cmd_default_info(state).await,
        'p' => cmd_pin_map(&mut args, state).await,
        'B' => cmd_set_board(&mut args, state).await,
        'T' => cmd_list_board_types(state).await,
        'R' => cmd_rbcp(state).await,
        'H' => cmd_set_host(&mut args, state).await,
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

    let chip = parser::require_chip(
        args.next_token(),
        state.chip,
        state.board,
        &mut state.editor,
    )
    .await?;
    let start = parser::get_addr(
        args.next_token(),
        state.range.start,
        "Start address",
        &mut state.editor,
    )
    .await?;
    let len = parser::get_addr(
        args.next_token(),
        state.range.len,
        "Length (0=full)",
        &mut state.editor,
    )
    .await?;
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
        "Reading {chip}, range {}, output {fmt}{}",
        state.range,
        host_suffix(state.host)
    ))
    .await?;
    do_read(board, chip, state, false).await
}

/// How long a fast read reads before checking for a key.
const FAST_READ_CHUNK: Duration = Duration::from_millis(250);

/// How often a fast read prints its progress.
const FAST_READ_STATUS: Duration = Duration::from_secs(1);

/// Read the chip continuously, as fast as Lab can, until a key is pressed.
async fn cmd_fast_read(args: &mut Args<'_>, state: &mut SessionState) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;

    let chip = parser::require_chip(
        args.next_token(),
        state.chip,
        state.board,
        &mut state.editor,
    )
    .await?;
    let delay = parser::get_delay(
        args.next_token(),
        state.fast_read_delay,
        "Read delay",
        &mut state.editor,
    )
    .await?;
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
    state.fast_read_delay = delay;
    state.cs = CsSettings { cs1, cs2, cs3 };
    require_cs(chip, state.cs)?;
    if chip.size_bytes() > fast_read::MAX_BYTES {
        return Err(Error::FastReadChipSize);
    }

    let mut reader = RomReader::new(board, chip, state.cs.to_polarities(), false)?;
    reader.init();
    let pins = reader.begin_fast_read();
    let result = match FastReader::new(&pins, chip.size_bytes(), delay) {
        Ok(mut fast) => fast_read_loop(chip, delay, &mut fast)
            .await
            .map(|elapsed| (fast, elapsed)),
        Err(e) => Err(e),
    };
    reader.end_read();
    let (fast, elapsed) = result?;

    send_line("Stopped.").await?;
    send_line(&fast_read_status(&fast, elapsed)).await?;
    for m in &fast.listed {
        send_line(&format!(
            "  Pass {} address {:#06x}: expected {:#04x}, read {:#04x}",
            m.pass, m.addr, m.expected, m.read
        ))
        .await?;
    }
    if fast.mismatches > fast.listed.len() as u64 {
        send_line(&format!(
            "  First {} mismatches listed.",
            fast_read::MAX_LISTED
        ))
        .await?;
    }
    send_line("").await
}

/// The passes of a fast read, until a key is pressed.  Returns how long the
/// passes ran.
async fn fast_read_loop(
    chip: ChipType,
    delay: u32,
    fast: &mut FastReader,
) -> Result<Duration, Error> {
    send_line(&format!(
        "Fast reading {chip}, read delay {delay} cycles. Press any key to stop."
    ))
    .await?;
    while let Ok(Some(_)) = usb::cdc_try_recv().await {}

    yield_to_usb().await;
    let sha1 = fast.read_reference();
    send_line(&format!("  Reference SHA1: {}", hex::encode(sha1))).await?;

    let start = Instant::now();
    let mut status_at = start + FAST_READ_STATUS;
    loop {
        let chunk_end = Instant::now() + FAST_READ_CHUNK;
        while Instant::now() < chunk_end {
            fast.compare_pass();
        }
        if usb::cdc_try_recv().await?.is_some() {
            return Ok(start.elapsed());
        }
        if Instant::now() >= status_at {
            send_line(&fast_read_status(fast, start.elapsed())).await?;
            status_at += FAST_READ_STATUS;
        }
    }
}

/// Toggles between checks of the clock.
const DRIVE_BURST: u32 = 4096;

/// Drive data lines as a square wave until a key is pressed.
async fn cmd_drive(args: &mut Args<'_>, state: &mut SessionState) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;

    let chip = parser::require_chip(
        args.next_token(),
        state.chip,
        state.board,
        &mut state.editor,
    )
    .await?;
    let lines = parser::get_lines(args.next_token(), state.drive_lines, &mut state.editor).await?;
    let delay = parser::get_delay(
        args.next_token(),
        state.drive_delay,
        "Toggle delay",
        &mut state.editor,
    )
    .await?;

    state.chip = Some(chip);
    state.drive_lines = lines;
    state.drive_delay = delay;
    require_cs(chip, state.cs)?;

    let mut reader = RomReader::new(board, chip, state.cs.to_polarities(), false)?;
    reader.init();
    let gpios = reader.begin_drive();
    let result = match Driver::new(&gpios, lines, delay) {
        Ok(mut driver) => drive_loop(delay, &mut driver)
            .await
            .map(|elapsed| (driver, elapsed)),
        Err(e) => Err(e),
    };
    reader.end_drive();
    reader.end_read();
    let (driver, elapsed) = result?;

    send_line("Stopped.").await?;
    send_line(&drive_status(&driver, elapsed)).await?;
    send_line("").await
}

async fn drive_loop(delay: u32, driver: &mut Driver) -> Result<Duration, Error> {
    let names: Vec<String> = (0..16)
        .filter(|n| driver.lines & (1 << n) != 0)
        .map(|n| format!("D{n}"))
        .collect();
    send_line(&format!(
        "Driving {}, delay {delay} cycles. Press any key to stop.",
        names.join(" ")
    ))
    .await?;
    while let Ok(Some(_)) = usb::cdc_try_recv().await {}

    let start = Instant::now();
    let mut status_at = start + FAST_READ_STATUS;
    loop {
        let chunk_end = Instant::now() + FAST_READ_CHUNK;
        while Instant::now() < chunk_end {
            driver.toggle(DRIVE_BURST);
        }
        if usb::cdc_try_recv().await?.is_some() {
            return Ok(start.elapsed());
        }
        if Instant::now() >= status_at {
            send_line(&drive_status(driver, start.elapsed())).await?;
            status_at += FAST_READ_STATUS;
        }
    }
}

fn drive_status(driver: &Driver, elapsed: Duration) -> String {
    let ms = elapsed.as_millis().max(1);
    // Two toggles make one cycle of the square wave.
    let centi_mhz = driver.toggles / 2 / 10 / ms;
    format!(
        "  {}.{}s  toggles {}.{}M  {}.{:02}MHz",
        ms / 1000,
        ms % 1000 / 100,
        driver.toggles / 1_000_000,
        driver.toggles % 1_000_000 / 100_000,
        centi_mhz / 100,
        centi_mhz % 100,
    )
}

/// A fast read's progress as one line, with the mismatches on each data line
/// that has any.
fn fast_read_status(fast: &FastReader, elapsed: Duration) -> String {
    let ms = elapsed.as_millis().max(1);
    let reads = fast.reads();
    let centi_mhz = reads / 10 / ms;
    let mut line = format!(
        "  {}.{}s  passes {}  reads {}.{}M  {}.{:02}MHz  mismatches {}",
        ms / 1000,
        ms % 1000 / 100,
        fast.passes,
        reads / 1_000_000,
        reads % 1_000_000 / 100_000,
        centi_mhz / 100,
        centi_mhz % 100,
        fast.mismatches,
    );
    for (n, &count) in fast.line_mismatches.iter().enumerate() {
        if count != 0 {
            line.push_str(&format!("  D{n} {count}"));
        }
    }
    line
}

/// A batch read, followed on each pass by an RBCP query where `rbcp` is set.
async fn cmd_batch(args: &mut Args<'_>, state: &mut SessionState, rbcp: bool) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;

    let chip = parser::require_chip(
        args.next_token(),
        state.chip,
        state.board,
        &mut state.editor,
    )
    .await?;
    let start = parser::get_addr(
        args.next_token(),
        state.range.start,
        "Start address",
        &mut state.editor,
    )
    .await?;
    let len = parser::get_addr(
        args.next_token(),
        state.range.len,
        "Length (0=full)",
        &mut state.editor,
    )
    .await?;
    let fmt = parser::get_format(args.next_token(), state.format, &mut state.editor).await?;
    let interval =
        parser::get_interval(args.next_token(), state.interval_secs, &mut state.editor).await?;
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

    if rbcp {
        require_cs(chip, state.cs)?;
    }

    send_line(&format!(
        "Batch read started{}. Press any key to stop.",
        host_suffix(state.host)
    ))
    .await?;

    // Drain any bytes buffered during command entry (e.g. trailing LF from CR+LF).
    while let Ok(Some(_)) = usb::cdc_try_recv().await {}

    let mut pass = 0u32;
    loop {
        pass += 1;
        send_line(&format!("--- Pass {} ---", pass)).await?;

        do_read(board, chip, state, rbcp).await?;

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
    let chip = parser::require_chip(
        args.next_token(),
        state.chip,
        state.board,
        &mut state.editor,
    )
    .await?;
    state.chip = Some(chip);

    let n_addr = chip.address_pins().len();
    let n_data = chip.data_pins().len();
    let size_bytes = chip.size_bytes();
    let size_kb = size_bytes >> 10;
    let size_kb_str = if size_kb > 0 {
        format!("{} KB", size_kb)
    } else if matches!(chip, ChipType::Chip2704) {
        "0.5 KB".to_string()
    } else {
        "0 KB".to_string()
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
    let chip = parser::require_chip(
        args.next_token(),
        state.chip,
        state.board,
        &mut state.editor,
    )
    .await?;
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
    send_line(&format!(
        "Tri-state testing {}",
        if state.tri_state { "on" } else { "off" }
    ))
    .await?;
    Ok(())
}

async fn cmd_quick_read(state: &mut SessionState) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;
    let chip = state.chip.ok_or(Error::ChipNotSet)?;
    send_line(&format!(
        "Reading {chip}, range {}, output {}{}",
        state.range,
        state.format,
        host_suffix(state.host)
    ))
    .await?;
    do_read(board, chip, state, false).await
}

async fn cmd_list_chips(state: &SessionState) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;
    let pins = board.chip_pins();

    let Some(names) = board_chip_names(board) else {
        send_line(&format!(
            "No chips known for {}-pin board '{}'.",
            pins,
            board.name()
        ))
        .await?;
        return Ok(());
    };

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
            } else if matches!(chip, ChipType::Chip2704) {
                "0.5 KB".to_string()
            } else {
                "0 KB".to_string()
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
    send_line(&format!("Delay:     {} cycles", state.fast_read_delay)).await?;
    send_line(&format!(
        "Tri-state: {}",
        if state.tri_state { "on" } else { "off" }
    ))
    .await?;
    send_line(&format!(
        "Host:      {}",
        state
            .host
            .map_or(String::from("off"), |host| host::driven(&host))
    ))
    .await?;
    send_line("").await?;
    Ok(())
}

/// Show the current board's physical socket-pin -> MCU GPIO map.
///
/// Read-only: never prompts, never mutates session state.  An inline chip
/// (`p:<chip>`) annotates each pin with that chip's role for a one-off view
/// without changing the session chip; with no inline chip the session chip is
/// used.  With no chip at all (an unreachable state today, since every board
/// defaults one) the map still renders as pin -> GPIO with the role column
/// omitted.
///
/// Every GPIO wired to a pin is shown, including the twinned pins on Fire32/40
/// where a socket pin routes to two GPIOs -- unlike the reader, which uses only
/// the first.  They share a net, so any one drives or samples the pin.
async fn cmd_pin_map(args: &mut Args<'_>, state: &SessionState) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;

    // An inline chip overrides the session chip for annotation only.
    let chip = match args.next_token() {
        Some(t) => Some(ChipType::try_from_str(t).ok_or(Error::InvalidChip)?),
        None => state.chip,
    };

    send_line("").await?;
    match chip {
        Some(c) => send_line(&format!("Board: {}   Chip: {}", board.name(), c.name())).await?,
        None => send_line(&format!("Board: {}   Chip: (none)", board.name())).await?,
    }
    send_line("").await?;

    send_line(" Pin  Signal  GPIO").await?;

    // Walk every physical board socket pin in order.  The GPIO comes from
    // socket_pin_map() (or "-" if the pin has none); the signal is what the
    // chip does at that pin, "NC" where the chip reaches nothing.
    let map = board.socket_pin_map();

    for pin in 1..=board.chip_pins() {
        let gpio_str = match map.iter().find(|&&(p, _)| p == pin) {
            Some(&(_, gpios)) => join_u8(gpios),
            None => String::from("-"),
        };
        let sig = match chip {
            Some(c) => {
                let s = signal_for_pin(c, board, pin);
                if s.is_empty() { String::from("NC") } else { s }
            }
            None if board.non_signal_pins().contains(&pin) => String::from("GND/VCC"),
            None => String::new(),
        };
        send_line(&format!(" {:>3}  {:<6}  {}", pin, sig, gpio_str)).await?;
    }

    let x = board.x_pin_map();
    if !x.is_empty() {
        send_line("").await?;
        let mut line = String::from("X header:");
        for &(xn, gpios) in x {
            line.push_str(&format!(" X{}={}", xn, join_u8(gpios)));
        }
        send_line(&line).await?;
    }

    send_line("").await?;
    Ok(())
}

/// Offset from a chip's own pin numbering to board socket numbering, for the
/// 24-pin-chip-in-28-pin-socket adapter case.  Mirrors `RomReader`.
fn socket_offset(chip: ChipType, board: Board) -> u8 {
    if chip.chip_pins() == 24 && board.chip_pins() == 28 {
        2
    } else {
        0
    }
}

/// Resolve the signal name a chip assigns to a board socket pin (e.g. `A7`,
/// `D0`, `CE`), or an empty string for pins the chip doesn't use.
///
/// The board socket pin is mapped back to the chip's own pin numbering,
/// undoing the 24-pin-chip-in-28-pin-socket offset that `RomReader` applies in
/// the forward direction, so this view stays consistent with what the reader
/// drives.
fn signal_for_pin(chip: ChipType, board: Board, socket_pin: u8) -> String {
    let offset = socket_offset(chip, board);
    let chip_pin = match socket_pin.checked_sub(offset) {
        Some(p) if p >= 1 => p,
        _ => return String::new(),
    };

    if let Some(i) = chip.address_pins().iter().position(|&p| p == chip_pin) {
        // 16-bit chips (27C200/27C400) expose A-1 as their first address line;
        // every other chip starts at A0.  Keyed off the bit mode rather than a
        // chip name, so any future 16-bit part is covered.  Mirrors rom.rs.
        let base: i32 = if chip.supports_bit_mode(16) { -1 } else { 0 };
        return format!("A{}", i as i32 + base);
    }
    if let Some(i) = chip.data_pins().iter().position(|&p| p == chip_pin) {
        return format!("D{}", i);
    }
    for ctrl in chip.control_lines() {
        if ctrl.pin == chip_pin {
            return ctrl.name.to_ascii_uppercase();
        }
    }
    if let Some(progs) = chip.programming_pins() {
        for spec in progs {
            if spec.pin == chip_pin {
                return spec.name.to_ascii_uppercase();
            }
        }
    }
    for spec in chip.power_pins() {
        if spec.pin == chip_pin {
            return spec.name.to_ascii_uppercase();
        }
    }
    String::new()
}

/// Join a slice of `u8` as a comma-separated string, or `-` if empty.
fn join_u8(vals: &[u8]) -> String {
    let mut s = String::new();
    for (i, v) in vals.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!("{}", v));
    }
    if s.is_empty() {
        s.push('-');
    }
    s
}

async fn cmd_set_board(args: &mut Args<'_>, state: &mut SessionState) -> Result<(), Error> {
    let board = parser::require_board(args.next_token(), state.board, &mut state.editor).await?;
    state.board = Some(board);
    crate::metadata::set_board(board);
    send_line(&format!("Board set to '{}'.", board.name())).await?;
    if state.chip.is_none()
        && let Some(chip) = default_chip_for_board(board)
    {
        state.chip = Some(chip);
        send_line(&format!("Chip defaulted to '{}'.", chip.name())).await?;
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

/// Query the One ROM in the socket over RBCP, using the session's chip type
/// and control line polarities.
async fn cmd_rbcp(state: &SessionState) -> Result<(), Error> {
    let board = state.board.ok_or(Error::BoardNotSet)?;
    let chip = state.chip.ok_or(Error::ChipNotSet)?;
    require_cs(chip, state.cs)?;
    send_line(&format!(
        "Querying a One ROM serving {chip} over RBCP{}",
        host_suffix(state.host)
    ))
    .await?;
    send_line("").await?;
    do_rbcp(board, chip, state.cs, true, state.host).await
}

/// Set the host bus timing, or turn it off.
async fn cmd_set_host(args: &mut Args<'_>, state: &mut SessionState) -> Result<(), Error> {
    let current = match state.host {
        None => String::from("off"),
        Some(host) if host == C64 => String::from("c64"),
        Some(host) => host.cycle_ns.to_string(),
    };
    let cycle_ns = match parser::get_host(args.next_token(), &current, &mut state.editor).await? {
        HostChoice::Off => return set_host(state, None).await,
        HostChoice::C64 => return set_host(state, Some(C64)).await,
        HostChoice::Cycle(ns) => ns,
    };

    // Fields left out default to the current profile, or to the C64's.
    let base = state.host.unwrap_or(C64);
    let extra = parser::get_number(
        args.next_token(),
        base.extra.into(),
        "Non-ROM cycles",
        0..=u8::MAX.into(),
        &mut state.editor,
    )
    .await?;
    let glitch_ns = parser::get_number(
        args.next_token(),
        base.glitch_ns,
        "Glitch (ns, 0=none)",
        0..=u32::MAX,
        &mut state.editor,
    )
    .await?;
    let cs_ns = parser::get_number(
        args.next_token(),
        base.cs_ns,
        "Chip select active (ns)",
        0..=u32::MAX,
        &mut state.editor,
    )
    .await?;
    let glitch_pos_ns = parser::get_number(
        args.next_token(),
        base.glitch_pos_ns,
        "Glitch position (ns)",
        0..=u32::MAX,
        &mut state.editor,
    )
    .await?;
    let glitch_every = parser::get_number(
        args.next_token(),
        base.glitch_every.get().into(),
        "Non-ROM cycles per glitch",
        1..=u8::MAX.into(),
        &mut state.editor,
    )
    .await?;

    let profile = HostProfile {
        cycle_ns,
        extra: extra as u8,
        glitch_ns,
        cs_ns,
        glitch_pos_ns,
        glitch_every: NonZeroU8::new(glitch_every as u8).ok_or(Error::Syntax)?,
    };
    profile.to_bus().map_err(Error::HostProfile)?;
    set_host(state, Some(profile)).await
}

async fn set_host(state: &mut SessionState, host: Option<HostProfile>) -> Result<(), Error> {
    state.host = host;
    let name = host.map_or(String::from("off"), |host| host::name(&host));
    send_line(&format!("Host set to '{name}'.")).await
}

/// The profile for the first line of a read or RBCP query, if one is set.
fn host_suffix(host: Option<HostProfile>) -> String {
    host.map_or(String::new(), |host| {
        format!(", host {}", host::name(&host))
    })
}

/// RBCP and a fast read require every configurable control line's polarity,
/// as a read does, and can't auto-detect one.
fn require_cs(chip: ChipType, cs: CsSettings) -> Result<(), Error> {
    let cs = cs.to_polarities();
    let unset = chip.control_lines().iter().any(|c| {
        c.line_type == ControlLineType::Configurable
            && match c.name {
                "cs1" => cs.cs1.is_none(),
                "cs2" => cs.cs2.is_none(),
                "cs3" => cs.cs3.is_none(),
                _ => false,
            }
    });
    if unset {
        Err(Error::CsPolarityNotSet)
    } else {
        Ok(())
    }
}

/// Query the One ROM over RBCP in each bit mode the chip has, and print what
/// it reports.
///
/// `full` prints the whole query, as `R` does. Otherwise each mode is one line,
/// as a batch pass prints it, plus any warning. A
/// failed query is printed and the next mode is still queried.
async fn do_rbcp(
    board: Board,
    chip: ChipType,
    cs: CsSettings,
    full: bool,
    host: Option<HostProfile>,
) -> Result<(), Error> {
    let modes = chip.bit_modes();
    let table = SignerTable::built_in();

    for &mode in modes {
        yield_to_usb().await;
        let mut reader = RomReader::new(board, chip, cs.to_polarities(), false)?;
        reader.init();
        reader.set_host(host)?;
        reader.begin_read(mode);
        let result = crate::rbcp::query(&mut reader, board, mode);
        reader.end_read();
        // A host bus fault replaces the RBCP error it caused.
        let result = reader
            .host_fault()
            .and_then(|()| result.map_err(Error::Rbcp));

        let label = mode_label(mode, modes.len());
        if full && modes.len() > 1 {
            send_line(&format!("{mode}-bit")).await?;
        }
        let report = match result {
            Ok(id) => Report::new(&id, &table).await,
            Err(e) => {
                let indent = if full { "" } else { "  " };
                send_line(&format!("{indent}{label}Error: {e}")).await?;
                if full {
                    send_line("").await?;
                }
                continue;
            }
        };

        if full {
            for line in report.full_lines() {
                send_line(&line).await?;
            }
            send_line("").await?;
        } else {
            send_line(&format!("  {label}Device: {}", report.line())).await?;
            if let Some(warning) = &report.warning {
                send_line(&format!("  {label}{warning}")).await?;
            }
        }
    }
    Ok(())
}

/// A One ROM's RBCP query as text.
struct Report {
    device: String,
    firmware: String,
    protocol: String,
    boot: String,
    /// `None` without host-control's Device Info commands.
    info: Option<InfoText>,
    /// Set where the board types differ.
    warning: Option<String>,
}

struct InfoText {
    board_type: String,
    board_size: String,
    chip_id: String,
    commissioned: CommissionedText,
}

enum CommissionedText {
    /// A `Commissioned:` value without lines beneath it.
    Bare(String),
    Yes {
        manufacturer: Option<String>,
        values: Vec<(&'static str, String)>,
        signature: &'static str,
    },
}

impl Report {
    async fn new(id: &crate::rbcp::Identity, table: &SignerTable) -> Self {
        let text =
            |f: &onerom_rbcp_host::AsciiField| String::from_utf8_lossy(f.as_bytes()).into_owned();
        let slot = |s: Option<u8>| s.map_or("unknown".to_string(), |s| s.to_string());
        let mut warning = None;
        let info = match &id.info {
            Some(info) => {
                let (text, board) = commissioned_text(info, table).await;
                if let Some(board) = board
                    && board != info.board_type
                {
                    warning = Some(format!(
                        "WARNING: Firmware board type '{}' does not match commissioned board type '{}'.",
                        escape_controls(&info.board_type),
                        escape_controls(&board)
                    ));
                }
                let commissioned = match &info.commissioning {
                    Commissioning::Yes(instance) => instance.size(),
                    Commissioning::No | Commissioning::UnknownVersion(_) => None,
                };
                Some(InfoText {
                    board_type: escape_controls(&info.board_type),
                    board_size: board_size(info.board_size, commissioned),
                    chip_id: hex::encode_upper(info.chip_id),
                    commissioned: text,
                })
            }
            None => None,
        };
        Report {
            device: text(&id.device_type),
            firmware: text(&id.device_version),
            protocol: id.protocol_version.to_string(),
            boot: format!(
                "flash slot {}, RAM slot {}",
                slot(id.boot.flash_slot),
                slot(id.boot.ram_slot)
            ),
            info,
            warning,
        }
    }

    /// The lines `R` prints.
    fn full_lines(&self) -> Vec<String> {
        let unknown = || "unknown".to_string();
        let info = self.info.as_ref();
        let commissioned = match info.map(|i| &i.commissioned) {
            Some(CommissionedText::Bare(value)) => value.clone(),
            Some(CommissionedText::Yes { .. }) => "yes".to_string(),
            None => unknown(),
        };
        let top = [
            ("Device:", self.device.clone()),
            (
                "Board type:",
                info.map_or_else(unknown, |i| i.board_type.clone()),
            ),
            (
                "Board size:",
                info.map_or_else(unknown, |i| i.board_size.clone()),
            ),
            ("Firmware:", self.firmware.clone()),
            ("RBCP:", self.protocol.clone()),
            ("Boot:", self.boot.clone()),
            ("Chip ID:", info.map_or_else(unknown, |i| i.chip_id.clone())),
            ("Commissioned:", commissioned),
        ];
        let mut lines: Vec<String> = self.warning.iter().cloned().collect();
        lines.extend(labelled(&top, ""));
        if let Some(CommissionedText::Yes {
            values, signature, ..
        }) = info.map(|i| &i.commissioned)
        {
            let mut values = values.clone();
            values.push(("Signature:", signature.to_string()));
            lines.extend(labelled(&values, "  "));
        }
        lines
    }

    /// The `a` line after `Device: `.
    fn line(&self) -> String {
        let mut line = format!("{} {}", self.device, self.firmware);
        let commissioned = match &self.info {
            Some(info) => {
                line.push_str(&format!(", {} ({})", info.board_type, info.board_size));
                match &info.commissioned {
                    CommissionedText::Bare(value) => value.clone(),
                    CommissionedText::Yes {
                        manufacturer,
                        signature,
                        ..
                    } => format!(
                        "{}, signature {signature}",
                        manufacturer.as_deref().unwrap_or("unknown")
                    ),
                }
            }
            None => "unknown".to_string(),
        };
        line.push_str(&format!(" - Commissioned: {commissioned}"));
        line
    }
}

/// Also returns the commissioned board type.
async fn commissioned_text(
    info: &crate::rbcp::DeviceInfo,
    table: &SignerTable,
) -> (CommissionedText, Option<String>) {
    let instance = match &info.commissioning {
        Commissioning::No => return (CommissionedText::Bare("no".to_string()), None),
        Commissioning::UnknownVersion(version) => {
            return (
                CommissionedText::Bare(format!("unknown (version {version})")),
                None,
            );
        }
        Commissioning::Yes(instance) => instance,
    };
    let chip_id = parse_chip_id(&hex::encode_upper(info.chip_id));
    let signature = match chip_id {
        Some(chip_id) => match crate::commissioning::check(instance, chip_id, table).await {
            Check::Verdict(verdict) => verdict_text(verdict),
            Check::Retired => "verified, key retired",
        },
        None => verdict_text(Verdict::BadSignature),
    };
    let manufacturer = instance.manufacturer().map(escape_controls);
    let mut lines = Vec::new();
    if let Some(board) = instance.board() {
        lines.push(("Board type:", escape_controls(board)));
    }
    if let Some(revision) = instance.revision() {
        lines.push(("Revision:", escape_controls(revision)));
    }
    if let Some(manufacturer) = &manufacturer {
        lines.push(("Manufacturer:", manufacturer.clone()));
    }
    if let Some(date) = instance.date() {
        lines.push(("Date:", format_date(date)));
    }
    if let Some(batch) = instance.batch() {
        lines.push(("Batch:", escape_controls(batch)));
    }
    if let Some(id) = instance.signer() {
        let name = match table.get(id) {
            Some(signer) => format!("{} ({id})", escape_controls(signer.name())),
            None => format!("signer {id}"),
        };
        lines.push(("Signing key:", name));
    }
    (
        CommissionedText::Yes {
            manufacturer,
            values: lines,
            signature,
        },
        instance.board().map(Into::into),
    )
}

/// `raw` is an `onerom_board_size_t`. Where the board was commissioned as
/// another size, that size follows.
fn board_size(raw: u32, commissioned: Option<MaybeKnown<OneromBoardSize>>) -> String {
    let size = u8::try_from(raw)
        .ok()
        .and_then(|raw| OneromBoardSize::try_from(raw).ok())
        .map_or(MaybeKnown::Unknown(raw), MaybeKnown::Known);
    let text = board_size_text(size);
    match commissioned {
        Some(commissioned) if commissioned != size => {
            format!("{text} (commissioned as {})", board_size_text(commissioned))
        }
        _ => text,
    }
}

/// Lines up the values past the longest label.
fn labelled(values: &[(&str, String)], indent: &str) -> Vec<String> {
    let width = values
        .iter()
        .map(|(label, _)| label.len() + 1)
        .max()
        .unwrap_or_default();
    values
        .iter()
        .map(|(label, value)| format!("{indent}{label:width$}{value}"))
        .collect()
}

async fn cmd_reset_to_bootloader() -> Result<(), Error> {
    send_line("Resetting to bootloader...").await?;
    Timer::after_millis(100).await;
    // 0x102 - Reboot to BOOTSEL, don't return
    // 100ms delay before rebooting
    // 0x1 in p0 - No mass storage device, matching One ROM.  A PICOBOOT host
    // can ask for one.  The datasheet swaps p0 and p1 - see rom_reboot() in
    // firmware/src/rp235x.c.
    embassy_rp::rom_data::reboot(0x102, 100, 0x1, 0);
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

/// Allow the USB task to send queued output before a whole-ROM read, which
/// blocks the executor until it finishes.  send_line's yield isn't enough as
/// the executor polls the calling task again before the USB task.
async fn yield_to_usb() {
    Timer::after_millis(1).await;
}

/// Route a read to the correct format handler, then query the One ROM over
/// RBCP where `rbcp` is set.
async fn do_read(
    board: Board,
    chip: ChipType,
    state: &SessionState,
    rbcp: bool,
) -> Result<(), Error> {
    let (range, fmt, cs, tristate, host) = (
        state.range,
        state.format,
        state.cs,
        state.tri_state,
        state.host,
    );
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
        return scan_cs(board, chip, cs, tristate, host).await;
    }

    let (start, count) = resolve_range(range, chip);
    let mut reader = RomReader::new(board, chip, cs.to_polarities(), tristate)?;
    reader.init();
    reader.set_host(host)?;

    match fmt {
        OutputFormat::Checksum => output_checksum(&mut reader, chip, start, count).await?,
        OutputFormat::HexDump => crate::output::hexdump::dump(&mut reader, start, count).await?,
        OutputFormat::IntelHex => crate::output::ihex::dump(&mut reader, start, count).await?,
        OutputFormat::Srec => crate::output::srec::dump(&mut reader, start, count).await?,
    }
    reader.host_fault()?;
    if rbcp {
        do_rbcp(board, chip, cs, false, host).await?;
    }
    if fmt == OutputFormat::Checksum || rbcp {
        send_line("").await?;
    }
    Ok(())
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

    yield_to_usb().await;
    let results = reader.read();
    reader.host_fault()?;

    send_line("").await?;
    for r in &results {
        let mode = mode_label(r.mode, results.len());
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
        send_line(&format!("  {:MODE_LABEL_WIDTH$}{matched}", "Match:")).await?;
    }

    if reader.tristate() {
        for r in &results {
            let mode = mode_label(r.mode, results.len());
            send_line(&format!("  {}tristate failures: {}", mode, r.failures)).await?;
        }
    }

    Ok(())
}

/// The bit mode a line reports, padded so 8-bit and 16-bit lines align, or
/// nothing where the chip has one mode.
fn mode_label(mode: u8, modes: usize) -> String {
    if modes == 1 {
        String::new()
    } else {
        format!("{:MODE_LABEL_WIDTH$}", format!("{mode}-bit"))
    }
}

const MODE_LABEL_WIDTH: usize = 8;

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

async fn scan_cs(
    board: Board,
    chip: ChipType,
    cs: CsSettings,
    tristate: bool,
    host: Option<HostProfile>,
) -> Result<(), Error> {
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

        let mut reader = RomReader::new(board, chip, test, tristate)?;
        reader.init();
        reader.set_host(host)?;

        let label = auto
            .iter()
            .enumerate()
            .map(|(i, &name)| format!("{}={}", name, if (combo >> i) & 1 == 1 { "1" } else { "0" }))
            .collect::<alloc::vec::Vec<_>>()
            .join(" ");

        for &mode in chip.bit_modes() {
            let mut sha = Sha1::new();
            let mut checksum = core::num::Wrapping(0u32);

            yield_to_usb().await;
            reader.begin_read(mode);
            for addr in 0..rom_bytes {
                let byte = reader.read_byte_at(addr, mode);
                sha.update([byte]);
                checksum += core::num::Wrapping(byte as u32);
            }
            reader.end_read();
            reader.host_fault()?;

            let mut sha1 = [0u8; 20];
            sha1.copy_from_slice(&sha.finalize());

            let trivial = sha1 == all_zeros || sha1 == all_ffs;
            let mode = mode_label(mode, chip.bit_modes().len());
            send_line(&format!(
                "  {}  {}SHA1: {}  checksum: {:#010X}  {}",
                label,
                mode,
                hex::encode(sha1),
                checksum.0,
                if trivial { "" } else { "*** candidate ***" },
            ))
            .await?;
        }
    }

    send_line("").await?;
    Ok(())
}
