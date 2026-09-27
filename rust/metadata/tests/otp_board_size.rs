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

/// With FLASH_DEVINFO in use, chip select 1's size decides the board size.
#[test]
fn chip_select_1_decides_the_size() {
    let cases = [
        (L_DEVINFO, OneromBoardSize::BoardSizeL),
        (0x09af, OneromBoardSize::BoardSizeM),
        (0xc9af, OneromBoardSize::BoardSizeOther),
        (0x19af, OneromBoardSize::BoardSizeOther),
    ];
    for (devinfo, size) in cases {
        assert_eq!(board_size([ENABLE; 3], devinfo), size, "{devinfo:#06x}");
    }
}
