// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for the in-memory OTP and the OTP readers.
//!
//! picoboot's `examples/otp.rs` ran its write and lock tests on real boards.
//! The in-memory OTP's first tests replay them.

use ed25519_dalek::{Signer as _, SigningKey};
use onerom_app::{
    BoardSize, EccRow, Interruption, LocalOtpAccess, MemoryOtp, OtpError, PageLock, Request,
    RequestDate, prepare, read_board_size, read_chip_id, read_commissioning, read_report,
};
use onerom_config::hw::Board;
use onerom_metadata::otp::pico_otp::ecc_encode;
use onerom_metadata::otp::pico_otp::whitelabel::WHITE_LABEL_SCHEMA_URL;
use onerom_metadata::otp::{
    CommissioningArea, CommissioningValues, NewCommissioningInstance, RowWrite,
};
use onerom_metadata::{
    OTP_COMMISSIONING_AREA_FIRST_ROW, OTP_GENERAL_STORE_FIRST_ROW, OTP_STORE_MAGIC,
    OTP_STORE_VERSION, OneromBoardSize,
};
use serde_json::json;

/// CHIPID from rows 0x000–0x003 of an RP2350 A4.
const CHIP_ID: [u16; 4] = [0x5b6b, 0x2f65, 0x9c23, 0xde3f];

/// Rows in the commissioning area.
const AREA_ROWS: u16 = 0x400;

/// Page n's LOCK1 word is row 0xf81 + 2n.
fn lock1_row(page: u16) -> u16 {
    0xf81 + 2 * page
}

/// A board with only CHIPID written.
fn board() -> MemoryOtp {
    let mut otp = MemoryOtp::new();
    for (row, value) in (0..).zip(CHIP_ID) {
        otp.set_raw(row, ecc_encode(value));
    }
    otp
}

/// Row `row`'s raw value.
fn raw(otp: &MemoryOtp, row: u16) -> u32 {
    otp.rows()[usize::from(row)]
}

// ---------------------------------------------------------------------------
// picoboot's device tests
// ---------------------------------------------------------------------------

/// ECC values for the overwrite tests. The chip can't store `CLASH` over
/// `FIRST` as it is or inverted because `FIRST` has a bit that `CLASH` lacks
/// and they share a set bit.
const FIRST: u16 = 0x1234;
const CLASH: u16 = 0x5678;

/// Reads `row` with ECC and raw and checks both.
async fn expect_ecc(otp: &mut MemoryOtp, row: u16, ecc: u16, raw: u32) {
    assert_eq!(otp.read_ecc(row, 1).await, Ok(vec![ecc]), "row {row:#05x}");
    assert_eq!(otp.read_raw(row, 1).await, Ok(vec![raw]), "row {row:#05x}");
}

/// `test-writes 0xe40` with picoboot's 4-row write made a row at a time.
#[tokio::test]
async fn memory_otp_passes_picoboots_write_tests() {
    let mut otp = MemoryOtp::new();
    let refused = Err(OtpError::UnsupportedModification);

    // In turn:
    // - an ECC write
    // - an overwrite the chip can't store
    // - a write of 0
    let a = 0xe40;
    otp.write_ecc(a, FIRST).await.unwrap();
    expect_ecc(&mut otp, a, FIRST, ecc_encode(FIRST)).await;
    assert_eq!(otp.write_ecc(a, CLASH).await, refused);
    expect_ecc(&mut otp, a, FIRST, ecc_encode(FIRST)).await;
    otp.write_ecc(a, 0).await.unwrap();
    expect_ecc(&mut otp, a, 0, 0xff_ffff).await;

    // An ECC row written raw with all ones.
    let b = a + 1;
    otp.write_ecc(b, 0xabcd).await.unwrap();
    expect_ecc(&mut otp, b, 0xabcd, ecc_encode(0xabcd)).await;
    otp.write_raw(b, 0xff_ffff).await.unwrap();
    expect_ecc(&mut otp, b, 0, 0xff_ffff).await;

    // Raw bits added one write at a time and then a refused write that would
    // clear one.
    let c = a + 2;
    otp.write_raw(c, 0x1).await.unwrap();
    assert_eq!(raw(&otp, c), 0x1);
    otp.write_raw(c, 0x3).await.unwrap();
    assert_eq!(raw(&otp, c), 0x3);
    assert_eq!(otp.write_raw(c, 0x2).await, refused);
    assert_eq!(raw(&otp, c), 0x3);

    // ECC writes in turn:
    // - FIRST to the third row
    // - 1 to the first row
    // - 2 to the second row
    // - CLASH to the third row
    //
    // The last write is refused. picoboot's 4-row write stops at that row so
    // the fourth row isn't written.
    let d = a + 3;
    otp.write_ecc(d + 2, FIRST).await.unwrap();
    otp.write_ecc(d, 1).await.unwrap();
    otp.write_ecc(d + 1, 2).await.unwrap();
    assert_eq!(otp.write_ecc(d + 2, CLASH).await, refused);
    assert_eq!(otp.read_ecc(d, 4).await, Ok(vec![1, 2, FIRST, 0]));
    assert_eq!(
        otp.read_raw(d, 4).await,
        Ok(vec![ecc_encode(1), ecc_encode(2), ecc_encode(FIRST), 0])
    );
}

