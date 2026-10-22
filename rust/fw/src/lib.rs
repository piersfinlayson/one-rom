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

use onerom_gen::{Builder, FileData};

use net::{fetch_rom_file, fetch_rom_file_async};

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
