// tests/otp_record.rs
//
// Tests for a CHIPID's text form and the lines of a signing key's record file.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata::otp::{RecordError, RecordLine, format_chip_id, parse_chip_id, parse_record};

/// CHIPID from rows 0x000–0x003 of an RP2350 A4.
const CHIP_ID: [u16; 4] = [0x5b6b, 0x2f65, 0x9c23, 0xde3f];

/// A signature whose bytes count up from 0.
fn signature() -> [u8; 64] {
    core::array::from_fn(|i| i as u8)
}

/// [`signature`]'s line. The hash of bytes 0x00–0x3f is from Python's hashlib.
const LINE: &str = "fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108";

#[test]
fn a_chip_id_is_written_row_0x003_first() {
    assert_eq!(format_chip_id(CHIP_ID), "DE3F9C232F655B6B");
    assert_eq!(parse_chip_id("DE3F9C232F655B6B"), Some(CHIP_ID));
}

#[test]
fn a_chip_id_in_any_other_form_is_refused() {
    for text in [
        "de3f9c232f655b6b",
        "DE3F9C232F655B6",
        "DE3F9C232F655B6B0",
        "DE3F9C232F655B6G",
        "+E3F9C232F655B6B",
        " DE3F9C232F655B6",
        "",
    ] {
        assert_eq!(parse_chip_id(text), None, "{text:?}");
    }
}

#[test]
fn a_record_line_is_the_signature_hash() {
    assert_eq!(RecordLine::new(&signature()).to_string(), LINE);
}

#[test]
fn a_record_reads_back_as_written() {
    let line = RecordLine::new(&signature());
    let other = RecordLine::new(&[0xff; 64]);
    let lines = parse_record(&format!("{line}\n{other}\n")).unwrap();
    assert_eq!(lines, [line, other]);
    assert!(!lines.contains(&RecordLine::new(&[0; 64])));
}

#[test]
fn blank_lines_crlf_and_a_missing_last_newline_are_accepted() {
    let lines = parse_record(&format!("\n{LINE}\r\n  \r\n{LINE}")).unwrap();
    assert_eq!(lines.len(), 2);
    assert!(parse_record("").unwrap().is_empty());
}

/// Blank lines count towards the line number.
#[test]
fn the_first_bad_line_is_reported() {
    let bad = [
        LINE.to_uppercase(),
        format!(" {LINE}"),
        format!("{LINE} "),
        format!("{LINE}00"),
        LINE[..LINE.len() - 2].to_string(),
        LINE.replace('f', "g"),
    ];
    for line in bad {
        let text = format!("{LINE}\n\n{line}\n{line}\n");
        assert_eq!(
            parse_record(&text),
            Err(RecordError { line: 3 }),
            "{line:?}"
        );
    }
}

/// A line from before the record dropped the CHIPID.
#[test]
fn a_chip_id_and_hash_line_is_refused() {
    let text = format!("{} {LINE}\n", format_chip_id(CHIP_ID));
    assert_eq!(parse_record(&text), Err(RecordError { line: 1 }));
}