/// `lock 58` and `test-lock 58` with a reset between.
#[tokio::test]
async fn memory_otp_passes_picoboots_lock_tests() {
    let page = 58;
    let lock_row = lock1_row(page);
    let first = page * 64;
    let mut otp = MemoryOtp::new();

    otp.write_raw(lock_row, 0x15_1515).await.unwrap();
    assert_eq!(raw(&otp, lock_row), 0x15_1515);
    // The lock loads at the next reset.
    otp.write_ecc(first + 1, 0x1234).await.unwrap();
    otp.reset();

    assert_eq!(otp.read_raw(lock_row, 1).await, Ok(vec![0x15_1515]));
    otp.read_raw(first, 64).await.unwrap();
    otp.read_ecc(first, 64).await.unwrap();
    let refused = Err(OtpError::NotPermitted);
    assert_eq!(otp.write_ecc(first, 0).await, refused);
    assert_eq!(otp.write_raw(lock_row, 0x15_1515).await, refused);
}

// ---------------------------------------------------------------------------
// The in-memory OTP's other rules
// ---------------------------------------------------------------------------

/// LOCK_BL is bits 5:4 of each copy. The datasheet says 2 behaves as 3.
#[tokio::test]
async fn an_inaccessible_page_refuses_reads_but_not_its_lock_words() {
    let page = 5;
    for lock in [0x20_2020, 0x30_3030] {
        let mut otp = MemoryOtp::new();
        otp.set_raw(lock1_row(page), lock);
        otp.reset();
        let refused = Err(OtpError::NotPermitted);
        assert_eq!(otp.read_raw(page * 64, 1).await, refused, "{lock:#08x}");
        assert_eq!(
            otp.read_ecc(page * 64 + 63, 1).await,
            Err(OtpError::NotPermitted),
            "{lock:#08x}"
        );
        // A read reaching into the page.
        assert_eq!(otp.read_raw(page * 64 - 1, 2).await, refused, "{lock:#08x}");
        assert_eq!(otp.read_raw(page * 64 - 1, 1).await, Ok(vec![0]));
        assert_eq!(
            otp.read_raw(lock1_row(page) - 1, 2).await,
            Ok(vec![0, lock])
        );
    }
}

/// The lock is each bit's majority across the lock word's three copies.
#[tokio::test]
async fn a_page_locks_where_two_copies_of_lock_bl_say_so() {
    let page = 5;
    for (lock, locked) in [
        (0x00_0010, false),
        (0x00_1010, true),
        (0x10_0010, true),
        (0x30_1000, true),
    ] {
        let mut otp = MemoryOtp::new();
        otp.set_raw(lock1_row(page), lock);
        otp.reset();
        let written = otp.write_ecc(page * 64, 1).await;
        assert_eq!(written.is_err(), locked, "{lock:#08x}");
    }
}

