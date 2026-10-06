// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The firmware's reading of OTP at boot.
//!
//! - The firmware's reader finds `COMMISSIONING_BOARD` in the last complete
//!   commissioning instance. It must agree with [`firmware_board`], a Rust copy
//!   of its rule, on any OTP. Where the host's reader, `onerom_metadata::otp`,
//!   finds nothing unexpected, the two must find the same board.
//! - The firmware's board size must agree with
//!   `onerom_metadata::otp::board_size` on a board with a secondary flash chip
//!   select. The firmware counts chip select 1 only on such a board, so on
//!   one without it must agree with the host's size for the same OTP with
//!   chip select 1's size cleared.
//! - The firmware serves on a board OTP commissions as its own, and asks for
//!   the bootloader on one OTP commissions as another.
//!
//! OTP outlives a boot in this process, as it does on a device, so [`run`]
//! clears it when it's done.

use onerom_config::hw::{BOARDS, Board};
use onerom_fw_emulator::Emulator;
use onerom_metadata::otp::{
    CommissioningArea, CommissioningValues, NewCommissioningInstance, RowWrite, board_size,
};
use onerom_metadata::{
    OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE, OTP_BOOT_FLAGS0_ROW, OTP_COMMISSIONING_AREA_FIRST_ROW,
    OTP_COMMISSIONING_AREA_LAST_ROW, OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT, OTP_FLASH_DEVINFO_ROW,
    OTP_FLASH_DEVINFO_SIZE_BITS, OTP_KEY_NONE, OTP_PAGE_ROWS, OTP_STORE_MAGIC, OTP_STORE_VERSION,
    OneromBoardSize, OneromOtpKey,
};
use rand::{RngExt, SeedableRng, rngs::StdRng};

use crate::report::TestReport;

/// The commissioning area's first row.
const FIRST: u16 = OTP_COMMISSIONING_AREA_FIRST_ROW;

/// The row after the commissioning area's last.
const END: u16 = OTP_COMMISSIONING_AREA_LAST_ROW + 1;

/// Seeds the random OTP, so a run is repeatable.
const SEED: u64 = 0x0e0c_0a0b;

/// How many random commissioning areas the readers are compared on.
const READER_CASES: usize = 5000;

/// How many random BOOT_FLAGS0 and FLASH_DEVINFO values the board sizes are
/// compared on.
const SIZE_CASES: usize = 1000;

/// The FLASH_DEVINFO a fire-40-a L board holds: 2MB on each chip select.
const L_DEVINFO: u16 = 0x99af;

/// Manufacturers for random instances. The last is long enough to take an
/// instance past a page.
const MANUFACTURERS: [&str; 3] = [
    "piers.rocks",
    "M",
    "A manufacturer whose name is long enough to take a commissioning instance past a page",
];

/// Runs the checks. The boots need a ROM slot to serve, so they run only where
/// `serves` is set.
pub fn run(board: Board, serves: bool, report: &mut TestReport) {
    report.add_check("commissioning: firmware reader", check_reader());
    report.add_check("commissioning: board size", check_board_size(board));
    if serves {
        report.add_check(
            "commissioning: serves on its own board",
            check_own_board(board),
        );
        report.add_check(
            "commissioning: bootloader on another board",
            check_other_board(board),
        );
    }
    Emulator::clear_otp();
}

// ── The reader ────────────────────────────────────────────────────────────────

fn check_reader() -> Result<(), String> {
    let mut rng = StdRng::seed_from_u64(SEED);
    let edges = full_areas().into_iter().map(|area| (area, false));
    let randoms = (0..READER_CASES).map(|_| random_area(&mut rng));
    for (case, (area, damaged)) in edges.chain(randoms).enumerate() {
        Emulator::set_otp_ecc(FIRST, &area);
        let firmware = Emulator::otp_commissioned_board();
        let rule = firmware_board(&area);
        if firmware != rule {
            return Err(format!(
                "case {case}: the firmware found {firmware:x?}, its rule {rule:x?}"
            ));
        }
        let parsed = CommissioningArea::parse(&area);
        if !damaged && parsed.issues().is_empty() {
            let host = parsed.current().and_then(|i| i.board()).map(str::as_bytes);
            let found = firmware.map(|(row, len)| board_bytes(&area, row, len));
            if found.as_deref() != host {
                return Err(format!(
                    "case {case}: the firmware found {found:?}, the host {host:?}"
                ));
            }
        }
        check_mismatch(&area, firmware).map_err(|e| format!("case {case}: {e}"))?;
    }
    Ok(())
}

