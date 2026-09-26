// tests/otp_white_label.rs
//
// Tests for the bootloader's USB white label, which One ROM builds with
// pico-otp.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_config::hw::{BOARDS, Board};
use onerom_metadata::OTP_USB_WHITE_LABEL_ROW;
use onerom_metadata::otp::pico_otp::whitelabel::OTP_ROW_UNRESERVED_END;
use onerom_metadata::otp::pico_otp::{OtpData, WhiteLabelStruct};
use onerom_metadata::otp::white_label;

/// USB_BOOT_FLAGS in RP2350 datasheet section 5.7.9's example: the volume
/// label and the table's address are valid.
const SPOON_BOOT_FLAGS: u32 = 0x0040_0100;

/// The rows of datasheet section 5.7.9's example, from the white label
/// table's first row. It sets the volume label to "SPOON".
fn spoon_rows() -> Vec<u16> {
    let mut rows = vec![0; 0x33];
    // 5 ASCII characters, starting 0x30 rows after the table.
    rows[8] = 0x3005;
    rows[0x30..].copy_from_slice(&[0x5053, 0x4f4f, 0x004e]);
    rows
}

/// pico-otp starts the string straight after the 16-row table, where the
/// example starts it 0x30 rows in. The rest matches the example.
#[test]
fn pico_otp_encodes_the_datasheet_example() {
    let mut white_label = WhiteLabelStruct::default();
    white_label.set_volume_label("SPOON").unwrap();
    let data = white_label.to_otp_data_strict().unwrap();
    let rows = data.rows();
    let offset = usize::from(rows[8] >> 8);
    assert_eq!(rows[8] & 0xff, 0x05);
    assert_eq!(rows[offset..], spoon_rows()[0x30..]);
    assert_eq!(data.usb_boot_flags(), SPOON_BOOT_FLAGS);
}

#[test]
fn pico_otp_parses_the_datasheet_example() {
    let data = OtpData::from_white_label_data(SPOON_BOOT_FLAGS, &spoon_rows(), true).unwrap();
    let white_label = WhiteLabelStruct::try_from(&data).unwrap();
    assert_eq!(
        white_label.volume_label().map(String::as_str),
        Some("SPOON")
    );
}

/// fire-24-f's white label, worked out by hand from OTP.md's table. Each
/// STRDEF holds its string's row offset from the table in its high byte and
/// its length in its low byte.
#[rustfmt::skip]
const FIRE_24_F_ROWS: [u16; 57] = [
    0x0000, 0x0000, 0x0000, 0x0000, 0x100b, 0x1612, 0x0000, 0x0000, // table
    0x1f06, 0x0000, 0x0000, 0x0000, 0x2212, 0x2b0a, 0x3007, 0x3409, // table
    0x6970, 0x7265, 0x2e73, 0x6f72, 0x6b63, 0x0073, // "piers.rocks"
    0x6e4f, 0x2065, 0x4f52, 0x204d, 0x6f42, 0x746f, 0x6f6c, 0x6461, 0x7265, // "One ROM Bootloader"
    0x4e4f, 0x5245, 0x4d4f, // "ONEROM"
    0x7468, 0x7074, 0x3a73, 0x2f2f, 0x6e6f, 0x7265, 0x6d6f, 0x6f2e, 0x6772, // "https://onerom.org"
    0x6e6f, 0x7265, 0x6d6f, 0x6f2e, 0x6772, // "onerom.org"
    0x6e4f, 0x2065, 0x4f52, 0x004d, // "One ROM"
    0x6966, 0x6572, 0x322d, 0x2d34, 0x0066, // "fire-24-f"
];

/// The valid bits for the manufacturer, product, volume label, both
/// INDEX.HTM strings, both INFO_UF2.TXT strings and the table's address.
const FIRE_24_F_BOOT_FLAGS: u32 = 0x0040_f130;

#[test]
fn fire_24_f_has_one_roms_white_label() {
    let data = white_label(Board::Fire24F).to_otp_data_strict().unwrap();
    assert_eq!(data.rows()[..], FIRE_24_F_ROWS);
    assert_eq!(data.usb_boot_flags(), FIRE_24_F_BOOT_FLAGS);
}

/// Every board's white label fits in pages 59 and 60, before the rows
/// Raspberry Pi reserves.
#[test]
fn every_white_label_fits_its_pages() {
    for board in BOARDS {
        let data = white_label(board)
            .to_otp_data_strict()
            .unwrap_or_else(|e| panic!("{board}: {e}"));
        assert!(
            data.rows().len() <= usize::from(OTP_ROW_UNRESERVED_END - OTP_USB_WHITE_LABEL_ROW),
            "{board}"
        );
    }
}
