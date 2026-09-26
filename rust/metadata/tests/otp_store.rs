// tests/otp_store.rs
//
// Tests for the OTP store: the rows a new commissioning instance writes, and
// the parsers for the commissioning area and the general store.
//
// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

use onerom_config::hw::Board;
use onerom_metadata::otp::{
    AreaIssue, BuildError, CommissioningArea, CommissioningValues, GeneralStore,
    NewCommissioningInstance, RowWrite, StoreEntry,
};
use onerom_metadata::{
    OTP_COMMISSIONING_AREA_FIRST_ROW, OTP_COMMISSIONING_AREA_LAST_ROW, OTP_GENERAL_STORE_FIRST_ROW,
    OTP_GENERAL_STORE_LAST_ROW, OTP_GENERAL_STORE_TERMINATOR_ROW,
    OTP_GENERAL_STORE_TERMINATOR_ROW_COUNT, OTP_STORE_MAGIC, OTP_STORE_VERSION, OneromOtpEntry,
    SerializeContext,
};

/// CHIPID from rows 0x000–0x003 of an RP2350 A4.
const CHIP_ID: [u16; 4] = [0x5b6b, 0x2f65, 0x9c23, 0xde3f];

/// Rows 0x0c0–0x0d9 of [`fire_24_f`]'s instance at 0x0c0, from its magic row
/// to its signer entry, worked out by hand from OTP.md.
const ROWS: [u16; 26] = [
    0x524f, 0x0001, // magic, version
    0x0001, 0x0009, 0x6966, 0x6572, 0x322d, 0x2d34, 0x0066, // board "fire-24-f"
    0x0003, 0x000b, 0x6970, 0x7265, 0x2e73, 0x6f72, 0x6b63, 0x0073, // "piers.rocks"
    0x0004, 0x0008, 0x3032, 0x3632, 0x3930, 0x3632, // date "20260926"
    0x0005, 0x0002, 0x0001, // signer 1
];

/// A signature whose bytes count up from 0.
fn signature() -> [u8; 64] {
    core::array::from_fn(|i| i as u8)
}

/// fire-24-f's values from piers.rocks and signer 1.
fn values() -> CommissioningValues {
    CommissioningValues::new(Board::Fire24F, "piers.rocks", "20260926", 1).unwrap()
}

/// fire-24-f's instance at `first_row`, from piers.rocks and signer 1.
fn fire_24_f(first_row: u16) -> NewCommissioningInstance {
    NewCommissioningInstance::new(&values(), first_row).unwrap()
}

/// An empty commissioning area.
fn blank_area() -> Vec<u16> {
    vec![0; usize::from(OTP_COMMISSIONING_AREA_LAST_ROW - OTP_COMMISSIONING_AREA_FIRST_ROW + 1)]
}

/// Writes `writes` to the commissioning area `area`.
fn apply(area: &mut [u16], writes: &[RowWrite]) {
    for write in writes {
        area[usize::from(write.row - OTP_COMMISSIONING_AREA_FIRST_ROW)] = write.value;
    }
}

/// Writes `instance` to `area`, signed with [`signature`].
fn commission(area: &mut [u16], instance: &NewCommissioningInstance) {
    apply(area, &instance.writes(&signature()));
}

/// `entry`'s rows as the generated writer lays them out.
fn rows_of(entry: &OneromOtpEntry) -> Vec<u16> {
    let mut buf = vec![0u8; 256];
    {
        let mut ctx = SerializeContext::new(0, 0, &mut buf);
        ctx.buf.fill(0);
        entry.write(&mut ctx, 0);
    }
    let len = usize::from(u16::from_le_bytes([buf[2], buf[3]]));
    buf.truncate(4 + len.next_multiple_of(2));
    buf.chunks(2)
        .map(|r| u16::from_le_bytes([r[0], r[1]]))
        .collect()
}

/// The rows of a valid instance's five entries, the signature last.
fn valid_entries() -> Vec<Vec<u16>> {
    vec![
        rows_of(&OneromOtpEntry::OtpKeyCommissioningBoard {
            name: "fire-24-f".into(),
        }),
        rows_of(&OneromOtpEntry::OtpKeyCommissioningManufacturer {
            name: "piers.rocks".into(),
        }),
        rows_of(&OneromOtpEntry::OtpKeyCommissioningDate {
            date: "20260926".into(),
        }),
        rows_of(&OneromOtpEntry::OtpKeyCommissioningSigner { id: 1 }),
        rows_of(&OneromOtpEntry::OtpKeyCommissioningSig {
            signature: signature(),
        }),
    ]
}