/// Page 5's lock words are rows 0xf8a and 0xf8b in page 62.
#[tokio::test]
async fn a_lock_word_takes_the_lock_of_the_page_it_locks() {
    let mut otp = MemoryOtp::new();
    otp.set_raw(lock1_row(5), 0x15_1515);
    otp.reset();
    let refused = Err(OtpError::NotPermitted);
    assert_eq!(otp.write_raw(lock1_row(5) - 1, 0x01).await, refused);
    assert_eq!(otp.write_raw(lock1_row(5), 0x15_1515).await, refused);
    otp.write_raw(lock1_row(6), 0x15_1515).await.unwrap();

    // Locking page 62 leaves the other pages' lock words in it writable.
    let mut otp = MemoryOtp::new();
    otp.set_raw(lock1_row(62), 0x15_1515);
    otp.reset();
    otp.write_raw(lock1_row(5), 0x15_1515).await.unwrap();
    assert_eq!(otp.write_raw(lock1_row(62), 0x15_1515).await, refused);
}

#[tokio::test]
async fn an_ecc_read_corrects_one_wrong_bit() {
    let value = 0x99af;
    let encoding = ecc_encode(value);
    let mut otp = MemoryOtp::new();
    for form in [encoding, !encoding & 0xff_ffff] {
        otp.set_raw(0x100, form);
        assert_eq!(otp.read_ecc(0x100, 1).await, Ok(vec![value]), "{form:#08x}");
        for bit in 0..22 {
            otp.set_raw(0x100, form ^ (1 << bit));
            assert_eq!(
                otp.read_ecc(0x100, 1).await,
                Ok(vec![value]),
                "{form:#08x} bit {bit}"
            );
        }
    }
}

/// Two wrong data bits can't be corrected so the read returns bits 15:0.
#[tokio::test]
async fn an_ecc_read_of_worse_damage_returns_bits_15_to_0() {
    let mut otp = MemoryOtp::new();
    otp.set_raw(0x100, ecc_encode(0x99af) ^ 0b11);
    assert_eq!(otp.read_ecc(0x100, 1).await, Ok(vec![0x99ac]));
    // A row written raw.
    otp.set_raw(0x101, 0x12_3456);
    assert_eq!(otp.read_ecc(0x101, 1).await, Ok(vec![0x3456]));
}

#[tokio::test]
async fn a_raw_write_ignores_bits_31_to_24() {
    let mut otp = MemoryOtp::new();
    otp.write_raw(0x100, 0xff00_0001).await.unwrap();
    assert_eq!(raw(&otp, 0x100), 0x1);
}

#[tokio::test]
async fn an_interrupted_write_lands_or_not() {
    for (interruption, landed) in [
        (Interruption::Landed, true),
        (Interruption::NotLanded, false),
    ] {
        let mut otp = MemoryOtp::new();
        otp.interrupt(2, interruption);
        otp.write_ecc(0x100, 1).await.unwrap();
        assert!(matches!(
            otp.write_ecc(0x101, 2).await,
            Err(OtpError::Transport(_))
        ));
        assert_eq!(raw(&otp, 0x101) == ecc_encode(2), landed);
        // Only write 2 is interrupted.
        otp.write_ecc(0x102, 3).await.unwrap();
        assert_eq!(otp.write_count(), 3);
    }
}

#[tokio::test]
async fn a_corrupted_write_flips_its_mask() {
    let mut otp = MemoryOtp::new();
    otp.corrupt(2, 0x10);
    otp.write_ecc(0x100, 1).await.unwrap();
    otp.write_ecc(0x101, 1).await.unwrap();
    otp.write_ecc(0x102, 1).await.unwrap();
    assert_eq!(raw(&otp, 0x100), ecc_encode(1));
    assert_eq!(raw(&otp, 0x101), ecc_encode(1) ^ 0x10);
    assert_eq!(raw(&otp, 0x102), ecc_encode(1));
}

#[tokio::test]
async fn refused_writes_count() {
    let mut otp = MemoryOtp::new();
    otp.write_raw(0x100, 0x3).await.unwrap();
    otp.write_raw(0x100, 0x1).await.unwrap_err();
    assert_eq!(otp.write_count(), 2);
}

#[tokio::test]
async fn an_access_past_the_last_row_fails() {
    let mut otp = MemoryOtp::new();
    otp.read_raw(0xfff, 1).await.unwrap();
    assert!(matches!(
        otp.read_raw(0xfff, 2).await,
        Err(OtpError::Transport(_))
    ));
    assert!(matches!(
        otp.write_raw(0x1000, 1).await,
        Err(OtpError::Transport(_))
    ));
}

