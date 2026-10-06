// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for commissioning a board and setting its size against the in-memory
//! OTP.

use ed25519_dalek::{Signature, Signer as _, SigningKey};
use onerom_app::{
    BoardSize, BoardSizeError, CommissionError, Interruption, LocalOtpAccess, MemoryOtp, OtpError,
    Plan, Request, RequestDate, RowValue, StepKind, plan_size, prepare, read_commissioning,
};
use onerom_config::hw::{Board, Model};
use onerom_metadata::otp::pico_otp::ecc_encode;
use onerom_metadata::otp::{
    AreaIssue, BuildError, CommissioningValues, NewCommissioningInstance, RowWrite, white_label,
};
use onerom_metadata::{OTP_STORE_MAGIC, OTP_STORE_VERSION, OneromBoardSize};
use serde_json::json;

/// CHIPID from rows 0x000–0x003 of an RP2350 A4.
const CHIP_ID: [u16; 4] = [0x5b6b, 0x2f65, 0x9c23, 0xde3f];

/// The date the tests commission on.
const DATE: &str = "20260926";

/// The day after [`DATE`].
const NEXT_DAY: &str = "20260927";

/// fire-24-f's white label from onerom-metadata's `tests/otp_white_label.rs`.
/// It was worked out by hand from OTP.md's table.
#[rustfmt::skip]
const FIRE_24_F_ROWS: [u16; 57] = [
    0x1209, 0xf540, 0x0000, 0x0000, 0x100b, 0x1612, 0x0000, 0x0000, // table
    0x1f06, 0x0000, 0x0000, 0x0000, 0x2212, 0x2b0a, 0x3007, 0x3409, // table
    0x6970, 0x7265, 0x2e73, 0x6f72, 0x6b63, 0x0073, // "piers.rocks"
    0x6e4f, 0x2065, 0x4f52, 0x204d, 0x6f42, 0x746f, 0x6f6c, 0x6461, 0x7265, // "One ROM Bootloader"
    0x4e4f, 0x5245, 0x4d4f, // "ONEROM"
    0x7468, 0x7074, 0x3a73, 0x2f2f, 0x6e6f, 0x7265, 0x6d6f, 0x6f2e, 0x6772, // "https://onerom.org"
    0x6e6f, 0x7265, 0x6d6f, 0x6f2e, 0x6772, // "onerom.org"
    0x6e4f, 0x2065, 0x4f52, 0x004d, // "One ROM"
    0x6966, 0x6572, 0x322d, 0x2d34, 0x0066, // "fire-24-f"
];

/// USB_BOOT_FLAGS for One ROM's white label. The same test holds it.
const USB_BOOT_FLAGS: u32 = 0x0040_f133;

/// The key the tests sign with.
fn key() -> SigningKey {
    SigningKey::from_bytes(&[1; 32])
}

/// A board with only CHIPID written.
fn board() -> MemoryOtp {
    let mut otp = MemoryOtp::new();
    for (row, value) in (0..).zip(CHIP_ID) {
        otp.set_raw(row, ecc_encode(value));
    }
    otp
}

/// A reset board holding `otp`'s rows. Its write count is 0.
fn copy(otp: &MemoryOtp) -> MemoryOtp {
    let mut copy = MemoryOtp::new();
    for (row, &raw) in (0..).zip(otp.rows()) {
        copy.set_raw(row, raw);
    }
    copy.reset();
    copy
}

/// A request to commission `board` as `size` from piers.rocks today with
/// signer 1.
fn request(board: Board, size: BoardSize) -> Request {
    Request {
        board,
        size,
        manufacturer: "piers.rocks".into(),
        date: RequestDate::Today(DATE.into()),
        signer: 1,
        force: false,
    }
}

/// Prepares `request` and plans it with its message signed by [`key`].
async fn plan<O: LocalOtpAccess>(otp: &mut O, request: &Request) -> Result<Plan, CommissionError> {
    let prepared = prepare(otp, request).await?;
    let signature = key().sign(prepared.message()).to_bytes();
    prepared.plan(&signature)
}

/// Commissions `otp` as `request` says.
async fn commission<O: LocalOtpAccess>(
    otp: &mut O,
    request: &Request,
) -> Result<(), CommissionError> {
    plan(otp, request).await?.execute(otp, |_| {}).await
}

/// The writes of `request`'s instance on `date` at `first_row`. [`key`] signs
/// it.
fn instance(request: &Request, date: &str, first_row: u16) -> Vec<RowWrite> {
    let values =
        CommissioningValues::new(request.board, &request.manufacturer, date, request.signer)
            .unwrap();
    let signature = key().sign(&values.message(CHIP_ID)).to_bytes();
    NewCommissioningInstance::new(&values, first_row)
        .unwrap()
        .writes(&signature)
}

/// Writes `writes` to `otp` as exact codewords. They don't add to the board's
/// write count.
fn put(otp: &mut MemoryOtp, writes: &[RowWrite]) {
    for write in writes {
        otp.set_raw(write.row, ecc_encode(write.value));
    }
}

/// The current instance's first row.
async fn current_row(otp: &mut MemoryOtp) -> Option<u16> {
    read_commissioning(otp)
        .await
        .unwrap()
        .current()
        .map(|instance| instance.first_row())
}

/// Row `row`'s raw value.
fn raw(otp: &MemoryOtp, row: u16) -> u32 {
    otp.rows()[usize::from(row)]
}

/// Checks every row of `otp` against `expected`.
fn assert_rows(otp: &MemoryOtp, expected: &[u32], at: &str) {
    for (row, (&actual, &expected)) in otp.rows().iter().zip(expected).enumerate() {
        assert_eq!(actual, expected, "{at}: row {row:#05x}");
    }
}

/// Page n's LOCK1 word is row 0xf81 + 2n.
fn lock1_row(page: u16) -> u16 {
    0xf81 + 2 * page
}

// ---------------------------------------------------------------------------
// Board sizes
// ---------------------------------------------------------------------------

#[test]
fn a_board_size_is_m_or_l_in_either_case() {
    for (text, size) in [
        ("m", BoardSize::M),
        ("M", BoardSize::M),
        ("l", BoardSize::L),
        ("L", BoardSize::L),
    ] {
        assert_eq!(text.parse(), Ok(size), "{text}");
    }
    // XL is refused like any other text.
    for text in ["xl", "XL", "Xl", "Q", "", "LL", "XXL"] {
        assert_eq!(
            text.parse::<BoardSize>(),
            Err(BoardSizeError::Unknown),
            "{text}"
        );
    }
}

#[test]
fn a_board_size_displays_and_serializes_as_its_letter() {
    for (size, letter) in [(BoardSize::M, "M"), (BoardSize::L, "L")] {
        assert_eq!(size.to_string(), letter);
        assert_eq!(serde_json::to_value(size).unwrap(), json!(letter));
    }
}

// ---------------------------------------------------------------------------
// Blank boards
// ---------------------------------------------------------------------------

/// The rows commissioning `request` leaves on a blank board. `white_label`
/// holds the white label's rows.
fn commissioned(request: &Request, white_label: &[u16]) -> Vec<u32> {
    let mut rows = board().rows().to_vec();
    let mut set = |row: u16, raw: u32| rows[usize::from(row)] = raw;
    for write in instance(request, DATE, 0x0c0) {
        set(write.row, ecc_encode(write.value));
    }
    set(lock1_row(3), 0x15_1515);
    for (row, &value) in (0xec0..).zip(white_label) {
        if value != 0 {
            set(row, ecc_encode(value));
        }
    }
    set(0x05c, ecc_encode(0xec0));
    for row in 0x059..=0x05b {
        set(row, USB_BOOT_FLAGS);
    }
    if request.size == BoardSize::L {
        set(0x054, 0x3a_99af);
        for row in 0x048..=0x04a {
            set(row, 0x20);
        }
    }
    rows
}

