// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM's log, read from its RAM in exclusive serial IN mode.
//!
//! Reading the log consumes it.
//!
//! picoboot is released between reads so other commands can use the device.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use log::debug;
use onerom_fw_parser::ParsedDevice;
use onerom_metadata::{OneromFirmwareFlag, Pointer};
use picoboot::Picoboot;

use crate::device::Device;
use crate::error::Error;
use crate::usb::{AccessError, AccessFailure, get_picoboot, read_memory_on, write_memory_on};

const POLL: Duration = Duration::from_millis(100);

// SEGGER's RTT control block layout.
const RTT_ID: &[u8] = b"SEGGER RTT\0";
const RTT_MAX_UP_OFFSET: usize = 16;
const RTT_HEADER_LEN: u32 = 24;
const RTT_DESC_LEN: u32 = 24;
const RTT_DESC_BUFFER_OFFSET: usize = 4;
const RTT_DESC_SIZE_OFFSET: usize = 8;
const RTT_DESC_WRITE_OFFSET: usize = 12;
const RTT_DESC_READ_OFFSET: usize = 16;

const RTT_LOG_UP: u32 = 1;

pub fn read_log_from_ram(device: &Device) -> bool {
    device
        .onerom()
        .and_then(ParsedDevice::as_schema)
        .and_then(|onerom| onerom.runtime())
        .is_some_and(|runtime| log_in_up1(runtime.firmware_flags))
}

fn log_in_up1(flags: OneromFirmwareFlag) -> bool {
    flags.log_in_up1 == Some(true)
}

/// Copy One ROM's log from RAM to stdout and `capture` until a read fails or
/// `stop` is set. Returns the number of bytes copied.
pub async fn stream(
    device: &Device,
    capture: Option<&Path>,
    stop: &AtomicBool,
) -> Result<u64, Error> {
    let mut picoboot = get_picoboot(device, false).await?;
    let log = RamLog::open(device, &mut picoboot).await?;
    let mut bytes = log.take(device, &mut picoboot).await?.unwrap_or_default();

    let mut capture_file = match capture {
        Some(path) => Some(
            std::fs::File::create(path)
                .map_err(|e| Error::Io(format!("Failed to create {}: {e}", path.display())))?,
        ),
        None => None,
    };
    let mut copied: u64 = 0;

    loop {
        if !bytes.is_empty() {
            copied += bytes.len() as u64;
            let mut stdout = std::io::stdout().lock();
            stdout
                .write_all(&bytes)
                .and_then(|()| stdout.flush())
                .map_err(|e| Error::Io(format!("Failed to write to stdout: {e}")))?;
            if let Some(file) = capture_file.as_mut() {
                file.write_all(&bytes)
                    .map_err(|e| Error::Io(format!("Failed to write capture file: {e}")))?;
            }
        }

        if stop.load(Ordering::Relaxed) {
            return Ok(copied);
        }
        tokio::time::sleep(POLL).await;

        bytes = match log.take(device, &mut picoboot).await {
            Ok(bytes) => bytes.unwrap_or_default(),
            Err(e) => {
                debug!("Stopped reading the log: {e}");
                return Ok(copied);
            }
        };
    }
}

struct RamLog {
    /// Address of the log's up buffer descriptor.
    desc: u32,
}

impl RamLog {
    async fn open(device: &Device, picoboot: &mut Picoboot) -> Result<Self, Error> {
        let rtt = device
            .onerom()
            .and_then(|onerom| onerom.as_schema())
            .and_then(|onerom| onerom.info())
            .map(|info| info.rtt);
        let Some(Pointer::Addr32(cb)) = rtt else {
            return Err(Error::LogNotFound(
                "The firmware doesn't provide the log's location.".to_string(),
            ));
        };

        let header = read_memory_on(picoboot, device, cb, RTT_HEADER_LEN).await;
        picoboot.disconnect();
        if !header_is_rtt(&header?) {
            return Err(Error::LogNotFound(format!(
                "RAM at {cb:#010x} doesn't contain the log's control block."
            )));
        }

        Ok(Self {
            desc: cb + RTT_HEADER_LEN + RTT_LOG_UP * RTT_DESC_LEN,
        })
    }

    /// Returns `None` when another program has the device's picoboot interface
    /// open.
    async fn take(
        &self,
        device: &Device,
        picoboot: &mut Picoboot,
    ) -> Result<Option<Vec<u8>>, Error> {
        if let Err(e) = picoboot.connect().await {
            if AccessError::classify(&e) == AccessFailure::NotOpened && still_present(device).await
            {
                debug!("Another program has the device: {e}");
                return Ok(None);
            }
            return Err(Error::Usb(e.to_string()));
        }
        let bytes = self.take_connected(device, picoboot).await;
        picoboot.disconnect();
        bytes.map(Some)
    }

    async fn take_connected(
        &self,
        device: &Device,
        picoboot: &mut Picoboot,
    ) -> Result<Vec<u8>, Error> {
        let desc = read_memory_on(picoboot, device, self.desc, RTT_DESC_LEN).await?;
        let ring = Ring::parse(&desc)?;

        let mut bytes = Vec::new();
        for (offset, len) in ring.spans() {
            let span = read_memory_on(picoboot, device, ring.buffer + offset, len).await?;
            bytes.extend_from_slice(&span);
        }

        // Written after the copy so One ROM can't overwrite bytes before
        // they're read.
        if !bytes.is_empty() {
            let read_offset = self.desc + RTT_DESC_READ_OFFSET as u32;
            write_memory_on(picoboot, device, read_offset, &ring.write.to_le_bytes()).await?;
        }
        Ok(bytes)
    }
}