// ---------------------------------------------------------------------------
// Reading CHIPID and the commissioning area
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chip_id_is_rows_0_to_3() {
    let mut otp = board();
    assert_eq!(read_chip_id(&mut otp).await, Ok(CHIP_ID));
}

/// A signature whose bytes count up from 0.
fn signature() -> [u8; 64] {
    core::array::from_fn(|i| i as u8)
}

/// fire-24-f's instance at `first_row` from `manufacturer`. It holds the
/// signature [`signature`] returns.
fn instance(first_row: u16, manufacturer: &str) -> Vec<RowWrite> {
    let values = CommissioningValues::new(Board::Fire24F, manufacturer, "20260926", 1).unwrap();
    NewCommissioningInstance::new(&values, first_row)
        .unwrap()
        .writes(&signature())
}

/// Writes `writes` to `otp` as exact codewords.
fn put(otp: &mut MemoryOtp, writes: &[RowWrite]) {
    for write in writes {
        otp.set_raw(write.row, ecc_encode(write.value));
    }
}

/// Writes `rows` to `otp` from `first_row` as exact codewords.
fn put_rows(otp: &mut MemoryOtp, first_row: u16, rows: &[u16]) {
    for (row, &value) in (first_row..).zip(rows) {
        otp.set_raw(row, ecc_encode(value));
    }
}

/// An entry's rows:
/// - its key
/// - its length
/// - its value
///
/// The value takes two bytes per row with the low byte first.
fn entry(key: u16, value: &[u8]) -> Vec<u16> {
    let mut rows = vec![key, value.len() as u16];
    rows.extend(
        value
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair.get(1).copied().unwrap_or(0)])),
    );
    rows
}

/// An instance's magic and version rows followed by `entries`.
fn instance_rows(entries: &[Vec<u16>]) -> Vec<u16> {
    [OTP_STORE_MAGIC, OTP_STORE_VERSION]
        .into_iter()
        .chain(entries.iter().flatten().copied())
        .collect()
}

/// A valid instance's five entries with the signature last.
fn valid_entries() -> Vec<Vec<u16>> {
    vec![
        entry(1, b"fire-24-f"),
        entry(3, b"piers.rocks"),
        entry(4, b"20260926"),
        entry(5, &1_u16.to_le_bytes()),
        entry(2, &signature()),
    ]
}

/// Checks that [`read_commissioning`] matches a parse of the whole area.
async fn expect_full_parse(otp: &mut MemoryOtp, at: &str) {
    let rows = otp
        .read_ecc(OTP_COMMISSIONING_AREA_FIRST_ROW, AREA_ROWS)
        .await
        .unwrap();
    assert_eq!(
        read_commissioning(otp).await.unwrap(),
        CommissioningArea::parse(&rows),
        "{at}"
    );
}