#[tokio::test]
async fn an_m_board_is_commissioned() {
    let request = request(Board::Fire24F, BoardSize::M);
    let mut otp = board();
    let prepared = prepare(&mut otp, &request).await.unwrap();
    assert_eq!(prepared.chip_id(), CHIP_ID);
    assert_eq!(prepared.date(), DATE);
    assert!(!prepared.date_from_current_instance());
    let values = CommissioningValues::new(Board::Fire24F, "piers.rocks", DATE, 1).unwrap();
    assert_eq!(prepared.message(), values.message(CHIP_ID));

    let signature = key().sign(prepared.message()).to_bytes();
    let plan = prepared.plan(&signature).unwrap();
    // The table rows that aren't 0.
    let table = plan
        .steps()
        .iter()
        .find(|step| step.kind == StepKind::WhiteLabelTable)
        .unwrap();
    let rows: Vec<u16> = table.writes.iter().map(|write| write.row).collect();
    assert_eq!(
        rows,
        [
            0xec0, 0xec1, 0xec4, 0xec5, 0xec8, 0xecc, 0xecd, 0xece, 0xecf
        ]
    );
    plan.execute(&mut otp, |_| {}).await.unwrap();

    assert_rows(&otp, &commissioned(&request, &FIRE_24_F_ROWS), "fire-24-f");
    let area = read_commissioning(&mut otp).await.unwrap();
    let current = area.current().unwrap();
    assert_eq!(current.first_row(), 0x0c0);
    assert_eq!(current.board(), Some("fire-24-f"));
    assert_eq!(current.manufacturer(), Some("piers.rocks"));
    assert_eq!(current.date(), Some(DATE));
    assert_eq!(current.signer(), Some(1));
    let signature = Signature::from_bytes(current.signature().unwrap());
    key()
        .verifying_key()
        .verify_strict(&current.message(CHIP_ID).unwrap(), &signature)
        .unwrap();
}

#[tokio::test]
async fn an_l_board_is_commissioned_in_otp_mds_order() {
    let request = request(Board::Fire40A, BoardSize::L);
    let mut otp = board();
    let plan = plan(&mut otp, &request).await.unwrap();
    assert!(
        plan.steps()
            .iter()
            .flat_map(|step| &step.writes)
            .all(|write| !write.holds)
    );
    let mut done = Vec::new();
    plan.execute(&mut otp, |step| done.push(step.kind))
        .await
        .unwrap();

    assert_eq!(
        done,
        [
            StepKind::FlashDevinfo,
            StepKind::Instance,
            StepKind::Lock { page: 3 },
            StepKind::WhiteLabelStrings,
            StepKind::WhiteLabelTable,
            StepKind::WhiteLabelAddr,
            StepKind::UsbBootFlags,
            StepKind::BootFlags0,
        ]
    );
    let white_label = white_label(Board::Fire40A).to_otp_data_strict().unwrap();
    assert_rows(
        &otp,
        &commissioned(&request, white_label.rows()),
        "fire-40-a",
    );
}

#[tokio::test]
async fn a_finished_board_writes_nothing_the_second_time() {
    for (board_type, size) in [
        (Board::Fire24F, BoardSize::M),
        (Board::Fire40A, BoardSize::L),
    ] {
        let request = request(board_type, size);
        let mut otp = board();
        commission(&mut otp, &request).await.unwrap();
        let writes = otp.write_count();

        let plan = plan(&mut otp, &request).await.unwrap();
        assert!(
            plan.steps()
                .iter()
                .flat_map(|step| &step.writes)
                .all(|write| write.holds),
            "{board_type}"
        );
        let mut steps = 0;
        plan.execute(&mut otp, |_| steps += 1).await.unwrap();
        assert_eq!(otp.write_count(), writes, "{board_type}");
        assert_eq!(steps, plan.steps().len(), "{board_type}");
    }
}

/// A board commissioned before the white label held the bootloader's VID and
/// PID can be given them by hand. Commissioning it again then writes nothing.
#[tokio::test]
async fn a_board_given_the_vid_and_pid_by_hand_writes_nothing() {
    let request = request(Board::Fire24F, BoardSize::M);
    let mut otp = board();
    commission(&mut otp, &request).await.unwrap();
    // The rows as commissioning left them before the VID and PID.
    otp.set_raw(0xec0, 0);
    otp.set_raw(0xec1, 0);
    for row in 0x059..=0x05b {
        otp.set_raw(row, 0x40_f130);
    }
    // The VID, the PID and their USB_BOOT_FLAGS bits, written by hand.
    otp.write_ecc(0xec0, 0x1209).await.unwrap();
    otp.write_ecc(0xec1, 0xf540).await.unwrap();
    for row in 0x059..=0x05b {
        otp.write_raw(row, 0x40_f133).await.unwrap();
    }
    let mut otp = copy(&otp);
    let plan = plan(&mut otp, &request).await.unwrap();
    assert!(
        plan.steps()
            .iter()
            .flat_map(|step| &step.writes)
            .all(|write| write.holds)
    );
    plan.execute(&mut otp, |_| {}).await.unwrap();
    assert_eq!(otp.write_count(), 0);
}

// ---------------------------------------------------------------------------
// Interrupted runs
// ---------------------------------------------------------------------------

/// Interrupts a run of `request` on `start` at each of its writes in turn.
/// Each write is interrupted once landing and once not. After each
/// interruption it resets the board and runs it again.
///
/// After each interruption:
/// - a USB_BOOT_FLAGS copy is set only where every white label row holds its
///   value
/// - a BOOT_FLAGS0 copy enables FLASH_DEVINFO only where FLASH_DEVINFO holds
///   its value
/// - the instance current before the run stays current until the new
///   instance's last row lands
///
/// Each run after an interruption ends with the rows of a run that wasn't
/// interrupted.
async fn interrupt_every_write(start: MemoryOtp, request: Request) {
    let mut reference = start.clone();
    let plan = plan(&mut reference, &request).await.unwrap();
    plan.execute(&mut reference, |_| {}).await.unwrap();
    let expected = reference.rows();
    let to_write = plan
        .steps()
        .iter()
        .flat_map(|step| &step.writes)
        .filter(|write| !write.holds)
        .count();
    assert_eq!(reference.write_count(), to_write);

    let rows_of = |kinds: &[StepKind]| -> Vec<u16> {
        plan.steps()
            .iter()
            .filter(|step| kinds.contains(&step.kind))
            .flat_map(|step| step.writes.iter().map(|write| write.row))
            .collect()
    };
    let instance = rows_of(&[StepKind::Instance]);
    let white_label = rows_of(&[
        StepKind::WhiteLabelStrings,
        StepKind::WhiteLabelTable,
        StepKind::WhiteLabelAddr,
    ]);
    let before = current_row(&mut start.clone()).await;
    let holds = |otp: &MemoryOtp, row: u16| raw(otp, row) == expected[usize::from(row)];

    for n in 1..=reference.write_count() {
        for interruption in [Interruption::Landed, Interruption::NotLanded] {
            let at = format!("write {n}, {interruption:?}");
            let mut otp = start.clone();
            otp.interrupt(n, interruption);
            let error = commission(&mut otp, &request).await.unwrap_err();
            assert!(
                matches!(
                    error,
                    CommissionError::Otp {
                        error: OtpError::Transport(_),
                        ..
                    }
                ),
                "{at}: {error}"
            );

            if (0x059..=0x05b).any(|row| raw(&otp, row) != 0) {
                for &row in &white_label {
                    assert!(holds(&otp, row), "{at}: row {row:#05x}");
                }
            }
            if (0x048..=0x04a).any(|row| raw(&otp, row) & 0x20 != 0) {
                assert!(holds(&otp, 0x054), "{at}");
            }
            let last = *instance.last().unwrap();
            let current = if holds(&otp, last) {
                Some(instance[0])
            } else {
                before
            };
            assert_eq!(current_row(&mut otp).await, current, "{at}");

            otp.reset();
            commission(&mut otp, &request)
                .await
                .unwrap_or_else(|e| panic!("{at}: {e}"));
            assert_rows(&otp, expected, &at);
        }
    }
}