/// The firmware's rule in Rust: `COMMISSIONING_BOARD` in the last complete
/// instance, as its value's first row and length. `area` holds the
/// commissioning area read with ECC, from its first row. `firmware/src/otp.c`
/// describes the rule.
fn firmware_board(area: &[u16]) -> Option<(u16, u16)> {
    let row = |r: u16| area[usize::from(r - FIRST)];
    let board_key = OneromOtpKey::OtpKeyCommissioningBoard as u16;
    let sig_key = OneromOtpKey::OtpKeyCommissioningSig as u16;
    let mut found = None;
    let mut boundary = FIRST;
    while boundary < END {
        match row(boundary) {
            0 => break,
            OTP_STORE_MAGIC => {}
            _ => return None,
        }
        let mut last = boundary + 1;
        match row(boundary + 1) {
            0 => {}
            OTP_STORE_VERSION => {
                let mut r = boundary + 2;
                let mut board = None;
                loop {
                    if r >= END {
                        last = END - 1;
                        break;
                    }
                    if r + 1 >= END {
                        return None;
                    }
                    let (key, len) = (row(r), row(r + 1));
                    if key == OTP_KEY_NONE && len == 0 {
                        last = r + 1;
                        break;
                    }
                    let next = u32::from(r) + 2 + u32::from(len).div_ceil(2);
                    if next > u32::from(END) {
                        return None;
                    }
                    if key == board_key {
                        board = Some((r + 2, len));
                    } else if key == sig_key {
                        found = board;
                        last = next as u16 - 1;
                        break;
                    }
                    r = next as u16;
                }
            }
            _ => return None,
        }
        boundary = (last / OTP_PAGE_ROWS + 1) * OTP_PAGE_ROWS;
    }
    found
}

/// Checks the firmware's comparison with `hw_rev`. The board it found matches
/// its own name and nothing else. Where it found none, or the metadata doesn't
/// hold a board name, nothing mismatches.
fn check_mismatch(area: &[u16], found: Option<(u16, u16)>) -> Result<(), String> {
    let Some((row, len)) = found else {
        return if Emulator::otp_board_mismatch(Some("fire-24-a")) {
            Err("a mismatch where the firmware found no board".into())
        } else {
            Ok(())
        };
    };
    if Emulator::otp_board_mismatch(None) {
        return Err("a mismatch without a board name in the metadata".into());
    }
    // Only a name without a NUL can reach the firmware as a C string.
    let Ok(name) = String::from_utf8(board_bytes(area, row, len)) else {
        return Ok(());
    };
    if name.contains('\0') {
        return Ok(());
    }
    if Emulator::otp_board_mismatch(Some(&name)) {
        return Err(format!("{name:?} doesn't match itself"));
    }
    let mut changed = name.clone();
    let last = changed.pop();
    changed.push(if last == Some('x') { 'y' } else { 'x' });
    for other in [format!("{name}x"), changed] {
        if !Emulator::otp_board_mismatch(Some(&other)) {
            return Err(format!("{other:?} matches {name:?}"));
        }
    }
    Ok(())
}

/// The `len` bytes of a value whose first row is `row`, two to a row, low byte
/// first.
fn board_bytes(area: &[u16], row: u16, len: u16) -> Vec<u8> {
    area[usize::from(row - FIRST)..]
        .iter()
        .flat_map(|r| r.to_le_bytes())
        .take(usize::from(len))
        .collect()
}