/// The fixtures of onerom-metadata's `tests/otp_store.rs`.
#[tokio::test]
async fn reading_a_page_at_a_time_matches_a_parse_of_the_whole_area() {
    let mut otp = board();
    expect_full_parse(&mut otp, "an empty area").await;

    put(&mut otp, &instance(0x0c0, "piers.rocks"));
    expect_full_parse(&mut otp, "an instance").await;

    // An unwritten version row.
    let mut otp = board();
    put_rows(&mut otp, 0x0c0, &[OTP_STORE_MAGIC]);
    put(&mut otp, &instance(0x100, "piers.rocks"));
    expect_full_parse(&mut otp, "an unwritten version row").await;

    // An unknown version.
    let mut otp = board();
    put(&mut otp, &instance(0x0c0, "piers.rocks"));
    put_rows(&mut otp, 0x100, &[OTP_STORE_MAGIC, 2]);
    put(&mut otp, &instance(0x140, "piers.rocks"));
    expect_full_parse(&mut otp, "an unknown version").await;

    // A lost parser with and without a later instance.
    let mut otp = board();
    put(&mut otp, &instance(0x0c0, "piers.rocks"));
    put_rows(&mut otp, 0x100, &[0x1234]);
    expect_full_parse(&mut otp, "a lost parser").await;
    put(&mut otp, &instance(0x140, "piers.rocks"));
    expect_full_parse(&mut otp, "a lost parser that restarts").await;

    // An entry in the instance at 0x100 that runs past the area.
    let mut otp = board();
    put(&mut otp, &instance(0x0c0, "piers.rocks"));
    put(&mut otp, &instance(0x100, "piers.rocks"));
    put_rows(&mut otp, 0x103, &[0xffff]);
    put(&mut otp, &instance(0x140, "piers.rocks"));
    expect_full_parse(&mut otp, "an entry running past the area").await;

    // A last instance without a date.
    let mut otp = board();
    put(&mut otp, &instance(0x0c0, "piers.rocks"));
    let mut entries = valid_entries();
    entries.remove(2);
    put_rows(&mut otp, 0x100, &instance_rows(&entries));
    expect_full_parse(&mut otp, "an invalid last instance").await;

    // The store's rules:
    // - an unknown key
    // - a deleted entry
    // - a repeated key
    // - values that don't parse
    let mut entries = valid_entries();
    entries.insert(1, entry(0x1234, &[1, 2, 3]));
    entries.insert(2, vec![0, 3, 0x0201, 0x0003]);
    entries.insert(3, entry(1, b"fire-28-a"));
    entries.insert(4, entry(5, &[1, 0, 0, 0]));
    entries.insert(5, entry(1, &[0xff, 0xfe]));
    let mut otp = board();
    put_rows(&mut otp, 0x0c0, &instance_rows(&entries));
    expect_full_parse(&mut otp, "the store rules").await;

    // An entry running from row 0x0c2 to row 0x18f whose value holds the magic
    // value at 0x100 and 0 at 0x101 and 0x140. A parse of pages 3 to 5 loses
    // its place in the entry and finds an instance at 0x100.
    let mut value = vec![0x55; 2 * (0x190 - 0x0c4)];
    for (row, pair) in [(0x100, OTP_STORE_MAGIC), (0x101, 0), (0x140, 0)] {
        let at = 2 * (row - 0x0c4);
        value[at..at + 2].copy_from_slice(&u16::to_le_bytes(pair));
    }
    let mut otp = board();
    put_rows(&mut otp, 0x0c0, &instance_rows(&[entry(0x10, &value)]));
    expect_full_parse(&mut otp, "an entry holding the magic value").await;
}

/// Fills the area a page at a time and checks after every write.
#[tokio::test]
async fn reading_a_page_at_a_time_matches_a_parse_of_the_whole_area_as_it_fills() {
    let mut otp = board();
    let mut first_row = Some(0x0c0);
    while let Some(row) = first_row {
        // The last page's instance fills the page.
        let manufacturer = if row == 0x480 {
            "piers.rocks (sample)"
        } else {
            "piers.rocks"
        };
        for (n, write) in instance(row, manufacturer).iter().enumerate() {
            put(&mut otp, &[*write]);
            expect_full_parse(&mut otp, &format!("instance {row:#05x}, {} writes", n + 1)).await;
        }
        first_row = read_commissioning(&mut otp)
            .await
            .unwrap()
            .next_instance_row();
    }
}

/// Records the first row and length of each ECC read.
struct Recorder {
    otp: MemoryOtp,
    reads: Vec<(u16, u16)>,
}

impl LocalOtpAccess for Recorder {
    async fn read_ecc(&mut self, row: u16, count: u16) -> Result<Vec<u16>, OtpError> {
        self.reads.push((row, count));
        self.otp.read_ecc(row, count).await
    }

    async fn read_raw(&mut self, row: u16, count: u16) -> Result<Vec<u32>, OtpError> {
        self.otp.read_raw(row, count).await
    }

    async fn write_ecc(&mut self, row: u16, value: u16) -> Result<(), OtpError> {
        self.otp.write_ecc(row, value).await
    }

    async fn write_raw(&mut self, row: u16, value: u32) -> Result<(), OtpError> {
        self.otp.write_raw(row, value).await
    }
}

/// The reads [`read_commissioning`] makes of `otp`.
async fn reads(otp: MemoryOtp) -> Vec<(u16, u16)> {
    let mut recorder = Recorder {
        otp,
        reads: Vec::new(),
    };
    read_commissioning(&mut recorder).await.unwrap();
    recorder.reads
}

