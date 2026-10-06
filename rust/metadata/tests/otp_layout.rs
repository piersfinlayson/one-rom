// tests/otp_layout.rs
//
// Tests that the OTP layout constants describe one unbroken run of rows, from
// the commissioning area to the bootloader's USB white label table.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata::{
    OTP_COMMISSIONING_AREA_FIRST_ROW, OTP_COMMISSIONING_AREA_LAST_ROW, OTP_GENERAL_STORE_FIRST_ROW,
    OTP_GENERAL_STORE_LAST_ROW, OTP_GENERAL_STORE_TERMINATOR_ROW,
    OTP_GENERAL_STORE_TERMINATOR_ROW_COUNT, OTP_PAGE_ROWS, OTP_USB_WHITE_LABEL_ROW,
};

/// The RP2350 locks OTP a page at a time, so an area starting part way through
/// a page shares that page's lock with the area before it.
#[test]
fn each_area_starts_a_page() {
    for (area, row) in [
        ("commissioning area", OTP_COMMISSIONING_AREA_FIRST_ROW),
        ("general store", OTP_GENERAL_STORE_FIRST_ROW),
        ("USB white label table", OTP_USB_WHITE_LABEL_ROW),
    ] {
        assert_eq!(
            row % OTP_PAGE_ROWS,
            0,
            "the {area} starts at row {row:#05x}, part way through a page"
        );
    }
}

#[test]
fn the_general_store_follows_the_commissioning_area() {
    assert_eq!(
        OTP_COMMISSIONING_AREA_LAST_ROW + 1,
        OTP_GENERAL_STORE_FIRST_ROW
    );
}

/// A full general store ends with key 0 and length 0 only because the
/// terminator's unwritten rows come straight after the store's last row.
#[test]
fn the_terminator_follows_the_general_store() {
    assert_eq!(
        OTP_GENERAL_STORE_LAST_ROW + 1,
        OTP_GENERAL_STORE_TERMINATOR_ROW
    );
}

/// The terminator ends where the white label table starts, so the two don't
/// overlap and no row sits unused between them.
#[test]
fn the_white_label_table_follows_the_terminator() {
    assert_eq!(
        OTP_GENERAL_STORE_TERMINATOR_ROW + OTP_GENERAL_STORE_TERMINATOR_ROW_COUNT,
        OTP_USB_WHITE_LABEL_ROW
    );
}