/// Writes an instance at `first_row`: the magic and version rows, then
/// `entries`.
fn put(area: &mut [u16], first_row: u16, entries: &[Vec<u16>]) {
    let rows = [OTP_STORE_MAGIC, OTP_STORE_VERSION]
        .into_iter()
        .chain(entries.iter().flatten().copied());
    let start = usize::from(first_row - OTP_COMMISSIONING_AREA_FIRST_ROW);
    for (i, row) in rows.enumerate() {
        area[start + i] = row;
    }
}

// ---------------------------------------------------------------------------
// Building an instance
// ---------------------------------------------------------------------------

/// Why building fire-24-f's values and placing them at `first_row` fails.
fn refusal(manufacturer: &str, date: &str, signer: u16, first_row: u16) -> Option<BuildError> {
    CommissioningValues::new(Board::Fire24F, manufacturer, date, signer)
        .and_then(|values| NewCommissioningInstance::new(&values, first_row))
        .err()
}

#[test]
fn an_instance_is_refused_before_signing() {
    assert_eq!(
        refusal("", "20260926", 1, 0x0c0),
        Some(BuildError::EmptyManufacturer)
    );
    for date in ["2026092", "202609261", "2026O926"] {
        assert_eq!(
            refusal("piers.rocks", date, 1, 0x0c0),
            Some(BuildError::BadDate),
            "{date}"
        );
    }
    assert_eq!(
        refusal("piers.rocks", "20260926", 0, 0x0c0),
        Some(BuildError::BadSigner)
    );
    for first_row in [0x0c1, 0x080, 0x4c0] {
        assert_eq!(
            refusal("piers.rocks", "20260926", 1, first_row),
            Some(BuildError::BadFirstRow),
            "{first_row:#05x}"
        );
    }
}

/// An instance at 0x480 has the area's last 64 rows. fire-24-f's instance
/// takes 54 rows plus the manufacturer's value rows.
#[test]
fn an_instance_must_end_before_the_general_store() {
    assert_eq!(refusal(&"x".repeat(20), "20260926", 1, 0x480), None);
    assert_eq!(
        refusal(&"x".repeat(21), "20260926", 1, 0x480),
        Some(BuildError::DoesNotFit)
    );
}

/// The area's 1024 rows hold fire-24-f's 54 rows and a manufacturer of up to
/// 1940 bytes. The values are refused before they're placed.
#[test]
fn values_too_long_for_the_area_are_refused() {
    let values = |len| CommissioningValues::new(Board::Fire24F, &"x".repeat(len), "20260926", 1);
    assert!(values(1940).is_ok());
    assert_eq!(values(1941).err(), Some(BuildError::DoesNotFit));
}

#[test]
fn an_instance_writes_each_key_row_after_its_value() {
    let order: [u16; 26] = [
        0x0c0, 0x0c1, // magic, version
        0x0c3, 0x0c4, 0x0c5, 0x0c6, 0x0c7, 0x0c8, 0x0c2, // board
        0x0ca, 0x0cb, 0x0cc, 0x0cd, 0x0ce, 0x0cf, 0x0d0, 0x0c9, // manufacturer
        0x0d2, 0x0d3, 0x0d4, 0x0d5, 0x0d6, 0x0d1, // date
        0x0d8, 0x0d9, 0x0d7, // signer
    ];
    let mut expected: Vec<(u16, u16)> = order
        .iter()
        .map(|&row| (row, ROWS[usize::from(row - 0x0c0)]))
        .collect();
    expected.push((0x0db, 64));
    expected.extend((0..32u16).map(|i| {
        let low = 2 * i as u8;
        (0x0dc + i, u16::from_le_bytes([low, low + 1]))
    }));
    expected.push((0x0da, 2));

    let writes: Vec<(u16, u16)> = fire_24_f(0x0c0)
        .writes(&signature())
        .iter()
        .map(|write| (write.row, write.value))
        .collect();
    assert_eq!(writes, expected);
}