/// The parser has to see the page boundary after the last instance. Where it
/// loses its place, the search for the next instance reads the whole area.
#[tokio::test]
async fn reading_stops_at_the_page_after_the_last_instance() {
    assert_eq!(reads(board()).await, [(0x0c0, 64)]);

    let mut otp = board();
    put(&mut otp, &instance(0x0c0, "piers.rocks"));
    assert_eq!(reads(otp.clone()).await, [(0x0c0, 64), (0x100, 64)]);

    put_rows(&mut otp, 0x100, &[0x1234]);
    let all: Vec<(u16, u16)> = (0x0c0..0x4c0).step_by(64).map(|row| (row, 64)).collect();
    assert_eq!(reads(otp).await, all);
}

// ---------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------

/// Commissions `board` as `size` from piers.rocks on 20260926 with signer 1.
async fn commission(otp: &mut MemoryOtp, board: Board, size: BoardSize) {
    let request = Request {
        board,
        size,
        manufacturer: "piers.rocks".into(),
        date: RequestDate::Today("20260926".into()),
        signer: 1,
        force: false,
    };
    let prepared = prepare(otp, &request).await.unwrap();
    let signature = SigningKey::from_bytes(&[1; 32])
        .sign(prepared.message())
        .to_bytes();
    let plan = prepared.plan(&signature).unwrap();
    plan.execute(otp, |_| {}).await.unwrap();
}

/// The pages whose locks [`read_report`] reads.
fn lock_pages() -> Vec<u16> {
    (3..=18).chain(59..=60).collect()
}

#[tokio::test]
async fn a_blank_boards_report_shows_an_m_board() {
    let mut otp = board();
    let report = read_report(&mut otp).await.unwrap();

    assert_eq!(report.chip_id, "DE3F9C232F655B6B");
    assert_eq!(report.size, Some(BoardSize::M));
    assert_eq!(report.boot_flags0, [0; 3]);
    let zero = EccRow {
        raw: 0,
        value: Some(0),
    };
    assert_eq!(report.flash_devinfo, zero);
    assert_eq!(report.flash_partition_slot_size, zero);
    assert_eq!(report.usb_boot_flags, [0; 3]);
    assert_eq!(report.usb_white_label_addr, zero);
    // The white label is unwritten so it isn't decoded.
    assert_eq!(report.white_label, None);
    assert_eq!(report.white_label_error, None);
    assert!(report.white_label_warnings.is_empty());
    assert!(report.commissioning.instances().is_empty());
    assert_eq!(report.general_store, None);
    let locks: Vec<PageLock> = lock_pages()
        .into_iter()
        .map(|page| PageLock { page, raw: 0 })
        .collect();
    assert_eq!(report.locks, locks);
}

#[tokio::test]
async fn an_l_boards_report_shows_its_commissioning() {
    let mut otp = board();
    commission(&mut otp, Board::Fire40A, BoardSize::L).await;
    let report = read_report(&mut otp).await.unwrap();

    assert_eq!(report.size, Some(BoardSize::L));
    assert_eq!(report.boot_flags0, [0x20; 3]);
    assert_eq!(
        report.flash_devinfo,
        EccRow {
            raw: 0x3a_99af,
            value: Some(0x99af)
        }
    );
    assert_eq!(report.usb_boot_flags, [0x40_f133; 3]);
    assert_eq!(
        report.usb_white_label_addr,
        EccRow {
            raw: ecc_encode(0xec0),
            value: Some(0xec0)
        }
    );
    assert_eq!(
        report.white_label,
        Some(json!({
            "$schema": WHITE_LABEL_SCHEMA_URL,
            "device": {
                "vid": "0x1209",
                "pid": "0xf540",
                "manufacturer": "piers.rocks",
                "product": "One ROM Bootloader",
            },
            "volume": {
                "label": "ONEROM",
                "redirect_url": "https://onerom.org",
                "redirect_name": "onerom.org",
                "model": "One ROM",
                "board_id": "fire-40-a",
            },
        }))
    );
    assert_eq!(report.white_label_error, None);
    assert!(
        report.white_label_warnings.is_empty(),
        "{:?}",
        report.white_label_warnings
    );
    let current = report.commissioning.current().unwrap();
    assert_eq!(current.first_row(), 0x0c0);
    assert_eq!(current.board(), Some("fire-40-a"));
    // --json shows the current instance as its first row.
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["commissioning"]["current"], 0x0c0);
    let locked: Vec<u16> = report
        .locks
        .iter()
        .filter(|lock| lock.raw != 0)
        .map(|lock| lock.page)
        .collect();
    assert_eq!(locked, [3]);
    assert_eq!(report.locks[0].raw, 0x15_1515);
}