#[tokio::test]
async fn an_l_run_interrupted_at_any_write_finishes_the_second_time() {
    interrupt_every_write(board(), request(Board::Fire40A, BoardSize::L)).await;
}

/// A commissioned board commissioned again with `force`.
#[tokio::test]
async fn a_forced_run_interrupted_at_any_write_finishes_the_second_time() {
    let mut otp = board();
    commission(&mut otp, &request(Board::Fire24F, BoardSize::M))
        .await
        .unwrap();
    let request = Request {
        manufacturer: "Acme".into(),
        force: true,
        ..request(Board::Fire24F, BoardSize::M)
    };
    interrupt_every_write(copy(&otp), request).await;
}

/// The new date's instance can't take the rows the first run wrote so it goes
/// on the next page.
#[tokio::test]
async fn a_next_day_run_after_an_interrupted_instance_starts_a_new_one() {
    let request = request(Board::Fire24F, BoardSize::M);
    let mut otp = board();
    // An M run writes the instance first. Its last write is the signature's
    // key row.
    let writes = instance(&request, DATE, 0x0c0).len();
    otp.interrupt(writes, Interruption::NotLanded);
    commission(&mut otp, &request).await.unwrap_err();
    assert_eq!(current_row(&mut otp).await, None);

    otp.reset();
    let next_day = Request {
        date: RequestDate::Today(NEXT_DAY.into()),
        ..request
    };
    commission(&mut otp, &next_day).await.unwrap();

    let area = read_commissioning(&mut otp).await.unwrap();
    let rows: Vec<u16> = area
        .instances()
        .iter()
        .map(|instance| instance.first_row())
        .collect();
    assert_eq!(rows, [0x0c0, 0x100]);
    let current = area.current().unwrap();
    assert_eq!(current.first_row(), 0x100);
    assert_eq!(current.date(), Some(NEXT_DAY));
    assert_eq!(raw(&otp, lock1_row(4)), 0x15_1515);
}

/// The instance is complete so a run on the next day finishes it with the
/// instance's date. A run given the next day's date is a new commissioning.
#[tokio::test]
async fn a_next_day_run_after_the_signature_finishes_with_the_instances_date() {
    let request = request(Board::Fire24F, BoardSize::M);
    let mut expected = board();
    commission(&mut expected, &request).await.unwrap();

    let mut otp = board();
    // The lock follows the instance.
    let lock = instance(&request, DATE, 0x0c0).len() + 1;
    otp.interrupt(lock, Interruption::NotLanded);
    commission(&mut otp, &request).await.unwrap_err();
    otp.reset();
    let unfinished = otp.rows().to_vec();

    let given = Request {
        date: RequestDate::Given(NEXT_DAY.into()),
        ..request.clone()
    };
    assert_eq!(
        commission(&mut otp, &given).await,
        Err(CommissionError::AlreadyCommissioned {
            row: 0x0c0,
            board: "fire-24-f".into(),
            manufacturer: "piers.rocks".into(),
            date: DATE.into(),
            signer: 1,
            only_date_differs: true,
        })
    );
    assert_rows(&otp, &unfinished, "given the next day");

    let today = Request {
        date: RequestDate::Today(NEXT_DAY.into()),
        ..request
    };
    let prepared = prepare(&mut otp, &today).await.unwrap();
    assert!(prepared.date_from_current_instance());
    assert_eq!(prepared.date(), DATE);
    let signature = key().sign(prepared.message()).to_bytes();
    let plan = prepared.plan(&signature).unwrap();
    plan.execute(&mut otp, |_| {}).await.unwrap();
    assert_rows(&otp, expected.rows(), "today the next day");
}

// ---------------------------------------------------------------------------
// Boards with data
// ---------------------------------------------------------------------------

#[tokio::test]
async fn force_makes_a_second_instance_current() {
    let mut otp = board();
    commission(&mut otp, &request(Board::Fire24F, BoardSize::M))
        .await
        .unwrap();
    otp.reset();
    let acme = Request {
        manufacturer: "Acme".into(),
        ..request(Board::Fire24F, BoardSize::M)
    };
    assert_eq!(
        commission(&mut otp, &acme).await,
        Err(CommissionError::AlreadyCommissioned {
            row: 0x0c0,
            board: "fire-24-f".into(),
            manufacturer: "piers.rocks".into(),
            date: DATE.into(),
            signer: 1,
            only_date_differs: false,
        })
    );

    let forced = Request {
        force: true,
        ..acme
    };
    // The forced run replaces the instance in use. One matching it doesn't.
    let prepared = prepare(&mut otp, &forced).await.unwrap();
    let replaced = prepared
        .replaced_instance()
        .map(|instance| instance.first_row());
    assert_eq!(replaced, Some(0x0c0));
    let matching = prepare(&mut otp, &request(Board::Fire24F, BoardSize::M))
        .await
        .unwrap();
    assert!(matching.replaced_instance().is_none());

    commission(&mut otp, &forced).await.unwrap();
    let area = read_commissioning(&mut otp).await.unwrap();
    assert_eq!(area.instances().len(), 2);
    let current = area.current().unwrap();
    assert_eq!(current.first_row(), 0x100);
    assert_eq!(current.manufacturer(), Some("Acme"));
    assert_eq!(raw(&otp, lock1_row(4)), 0x15_1515);
}

/// A run interrupted after the magic row leaves an instance ending at its
/// version row.
#[tokio::test]
async fn an_instance_with_an_unwritten_version_row_is_finished() {
    let request = request(Board::Fire24F, BoardSize::M);
    let mut expected = board();
    commission(&mut expected, &request).await.unwrap();

    let mut otp = board();
    otp.set_raw(0x0c0, ecc_encode(OTP_STORE_MAGIC));
    commission(&mut otp, &request).await.unwrap();
    assert_rows(&otp, expected.rows(), "magic row written");
}

