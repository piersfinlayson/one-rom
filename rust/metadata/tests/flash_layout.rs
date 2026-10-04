// tests/flash_layout.rs
//
// Tests that the regions of the flash chip on chip select 0 don't overlap, and
// that MIN_FLASH_CS0_SIZE is the smallest chip holding them all. Tests that
// FLASH_LAYOUTS has one layout for each board size, with a first chip at least
// MIN_FLASH_CS0_SIZE and the total flash of its board size, and that a layout's
// board size comes from its total flash.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_config::hw::BoardSize;
use onerom_metadata::otp::{FLASH_LAYOUTS, FlashLayout, flash_size_bytes};
use onerom_metadata::{
    FIRMWARE_OFFSET, FIRMWARE_SIZE, METADATA_OFFSET, METADATA_SIZE, MIN_FLASH_CS0_SIZE,
    OneromFlashSize, SYSTEM_PLUGIN_OFFSET, SYSTEM_PLUGIN_SIZE, TOTAL_FLASH_SIZE_L,
    TOTAL_FLASH_SIZE_M, USER_PLUGIN_OFFSET, USER_PLUGIN_SIZE,
};

/// Every region, as its name, its offset from the start of flash and its size
/// in bytes.
const REGIONS: [(&str, usize, usize); 4] = [
    ("firmware", FIRMWARE_OFFSET as usize, FIRMWARE_SIZE),
    ("metadata", METADATA_OFFSET as usize, METADATA_SIZE),
    (
        "system plugin",
        SYSTEM_PLUGIN_OFFSET as usize,
        SYSTEM_PLUGIN_SIZE,
    ),
    ("user plugin", USER_PLUGIN_OFFSET as usize, USER_PLUGIN_SIZE),
];

#[test]
fn no_two_regions_overlap() {
    for (i, (a, a_start, a_size)) in REGIONS.iter().enumerate() {
        for (b, b_start, b_size) in &REGIONS[i + 1..] {
            assert!(
                a_start + a_size <= *b_start || b_start + b_size <= *a_start,
                "the {a} region ({a_size:#x} bytes at {a_start:#x}) overlaps the {b} region \
                 ({b_size:#x} bytes at {b_start:#x})"
            );
        }
    }
}

/// Chip sizes are powers of two, so the smallest chip holding every region is
/// the first power of two at or above the end of the last region.
#[test]
fn min_flash_cs0_size_is_the_smallest_chip_holding_every_region() {
    let end = REGIONS
        .iter()
        .map(|(_, start, size)| start + size)
        .max()
        .expect("there is at least one region");
    assert_eq!(
        MIN_FLASH_CS0_SIZE,
        end.next_power_of_two(),
        "the regions end at {end:#x}"
    );
}

#[test]
fn every_board_size_has_one_layout() {
    for &size in BoardSize::supported_values() {
        let rows = FLASH_LAYOUTS
            .iter()
            .filter(|(row_size, _)| *row_size == size);
        assert_eq!(rows.count(), 1, "{size}");
        assert_eq!(FlashLayout::of(size).board_size(), Some(size), "{size}");
    }
}

#[test]
fn every_layouts_first_chip_holds_every_region() {
    for (size, layout) in FLASH_LAYOUTS {
        let first = flash_size_bytes(layout.cs0);
        assert!(
            first >= MIN_FLASH_CS0_SIZE,
            "{size}'s first chip has {first:#x} bytes"
        );
    }
}

#[test]
fn every_layout_has_its_board_sizes_total_flash() {
    for (size, layout) in FLASH_LAYOUTS {
        let total = flash_size_bytes(layout.cs0) + flash_size_bytes(layout.cs1);
        let expected = match size {
            BoardSize::M => TOTAL_FLASH_SIZE_M,
            BoardSize::L => TOTAL_FLASH_SIZE_L,
        };
        assert_eq!(total, expected, "{size}");
    }
}

/// A layout's board size comes from its total flash, whether or not the tools
/// make that layout.
#[test]
fn a_layouts_board_size_comes_from_its_total_flash() {
    use OneromFlashSize::*;
    let cases = [
        (FlashSize1mb, FlashSize1mb, Some(BoardSize::M)),
        (FlashSize4mb, FlashSizeNone, Some(BoardSize::L)),
        (FlashSize2mb, FlashSize1mb, None),
        (FlashSize16mb, FlashSize16mb, None),
    ];
    for (cs0, cs1, size) in cases {
        let layout = FlashLayout { cs0, cs1 };
        assert_eq!(layout.board_size(), size, "{layout:?}");
    }
}