#[tokio::test]
async fn an_m_boards_report_shows_an_m_board() {
    let mut otp = board();
    commission(&mut otp, Board::Fire24F, BoardSize::M).await;
    let report = read_report(&mut otp).await.unwrap();
    assert_eq!(report.size, Some(BoardSize::M));
    assert!(report.commissioning.current().is_some());
}

/// Boards whose OTP configures each size, with the size. `None` is neither M
/// nor L. Each BOOT_FLAGS0 bit comes from the majority of its three copies,
/// FLASH_DEVINFO is read with ECC, and L is 4MB in total.
fn size_cases() -> Vec<(MemoryOtp, Option<BoardSize>)> {
    let otp = |devinfo: u32, flags: [u32; 3]| -> MemoryOtp {
        let mut otp = board();
        otp.set_raw(0x054, devinfo);
        for (row, value) in (0x048..).zip(flags) {
            otp.set_raw(row, value);
        }
        otp
    };
    vec![
        // Enabled by two copies.
        (otp(ecc_encode(0x99af), [0x20, 0x20, 0]), Some(BoardSize::L)),
        // Enabled by one copy.
        (otp(ecc_encode(0x99af), [0x20, 0, 0]), Some(BoardSize::M)),
        // A 16MB second chip.
        (otp(ecc_encode(0xc9af), [0x20; 3]), None),
        // FLASH_DEVINFO with a wrong bit, which ECC corrects.
        (otp(ecc_encode(0x99af) ^ 1, [0x20; 3]), Some(BoardSize::L)),
        // FLASH_DEVINFO written and not enabled.
        (otp(ecc_encode(0x99af), [0; 3]), Some(BoardSize::M)),
        // A factory bit in FLASH_DEVINFO.
        (otp(0x40, [0; 3]), Some(BoardSize::M)),
    ]
}

/// BOOT_FLAGS0's copies and FLASH_DEVINFO, raw, for a failure's message.
fn size_rows(otp: &MemoryOtp) -> String {
    format!("{:x?} {:#08x}", &otp.rows()[0x048..=0x04a], raw(otp, 0x054))
}

#[tokio::test]
async fn a_report_shows_the_size_otp_configures() {
    for (mut otp, size) in size_cases() {
        let report = read_report(&mut otp).await.unwrap();
        assert_eq!(report.size, size, "{}", size_rows(&otp));
        // --json shows the letter, or `other` for neither M nor L.
        let json = serde_json::to_value(&report).unwrap();
        let expected = size.map_or("other".to_string(), |size| size.to_string());
        assert_eq!(json["size"], expected, "{}", size_rows(&otp));
    }
}

/// [`read_board_size`] reads the size by the report's rule.
#[tokio::test]
async fn read_board_size_reads_the_size_otp_configures() {
    for (mut otp, size) in size_cases() {
        let expected = match size {
            Some(BoardSize::M) => OneromBoardSize::BoardSizeM,
            Some(BoardSize::L) => OneromBoardSize::BoardSizeL,
            None => OneromBoardSize::BoardSizeOther,
        };
        let read = read_board_size(&mut otp).await.unwrap();
        assert_eq!(read, expected, "{}", size_rows(&otp));
    }
}

#[tokio::test]
async fn a_report_shows_a_started_general_store() {
    let mut otp = board();
    put_rows(
        &mut otp,
        OTP_GENERAL_STORE_FIRST_ROW,
        &[OTP_STORE_MAGIC, OTP_STORE_VERSION],
    );
    let report = read_report(&mut otp).await.unwrap();
    let store = report.general_store.unwrap();
    assert!(store.entries().is_empty());
    assert!(store.issues().is_empty());
}

/// USB_BOOT_FLAGS for One ROM's white label.
const USB_BOOT_FLAGS: u32 = 0x40_f133;