/// The bootrom writes a row's encoding inverted where the encoding lacks the
/// row's factory bit.
#[tokio::test]
async fn a_factory_bit_is_kept_by_the_inverted_encoding() {
    let request = request(Board::Fire40A, BoardSize::L);
    let magic = ecc_encode(OTP_STORE_MAGIC);
    // Rows 0x0c2 and 0xed0 hold the board's key and the first two characters
    // of "piers.rocks".
    let values = [
        (0x0c0, OTP_STORE_MAGIC),
        (0x0c2, 1),
        (0xed0, 0x6970),
        (0x054, 0x99af),
        (0x05c, 0xec0),
    ];
    let mut otp = board();
    let mut expected = Vec::new();
    for (row, value) in values {
        let encoding = ecc_encode(value);
        // The magic row's factory bit is one its encoding has.
        let bit = if row == 0x0c0 {
            magic.isolate_lowest_one()
        } else {
            1 << (0..22).find(|bit| encoding & (1 << bit) == 0).unwrap()
        };
        otp.set_raw(row, bit);
        let kept = if encoding & bit == 0 {
            !encoding & 0xff_ffff
        } else {
            encoding
        };
        expected.push((row, kept));
    }
    // A factory bit in a BOOT_FLAGS0 copy.
    otp.set_raw(0x049, 0x100);

    commission(&mut otp, &request).await.unwrap();

    for (row, kept) in expected {
        assert_eq!(raw(&otp, row), kept, "row {row:#05x}");
    }
    assert_eq!(raw(&otp, 0x0c2) >> 22, 0b11);
    assert_eq!(raw(&otp, 0x049), 0x120);
    let area = read_commissioning(&mut otp).await.unwrap();
    assert_eq!(area.current().unwrap().board(), Some("fire-40-a"));
}

/// Finishing the matching instance at 0x0c0 wouldn't make it current. The
/// parser would still lose its place at 0x100 and not find a later instance.
/// The new instance goes after the last written row.
#[tokio::test]
async fn an_area_the_parser_abandons_takes_the_page_after_the_last_written_row() {
    let request = request(Board::Fire24F, BoardSize::M);
    let mut otp = board();
    put(&mut otp, &instance(&request, DATE, 0x0c0));
    otp.set_raw(0x100, ecc_encode(0x1234));
    otp.set_raw(0x105, ecc_encode(0x1234));

    commission(&mut otp, &request).await.unwrap();

    let area = read_commissioning(&mut otp).await.unwrap();
    assert_eq!(area.issues(), [AreaIssue::LostPlace { row: 0x100 }]);
    assert_eq!(area.current().unwrap().first_row(), 0x140);
    assert_eq!(raw(&otp, lock1_row(5)), 0x15_1515);
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// Why commissioning `request` on `otp` is refused. Checks that the run didn't
/// write anything.
async fn refusal(mut otp: MemoryOtp, request: &Request) -> CommissionError {
    let before = otp.rows().to_vec();
    let writes = otp.write_count();
    let error = commission(&mut otp, request).await.unwrap_err();
    assert_eq!(otp.write_count(), writes, "{error}");
    assert_rows(&otp, &before, &error.to_string());
    error
}

/// `request` with `force` set.
fn forced(request: &Request) -> Request {
    Request {
        force: true,
        ..request.clone()
    }
}

/// A board holding a complete instance of `request`'s at 0x0c0.
fn with_instance(request: &Request) -> MemoryOtp {
    let mut otp = board();
    put(&mut otp, &instance(request, DATE, 0x0c0));
    otp
}

/// An entry's rows:
/// - its key
/// - its length
/// - its value
///
/// The value takes two bytes per row, low byte first.
fn entry(key: u16, value: &[u8]) -> Vec<u16> {
    let mut rows = vec![key, value.len() as u16];
    rows.extend(
        value
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair.get(1).copied().unwrap_or(0)])),
    );
    rows
}

/// Newer tooling probably wrote an unknown version or an unknown key so
/// `force` doesn't override either refusal.
#[tokio::test]
async fn newer_data_is_refused_even_with_force() {
    let m = request(Board::Fire24F, BoardSize::M);
    let mut newer = board();
    newer.set_raw(0x0c0, ecc_encode(OTP_STORE_MAGIC));
    newer.set_raw(0x0c1, ecc_encode(2));
    for request in [m.clone(), forced(&m)] {
        assert_eq!(
            refusal(newer.clone(), &request).await,
            CommissionError::NewerData {
                row: 0x0c0,
                version: 2
            }
        );
    }

    // A valid instance with an unknown key after its board.
    let rows: Vec<u16> = [
        vec![OTP_STORE_MAGIC, OTP_STORE_VERSION],
        entry(1, b"fire-24-f"),
        entry(0x10, &[1, 2]),
        entry(3, b"piers.rocks"),
        entry(4, DATE.as_bytes()),
        entry(5, &1_u16.to_le_bytes()),
        entry(2, &[0; 64]),
    ]
    .concat();
    let mut unknown = board();
    for (row, value) in (0x0c0..).zip(rows) {
        unknown.set_raw(row, ecc_encode(value));
    }
    for request in [m.clone(), forced(&m)] {
        assert_eq!(
            refusal(unknown.clone(), &request).await,
            CommissionError::UnknownKey {
                row: 0x0c9,
                key: 0x10
            }
        );
    }
}

/// Every Ice board is refused whatever `force` says. Every Fire board is
/// prepared.
#[tokio::test]
async fn only_a_fire_board_is_commissioned() {
    for &board_type in Model::Ice.boards() {
        for request in [
            request(board_type, BoardSize::M),
            forced(&request(board_type, BoardSize::L)),
        ] {
            assert_eq!(
                refusal(board(), &request).await,
                CommissionError::NotFire(board_type)
            );
        }
    }
    for &board_type in Model::Fire.boards() {
        prepare(&mut board(), &request(board_type, BoardSize::M))
            .await
            .unwrap_or_else(|e| panic!("{board_type}: {e}"));
    }
}

#[tokio::test]
async fn values_that_cant_make_an_instance_are_refused() {
    let m = request(Board::Fire24F, BoardSize::M);
    let cases = [
        (
            Request {
                manufacturer: String::new(),
                ..m.clone()
            },
            BuildError::EmptyManufacturer,
        ),
        (
            Request {
                date: RequestDate::Given("2026926".into()),
                ..m.clone()
            },
            BuildError::BadDate,
        ),
        (
            Request {
                signer: 0,
                ..m.clone()
            },
            BuildError::BadSigner,
        ),
        (
            Request {
                manufacturer: "x".repeat(1941),
                ..m.clone()
            },
            BuildError::DoesNotFit,
        ),
    ];
    for (request, error) in cases {
        assert_eq!(
            refusal(board(), &request).await,
            CommissionError::Build(error)
        );
    }
}

#[tokio::test]
async fn an_l_board_needs_an_external_flash_pin_and_a_valid_slot_size() {
    let l = request(Board::Fire40A, BoardSize::L);
    assert_eq!(
        refusal(board(), &request(Board::Fire24F, BoardSize::L)).await,
        CommissionError::NoExternalFlash(Board::Fire24F)
    );

    // An ECC read corrects a single wrong bit. A row with one is refused
    // whatever `force` says.
    let mut slot = board();
    slot.set_raw(0x055, 0x100);
    for request in [l.clone(), forced(&l)] {
        assert_eq!(
            refusal(slot.clone(), &request).await,
            CommissionError::SlotSizeInvalid { raw: 0x100 }
        );
    }

    let mut devinfo = board();
    devinfo.set_raw(0x054, ecc_encode(0x99a0));
    assert_eq!(
        refusal(devinfo, &l).await,
        CommissionError::RowWritten {
            row: 0x054,
            raw: ecc_encode(0x99a0),
            value: RowValue::Ecc(0x99af),
        }
    );
}

