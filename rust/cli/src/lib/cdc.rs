// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The USB CDC serial port a running One ROM presents.
//!
//! A One ROM running the USB system plugin enumerates a CDC ACM interface
//! alongside the vendor interface picoboot uses. The plugin forwards One ROM's
//! log into that port's IN endpoint while a terminal has the port open, so
//! reading it is how the CLI shows firmware and plugin logging.
//!
//! The port is reached through the operating system's own CDC class driver -
//! cdc_acm, AppleUSBCDCACM or usbser - rather than by claiming the interface
//! over USB directly, because that driver owns the interface on every platform
//! the CLI ships for.
//!
//! Two things gate what arrives:
//!
//! - The device forwards only while DTR is asserted, which is what marks a
//!   terminal as having the port open. [`stream`] asserts it explicitly rather
//!   than relying on the platform doing so at open.
//! - A debug probe reading the log over SWD consumes the same bytes. Both
//!   advance the same read position, so a probe and this command running at
//!   once split the stream arbitrarily between them and neither sees all of it.

use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use log::warn;
use serialport::{SerialPortInfo, SerialPortType};

use crate::device::Device;
use crate::error::Error;

/// How long a freshly attached port may stay silent before the caller is told.
///
/// The USB plugin writes a banner when a terminal opens the port, so nothing at
/// all within this window means the bytes are not reaching us - the wrong port,
/// a plugin that does not forward, or another consumer holding the log.
pub const SILENCE_TIMEOUT: Duration = Duration::from_secs(2);

/// Line speed requested when opening the port.
///
/// CDC ACM carries no line at all, so the device ignores this, but the platform
/// APIs all require a rate to open with.
const BAUD: u32 = 115_200;

/// How long a read waits before returning so the silence check can run.
const READ_TIMEOUT: Duration = Duration::from_millis(100);

/// Bytes read per pass.
const READ_CHUNK: usize = 256;

/// Find the serial port `device` presents.
///
/// Returns [`Error::SerialPortNotFound`] if the device has no port, which is
/// what a One ROM running a USB plugin that predates the CDC interface looks
/// like from here.
pub fn find_port(device: &Device) -> Result<String, Error> {
    let ports = serialport::available_ports()
        .map_err(|e| Error::SerialPort(format!("Failed to list serial ports: {e}")))?;

    let mut matches = select_ports(&ports, device.vid, device.pid, device.serial.as_deref());

    match matches.len() {
        0 => Err(Error::SerialPortNotFound(device.to_string())),
        1 => Ok(matches.remove(0)),
        _ => {
            // Only reachable with two devices presenting the same serial, which
            // makes every other command ambiguous too. Say which was taken
            // rather than choosing in silence.
            warn!(
                "More than one serial port matches this One ROM, using the first: {}",
                matches.join(", ")
            );
            Ok(matches.remove(0))
        }
    }
}

/// Pick the ports belonging to one USB device out of an enumerated list.
///
/// Matching is on vendor, product and serial. Serial comparison ignores case
/// because the platforms differ in how they render the string and a One ROM
/// serial is hex, so case can never be what distinguishes two of them.
///
/// A device with no serial is matched on vendor and product alone. Nothing else
/// is available to tell two apart, and refusing outright would be worse than
/// naming the ambiguity, which the caller does.
///
/// macOS lists both nodes of each port, the callout `/dev/cu.*` and the dialin
/// `/dev/tty.*`. Only the callout node is usable: opening the dialin node
/// blocks until carrier is asserted, which a CDC device never does, so the
/// command would hang rather than fail. Dropping names beginning `tty.` leaves
/// Linux untouched, where the node is `/dev/ttyACM0` and has no dot.
fn select_ports(ports: &[SerialPortInfo], vid: u16, pid: u16, serial: Option<&str>) -> Vec<String> {
    ports
        .iter()
        .filter(|port| {
            let SerialPortType::UsbPort(info) = &port.port_type else {
                return false;
            };
            if info.vid != vid || info.pid != pid {
                return false;
            }
            match (serial, info.serial_number.as_deref()) {
                (Some(wanted), Some(found)) => wanted.eq_ignore_ascii_case(found),
                (Some(_), None) => false,
                (None, _) => true,
            }
        })
        .map(|port| port.port_name.clone())
        .filter(|name| !is_dialin_node(name))
        .collect()
}

/// Whether a port name is a macOS dialin node.
fn is_dialin_node(port_name: &str) -> bool {
    Path::new(port_name)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("tty."))
}