#[test]
fn the_message_is_the_prefix_chipid_and_rows_before_the_signature() {
    let mut expected = b"onerom-commissioning-sig-v1".to_vec();
    expected.extend([0x6b, 0x5b, 0x65, 0x2f, 0x23, 0x9c, 0x3f, 0xde]);
    expected.extend(ROWS.iter().flat_map(|row| row.to_le_bytes()));
    assert_eq!(values().message(CHIP_ID), expected);
}

// ---------------------------------------------------------------------------
// Parsing the commissioning area
// ---------------------------------------------------------------------------

#[test]
fn a_written_instance_parses_back() {
    let mut area = blank_area();
    commission(&mut area, &fire_24_f(0x0c0));
    let parsed = CommissioningArea::parse(&area);
    assert!(parsed.issues().is_empty(), "{:?}", parsed.issues());
    assert_eq!(parsed.instances().len(), 1);
    assert_eq!(parsed.next_instance_row(), Some(0x100));

    let current = parsed.current().expect("a current instance");
    assert_eq!(current.first_row(), 0x0c0);
    assert_eq!(current.board(), Some("fire-24-f"));
    assert_eq!(current.manufacturer(), Some("piers.rocks"));
    assert_eq!(current.date(), Some("20260926"));
    assert_eq!(current.signer(), Some(1));
    assert_eq!(current.signature(), Some(&signature()));
    assert!(current.is_valid());
    assert_eq!(current.message(CHIP_ID), Some(values().message(CHIP_ID)));
}

#[test]
fn an_empty_area_has_no_instances() {
    let parsed = CommissioningArea::parse(&blank_area());
    assert!(parsed.instances().is_empty());
    assert!(parsed.issues().is_empty());
    assert!(parsed.current().is_none());
    assert_eq!(parsed.next_instance_row(), Some(0x0c0));
}

/// Fills the area a page at a time and parses it after every write. Until an
/// instance's last write, the instance before it stays current. The last
/// page's instance fills the page, so one cut short leaves no room for key 0
/// and length 0.
#[test]
fn an_interrupted_instance_leaves_the_one_before_current() {
    let mut area = blank_area();
    let mut previous = None;
    while let Some(first_row) = CommissioningArea::parse(&area).next_instance_row() {
        let manufacturer = if first_row == 0x480 {
            "piers.rocks (sample)"
        } else {
            "piers.rocks"
        };
        let values = CommissioningValues::new(Board::Fire24F, manufacturer, "20260926", 1).unwrap();
        let writes = NewCommissioningInstance::new(&values, first_row)
            .unwrap()
            .writes(&signature());
        for n in 1..=writes.len() {
            apply(&mut area, &writes[n - 1..n]);
            let parsed = CommissioningArea::parse(&area);
            let at = format!("instance {first_row:#05x}, {n} writes");
            assert!(parsed.issues().is_empty(), "{at}: {:?}", parsed.issues());
            let current = parsed.current().map(|instance| instance.first_row());
            if n < writes.len() {
                assert_eq!(current, previous, "{at}");
            } else {
                assert_eq!(current, Some(first_row), "{at}");
            }
        }
        previous = Some(first_row);
    }
    assert_eq!(CommissioningArea::parse(&area).instances().len(), 16);
}

/// An instance whose version row is unwritten ends there, and the next
/// instance starts at the following page.
#[test]
fn an_unwritten_version_row_ends_the_instance() {
    let mut area = blank_area();
    area[0] = OTP_STORE_MAGIC;
    commission(&mut area, &fire_24_f(0x100));
    let parsed = CommissioningArea::parse(&area);
    assert_eq!(parsed.instances().len(), 2);
    assert!(parsed.instances()[0].entries().is_empty());
    assert_eq!(parsed.current().map(|i| i.first_row()), Some(0x100));
}