/// An M run writes neither row so `force` lets it go ahead.
#[tokio::test]
async fn an_m_board_configuring_a_second_chip_needs_force() {
    let m = request(Board::Fire40A, BoardSize::M);
    let mut devinfo = board();
    devinfo.set_raw(0x054, ecc_encode(0x99af));
    let mut enabled = board();
    enabled.set_raw(0x04a, 0x20);
    for otp in [devinfo, enabled] {
        assert_eq!(
            refusal(otp.clone(), &m).await,
            CommissionError::SecondChipConfigured(Board::Fire40A)
        );
        let mut otp = otp;
        let before: Vec<u32> = [0x048, 0x049, 0x04a, 0x054]
            .map(|row| raw(&otp, row))
            .to_vec();
        commission(&mut otp, &forced(&m)).await.unwrap();
        let after: Vec<u32> = [0x048, 0x049, 0x04a, 0x054]
            .map(|row| raw(&otp, row))
            .to_vec();
        assert_eq!(after, before);
    }
}

/// Every BOOT_FLAGS0 copy enables FLASH_DEVINFO and it has a 2MB chip on chip
/// select 1. Bits can't be cleared so `force` can't make the board M.
#[tokio::test]
async fn an_m_request_for_an_l_board_is_refused_even_with_force() {
    let m = request(Board::Fire40A, BoardSize::M);
    let mut otp = board();
    otp.set_raw(0x054, ecc_encode(0x99af));
    for row in 0x048..=0x04a {
        otp.set_raw(row, 0x20);
    }
    for request in [m.clone(), forced(&m)] {
        assert_eq!(
            refusal(otp.clone(), &request).await,
            CommissionError::SizeAlreadySet {
                size: OneromBoardSize::BoardSizeL,
                requested: BoardSize::M,
            }
        );
    }
}

/// FLASH_DEVINFO has 4MB on chip select 1 and every BOOT_FLAGS0 copy enables
/// it. The board is neither M nor L so every size is refused, whatever `force`
/// says.
#[tokio::test]
async fn a_board_of_size_other_is_refused_every_size_even_with_force() {
    let mut otp = board();
    otp.set_raw(0x054, ecc_encode(0xa9af));
    for row in 0x048..=0x04a {
        otp.set_raw(row, 0x20);
    }
    for &size in BoardSize::supported_values() {
        let request = request(Board::Fire40A, size);
        for request in [request.clone(), forced(&request)] {
            assert_eq!(
                refusal(otp.clone(), &request).await,
                CommissionError::SizeAlreadySet {
                    size: OneromBoardSize::BoardSizeOther,
                    requested: size,
                },
                "{size}"
            );
        }
    }
}

#[tokio::test]
async fn a_white_label_row_holding_another_value_is_refused() {
    let m = request(Board::Fire24F, BoardSize::M);
    let other = ecc_encode(0x1234);
    // Rows:
    // - the first string row
    // - the manufacturer's table row
    // - USB_WHITE_LABEL_ADDR
    for (row, value) in [(0xed0, 0x6970), (0xec4, 0x100b), (0x05c, 0xec0)] {
        let mut otp = board();
        otp.set_raw(row, other);
        assert_eq!(
            refusal(otp, &m).await,
            CommissionError::RowWritten {
                row,
                raw: other,
                value: RowValue::Ecc(value),
            }
        );
    }

    let mut otp = board();
    otp.set_raw(0x05a, 0x40_0000);
    assert_eq!(
        refusal(otp, &m).await,
        CommissionError::RowWritten {
            row: 0x05a,
            raw: 0x40_0000,
            value: RowValue::Raw(USB_BOOT_FLAGS),
        }
    );
}

#[tokio::test]
async fn a_full_area_is_refused() {
    let m = request(Board::Fire24F, BoardSize::M);
    // An instance on every page. The last page's instance fills its page.
    let mut full = board();
    for first_row in (0x0c0..0x4c0).step_by(64) {
        let manufacturer = if first_row == 0x480 {
            "piers.rocks (sample)"
        } else {
            "piers.rocks"
        };
        let filler = Request {
            manufacturer: manufacturer.into(),
            ..m.clone()
        };
        put(&mut full, &instance(&filler, DATE, first_row));
    }
    let acme = Request {
        manufacturer: "Acme".into(),
        ..forced(&m)
    };
    assert_eq!(refusal(full, &acme).await, CommissionError::AreaFull);

    // An area the parser abandons. Its last row is written.
    let mut abandoned = with_instance(&m);
    abandoned.set_raw(0x100, ecc_encode(0x1234));
    abandoned.set_raw(0x4bf, ecc_encode(0x1234));
    assert_eq!(refusal(abandoned, &m).await, CommissionError::AreaFull);
}

/// Row 0x0c5 is the board entry's second value row. It holds "re".
#[tokio::test]
async fn an_instance_row_holding_another_value_is_refused() {
    let mut otp = board();
    otp.set_raw(0x0c5, ecc_encode(0x1234));
    assert_eq!(
        refusal(otp, &request(Board::Fire24F, BoardSize::M)).await,
        CommissionError::RowWritten {
            row: 0x0c5,
            raw: ecc_encode(0x1234),
            value: RowValue::Ecc(0x6572),
        }
    );
}

#[tokio::test]
async fn a_page_with_a_lock_word_in_the_way_is_refused() {
    let m = request(Board::Fire24F, BoardSize::M);
    let mut otp = board();
    otp.set_raw(lock1_row(3), 0x1);
    assert_eq!(
        refusal(otp, &m).await,
        CommissionError::RowWritten {
            row: lock1_row(3),
            raw: 0x1,
            value: RowValue::Raw(0x15_1515),
        }
    );

    let mut otp = board();
    otp.set_raw(lock1_row(3), 0x15_1515);
    assert_eq!(
        refusal(otp, &m).await,
        CommissionError::PageLocked { page: 3 }
    );
}

/// Page 59 holds the first white label rows. Page 1 holds
/// USB_WHITE_LABEL_ADDR and USB_BOOT_FLAGS.
#[tokio::test]
async fn a_locked_page_outside_the_area_is_refused_before_any_write() {
    let m = request(Board::Fire24F, BoardSize::M);
    for page in [59, 1] {
        let mut otp = board();
        otp.set_raw(lock1_row(page), 0x15_1515);
        otp.reset();
        assert_eq!(refusal(otp, &m).await, CommissionError::PageLocked { page });
    }
}

/// LOCK1's other locks don't stop the bootloader writing.
#[tokio::test]
async fn a_page_locked_only_against_other_access_is_commissioned() {
    let request = request(Board::Fire24F, BoardSize::M);
    let mut otp = board();
    for page in [1, 59, 60] {
        otp.set_raw(lock1_row(page), 0x0f_0f0f);
    }
    otp.reset();
    let mut expected = commissioned(&request, &FIRE_24_F_ROWS);
    for page in [1, 59, 60] {
        expected[usize::from(lock1_row(page))] = 0x0f_0f0f;
    }

    commission(&mut otp, &request).await.unwrap();

    assert_rows(&otp, &expected, "other locks");
}

// ---------------------------------------------------------------------------
// Runs stopped part way
// ---------------------------------------------------------------------------