/// Two areas filled to the end, whose last instance is cut short in the last
/// page, before its signature's key row. One instance fills the last page, and
/// the other stops a row short. A random area rarely ends that close to the
/// area's end.
fn full_areas() -> Vec<Vec<u16>> {
    // With fire-24-a, a 20-character manufacturer makes an instance a page
    // long, and an 18-character one a row shorter.
    let pages = (END - FIRST) / OTP_PAGE_ROWS;
    [20, 18]
        .into_iter()
        .map(|last_len| {
            let mut area = vec![0u16; usize::from(END - FIRST)];
            for page in 0..pages {
                let last = page + 1 == pages;
                let manufacturer = "m".repeat(if last { last_len } else { 20 });
                let values = CommissioningValues::new(Board::Fire24A, &manufacturer, "20260927", 1)
                    .expect("valid commissioning values");
                let first_row = FIRST + page * OTP_PAGE_ROWS;
                let writes = NewCommissioningInstance::new(&values, first_row)
                    .expect("an instance that fits")
                    .writes(&[0; 64]);
                let count = if last { writes.len() - 1 } else { writes.len() };
                for write in &writes[..count] {
                    area[usize::from(write.row - FIRST)] = write.value;
                }
            }
            area
        })
        .collect()
}

/// A random commissioning area and whether it's damaged. It holds instances
/// where commission places them, any of which can be cut short as an
/// interrupted commission leaves it. Some areas are filled to the end. A
/// damaged area then has rows changed.
fn random_area(rng: &mut StdRng) -> (Vec<u16>, bool) {
    let mut area = vec![0u16; usize::from(END - FIRST)];
    let instances = if rng.random_bool(0.2) {
        usize::from(END - FIRST)
    } else {
        rng.random_range(0..=3)
    };
    for _ in 0..instances {
        let Some(first_row) = CommissioningArea::parse(&area).next_instance_row() else {
            break;
        };
        let Some(writes) = random_instance(rng, first_row) else {
            break;
        };
        let count = if rng.random_bool(0.3) {
            rng.random_range(0..writes.len())
        } else {
            writes.len()
        };
        for write in &writes[..count] {
            area[usize::from(write.row - FIRST)] = write.value;
        }
    }

    let damaged = rng.random_bool(0.5);
    if damaged {
        // Up to a page past the last written row, so a damaged row can start
        // an instance there too.
        let written = area.iter().rposition(|&r| r != 0).map_or(0, |i| i + 1);
        let span = (written + usize::from(OTP_PAGE_ROWS)).min(area.len());
        for _ in 0..rng.random_range(1..=3) {
            let row = rng.random_range(0..span);
            area[row] = random_row(rng);
        }
    }
    (area, damaged)
}

/// An instance's writes, as commission makes them.
fn random_instance(rng: &mut StdRng, first_row: u16) -> Option<Vec<RowWrite>> {
    let board = BOARDS[rng.random_range(0..BOARDS.len())];
    let manufacturer = MANUFACTURERS[rng.random_range(0..MANUFACTURERS.len())];
    let signer = rng.random_range(1..=300);
    let values = CommissioningValues::new(board, manufacturer, "20260927", signer).ok()?;
    let instance = NewCommissioningInstance::new(&values, first_row).ok()?;
    let mut signature = [0u8; 64];
    rng.fill(&mut signature[..]);
    Some(instance.writes(&signature))
}

/// A row value, weighted to the values the layout uses.
fn random_row(rng: &mut StdRng) -> u16 {
    match rng.random_range(0..6) {
        0 => 0,
        1 => OTP_STORE_MAGIC,
        2 => rng.random_range(0..=3),
        3 => rng.random_range(0..=6),
        4 => rng.random_range(0..=80),
        _ => rng.random(),
    }
}

// ── The board size ────────────────────────────────────────────────────────────

