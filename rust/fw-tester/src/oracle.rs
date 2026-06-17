// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Oracle: expected byte sequence for a given chip configuration.
//!
//! Loads the raw ROM image from disk or URL and applies size handling to
//! produce exactly the number of bytes the chip serves.  The result is the
//! ground truth against which every served byte is compared.

use onerom_config::chip::ChipType;
use onerom_fw::net::fetch_rom_file;
use onerom_gen::{ChipConfig, SizeHandling};

/// Load and size-adjust the oracle bytes for `chip_config`.
///
/// Returns a `Vec<u8>` whose length equals the number of bytes served:
/// - `chip_type.size_bytes()` for all chips except 27C080.
/// - `chip_type.size_bytes() / 2` for 27C080, because a single One ROM
///   board only serves the lower half of that chip's address space.
///
/// # Panics
/// Panics on I/O failure, unsupported `location` field, size mismatches
/// inconsistent with `size_handling`, or a source that cannot satisfy the
/// requested size handling.
pub fn load(chip_config: &ChipConfig, chip_type: ChipType, base_dir: &std::path::Path) -> Vec<u8> {
    if chip_config.location.is_some() {
        panic!(
            "ROM image '{}': location-based extraction is not supported by the firmware tester",
            chip_config.file
        );
    }

    let source =
        if chip_config.file.starts_with("http://") || chip_config.file.starts_with("https://") {
            chip_config.file.clone()
        } else {
            base_dir
                .join(&chip_config.file)
                .to_string_lossy()
                .into_owned()
        };

    let (raw, _) = fetch_rom_file(&source, &[], chip_config.extract.clone(), false)
        .unwrap_or_else(|e| panic!("Failed to load ROM image '{}': {}", source, e));

    // 27C080: one board serves only the lower 512 KB of the 1 MB space.
    let target = if chip_type == ChipType::Chip27C080 {
        chip_type.size_bytes() / 2
    } else {
        chip_type.size_bytes()
    };

    match chip_config.size_handling {
        SizeHandling::None => {
            assert_eq!(
                raw.len(),
                target,
                "ROM image '{}' is {} bytes; {} expects exactly {} bytes \
                 (use size_handling to override)",
                source,
                raw.len(),
                chip_type.name(),
                target,
            );
            raw
        }

        SizeHandling::Truncate => {
            assert!(
                raw.len() >= target,
                "ROM image '{}' is {} bytes — too small to truncate to {} bytes for {}",
                source,
                raw.len(),
                target,
                chip_type.name(),
            );
            raw[..target].to_vec()
        }

        SizeHandling::Duplicate => {
            assert_eq!(
                target % raw.len(),
                0,
                "ROM image '{}' ({} bytes) does not divide evenly into \
                 {} bytes for {} (size_handling = duplicate)",
                source,
                raw.len(),
                target,
                chip_type.name(),
            );
            raw.iter().copied().cycle().take(target).collect()
        }

        SizeHandling::Pad => {
            assert!(
                raw.len() <= target,
                "ROM image '{}' ({} bytes) is larger than {} bytes for {} \
                 — use truncate, not pad",
                source,
                raw.len(),
                target,
                chip_type.name(),
            );
            let mut result = raw;
            result.resize(target, 0xAA);
            result
        }
    }
}