/// An M run's third write is the board entry's length row.
#[tokio::test]
async fn a_row_reading_back_wrong_stops_the_run() {
    let mut otp = board();
    otp.corrupt(3, 0x1);
    assert_eq!(
        commission(&mut otp, &request(Board::Fire24F, BoardSize::M)).await,
        Err(CommissionError::ReadBack {
            row: 0x0c3,
            value: RowValue::Ecc(9),
            raw: ecc_encode(9) ^ 0x1,
        })
    );
    assert_eq!(otp.write_count(), 3);
}

/// Passes each access to a [`MemoryOtp`]. Read `nth` of `row` on its own
/// returns the row with the bits of `mask` flipped.
struct FlakyRead {
    otp: MemoryOtp,
    row: u16,
    nth: usize,
    mask: u32,
    reads: usize,
}

impl FlakyRead {
    fn new(row: u16, nth: usize, mask: u32) -> Self {
        Self {
            otp: board(),
            row,
            nth,
            mask,
            reads: 0,
        }
    }
}

impl LocalOtpAccess for FlakyRead {
    async fn read_ecc(&mut self, row: u16, count: u16) -> Result<Vec<u16>, OtpError> {
        self.otp.read_ecc(row, count).await
    }

    async fn read_raw(&mut self, row: u16, count: u16) -> Result<Vec<u32>, OtpError> {
        let mut rows = self.otp.read_raw(row, count).await?;
        if (row, count) == (self.row, 1) {
            self.reads += 1;
            if self.reads == self.nth {
                rows[0] ^= self.mask;
            }
        }
        Ok(rows)
    }

    async fn write_ecc(&mut self, row: u16, value: u16) -> Result<(), OtpError> {
        self.otp.write_ecc(row, value).await
    }

    async fn write_raw(&mut self, row: u16, value: u32) -> Result<(), OtpError> {
        self.otp.write_raw(row, value).await
    }
}

/// The first read of row 0xed0 on its own is the read-back after its write.
/// The second comes before USB_BOOT_FLAGS.
#[tokio::test]
async fn the_white_label_is_read_back_before_usb_boot_flags() {
    let mut otp = FlakyRead::new(0xed0, 2, 0x1);
    assert_eq!(
        commission(&mut otp, &request(Board::Fire24F, BoardSize::M)).await,
        Err(CommissionError::ReadBack {
            row: 0xed0,
            value: RowValue::Ecc(0x6970),
            raw: ecc_encode(0x6970) ^ 0x1,
        })
    );
    assert_eq!(raw(&otp.otp, 0x05c), ecc_encode(0xec0));
    assert_eq!(raw(&otp.otp, 0x059), 0);
}

/// The first read of row 0x054 on its own is the read-back after its write.
/// The second comes before BOOT_FLAGS0.
#[tokio::test]
async fn flash_devinfo_is_read_back_before_boot_flags0() {
    let mut otp = FlakyRead::new(0x054, 2, 0x1);
    assert_eq!(
        commission(&mut otp, &request(Board::Fire40A, BoardSize::L)).await,
        Err(CommissionError::ReadBack {
            row: 0x054,
            value: RowValue::Ecc(0x99af),
            raw: 0x3a_99af ^ 0x1,
        })
    );
    assert_eq!(raw(&otp.otp, 0x059), USB_BOOT_FLAGS);
    assert_eq!(raw(&otp.otp, 0x048), 0);
}

/// Row 0x055 is read on its own only before BOOT_FLAGS0.
#[tokio::test]
async fn the_slot_size_is_checked_before_boot_flags0() {
    let mut otp = FlakyRead::new(0x055, 1, 0x100);
    assert_eq!(
        commission(&mut otp, &request(Board::Fire40A, BoardSize::L)).await,
        Err(CommissionError::SlotSizeInvalid { raw: 0x100 })
    );
    assert_eq!(raw(&otp.otp, 0x059), USB_BOOT_FLAGS);
    assert_eq!(raw(&otp.otp, 0x048), 0);
}

/// A host whose access has `Send` futures can commission on a multi-threaded
/// executor.
#[test]
fn preparing_with_a_send_access_is_send() {
    fn send<T: Send>(_: T) {}
    let mut otp = board();
    let request = request(Board::Fire24F, BoardSize::M);
    send(prepare(&mut otp, &request));
}

// ---------------------------------------------------------------------------
// Setting a board's size
// ---------------------------------------------------------------------------

#[test]
fn every_size_parses_and_its_refusal_lists_every_size() {
    let refusal = "XL".parse::<BoardSize>().unwrap_err().to_string();
    let words: Vec<&str> = refusal.split(|c: char| !c.is_alphanumeric()).collect();
    for &size in BoardSize::supported_values() {
        let text = size.to_string();
        assert_eq!(text.parse(), Ok(size), "{text}");
        assert_eq!(text.to_lowercase().parse(), Ok(size), "{text}");
        assert!(words.contains(&text.as_str()), "{refusal}");
    }
}

/// The rows setting `size` leaves on a blank fire-40-a and the steps writing
/// them. OTP.md's "Board Sizes" section contains fire-40-a's FLASH_DEVINFO.
fn sized(size: BoardSize) -> (Vec<u32>, Vec<StepKind>) {
    let mut rows = board().rows().to_vec();
    let steps = match size {
        BoardSize::M => Vec::new(),
        BoardSize::L => {
            rows[0x054] = 0x3a_99af;
            // FLASH_DEVINFO_ENABLE in BOOT_FLAGS0 and its two copies.
            rows[0x048..=0x04a].fill(0x20);
            vec![StepKind::FlashDevinfo, StepKind::BootFlags0]
        }
    };
    (rows, steps)
}

/// Sets `otp`'s size to `size` as `board_type`.
async fn set_size<O: LocalOtpAccess>(
    otp: &mut O,
    board_type: Board,
    size: BoardSize,
) -> Result<(), CommissionError> {
    plan_size(otp, board_type, size)
        .await?
        .execute(otp, |_| {})
        .await
}

#[tokio::test]
async fn each_size_is_set_on_a_blank_board() {
    for &size in BoardSize::supported_values() {
        let (rows, steps) = sized(size);
        let mut otp = board();
        let plan = plan_size(&mut otp, Board::Fire40A, size).await.unwrap();
        let kinds: Vec<StepKind> = plan.steps().iter().map(|step| step.kind).collect();
        assert_eq!(kinds, steps, "{size}");
        plan.execute(&mut otp, |_| {}).await.unwrap();
        assert_rows(&otp, &rows, &size.to_string());
    }
}

#[tokio::test]
async fn a_board_already_its_size_writes_nothing() {
    for &size in BoardSize::supported_values() {
        let mut once = board();
        set_size(&mut once, Board::Fire40A, size).await.unwrap();
        let mut otp = copy(&once);
        let plan = plan_size(&mut otp, Board::Fire40A, size).await.unwrap();
        assert!(
            plan.steps()
                .iter()
                .flat_map(|step| &step.writes)
                .all(|write| write.holds),
            "{size}"
        );
        plan.execute(&mut otp, |_| {}).await.unwrap();
        assert_eq!(otp.write_count(), 0, "{size}");
        assert_rows(&otp, once.rows(), &size.to_string());
    }
}

