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
use onerom_gen::{ChipConfig, SizeHandling, Transform, num_excess_addr_lines};

/// Apply a chip's `transform` list to its loaded image.
///
/// # Panics
/// Panics on a transform this tester does not model, or on an image the
/// transform cannot be applied to.  The odd-length `swap_bytes` case is
/// deliberately unsupported: resolving it depends on `size_handling`, which is
/// applied later, and no tester image needs it.
fn apply_transforms(raw: &[u8], transforms: &[Transform], source: &str) -> Vec<u8> {
    let mut data = raw.to_vec();

    for transform in transforms {
        data = match transform {
            Transform::SwapBytes => {
                assert!(
                    data.len().is_multiple_of(2),
                    "ROM image '{source}': swap_bytes needs an even-length image, got {} bytes \
                     (the odd-length size_handling interaction is not modelled by the tester)",
                    data.len(),
                );
                data.chunks_exact(2).flat_map(|w| [w[1], w[0]]).collect()
            }

            Transform::Deinterleave {
                offset,
                stride,
                bytes,
            } => {
                assert!(
                    *bytes >= 1,
                    "ROM image '{source}': deinterleave lane width is 0"
                );
                assert!(
                    *stride >= 2,
                    "ROM image '{source}': deinterleave stride is {stride}"
                );
                assert!(
                    offset < stride,
                    "ROM image '{source}': deinterleave offset {offset} is not below stride {stride}"
                );
                assert!(
                    data.len().is_multiple_of(bytes * stride),
                    "ROM image '{source}': {} bytes is not a multiple of the {} byte \
                     deinterleave group",
                    data.len(),
                    bytes * stride,
                );

                data.chunks_exact(*bytes)
                    .enumerate()
                    .filter(|(i, _)| i % stride == *offset)
                    .flat_map(|(_, lane)| lane.iter().copied())
                    .collect()
            }

            other => panic!(
                "ROM image '{source}': transform '{other}' is not modelled by the firmware tester"
            ),
        };
    }

    data
}

/// Number of bytes One ROM actually serves for `chip_type`.
///
/// This is `chip_type.size_bytes()` for every chip whose address space fits in
/// `MAX_IMAGE_SIZE`. Above that, each excess top address line is repurposed as
/// a half-select and halves what one board serves: the 27C080 is the only such
/// chip today, where a single board serves the lower or upper 512 KB of its
/// 1 MB space (A19 becomes the chip select), so two stacked boards — one with
/// cs1 active_low, one active_high — together serve the whole device.
///
/// The excess count is `onerom_gen`'s own, shared rather than recomputed, so
/// the tester cannot disagree with the firmware it is testing about how big the
/// served region is.
///
/// This is the authority for "how many bytes does this chip serve" — used by
/// the oracle to size its expected image, and by the address-range and
/// reprogram-length tests so they stay within the served region.
pub fn served_size(chip_type: ChipType) -> usize {
    chip_type.size_bytes() >> num_excess_addr_lines(&chip_type)
}

/// Load and size-adjust the oracle bytes for `chip_config`.
///
/// Returns a `Vec<u8>` whose length equals the number of bytes served (see
/// [`served_size`]).
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

    // Transforms rearrange the image before size handling, mirroring the
    // generator's pipeline.  Written out here rather than called from
    // `onerom-gen` so the oracle stays an independent statement of what the
    // firmware should serve, in the same way size handling is below.
    let raw = apply_transforms(&raw, &chip_config.transform, &source);

    // 27C080: one board serves only the lower 512 KB of the 1 MB space.
    let target = served_size(chip_type);

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
