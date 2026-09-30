// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

pub mod error;
pub mod lab;
pub mod net;

pub use error::Error;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;

use onerom_config::fw::FirmwareProperties;
use onerom_gen::{Builder, FileData, FlashChips};
use onerom_gen::{FIRMWARE_SIZE, MAX_METADATA_LEN};

use net::{fetch_rom_file, fetch_rom_file_async};

pub fn validate_sizes(
    fw_props: &FirmwareProperties,
    firmware_data: &[u8],
    metadata: &Option<Vec<u8>>,
    image_data: &Option<Vec<u8>>,
) -> Result<(), Error> {
    let mut total_size = 0;

    let fw_size = firmware_data.len();
    debug!("Firmware size: {} bytes", fw_size);
    if fw_size > FIRMWARE_SIZE {
        return Err(Error::too_large(
            "Firmware".to_string(),
            fw_size,
            FIRMWARE_SIZE,
        ));
    }
    total_size += fw_size;

    if let Some(meta) = metadata {
        // Padding after firmware
        total_size += FIRMWARE_SIZE - total_size;

        let meta_size = meta.len();
        debug!("Metadata size: {} bytes", meta_size);
        if meta_size > MAX_METADATA_LEN {
            return Err(Error::too_large(
                "Metadata".to_string(),
                meta_size,
                MAX_METADATA_LEN,
            ));
        }
        total_size += meta_size;
    }

    if let Some(image) = image_data {
        // Padding after metadata
        total_size += MAX_METADATA_LEN + FIRMWARE_SIZE - total_size;

        let image_size = image.len();
        debug!("Image data size: {} bytes", image_size);
        total_size += image_size;
    }

    // An image for a board with a second flash chip holds the first chip's
    // contents and then the second chip's.
    let chips = FlashChips::new(fw_props.mcu_variant(), fw_props.board_size());
    let max_size = chips.first().len() + chips.second().map_or(0, |chip| chip.len());
    debug!(
        "Total firmware size: {} bytes (max {})",
        total_size, max_size
    );
    debug!("Flash size: {} bytes", max_size);
    if total_size > max_size {
        return Err(Error::too_large(
            "Total firmware".to_string(),
            total_size,
            max_size,
        ));
    }

    Ok(())
}

pub fn assemble_firmware(
    firmware_data: Vec<u8>,
    metadata: Option<Vec<u8>>,
    image_data: Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    let firmware_size = firmware_data.len();
    assert!(firmware_size <= FIRMWARE_SIZE);

    if metadata.is_none() {
        assert!(image_data.is_none());
        return Ok(firmware_data);
    }

    let metadata = metadata.unwrap();
    let metadata_size = metadata.len();
    assert!(metadata_size <= MAX_METADATA_LEN);

    let pad_fw = FIRMWARE_SIZE - firmware_size;

    let (image_data, pad_meta) = if let Some(image_data) = image_data {
        (image_data, MAX_METADATA_LEN - metadata_size)
    } else {
        (vec![], 0)
    };

    let total = FIRMWARE_SIZE + pad_meta + metadata_size + image_data.len();
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(&firmware_data);
    buf.extend(std::iter::repeat_n(0xFF, pad_fw));
    buf.extend_from_slice(&metadata);
    if pad_meta > 0 || !image_data.is_empty() {
        buf.extend(std::iter::repeat_n(0xFF, pad_meta));
        buf.extend_from_slice(&image_data);
    }

    Ok(buf)
}

pub fn create_firmware(
    out_path: &str,
    firmware_data: Vec<u8>,
    metadata: Option<Vec<u8>>,
    image_data: Option<Vec<u8>>,
) -> Result<usize, Error> {
    let buf = assemble_firmware(firmware_data, metadata, image_data)?;
    let size = buf.len();
    std::fs::write(out_path, &buf).map_err(|e| Error::write(out_path.to_string(), e))?;
    Ok(size)
}

pub fn get_rom_files(builder: &mut Builder) -> Result<(), Error> {
    // Get firmware files
    let file_specs = builder.file_specs();
    let mut cached_files: HashMap<String, Vec<u8>> = HashMap::new();
    for spec in file_specs {
        let source = spec.source;
        let extract = spec.extract;

        // See if we hae the file in our cache
        let cache = if let Some(data) = cached_files.get(&source) {
            data.as_slice()
        } else {
            &[]
        };

        let (data, cache) = fetch_rom_file(&source, cache, extract, true)?;

        builder
            .add_file(FileData::new(spec.id, data))
            .map_err(Error::build)?;

        // Cache the returned file
        if !cache.is_empty() {
            cached_files.insert(source, cache);
        }
    }

    Ok(())
}

pub async fn get_rom_files_async(builder: &mut Builder) -> Result<(), Error> {
    // Get firmware files
    let file_specs = builder.file_specs();
    let mut cached_files: HashMap<String, Vec<u8>> = HashMap::new();
    for spec in file_specs {
        let source = spec.source;
        let extract = spec.extract;

        // See if we have the file in our cache
        let cache = if let Some(data) = cached_files.get(&source) {
            data.as_slice()
        } else {
            &[]
        };

        let (data, cache) = fetch_rom_file_async(&source, cache, extract, true).await?;

        builder
            .add_file(FileData::new(spec.id, data))
            .map_err(Error::build)?;

        // Cache the returned file
        if !cache.is_empty() {
            cached_files.insert(source, cache);
        }
    }

    Ok(())
}

pub fn read_rom_config(rom_config_filename: &str) -> Result<String, Error> {
    // Load the config file
    std::fs::read_to_string(rom_config_filename)
        .map_err(|e| Error::read(rom_config_filename.to_string(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use onerom_config::fw::{FirmwareVersion, ServeAlg};
    use onerom_config::hw::{Board, BoardSize};
    use onerom_config::mcu::Variant;

    const MB: usize = 1024 * 1024;

    fn props(size: BoardSize) -> FirmwareProperties {
        FirmwareProperties::new(
            FirmwareVersion::new(0, 8, 0, 0),
            Board::Fire32B,
            Variant::RP2350,
            ServeAlg::Default,
            false,
        )
        .unwrap()
        .with_board_size(size)
    }

    /// Whether firmware, metadata and `image_len` bytes of ROM data fit a
    /// `size` board.
    fn fits(size: BoardSize, image_len: usize) -> bool {
        let result = validate_sizes(
            &props(size),
            &[0; 1024],
            &Some(vec![0; MAX_METADATA_LEN]),
            &Some(vec![0; image_len]),
        );
        match result {
            Ok(()) => true,
            Err(Error::TooLarge { .. }) => false,
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    /// ROM data follows the firmware and metadata regions and can fill the
    /// rest of the flash.
    #[test]
    fn an_m_board_takes_rom_data_to_the_end_of_its_one_chip() {
        let space = 2 * MB - FIRMWARE_SIZE - MAX_METADATA_LEN;
        assert!(fits(BoardSize::M, space));
        assert!(!fits(BoardSize::M, space + 1));
    }

    #[test]
    fn an_l_board_takes_rom_data_to_the_end_of_its_second_chip() {
        let space = 4 * MB - FIRMWARE_SIZE - MAX_METADATA_LEN;
        assert!(fits(BoardSize::L, space));
        assert!(!fits(BoardSize::L, space + 1));
        assert!(!fits(BoardSize::M, space));
    }
}