/// Interrupts a run setting each size on a blank fire-40-a at each of its
/// writes in turn. Each write is interrupted once landing and once not. After
/// each interruption it resets the board and runs it again.
///
/// After each interruption a BOOT_FLAGS0 copy enables FLASH_DEVINFO only where
/// FLASH_DEVINFO contains its value. Each run after an interruption ends with
/// the rows of a run that wasn't interrupted.
#[tokio::test]
async fn a_size_run_interrupted_at_any_write_completes_the_second_time() {
    for &size in BoardSize::supported_values() {
        let (expected, _) = sized(size);
        let mut reference = board();
        set_size(&mut reference, Board::Fire40A, size)
            .await
            .unwrap();
        let changed = expected
            .iter()
            .zip(board().rows())
            .filter(|(expected, blank)| expected != blank)
            .count();
        assert_eq!(reference.write_count(), changed, "{size}");

        for n in 1..=reference.write_count() {
            for interruption in [Interruption::Landed, Interruption::NotLanded] {
                let at = format!("{size}: write {n}, {interruption:?}");
                let mut otp = board();
                otp.interrupt(n, interruption);
                let error = set_size(&mut otp, Board::Fire40A, size).await.unwrap_err();
                assert!(
                    matches!(
                        error,
                        CommissionError::Otp {
                            error: OtpError::Transport(_),
                            ..
                        }
                    ),
                    "{at}: {error}"
                );
                if (0x048..=0x04a).any(|row| raw(&otp, row) & 0x20 != 0) {
                    assert_eq!(raw(&otp, 0x054), expected[0x054], "{at}");
                }

                otp.reset();
                set_size(&mut otp, Board::Fire40A, size)
                    .await
                    .unwrap_or_else(|e| panic!("{at}: {e}"));
                assert_rows(&otp, &expected, &at);
            }
        }
    }
}

/// A board sized before it's commissioned ends with the rows of a board only
/// commissioned. `prepare` finds the size's rows already present.
#[tokio::test]
async fn a_sized_board_is_commissioned_as_a_blank_one_is() {
    let is_size_step =
        |kind: StepKind| matches!(kind, StepKind::FlashDevinfo | StepKind::BootFlags0);
    for &size in BoardSize::supported_values() {
        let request = request(Board::Fire40A, size);
        let mut commissioned = board();
        commission(&mut commissioned, &request).await.unwrap();

        let mut otp = board();
        let size_plan = plan_size(&mut otp, Board::Fire40A, size).await.unwrap();
        size_plan.execute(&mut otp, |_| {}).await.unwrap();
        let commission_plan = plan(&mut otp, &request).await.unwrap();
        let size_rows: Vec<(u16, RowValue, bool)> = commission_plan
            .steps()
            .iter()
            .filter(|step| is_size_step(step.kind))
            .flat_map(|step| &step.writes)
            .map(|write| (write.row, write.value, write.holds))
            .collect();
        let present: Vec<(u16, RowValue, bool)> = size_plan
            .steps()
            .iter()
            .flat_map(|step| &step.writes)
            .map(|write| (write.row, write.value, true))
            .collect();
        assert_eq!(size_rows, present, "{size}");

        commission_plan.execute(&mut otp, |_| {}).await.unwrap();
        assert_rows(&otp, commissioned.rows(), &size.to_string());
    }
}

/// Why setting `otp`'s size to `size` as `board_type` is refused. Checks that
/// the run didn't write anything.
async fn size_refusal(mut otp: MemoryOtp, board_type: Board, size: BoardSize) -> CommissionError {
    let before = otp.rows().to_vec();
    let writes = otp.write_count();
    let error = set_size(&mut otp, board_type, size).await.unwrap_err();
    assert_eq!(otp.write_count(), writes, "{error}");
    assert_rows(&otp, &before, &error.to_string());
    error
}

/// A board holding a complete instance at 0x0c0 made of `entries`, each built
/// by [`entry`]. A signature of zeros ends it.
fn with_entries(entries: &[Vec<u16>]) -> MemoryOtp {
    let rows: Vec<u16> = [vec![OTP_STORE_MAGIC, OTP_STORE_VERSION]]
        .into_iter()
        .chain(entries.iter().cloned())
        .chain([entry(2, &[0; 64])])
        .flatten()
        .collect();
    let mut otp = board();
    for (row, value) in (0x0c0..).zip(rows) {
        otp.set_raw(row, ecc_encode(value));
    }
    otp
}

/// A board holding a complete instance of `complete`'s at 0x0c0 and an
/// incomplete instance of `incomplete`'s at 0x100. The later instance lacks
/// its last row, the signature's key row.
async fn with_incomplete_instance(complete: &Request, incomplete: &Request) -> MemoryOtp {
    let mut otp = with_instance(complete);
    let writes = instance(incomplete, DATE, 0x100);
    put(&mut otp, &writes[..writes.len() - 1]);
    let area = read_commissioning(&mut otp).await.unwrap();
    let last = area.instances().last().unwrap();
    assert_eq!(last.first_row(), 0x100);
    assert!(!last.is_complete());
    otp
}

/// The firmware checks its board against the last complete instance's without
/// checking the instance's other values or its signature.
#[tokio::test]
async fn a_board_commissioned_as_another_is_refused() {
    let fire_24_f = request(Board::Fire24F, BoardSize::M);
    let fire_40_a = request(Board::Fire40A, BoardSize::L);
    let values = [
        entry(3, b"piers.rocks"),
        entry(4, DATE.as_bytes()),
        entry(5, &1_u16.to_le_bytes()),
    ];
    let board_entry = entry(1, b"fire-24-f");
    // Every value, with a signature of zeros.
    let unsigned = with_entries(&[&[board_entry.clone()][..], &values].concat());
    // Without its signer, so it isn't the current instance.
    let mut no_signer = with_entries(&[board_entry, values[0].clone(), values[1].clone()]);
    let area = read_commissioning(&mut no_signer).await.unwrap();
    assert!(area.current().is_none());
    assert!(area.instances()[0].is_complete());

    let refused = [
        with_instance(&fire_24_f),
        unsigned,
        no_signer,
        with_incomplete_instance(&fire_24_f, &fire_40_a).await,
    ];
    for otp in refused {
        for &size in BoardSize::supported_values() {
            assert_eq!(
                size_refusal(otp.clone(), Board::Fire40A, size).await,
                CommissionError::CommissionedAsAnotherBoard {
                    board: "fire-24-f".into(),
                    requested: Board::Fire40A,
                },
                "{size}"
            );
        }
    }
}

/// Where the last complete instance doesn't contain another board, the size is
/// set.
#[tokio::test]
async fn a_board_commissioned_as_itself_or_without_a_board_is_sized() {
    let fire_24_f = request(Board::Fire24F, BoardSize::M);
    let fire_40_a = request(Board::Fire40A, BoardSize::L);
    let mut no_board = with_entries(&[
        entry(3, b"piers.rocks"),
        entry(4, DATE.as_bytes()),
        entry(5, &1_u16.to_le_bytes()),
    ]);
    let area = read_commissioning(&mut no_board).await.unwrap();
    assert!(area.instances()[0].is_complete());
    assert_eq!(area.instances()[0].board(), None);

    let accepted = [
        ("blank", board()),
        ("the same board", with_instance(&fire_40_a)),
        ("without a board", no_board),
        (
            "a later incomplete instance for another board",
            with_incomplete_instance(&fire_40_a, &fire_24_f).await,
        ),
    ];
    for (case, mut otp) in accepted {
        set_size(&mut otp, Board::Fire40A, BoardSize::L)
            .await
            .unwrap_or_else(|e| panic!("{case}: {e}"));
        assert_eq!(raw(&otp, 0x054), 0x3a_99af, "{case}");
        assert_eq!(raw(&otp, 0x048), 0x20, "{case}");
    }
}

