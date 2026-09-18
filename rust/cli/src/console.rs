// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! `onerom console` - talking to the retro system.
//!
//! One ROM's serial port carries both directions.  Received bytes are displayed
//! as `onerom monitor log` displays them.  Typed bytes are written to the port.
//! The USB plugin puts them on log channel 1 and the host-control plugin serves
//! them to the retro system.
//!
//! Reading and writing use separate threads on two handles to the port, so a
//! blocked write never blocks the display.

use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal;

use crate::args::console::{ConsoleArgs, LineEnding};
use crate::utils::check_device_running;
use onerom_cli::cdc::{SILENCE_TIMEOUT, find_port, open, send, stream_port};
use onerom_cli::{Error, Options};

/// How often the raw-mode key loop checks whether the session has ended.
const KEY_POLL: Duration = Duration::from_millis(100);

pub async fn cmd_console(options: &Options, args: &ConsoleArgs) -> Result<(), Error> {
    check_device_running(options, args)?;
    let device = options.device.as_ref().unwrap();
    let capture = args.output.as_ref().map(PathBuf::from);

    let port_name = find_port(device)?;
    if options.verbose {
        eprintln!("Using serial port {port_name}");
    }
    let port = open(&port_name)?;
    let writer = port
        .try_clone()
        .map_err(|e| Error::SerialPort(format!("Failed to open the port for writing: {e}")))?;

    match capture.as_deref() {
        Some(path) => eprintln!(
            "Console open, writing to {} - press Ctrl-C to stop",
            path.display()
        ),
        None => eprintln!("Console open - press Ctrl-C to stop"),
    }

    let stop = Arc::new(AtomicBool::new(false));

    // In raw mode Ctrl-C arrives as a key and the key loop handles it.
    // Otherwise it is a signal, caught here so the session ends cleanly and
    // the byte count can be reported.
    let interactive = std::io::stdin().is_terminal();
    if !(interactive && args.raw) {
        let signalled = stop.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                signalled.store(true, Ordering::Relaxed);
            }
        });
    }

    let input = {
        let stop = stop.clone();
        let ending = args.line_ending;
        let raw = args.raw;
        let echo = !args.no_echo;
        std::thread::spawn(move || {
            let mut writer = writer;
            let result = if !interactive {
                send_piped(writer.as_mut(), &stop)
            } else if raw {
                send_keys(writer.as_mut(), ending, echo, &stop)
            } else {
                send_lines(writer.as_mut(), ending, &stop)
            };
            if let Err(e) = result {
                eprintln!("{e}");
            }
        })
    };

    let reported = capture.clone();
    let reader = stop.clone();
    let result = tokio::task::spawn_blocking(move || {
        stream_port(port, capture.as_deref(), SILENCE_TIMEOUT, &reader)
    })
    .await
    .map_err(|e| Error::Other(format!("Failed to read from One ROM: {e}")))?;

    // The session is over.  Wait for the input thread only in raw mode, where
    // it polls the stop flag and must restore the terminal first.  In line
    // mode it is blocked reading stdin and ends with the process.
    let interrupted = stop.load(Ordering::Relaxed);
    stop.store(true, Ordering::Relaxed);
    if interactive && args.raw {
        let _ = input.join();
    }

    let copied = result?;
    if interrupted {
        eprintln!("Console closed");
    } else {
        eprintln!("One ROM disconnected");
    }
    if let Some(path) = reported {
        eprintln!("Wrote {copied} bytes to {}", path.display());
    }

    Ok(())
}

/// Tell the user, on stderr, that input has stalled or resumed.
fn stall_notice(stalled: bool) {
    if stalled {
        eprintln!("One ROM is not reading the input data");
    } else {
        eprintln!("Input resumed");
    }
}

/// Send stdin unchanged.  Used when stdin is not a terminal.
fn send_piped(port: &mut dyn serialport::SerialPort, stop: &AtomicBool) -> Result<(), Error> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|e| Error::Io(format!("Failed to read stdin: {e}")))?;
    let mut stalled = false;
    send(port, &bytes, stop, &mut stalled, &mut stall_notice)
}

/// Send stdin line by line, replacing each newline with `ending`.
fn send_lines(
    port: &mut dyn serialport::SerialPort,
    ending: LineEnding,
    stop: &AtomicBool,
) -> Result<(), Error> {
    let stdin = std::io::stdin();
    let mut stalled = false;
    let mut line = String::new();
    loop {
        line.clear();
        let n = stdin
            .read_line(&mut line)
            .map_err(|e| Error::Io(format!("Failed to read stdin: {e}")))?;
        if n == 0 || stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let bytes = with_ending(&line, ending);
        send(port, &bytes, stop, &mut stalled, &mut stall_notice)?;
    }
}

/// A typed line with the terminal's newline replaced by `ending`.
fn with_ending(line: &str, ending: LineEnding) -> Vec<u8> {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let body = body.strip_suffix('\r').unwrap_or(body);
    let mut bytes = body.as_bytes().to_vec();
    bytes.extend_from_slice(ending.bytes());
    bytes
}

