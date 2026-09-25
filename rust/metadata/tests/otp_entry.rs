// tests/otp_entry.rs
//
// Tests for the generated OTP store entry: the writer lays out the rows the
// store holds, and the parser reads them back.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_metadata::{
    DeviceMemoryView, Generations, OTP_COMMISSIONING_SIG_LEN, OneromOtpEntry, ParseError,
    SerializeContext, SerializeError,
};

/// Where the tests place an entry.  Any address serves, because the view maps
/// it to the buffer.
const BASE: u32 = 0x4013_0000;

/// An entry's bytes as the generated writer lays them out, in whole rows.
///
/// Empty OTP reads 0, so the writer's buffer is zeroed before the entry goes
/// in.
fn written(entry: &OneromOtpEntry) -> Vec<u8> {
    let mut buf = vec![0u8; 256];
    {
        let mut ctx = SerializeContext::new(BASE, 0, &mut buf);
        ctx.buf.fill(0);
        entry.write(&mut ctx, BASE);
    }
    let len = u16::from_le_bytes([buf[2], buf[3]]) as usize;
    buf.truncate(4 + len.next_multiple_of(2));
    buf
}

/// Parse an entry from `bytes` placed at [`BASE`].
fn parsed(bytes: &[u8]) -> Result<OneromOtpEntry, ParseError> {
    let view = DeviceMemoryView::new(bytes, BASE);
    OneromOtpEntry::parse(&view, BASE, Generations::UNKNOWN)
}

/// `bytes` as 16-bit rows, low byte first.
fn rows(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks(2)
        .map(|r| u16::from_le_bytes([r[0], r[1]]))
        .collect()
}

/// A key row, a length row, then the value two bytes per row, low byte first,
/// with a zero byte padding a value of odd length.
#[test]
fn a_board_entry_is_written_as_the_store_holds_it() {
    let entry = OneromOtpEntry::OtpKeyCommissioningBoard {
        name: "fire-24-f".into(),
    };
    assert_eq!(
        rows(&written(&entry)),
        [0x0001, 0x0009, 0x6966, 0x6572, 0x322d, 0x2d34, 0x0066]
    );
}

#[test]
fn every_entry_reads_back_as_written() {
    let entries = [
        OneromOtpEntry::OtpKeyCommissioningBoard {
            name: "fire-40-a".into(),
        },
        OneromOtpEntry::OtpKeyCommissioningSig {
            signature: core::array::from_fn(|i| i as u8),
        },
        OneromOtpEntry::OtpKeyCommissioningManufacturer {
            name: "piers.rocks".into(),
        },
        OneromOtpEntry::OtpKeyCommissioningDate {
            date: "20260925".into(),
        },
        OneromOtpEntry::Unknown {
            key: 0x1234,
            params: vec![1, 2, 3],
        },
    ];
    for entry in entries {
        assert_eq!(parsed(&written(&entry)), Ok(entry));
    }
}

/// Read at its full size, a signature whose length row is short takes the
/// rows of whatever entry follows.
#[test]
fn a_signature_shorter_than_its_key_needs_is_refused() {
    let mut bytes = written(&OneromOtpEntry::OtpKeyCommissioningSig { signature: [0; 64] });
    bytes[2] = 10;
    assert_eq!(
        parsed(&bytes),
        Err(ParseError::ParamsTooShort {
            addr: BASE,
            len: 10,
            needed: OTP_COMMISSIONING_SIG_LEN,
        })
    );
}

#[test]
fn a_name_that_is_not_utf8_is_refused() {
    let bytes = [0x01, 0x00, 0x02, 0x00, 0xff, 0xfe];
    assert_eq!(parsed(&bytes), Err(ParseError::InvalidUtf8));
}

/// The length row is 16 bits, so a longer value has no way to say its length.
#[test]
fn a_name_too_long_for_the_length_row_is_refused() {
    let mut buf = vec![0u8; 16];
    let mut ctx = SerializeContext::new(BASE, 0, &mut buf);
    let entry = OneromOtpEntry::OtpKeyCommissioningBoard {
        name: "x".repeat(65536),
    };
    assert_eq!(
        entry.layout(&mut ctx),
        Err(SerializeError::CountOverflow {
            field: "onerom_otp_entry_t.len",
        })
    );
}
