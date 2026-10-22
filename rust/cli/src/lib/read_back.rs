// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Reading a One ROM back over USB into the config that builds it, or reading
//! one slot's ROMs.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use onerom_app::{ReadBack, ReadBackError, RecoveryProblem, known_board_size};
use onerom_fw_parser::{ParsedDevice, Parser, Reader};
use onerom_gen::{Config, INLINE_BASE64};
use picoboot::{Picoboot, Reader as PicobootReader};

use crate::usb::{FLASH_BASE, RAM_BASE, READ_CHUNK_BYTES, get_picoboot, read_board_size};
use crate::{Device, Error};

/// Reads `device` back into the config that builds it.
pub async fn read_back(device: &Device) -> Result<ReadBack, Error> {
    let parsed = parsed(device)?;
    let mut reader = reader(device).await?;
    onerom_app::read_back(parsed, &mut reader, known_board_size(device.board_size()))
        .await
        .map_err(device_read_back_error)
}

/// Reads the ROMs in `device`'s ROM slot `slot`, numbered from 0 without
/// plugins. Only that slot is read.
pub async fn read_slot_roms(device: &Device, slot: usize) -> Result<Vec<Vec<u8>>, Error> {
    let parsed = parsed(device)?;
    let mut reader = reader(device).await?;
    onerom_app::read_slot_roms(parsed, &mut reader, slot)
        .await
        .map_err(device_read_back_error)
}

/// Reads the One ROM on `picoboot` back into the config that builds it.
pub async fn read_back_picoboot(picoboot: &Picoboot) -> Result<ReadBack, Error> {
    let size = known_board_size(read_board_size(picoboot).await);
    let reader = PicobootReader::new(picoboot.clone())
        .await
        .map_err(Error::Usb)?;
    let mut reader = ChunkedReader(reader);
    let parsed = Parser::with_base_flash_address(&mut reader, FLASH_BASE, RAM_BASE)
        .parse_device()
        .await;
    onerom_app::read_back(&parsed, &mut reader, size)
        .await
        .map_err(device_read_back_error)
}

/// `read_back`'s config with each file `read_back` contains inlined as base64,
/// which `onerom_fw` reads as the file.
///
/// Change the result, not `read_back`'s config. A chip changed first to a
/// file with the same name as a read-back file would be inlined with the
/// read-back file's bytes.
pub fn inline_files(read_back: &ReadBack) -> Config {
    let mut config = read_back.config.clone();
    for chip in config
        .chip_sets
        .iter_mut()
        .flat_map(|set| set.chips.iter_mut())
    {
        if let Some(data) = read_back.file(&chip.file) {
            chip.file = format!("{INLINE_BASE64}{}", BASE64.encode(data));
        }
    }
    config
}

/// The error for reading the firmware file `file` back.
pub fn read_back_error<E: std::fmt::Debug>(file: &str, error: ReadBackError<E>) -> Error {
    read_error(file, file, error)
}

/// The error for reading the One ROM back.
pub fn device_read_back_error<E: std::fmt::Debug>(error: ReadBackError<E>) -> Error {
    read_error("the One ROM", "The One ROM", error)
}

/// The error for reading back `what`, which is `subject` at the start of a
/// sentence.
fn read_error<E: std::fmt::Debug>(what: &str, subject: &str, error: ReadBackError<E>) -> Error {
    match error {
        ReadBackError::NoSlot { slot, slots } => Error::NoSlot { slot, slots },
        ReadBackError::NoImage { slot } => Error::NoImage { slot },
        ReadBackError::FirmwareTooNew { version } => Error::FirmwareTooNew {
            what: subject.to_string(),
            version,
        },
        error @ (ReadBackError::FileNotOneRom
        | ReadBackError::FlashNotOneRom
        | ReadBackError::TooOld { .. }
        | ReadBackError::Unparsed
        | ReadBackError::Read(_)
        | ReadBackError::ReadFirmware { .. }
        | ReadBackError::Recover(_)
        | ReadBackError::Unscramble { .. }
        | _) => Error::ReadBack {
            what: what.to_string(),
            reason: error.to_string(),
        },
    }
}

/// The error for a setting `read_back`'s config couldn't recover.
pub fn problem_error(read_back: &ReadBack, problem: &RecoveryProblem) -> Error {
    let subject = read_back.problem_subject(problem);
    // A slot's chip-select settings are plural, an override singular.
    let verb = match problem {
        RecoveryProblem::ChipSelect { .. } => "aren't",
        RecoveryProblem::Override { .. } | _ => "isn't",
    };
    Error::NotRecognised(format!("{subject} {verb} recognised"))
}

fn parsed(device: &Device) -> Result<&ParsedDevice, Error> {
    device
        .onerom()
        .ok_or_else(|| device_read_back_error(ReadBackError::<String>::FlashNotOneRom))
}

async fn reader(device: &Device) -> Result<ChunkedReader, Error> {
    let picoboot = get_picoboot(device, false).await?;
    let reader = PicobootReader::new(picoboot).await.map_err(Error::Usb)?;
    Ok(ChunkedReader(reader))
}

/// Reads a One ROM's memory [`READ_CHUNK_BYTES`] at a time.
struct ChunkedReader(PicobootReader);

impl Reader for ChunkedReader {
    type Error = String;

    async fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), Self::Error> {
        let mut chunk_addr = addr;
        for chunk in buf.chunks_mut(READ_CHUNK_BYTES as usize) {
            self.0.read(chunk_addr, chunk).await?;
            chunk_addr += chunk.len() as u32;
        }
        Ok(())
    }

    fn update_base_address(&mut self, new_base: u32) {
        self.0.update_base_address(new_base);
    }
}

#[cfg(test)]
mod tests {
    use onerom_config::fw::FirmwareVersion;

    use super::*;

    /// Firmware newer than this build supports is reported with what was read
    /// as the subject and the firmware's version.
    #[test]
    fn newer_firmware_is_reported_with_its_version() {
        let version = FirmwareVersion::new(0, 8, 2, 0);
        let newer = || ReadBackError::<String>::FirmwareTooNew { version };
        for (e, subject) in [
            (device_read_back_error(newer()), "The One ROM"),
            (read_back_error("new.bin", newer()), "new.bin"),
        ] {
            assert!(
                matches!(&e, Error::FirmwareTooNew { what, version: v } if what == subject && *v == version),
                "{e:?}"
            );
        }
    }
}