/// What a key press does in raw mode.
#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    /// Send these bytes.
    Send(Vec<u8>),
    /// End the session.
    Stop,
    /// Nothing.  A key with no byte of its own, or a release.
    Ignore,
}

/// The bytes a key press sends.
///
/// Enter sends `ending`.  Ctrl plus a letter sends the control character,
/// except Ctrl-C, which exits.  Keys with no character, such as arrows, send
/// nothing.
///
/// The wildcard is deliberate.  A key crossterm adds sends nothing until this
/// function says otherwise.
#[allow(clippy::wildcard_enum_match_arm)]
fn key_action(key: &KeyEvent, ending: LineEnding) -> KeyAction {
    if key.kind == KeyEventKind::Release {
        return KeyAction::Ignore;
    }
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyAction::Stop,
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            match c.to_ascii_lowercase() {
                c @ 'a'..='z' => KeyAction::Send(vec![(c as u8) & 0x1F]),
                _ => KeyAction::Ignore,
            }
        }
        KeyCode::Char(c) => KeyAction::Send(c.to_string().into_bytes()),
        KeyCode::Enter => KeyAction::Send(ending.bytes().to_vec()),
        KeyCode::Backspace => KeyAction::Send(vec![0x08]),
        KeyCode::Tab => KeyAction::Send(vec![0x09]),
        KeyCode::Esc => KeyAction::Send(vec![0x1B]),
        KeyCode::Delete => KeyAction::Send(vec![0x7F]),
        _ => KeyAction::Ignore,
    }
}

/// Send each key as it is pressed, with the terminal in raw mode.
fn send_keys(
    port: &mut dyn serialport::SerialPort,
    ending: LineEnding,
    echo: bool,
    stop: &AtomicBool,
) -> Result<(), Error> {
    terminal::enable_raw_mode().map_err(|e| Error::io("terminal", e))?;
    let result = key_loop(port, ending, echo, stop);
    // Always restore the terminal.
    terminal::disable_raw_mode().map_err(|e| Error::io("terminal", e))?;
    result
}

fn key_loop(
    port: &mut dyn serialport::SerialPort,
    ending: LineEnding,
    echo: bool,
    stop: &AtomicBool,
) -> Result<(), Error> {
    let mut stalled = false;
    let mut stdout = std::io::stdout();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        if !event::poll(KEY_POLL).map_err(|e| Error::io("terminal", e))? {
            continue;
        }
        let Event::Key(key) = event::read().map_err(|e| Error::io("terminal", e))? else {
            continue;
        };
        match key_action(&key, ending) {
            KeyAction::Ignore => {}
            KeyAction::Stop => {
                stop.store(true, Ordering::Relaxed);
                return Ok(());
            }
            KeyAction::Send(bytes) => {
                if echo {
                    // A raw terminal only moves down on a line feed, so Enter
                    // is displayed as CRLF whatever it sends.
                    let shown: &[u8] = if key.code == KeyCode::Enter {
                        b"\r\n"
                    } else {
                        &bytes
                    };
                    stdout
                        .write_all(shown)
                        .and_then(|()| stdout.flush())
                        .map_err(|e| Error::Io(format!("Failed to write to stdout: {e}")))?;
                }
                send(port, &bytes, stop, &mut stalled, &mut stall_notice)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn enter_sends_the_chosen_ending() {
        for (ending, want) in [
            (LineEnding::Cr, b"\r".as_slice()),
            (LineEnding::Lf, b"\n"),
            (LineEnding::Crlf, b"\r\n"),
        ] {
            assert_eq!(
                key_action(&key(KeyCode::Enter, KeyModifiers::NONE), ending),
                KeyAction::Send(want.to_vec())
            );
        }
    }

    #[test]
    fn ctrl_c_stops_and_other_controls_send() {
        assert_eq!(
            key_action(
                &key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                LineEnding::Cr
            ),
            KeyAction::Stop
        );
        assert_eq!(
            key_action(
                &key(KeyCode::Char('a'), KeyModifiers::CONTROL),
                LineEnding::Cr
            ),
            KeyAction::Send(vec![0x01])
        );
        assert_eq!(
            key_action(
                &key(KeyCode::Char('Z'), KeyModifiers::CONTROL),
                LineEnding::Cr
            ),
            KeyAction::Send(vec![0x1A])
        );
    }

    #[test]
    fn characters_go_as_typed_and_arrows_do_not() {
        assert_eq!(
            key_action(&key(KeyCode::Char('é'), KeyModifiers::NONE), LineEnding::Cr),
            KeyAction::Send("é".as_bytes().to_vec())
        );
        assert_eq!(
            key_action(&key(KeyCode::Left, KeyModifiers::NONE), LineEnding::Cr),
            KeyAction::Ignore
        );
    }

    #[test]
    fn a_typed_line_gets_the_chosen_ending() {
        assert_eq!(with_ending("hello\n", LineEnding::Cr), b"hello\r");
        assert_eq!(with_ending("hello\r\n", LineEnding::Crlf), b"hello\r\n");
        assert_eq!(with_ending("hello", LineEnding::Lf), b"hello\n");
    }
}