async fn still_present(device: &Device) -> bool {
    let id = device.device_info.id();
    match nusb::list_devices().await {
        Ok(mut devices) => devices.any(|info| info.id() == id),
        Err(e) => {
            debug!("Failed to list USB devices: {e}");
            false
        }
    }
}

fn header_is_rtt(header: &[u8]) -> bool {
    header.starts_with(RTT_ID) && u32_at(header, RTT_MAX_UP_OFFSET) > RTT_LOG_UP
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ring {
    buffer: u32,
    size: u32,
    write: u32,
    read: u32,
}

impl Ring {
    fn parse(desc: &[u8]) -> Result<Self, Error> {
        let buffer = u32_at(desc, RTT_DESC_BUFFER_OFFSET);
        let size = u32_at(desc, RTT_DESC_SIZE_OFFSET);
        let write = u32_at(desc, RTT_DESC_WRITE_OFFSET);
        let mut read = u32_at(desc, RTT_DESC_READ_OFFSET);

        if buffer == 0 || size < 2 {
            return Err(Error::LogNotFound(format!(
                "The log's control block has a buffer of {size} bytes at {buffer:#010x}."
            )));
        }
        if write >= size {
            return Err(Error::LogNotFound(format!(
                "The log's write offset {write} is outside its {size} bytes."
            )));
        }
        // A debug probe also writes the read offset. The firmware clamps it the
        // same way.
        if read >= size {
            read = 0;
        }

        Ok(Self {
            buffer,
            size,
            write,
            read,
        })
    }

    fn spans(&self) -> Vec<(u32, u32)> {
        let spans = if self.read <= self.write {
            [(self.read, self.write - self.read), (0, 0)]
        } else {
            [(self.read, self.size - self.read), (0, self.write)]
        };
        spans.into_iter().filter(|&(_, len)| len > 0).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_is_in_ram_only_where_the_flag_is_set() {
        use onerom_config::fw::FirmwareVersion;
        use onerom_metadata::onerom_firmware_flag_t;

        let flag = onerom_firmware_flag_t::FIRMWARE_FLAG_LOG_IN_UP1;
        let v0_8_1 = Some(FirmwareVersion::new(0, 8, 1, 0));
        let v0_8_0 = Some(FirmwareVersion::new(0, 8, 0, 0));
        assert!(log_in_up1(OneromFirmwareFlag::from_raw(flag, v0_8_1)));
        assert!(!log_in_up1(OneromFirmwareFlag::from_raw(0, v0_8_1)));
        // Firmware before v0.8.1 doesn't have the flag.
        assert!(!log_in_up1(OneromFirmwareFlag::from_raw(flag, v0_8_0)));
        assert!(!log_in_up1(OneromFirmwareFlag::from_raw(flag, None)));
    }

    fn rtt_header(id: &[u8], max_up: u32) -> Vec<u8> {
        let mut header = vec![0u8; RTT_HEADER_LEN as usize];
        header[..id.len()].copy_from_slice(id);
        header[RTT_MAX_UP_OFFSET..RTT_MAX_UP_OFFSET + 4].copy_from_slice(&max_up.to_le_bytes());
        header
    }

    #[test]
    fn a_control_block_needs_the_id_and_the_logs_up_buffer() {
        assert!(header_is_rtt(&rtt_header(b"SEGGER RTT\0", 3)));
        assert!(header_is_rtt(&rtt_header(b"SEGGER RTT\0", 2)));
        assert!(!header_is_rtt(&rtt_header(b"SEGGER RTT\0", 1)));
        assert!(!header_is_rtt(&rtt_header(b"SEGGER RTX\0", 3)));
        // The identifier is written last so it's empty mid-way through
        // initialisation.
        assert!(!header_is_rtt(&rtt_header(b"\0", 3)));
    }

    fn desc(buffer: u32, size: u32, write: u32, read: u32) -> Vec<u8> {
        let mut desc = vec![0u8; RTT_DESC_LEN as usize];
        for (at, value) in [
            (RTT_DESC_BUFFER_OFFSET, buffer),
            (RTT_DESC_SIZE_OFFSET, size),
            (RTT_DESC_WRITE_OFFSET, write),
            (RTT_DESC_READ_OFFSET, read),
        ] {
            desc[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        desc
    }

    #[test]
    fn a_descriptor_is_checked_and_its_read_offset_clamped() {
        let ring = Ring::parse(&desc(0x5010_0800, 0x800, 10, 4)).unwrap();
        assert_eq!(
            ring,
            Ring {
                buffer: 0x5010_0800,
                size: 0x800,
                write: 10,
                read: 4
            }
        );
        assert_eq!(
            Ring::parse(&desc(0x5010_0800, 0x800, 10, 0x800))
                .unwrap()
                .read,
            0
        );
        assert!(Ring::parse(&desc(0, 0x800, 0, 0)).is_err());
        assert!(Ring::parse(&desc(0x5010_0800, 1, 0, 0)).is_err());
        assert!(Ring::parse(&desc(0x5010_0800, 0x800, 0x800, 0)).is_err());
    }

    fn ring(size: u32, write: u32, read: u32) -> Ring {
        Ring {
            buffer: 0x2000_0000,
            size,
            write,
            read,
        }
    }

    #[test]
    fn the_unread_bytes_split_where_they_wrap() {
        assert_eq!(ring(16, 10, 4).spans(), vec![(4, 6)]);
        assert_eq!(ring(16, 10, 10).spans(), vec![]);
        assert_eq!(ring(16, 3, 12).spans(), vec![(12, 4), (0, 3)]);
        assert_eq!(ring(16, 0, 12).spans(), vec![(12, 4)]);
    }
}