fn check_board_size(board: Board) -> Result<(), String> {
    // The FLASH_DEVINFO bits the firmware ignores on this board.
    let ignored = if board.external_flash_cs_pin().is_some() {
        0
    } else {
        OTP_FLASH_DEVINFO_SIZE_BITS << OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT
    };
    let mut rng = StdRng::seed_from_u64(SEED);
    for case in 0..SIZE_CASES {
        let boot_flags0: [u32; 3] = core::array::from_fn(|_| match rng.random_range(0..3) {
            0 => 0,
            1 => OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE,
            _ => rng.random::<u32>() & 0xff_ffff,
        });
        let devinfo = match rng.random_range(0..4) {
            0 => L_DEVINFO,
            1 => 0x09af,
            2 => 0xc9af,
            _ => rng.random(),
        };
        Emulator::set_otp_raw(OTP_BOOT_FLAGS0_ROW, &boot_flags0);
        Emulator::set_otp_ecc(OTP_FLASH_DEVINFO_ROW, &[devinfo]);
        let firmware = Emulator::otp_board_size();
        let host = board_size(boot_flags0, devinfo & !ignored) as u8;
        if firmware != host {
            return Err(format!(
                "case {case}: BOOT_FLAGS0 {boot_flags0:x?} and FLASH_DEVINFO {devinfo:#06x}: \
                 the firmware found {firmware:#04x}, the host {host:#04x}"
            ));
        }
    }
    Ok(())
}

// ── Boots ─────────────────────────────────────────────────────────────────────

fn check_own_board(board: Board) -> Result<(), String> {
    let emulator = boot_with(board, &commissioned_as(board));
    if emulator.bootloader_entered() {
        return Err("the firmware asked for the bootloader".into());
    }
    if !emulator.pios_enabled() {
        return Err("PIO state machines not enabled after boot".into());
    }
    // The OTP is an L board's, and chip select 1 counts only on a board with a
    // secondary flash chip select.
    let expected = if board.external_flash_cs_pin().is_some() {
        OneromBoardSize::BoardSizeL
    } else {
        OneromBoardSize::BoardSizeM
    };
    let size = emulator.board_size();
    if size != expected as u8 {
        return Err(format!(
            "the firmware recorded board size {size:#04x}, not {expected}"
        ));
    }
    Ok(())
}

fn check_other_board(board: Board) -> Result<(), String> {
    let other = BOARDS
        .into_iter()
        .find(|&b| b != board)
        .expect("more than one board");
    let emulator = boot_with(board, &commissioned_as(other));
    if !emulator.bootloader_entered() {
        return Err(format!(
            "the firmware didn't ask for the bootloader on a board commissioned as {}",
            other.name()
        ));
    }
    // Both come from runtime info, which every boot starts afresh. The PIO
    // state doesn't, so it can't show what this boot did.
    if emulator.sel_image() != 0xFF {
        return Err("the firmware read the image select pins".into());
    }
    if emulator.serving_alg().is_some() {
        return Err("the firmware serves a ROM slot".into());
    }
    Ok(())
}

/// A commissioning area holding one instance for `board`.
fn commissioned_as(board: Board) -> Vec<u16> {
    let values = CommissioningValues::new(board, "piers.rocks", "20260927", 1)
        .expect("valid commissioning values");
    let instance = NewCommissioningInstance::new(&values, FIRST).expect("an instance that fits");
    let mut area = vec![0u16; usize::from(END - FIRST)];
    for write in instance.writes(&[0; 64]) {
        area[usize::from(write.row - FIRST)] = write.value;
    }
    area
}

/// Boots set 0 with `area` in the commissioning area, and OTP configuring an L
/// board.
fn boot_with(board: Board, area: &[u16]) -> Emulator {
    Emulator::clear_otp();
    Emulator::set_otp_ecc(FIRST, area);
    Emulator::set_otp_raw(
        OTP_BOOT_FLAGS0_ROW,
        &[OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE; 3],
    );
    Emulator::set_otp_ecc(OTP_FLASH_DEVINFO_ROW, &[L_DEVINFO]);
    Emulator::set_rp_variant(board.rp_variant());
    Emulator::set_sel_image(0);
    Emulator::boot()
}