/// Open `port_name`, assert DTR, and copy what the device sends to stdout until
/// it goes away.
///
/// Returns the number of bytes copied. Returning `Ok` at all means the session
/// ended in one of its two normal ways: the device went away - unplugged,
/// rebooted or stopped - or `stop` was set.
///
/// `stop` is how a caller ends a session it started, and is read once per read
/// timeout rather than continuously, so it takes effect within `READ_TIMEOUT`.
/// It exists so that a Ctrl-C leaves through this function rather than taking
/// the process with it, which is what lets the caller report what was copied.
///
/// Device bytes go to stdout untouched, so the stream can be redirected on its
/// own. When `capture` is given the same bytes are also written there, and
/// nothing else is: the file is a transcript of the device, not of this
/// command.
///
/// Blocks for as long as the device is attached, so callers on an async runtime
/// need [`tokio::task::spawn_blocking`] or equivalent.
///
/// Returns [`Error::LogSilent`] if nothing arrives within `silence` of the port
/// being opened. The check is armed until the first byte and not afterwards - a
/// device that has said its piece and gone quiet is working normally.
pub fn stream(
    port_name: &str,
    capture: Option<&Path>,
    silence: Duration,
    stop: &AtomicBool,
) -> Result<u64, Error> {
    let mut port = serialport::new(port_name, BAUD)
        .timeout(READ_TIMEOUT)
        .open()
        .map_err(|e| Error::SerialPortOpen(port_name.to_string(), e.to_string()))?;

    // The device forwards only while DTR is asserted. Every platform raises it
    // at open, but this is the whole reason the stream flows, so say it rather
    // than inherit it.
    port.write_data_terminal_ready(true)
        .map_err(|e| Error::SerialPortOpen(port_name.to_string(), e.to_string()))?;

    // Deliberately unbuffered. Ctrl-C is how a session normally ends, and it
    // takes the process with it, so anything held back here would be lost from
    // the transcript.
    let mut capture_file = match capture {
        Some(path) => Some(
            std::fs::File::create(path)
                .map_err(|e| Error::Io(format!("Failed to create {}: {e}", path.display())))?,
        ),
        None => None,
    };

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    let mut buf = [0u8; READ_CHUNK];
    let opened = Instant::now();
    let mut copied: u64 = 0;

    loop {
        match port.read(&mut buf) {
            Ok(0) => return Ok(copied),
            Ok(count) => {
                copied += count as u64;
                let bytes = &buf[..count];

                // Flush every pass. A log line the device wrote is worth
                // nothing to the person watching if it sits here until the next
                // one fills the buffer.
                stdout
                    .write_all(bytes)
                    .and_then(|()| stdout.flush())
                    .map_err(|e| Error::Io(format!("Failed to write to stdout: {e}")))?;

                if let Some(file) = capture_file.as_mut() {
                    file.write_all(bytes)
                        .map_err(|e| Error::Io(format!("Failed to write capture file: {e}")))?;
                }
            }
            Err(e) if e.kind() == ErrorKind::TimedOut => {
                // Before the silence check, so that stopping a session inside
                // the attach window ends it rather than reporting a fault the
                // user did not wait to find out about.
                if stop.load(Ordering::Relaxed) {
                    return Ok(copied);
                }
                if copied == 0 && opened.elapsed() >= silence {
                    return Err(Error::LogSilent(silence.as_secs_f32()));
                }
            }
            // Anything else is the port going away under us, which is a device
            // that has been unplugged, rebooted or stopped.
            Err(_) => return Ok(copied),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serialport::UsbPortInfo;

    const VID: u16 = 0x1209;
    const PID: u16 = 0xF542;

    fn usb_port(name: &str, vid: u16, pid: u16, serial: Option<&str>) -> SerialPortInfo {
        SerialPortInfo {
            port_name: name.to_string(),
            port_type: SerialPortType::UsbPort(UsbPortInfo {
                vid,
                pid,
                serial_number: serial.map(str::to_string),
                manufacturer: None,
                product: None,
                interface: None,
            }),
        }
    }

    fn other_port(name: &str) -> SerialPortInfo {
        SerialPortInfo {
            port_name: name.to_string(),
            port_type: SerialPortType::Unknown,
        }
    }

    #[test]
    fn picks_the_port_with_the_matching_serial() {
        let ports = [
            usb_port("/dev/ttyACM0", VID, PID, Some("deadbeef")),
            usb_port("/dev/ttyACM1", VID, PID, Some("cafef00d")),
        ];
        assert_eq!(
            select_ports(&ports, VID, PID, Some("cafef00d")),
            vec!["/dev/ttyACM1".to_string()]
        );
    }

    #[test]
    fn ignores_serial_case() {
        let ports = [usb_port("/dev/ttyACM0", VID, PID, Some("DEADBEEF"))];
        assert_eq!(
            select_ports(&ports, VID, PID, Some("deadbeef")),
            vec!["/dev/ttyACM0".to_string()]
        );
    }

    #[test]
    fn rejects_a_different_device() {
        let ports = [
            usb_port("/dev/ttyACM0", 0x2e8a, 0x000f, Some("deadbeef")),
            other_port("/dev/ttyS0"),
        ];
        assert!(select_ports(&ports, VID, PID, Some("deadbeef")).is_empty());
    }

    #[test]
    fn rejects_a_matching_device_with_the_wrong_serial() {
        let ports = [usb_port("/dev/ttyACM0", VID, PID, Some("deadbeef"))];
        assert!(select_ports(&ports, VID, PID, Some("cafef00d")).is_empty());
    }

    /// The dialin node must never be chosen - opening it hangs.
    #[test]
    fn takes_the_macos_callout_node_not_the_dialin_node() {
        let ports = [
            usb_port("/dev/tty.usbmodem1103", VID, PID, Some("deadbeef")),
            usb_port("/dev/cu.usbmodem1103", VID, PID, Some("deadbeef")),
        ];
        assert_eq!(
            select_ports(&ports, VID, PID, Some("deadbeef")),
            vec!["/dev/cu.usbmodem1103".to_string()]
        );
    }

    /// Linux nodes start with `tty` and must survive the dialin filter.
    #[test]
    fn keeps_the_linux_node() {
        let ports = [usb_port("/dev/ttyACM0", VID, PID, Some("deadbeef"))];
        assert_eq!(
            select_ports(&ports, VID, PID, Some("deadbeef")),
            vec!["/dev/ttyACM0".to_string()]
        );
    }

    #[test]
    fn matches_on_vid_pid_when_the_device_has_no_serial() {
        let ports = [
            usb_port("/dev/ttyACM0", VID, PID, Some("deadbeef")),
            usb_port("/dev/ttyACM1", 0x2e8a, 0x000f, None),
        ];
        assert_eq!(
            select_ports(&ports, VID, PID, None),
            vec!["/dev/ttyACM0".to_string()]
        );
    }
}