#[tokio::test]
async fn a_size_isnt_set_over_data_from_a_newer_version() {
    let mut newer = board();
    newer.set_raw(0x0c0, ecc_encode(OTP_STORE_MAGIC));
    newer.set_raw(0x0c1, ecc_encode(2));
    for &size in BoardSize::supported_values() {
        assert_eq!(
            size_refusal(newer.clone(), Board::Fire40A, size).await,
            CommissionError::NewerData {
                row: 0x0c0,
                version: 2
            },
            "{size}"
        );
    }
}

#[tokio::test]
async fn only_a_fire_boards_size_is_set() {
    for &board_type in Model::Ice.boards() {
        for &size in BoardSize::supported_values() {
            assert_eq!(
                size_refusal(board(), board_type, size).await,
                CommissionError::NotFire(board_type),
                "{board_type} {size}"
            );
        }
    }
}

#[tokio::test]
async fn l_needs_an_external_flash_pin_and_a_valid_slot_size() {
    assert_eq!(
        size_refusal(board(), Board::Fire24F, BoardSize::L).await,
        CommissionError::NoExternalFlash(Board::Fire24F)
    );

    // An ECC read corrects a single wrong bit. A row with one is refused.
    let mut slot = board();
    slot.set_raw(0x055, 0x100);
    assert_eq!(
        size_refusal(slot, Board::Fire40A, BoardSize::L).await,
        CommissionError::SlotSizeInvalid { raw: 0x100 }
    );
}

/// Row 0x055 is read on its own only before BOOT_FLAGS0.
#[tokio::test]
async fn the_slot_size_is_checked_again_before_boot_flags0() {
    let mut otp = FlakyRead::new(0x055, 1, 0x100);
    assert_eq!(
        set_size(&mut otp, Board::Fire40A, BoardSize::L).await,
        Err(CommissionError::SlotSizeInvalid { raw: 0x100 })
    );
    assert_eq!(raw(&otp.otp, 0x054), 0x3a_99af);
    assert_eq!(raw(&otp.otp, 0x048), 0);
}

/// FLASH_DEVINFO with 4MB on chip select 1, enabled by every BOOT_FLAGS0
/// copy. The board is neither M nor L.
fn neither_m_nor_l() -> MemoryOtp {
    let mut otp = board();
    otp.set_raw(0x054, ecc_encode(0xa9af));
    for row in 0x048..=0x04a {
        otp.set_raw(row, 0x20);
    }
    otp
}

/// Bits can't be cleared so a size other than M can't change once it's set.
#[tokio::test]
async fn a_size_other_than_m_is_refused_every_other_size() {
    let mut l = board();
    set_size(&mut l, Board::Fire40A, BoardSize::L)
        .await
        .unwrap();
    assert_eq!(
        size_refusal(copy(&l), Board::Fire40A, BoardSize::M).await,
        CommissionError::SizeAlreadySet {
            size: OneromBoardSize::BoardSizeL,
            requested: BoardSize::M,
        }
    );
    for &size in BoardSize::supported_values() {
        assert_eq!(
            size_refusal(neither_m_nor_l(), Board::Fire40A, size).await,
            CommissionError::SizeAlreadySet {
                size: OneromBoardSize::BoardSizeOther,
                requested: size,
            },
            "{size}"
        );
    }

    // The size is checked after the board's external flash and before row
    // 0x055.
    let mut slot = neither_m_nor_l();
    slot.set_raw(0x055, 0x100);
    assert_eq!(
        size_refusal(slot.clone(), Board::Fire40A, BoardSize::L).await,
        CommissionError::SizeAlreadySet {
            size: OneromBoardSize::BoardSizeOther,
            requested: BoardSize::L,
        }
    );
    assert_eq!(
        size_refusal(slot, Board::Fire24F, BoardSize::L).await,
        CommissionError::NoExternalFlash(Board::Fire24F)
    );
}

/// OTP still configures M where FLASH_DEVINFO is written but not enabled, or
/// only one BOOT_FLAGS0 copy enables it. M goes ahead without writing
/// anything.
#[tokio::test]
async fn m_is_set_where_otp_configures_a_second_chip_but_not_its_size() {
    let mut devinfo = board();
    devinfo.set_raw(0x054, ecc_encode(0x99af));
    let mut enabled = board();
    enabled.set_raw(0x04a, 0x20);
    for otp in [devinfo, enabled] {
        let before = otp.rows().to_vec();
        let mut otp = copy(&otp);
        let plan = plan_size(&mut otp, Board::Fire40A, BoardSize::M)
            .await
            .unwrap();
        assert!(plan.steps().is_empty());
        plan.execute(&mut otp, |_| {}).await.unwrap();
        assert_eq!(otp.write_count(), 0);
        assert_rows(&otp, &before, "M");
    }
}

/// FLASH_DEVINFO containing another value, whether or not OTP configures L.
#[tokio::test]
async fn l_is_refused_where_flash_devinfo_contains_another_value() {
    let mut devinfo = board();
    devinfo.set_raw(0x054, ecc_encode(0x99a0));
    let mut enabled = devinfo.clone();
    for row in 0x048..=0x04a {
        enabled.set_raw(row, 0x20);
    }
    for otp in [devinfo, enabled] {
        assert_eq!(
            size_refusal(otp, Board::Fire40A, BoardSize::L).await,
            CommissionError::RowWritten {
                row: 0x054,
                raw: ecc_encode(0x99a0),
                value: RowValue::Ecc(0x99af),
            }
        );
    }
}

/// Page 1 contains FLASH_DEVINFO and BOOT_FLAGS0. A locked page is refused
/// only where a row on it is still to be written.
#[tokio::test]
async fn a_locked_page_1_is_refused_where_a_row_needs_writing() {
    let mut blank = board();
    blank.set_raw(lock1_row(1), 0x15_1515);
    blank.reset();
    assert_eq!(
        size_refusal(blank, Board::Fire40A, BoardSize::L).await,
        CommissionError::PageLocked { page: 1 }
    );

    let mut l = board();
    set_size(&mut l, Board::Fire40A, BoardSize::L)
        .await
        .unwrap();
    let mut l = copy(&l);
    l.set_raw(lock1_row(1), 0x15_1515);
    l.reset();
    set_size(&mut l, Board::Fire40A, BoardSize::L)
        .await
        .unwrap();
    assert_eq!(l.write_count(), 0);
}

/// An L run's first write is FLASH_DEVINFO.
#[tokio::test]
async fn a_size_row_reading_back_wrong_stops_the_run() {
    let mut otp = board();
    otp.corrupt(1, 0x1);
    assert_eq!(
        set_size(&mut otp, Board::Fire40A, BoardSize::L).await,
        Err(CommissionError::ReadBack {
            row: 0x054,
            value: RowValue::Ecc(0x99af),
            raw: 0x3a_99af ^ 0x1,
        })
    );
    assert_eq!(otp.write_count(), 1);
    assert_eq!(raw(&otp, 0x048), 0);
}

/// A host whose access has `Send` futures can set a board's size on a
/// multi-threaded executor.
#[test]
fn planning_a_size_with_a_send_access_is_send() {
    fn send<T: Send>(_: T) {}
    let mut otp = board();
    send(plan_size(&mut otp, Board::Fire40A, BoardSize::L));
}