#[test]
fn an_unknown_version_leaves_no_current_instance() {
    let mut area = blank_area();
    commission(&mut area, &fire_24_f(0x0c0));
    area[0x40..0x42].copy_from_slice(&[OTP_STORE_MAGIC, 2]);
    commission(&mut area, &fire_24_f(0x140));
    let parsed = CommissioningArea::parse(&area);
    assert_eq!(
        parsed.issues(),
        [AreaIssue::UnknownVersion {
            row: 0x100,
            version: 2
        }]
    );
    assert_eq!(parsed.instances().len(), 1);
    assert!(parsed.current().is_none());
    assert_eq!(parsed.next_instance_row(), None);
}

/// A page boundary after an instance holding neither the magic value nor 0
/// loses the parser its place. It restarts at the next page boundary holding
/// the magic value.
#[test]
fn a_lost_parser_restarts_at_the_next_instance() {
    let mut area = blank_area();
    commission(&mut area, &fire_24_f(0x0c0));
    area[0x40] = 0x1234;
    commission(&mut area, &fire_24_f(0x140));
    let parsed = CommissioningArea::parse(&area);
    assert_eq!(parsed.issues(), [AreaIssue::LostPlace { row: 0x100 }]);
    assert_eq!(parsed.current().map(|i| i.first_row()), Some(0x140));
    assert_eq!(parsed.next_instance_row(), Some(0x180));
}

#[test]
fn a_lost_parser_without_a_later_instance_leaves_no_current_instance() {
    let mut area = blank_area();
    commission(&mut area, &fire_24_f(0x0c0));
    area[0x40] = 0x1234;
    let parsed = CommissioningArea::parse(&area);
    assert_eq!(parsed.issues(), [AreaIssue::LostPlace { row: 0x100 }]);
    assert!(parsed.current().is_none());
    assert_eq!(parsed.next_instance_row(), None);
}

#[test]
fn an_entry_running_past_the_area_loses_the_parser_its_place() {
    let mut area = blank_area();
    commission(&mut area, &fire_24_f(0x0c0));
    commission(&mut area, &fire_24_f(0x100));
    // The board entry's length row, in the instance at 0x100.
    area[0x43] = 0xffff;
    commission(&mut area, &fire_24_f(0x140));
    let parsed = CommissioningArea::parse(&area);
    assert_eq!(parsed.issues(), [AreaIssue::LostPlace { row: 0x102 }]);
    assert_eq!(parsed.instances().len(), 3);
    assert_eq!(parsed.current().map(|i| i.first_row()), Some(0x140));
}

/// The last complete instance is current only where it's valid. An earlier
/// valid instance doesn't take its place.
#[test]
fn an_invalid_last_instance_leaves_no_current_instance() {
    let mut area = blank_area();
    commission(&mut area, &fire_24_f(0x0c0));
    let mut entries = valid_entries();
    entries.remove(2); // the date
    put(&mut area, 0x100, &entries);
    let parsed = CommissioningArea::parse(&area);
    assert!(parsed.issues().is_empty(), "{:?}", parsed.issues());
    let last = &parsed.instances()[1];
    assert_eq!(last.date(), None);
    assert!(!last.is_valid());
    assert!(parsed.current().is_none());
}

/// The last entry with a key is used. An unknown key and a deleted entry are
/// skipped using their lengths.
#[test]
fn entries_follow_the_store_rules() {
    let unknown = OneromOtpEntry::Unknown {
        key: 0x1234,
        params: vec![1, 2, 3],
    };
    let mut entries = valid_entries();
    entries.insert(1, rows_of(&unknown)); // rows 0x0c9–0x0cc
    entries.insert(2, vec![0, 3, 0x0201, 0x0003]); // rows 0x0cd–0x0d0
    entries.insert(
        3,
        rows_of(&OneromOtpEntry::OtpKeyCommissioningBoard {
            name: "fire-28-a".into(),
        }),
    );
    let mut area = blank_area();
    put(&mut area, 0x0c0, &entries);
    let parsed = CommissioningArea::parse(&area);
    let current = parsed.current().expect("a current instance");
    assert_eq!(current.board(), Some("fire-28-a"));
    let parsed_entries = current.entries();
    assert!(parsed_entries.contains(&StoreEntry::Entry {
        row: 0x0c9,
        entry: unknown
    }));
    assert!(parsed_entries.contains(&StoreEntry::Deleted { row: 0x0cd, len: 3 }));
}