/// Sets USB_BOOT_FLAGS and its two copies to `flags`.
fn set_usb_boot_flags(otp: &mut MemoryOtp, flags: u32) {
    for row in 0x059..=0x05b {
        otp.set_raw(row, flags);
    }
}

/// Whether one of `report`'s white label warnings mentions `text`.
fn warns(report: &onerom_app::OtpReport, text: &str) -> bool {
    report
        .white_label_warnings
        .iter()
        .any(|warning| warning.contains(text))
}

/// The report carries the warnings pico-otp raises about white label data it
/// can't make sense of. The parts it can decode are still there.
#[tokio::test]
async fn a_report_carries_pico_otps_white_label_warnings() {
    // The white label written without USB_BOOT_FLAGS. An interrupted
    // commission can leave it this way.
    let mut otp = board();
    commission(&mut otp, Board::Fire24F, BoardSize::M).await;
    set_usb_boot_flags(&mut otp, 0);
    let report = read_report(&mut otp).await.unwrap();
    assert_eq!(
        report.white_label,
        Some(json!({ "$schema": WHITE_LABEL_SCHEMA_URL }))
    );
    assert_eq!(report.white_label_error, None);
    for text in ["WHITE_LABEL_ADDR_VALID", "usb_manufacturer", "uf2_board_id"] {
        assert!(
            warns(&report, text),
            "{text}: {:?}",
            report.white_label_warnings
        );
    }

    // A string USB_BOOT_FLAGS marks valid whose table row is 0. Row 0xec4 is
    // the manufacturer's.
    let mut otp = board();
    commission(&mut otp, Board::Fire24F, BoardSize::M).await;
    otp.set_raw(0xec4, 0);
    let report = read_report(&mut otp).await.unwrap();
    let white_label = report.white_label.as_ref().unwrap();
    assert_eq!(white_label["device"].get("manufacturer"), None);
    assert_eq!(white_label["device"]["product"], "One ROM Bootloader");
    assert_eq!(report.white_label_error, None);
    assert!(
        warns(&report, "usb_manufacturer"),
        "{:?}",
        report.white_label_warnings
    );

    // USB_BOOT_FLAGS bits pico-otp doesn't expect.
    let mut otp = board();
    commission(&mut otp, Board::Fire24F, BoardSize::M).await;
    set_usb_boot_flags(&mut otp, USB_BOOT_FLAGS | 1 << 23 | 1 << 16);
    let report = read_report(&mut otp).await.unwrap();
    assert!(report.white_label.is_some());
    assert_eq!(report.white_label_error, None);
    for text in ["DPDM_SWAP", "invalid bits"] {
        assert!(
            warns(&report, text),
            "{text}: {:?}",
            report.white_label_warnings
        );
    }
}

/// The report decodes the white label where any of it is written. A row counts
/// as written even where an ECC read of it returns 0. It doesn't decode an
/// unwritten white label.
#[tokio::test]
async fn a_report_decodes_a_white_label_where_anything_is_written() {
    let decoded = async |setup: fn(&mut MemoryOtp)| {
        let mut otp = board();
        setup(&mut otp);
        let report = read_report(&mut otp).await.unwrap();
        assert_eq!(report.white_label_error, None);
        report.white_label.is_some()
    };

    // A string row an ECC write of 0 zeroed. An ECC read of it returns 0.
    assert!(decoded(|otp| otp.set_raw(0xed0, 0xff_ffff)).await);
    // USB_WHITE_LABEL_ADDR alone.
    assert!(decoded(|otp| otp.set_raw(0x05c, ecc_encode(0xec0))).await);
    // WHITE_LABEL_ADDR_VALID in two of the three USB_BOOT_FLAGS copies.
    assert!(
        decoded(|otp| {
            otp.set_raw(0x059, 1 << 22);
            otp.set_raw(0x05a, 1 << 22);
        })
        .await
    );

    // WHITE_LABEL_ADDR_VALID in one copy isn't the majority's.
    assert!(!decoded(|otp| otp.set_raw(0x059, 1 << 22)).await);
    // A row can leave the factory with one bit set.
    assert!(!decoded(|otp| otp.set_raw(0xed0, 1 << 5)).await);
}
