// tests/otp_board_size.rs
//
// Tests the board size read from BOOT_FLAGS0 and FLASH_DEVINFO.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata::OneromBoardSize;
use onerom_metadata::otp::board_size;

/// FLASH_DEVINFO_ENABLE in BOOT_FLAGS0.
const ENABLE: u32 = 0x20;

/// The FLASH_DEVINFO a fire-40-a L board holds: 2MB on each chip select.
const L_DEVINFO: u16 = 0x99af;

/// Each BOOT_FLAGS0 bit comes from two of the three copies.
#[test]
fn most_copies_decide_whether_flash_devinfo_is_used() {
    let cases = [
        ([ENABLE; 3], OneromBoardSize::BoardSizeL),
        ([ENABLE, ENABLE, 0], OneromBoardSize::BoardSizeL),
        ([0, ENABLE, ENABLE], OneromBoardSize::BoardSizeL),
        ([ENABLE, 0, 0], OneromBoardSize::BoardSizeM),
        ([0; 3], OneromBoardSize::BoardSizeM),
    ];
    for (flags, size) in cases {
        assert_eq!(board_size(flags, L_DEVINFO), size, "{flags:x?}");
    }
}

/// With FLASH_DEVINFO in use, the total size of the two chips decides the
/// board size.
#[test]
fn the_total_flash_decides_the_size() {
    let cases = [
        // 2MB on each chip select.
        (L_DEVINFO, OneromBoardSize::BoardSizeL),
        // 2MB and no chip.
        (0x09af, OneromBoardSize::BoardSizeM),
        // 4MB and no chip.
        (0x0aaf, OneromBoardSize::BoardSizeL),
        // 2MB and 4MB.
        (0xa9af, OneromBoardSize::BoardSizeOther),
        // 2MB and 16MB.
        (0xc9af, OneromBoardSize::BoardSizeOther),
        // 2MB and 8KB.
        (0x19af, OneromBoardSize::BoardSizeOther),
        // 2MB and a code above 16MB, which counts as no chip.
        (0xd9af, OneromBoardSize::BoardSizeM),
    ];
    for (devinfo, size) in cases {
        assert_eq!(board_size([ENABLE; 3], devinfo), size, "{devinfo:#06x}");
    }
}