/// A string that isn't UTF-8, or a fixed-size value at another length, is
/// unreadable, and its instance isn't valid.
#[test]
fn an_unreadable_value_makes_its_instance_invalid() {
    let mut long_signature = vec![2, 66];
    long_signature.extend([0; 33]);
    let cases: [(usize, Vec<u16>); 4] = [
        (0, vec![1, 2, 0xfeff]),         // board, not UTF-8
        (3, vec![5, 4, 1, 0]),           // signer, 4 bytes
        (4, long_signature),             // signature, 66 bytes
        (4, vec![2, 10, 0, 0, 0, 0, 0]), // signature, 10 bytes
    ];
    for (index, rows) in cases {
        let (key, len) = (rows[0], rows[1]);
        let mut entries = valid_entries();
        entries[index] = rows;
        let row = 0x0c2 + entries[..index].iter().map(Vec::len).sum::<usize>() as u16;
        let mut area = blank_area();
        put(&mut area, 0x0c0, &entries);
        let parsed = CommissioningArea::parse(&area);
        let instance = &parsed.instances()[0];
        assert!(
            instance
                .entries()
                .contains(&StoreEntry::Unreadable { row, key, len }),
            "key {key}, length {len}: {:?}",
            instance.entries()
        );
        assert!(!instance.is_valid(), "key {key}, length {len}");
        assert!(parsed.current().is_none(), "key {key}, length {len}");
    }
}

// ---------------------------------------------------------------------------
// Parsing the general store
// ---------------------------------------------------------------------------

/// An empty general store and its terminator.
fn blank_store() -> Vec<u16> {
    let end = OTP_GENERAL_STORE_TERMINATOR_ROW + OTP_GENERAL_STORE_TERMINATOR_ROW_COUNT;
    vec![0; usize::from(end - OTP_GENERAL_STORE_FIRST_ROW)]
}

/// A tool starts the store with its magic row, then its version row.
#[test]
fn a_general_store_is_absent_until_its_version_row_is_written() {
    let mut store = blank_store();
    assert_eq!(GeneralStore::parse(&store), None);
    store[0] = OTP_STORE_MAGIC;
    assert_eq!(GeneralStore::parse(&store), None);
}

#[test]
fn a_general_store_with_an_unknown_version_is_reported_and_not_parsed() {
    let mut store = blank_store();
    store[..5].copy_from_slice(&[OTP_STORE_MAGIC, 2, 0x0010, 0x0002, 0x1234]);
    let parsed = GeneralStore::parse(&store).expect("a general store");
    assert!(parsed.entries().is_empty());
    assert_eq!(
        parsed.issues(),
        [AreaIssue::UnknownVersion {
            row: 0x4c0,
            version: 2
        }]
    );
}

/// The terminator's unwritten rows end a full store with key 0 and length 0.
#[test]
fn a_full_general_store_ends_at_its_terminator() {
    // One entry fills the store: key and length rows at 0x4c2–0x4c3, and its
    // value to the store's last row.
    let value_rows = usize::from(OTP_GENERAL_STORE_LAST_ROW - 0x4c4 + 1);
    let mut store = blank_store();
    store[..4].copy_from_slice(&[
        OTP_STORE_MAGIC,
        OTP_STORE_VERSION,
        0x0010,
        2 * value_rows as u16,
    ]);
    store[4..4 + value_rows].fill(0xaaaa);
    let parsed = GeneralStore::parse(&store).expect("a general store");
    assert!(parsed.issues().is_empty(), "{:?}", parsed.issues());
    assert_eq!(
        parsed.entries(),
        [StoreEntry::Entry {
            row: 0x4c2,
            entry: OneromOtpEntry::Unknown {
                key: 0x0010,
                params: vec![0xaa; 2 * value_rows],
            },
        }]
    );
}

#[test]
fn a_general_store_entry_running_past_the_end_loses_the_parser_its_place() {
    let mut store = blank_store();
    store[..4].copy_from_slice(&[OTP_STORE_MAGIC, OTP_STORE_VERSION, 0x0010, 0xffff]);
    let parsed = GeneralStore::parse(&store).expect("a general store");
    assert!(parsed.entries().is_empty());
    assert_eq!(parsed.issues(), [AreaIssue::LostPlace { row: 0x4c2 }]);
}
