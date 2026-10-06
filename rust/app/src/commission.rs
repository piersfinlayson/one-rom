// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Commissioning a board and setting its size. `docs/OTP.md`'s
//! "Commissioning" and "Setting a Board's Size" sections specify them.
//!
//! - [`prepare`] reads the board and checks the request before a signature is
//!   asked for.
//! - [`Prepared::plan`] places the signed instance and lists every row to
//!   write. It doesn't read or write OTP.
//! - [`plan_size`] reads the board and lists the rows setting its size.
//! - [`Plan::execute`] writes the rows and reads each one back.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use onerom_config::hw::{Board, BoardSize, Model};
use onerom_metadata::otp::pico_otp::whitelabel::OTP_ROW_USB_WHITE_LABEL_DATA;
use onerom_metadata::otp::{
    AreaIssue, BuildError, CommissioningArea, CommissioningInstance, CommissioningValues,
    NewCommissioningInstance, RowWrite, StoreEntry, board_size, white_label,
};
use onerom_metadata::{
    OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE, OTP_COMMISSIONING_AREA_FIRST_ROW, OTP_FLASH_DEVINFO_ROW,
    OTP_FLASH_PARTITION_SLOT_SIZE_ROW, OTP_PAGE_ROWS, OTP_USB_WHITE_LABEL_ROW, OneromBoardSize,
    OneromOtpEntry,
};

use crate::otp::{
    AREA_FIRST_PAGE, AREA_LAST_PAGE, AREA_ROWS, BOOT_FLAGS0_ROWS, BootRows, CHIP_ID_ROW,
    LOCK_BL_READ_ONLY, LOCK1_READ_ONLY, LocalOtpAccess, OtpError, PageLock, USB_BOOT_FLAGS_ROWS,
    WHITE_LABEL_FIRST_PAGE, WHITE_LABEL_LAST_PAGE, WHITE_LABEL_PAGES_ROWS, WHITE_LABEL_TABLE_ROWS,
    bootloader_lock, codeword, flash_devinfo, holds, is_unwritten, lock1_row, read_area,
    read_chip_id, read_ecc_rows, read_locks, read_raw_rows,
};

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

/// A request to commission a board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The board. The instance's `COMMISSIONING_BOARD` holds its name.
    pub board: Board,
    /// The board's size.
    pub size: BoardSize,
    /// The manufacturer's name. `COMMISSIONING_MANUFACTURER` holds it.
    pub manufacturer: String,
    /// The commissioning date.
    pub date: RequestDate,
    /// The ID of the key signing the instance. `COMMISSIONING_SIGNER` holds
    /// it.
    pub signer: u16,
    /// Whether to go ahead despite:
    /// - a current instance holding other values
    /// - FLASH_DEVINFO written or enabled by a BOOT_FLAGS0 copy on a board OTP
    ///   still configures as M
    pub force: bool,
}

/// A request's `YYYYMMDD` commissioning date.
///
/// Where the current instance differs from the request only in its date:
/// - a run given a date is a new commissioning and needs `force`
/// - a run using today's date finishes that instance with the instance's date
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestDate {
    /// A date the user provided.
    Given(String),
    /// Today's UTC date. A request takes it where the user didn't provide one.
    Today(String),
}

impl RequestDate {
    fn as_str(&self) -> &str {
        match self {
            Self::Given(date) | Self::Today(date) => date,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A row's value and how it's written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowValue {
    /// A 16-bit value written with ECC.
    Ecc(u16),
    /// A 24-bit value written raw.
    Raw(u32),
}

impl fmt::Display for RowValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ecc(value) => write!(f, "{value:#06x}"),
            Self::Raw(value) => write!(f, "{value:#08x}"),
        }
    }
}

/// Why commissioning a board or setting its size stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommissionError {
    /// An OTP access failed.
    #[error("OTP row {row:#05x}: {error}")]
    Otp {
        /// The row written or the first row read.
        row: u16,
        /// Why the access failed.
        error: OtpError,
    },

    /// A commissioning instance has a version this crate doesn't read. Newer
    /// tooling probably wrote it.
    #[error("the commissioning instance at row {row:#05x} has unknown version {version}")]
    NewerData {
        /// The instance's first row.
        row: u16,
        /// The instance's version.
        version: u16,
    },

    /// The current instance has an entry with an unknown key. Newer tooling
    /// probably wrote it.
    #[error("the commissioning instance has unknown key {key} at row {row:#05x}")]
    UnknownKey {
        /// The entry's key row.
        row: u16,
        /// The key.
        key: u16,
    },

    /// The current instance holds values other than the request's.
    #[error(
        "the board is already commissioned as {board} by {manufacturer} on {date} with signer {signer}"
    )]
    AlreadyCommissioned {
        /// The current instance's first row.
        row: u16,
        /// The current instance's board.
        board: String,
        /// The current instance's manufacturer.
        manufacturer: String,
        /// The current instance's `YYYYMMDD` date.
        date: String,
        /// The ID of the key that signed the current instance.
        signer: u16,
        /// Whether the date is the only value that differs.
        only_date_differs: bool,
    },

    /// The last complete instance is for another board. Only [`plan_size`]
    /// returns it.
    #[error("the board is commissioned as {board} not {}", .requested.name())]
    CommissionedAsAnotherBoard {
        /// The instance's board.
        board: String,
        /// The board the request is for.
        requested: Board,
    },

    /// The commissioning area doesn't have room for the instance.
    #[error("the commissioning area is full")]
    AreaFull,

    /// The request's values can't make an instance.
    #[error("{}", build_error_text(.0))]
    Build(BuildError),

    /// The board isn't a Fire board. Commissioning writes an RP2350's OTP. Only
    /// Fire boards have an RP2350.
    #[error("{0} isn't a Fire board")]
    NotFire(Board),

    /// The tools currently configure L with a second chip on chip select 1, which
    /// requires an external flash chip select pin. This board doesn't have one.
    #[error("{0} doesn't have an external flash chip select pin")]
    NoExternalFlash(Board),

    /// A request for a size other than the one [`board_size`] says OTP
    /// configures, where that isn't M. A set bit can't be cleared so nothing
    /// overrides this.
    #[error(
        "this board is already programmed as size {} and cannot be reprogrammed as size {requested}",
        configured_text(*.size)
    )]
    SizeAlreadySet {
        /// The size OTP configures.
        size: OneromBoardSize,
        /// The size the request is for.
        requested: BoardSize,
    },

    /// OTP configures a second flash chip for an M board. FLASH_DEVINFO is
    /// written or a BOOT_FLAGS0 copy enables it. Carries the board. Only
    /// [`prepare`] returns it.
    #[error("this board is partly programmed as size L")]
    SecondChipConfigured(Board),

    /// FLASH_PARTITION_SLOT_SIZE isn't an exact codeword. The chip wouldn't
    /// boot with FLASH_DEVINFO_ENABLE set.
    #[error(
        "size L cannot be programmed as row {:#05x} contains invalid value {raw:#08x}",
        OTP_FLASH_PARTITION_SLOT_SIZE_ROW
    )]
    SlotSizeInvalid {
        /// The row's raw value.
        raw: u32,
    },

    /// A row to write contains another value.
    #[error(
        "row {row:#05x} already contains {} and cannot be programmed with new value {value}",
        contents(*.raw, *.value)
    )]
    RowWritten {
        /// The row.
        row: u16,
        /// The row's raw value.
        raw: u32,
        /// The value to write.
        value: RowValue,
    },

    /// A page is locked and rows on it still need writing.
    #[error("page {page} cannot be written because it is locked")]
    PageLocked {
        /// The page.
        page: u16,
    },

    /// A row doesn't contain its value after it was written.
    #[error(
        "row {row:#05x} failed to verify. It was programmed with new value {value} but contains {}",
        contents(*.raw, *.value)
    )]
    ReadBack {
        /// The row.
        row: u16,
        /// The value it should hold.
        value: RowValue,
        /// The row's raw value.
        raw: u32,
    },
}

/// What a row with raw value `raw` contains, read the way `value` is written.
fn contents(raw: u32, value: RowValue) -> String {
    match value {
        RowValue::Ecc(_) => match codeword(raw) {
            Some(decoded) => format!("value {decoded:#06x}"),
            None => format!("invalid value {raw:#08x}"),
        },
        RowValue::Raw(_) => format!("value {raw:#08x}"),
    }
}

/// The text for a size OTP configures, as [`CommissionError::SizeAlreadySet`]
/// shows it.
fn configured_text(size: OneromBoardSize) -> &'static str {
    match size {
        // SizeAlreadySet is never returned for M.
        OneromBoardSize::BoardSizeM => "M",
        OneromBoardSize::BoardSizeL => "L",
        OneromBoardSize::BoardSizeOther => "other",
        // board_size() never returns it.
        OneromBoardSize::BoardSizeUnknown => "unknown",
    }
}

/// The text for a [`BuildError`]. `BuildError` doesn't implement `Display`.
fn build_error_text(error: &BuildError) -> &'static str {
    match error {
        BuildError::EmptyManufacturer => "the manufacturer's name is empty",
        BuildError::BadManufacturer => {
            "the manufacturer's name must be printable ASCII without '*' or a leading or trailing space"
        }
        BuildError::BadDate => "the date isn't 8 digits",
        BuildError::BadSigner => "signer ID 0 is invalid",
        BuildError::BadFirstRow => {
            "the instance's first row isn't a page boundary in the commissioning area"
        }
        BuildError::DoesNotFit => "the instance doesn't fit in the commissioning area",
    }
}

/// A [`CommissionError`] for an [`OtpError`] from an access starting at
/// `row`.
fn at(row: u16) -> impl FnOnce(OtpError) -> CommissionError {
    move |error| CommissionError::Otp { row, error }
}

// ---------------------------------------------------------------------------
// Preparing
// ---------------------------------------------------------------------------

/// Reads the board and checks `request` against it before a signature is
/// asked for.
///
/// It reads with ECC:
/// - CHIPID
/// - the commissioning area
/// - FLASH_DEVINFO
///
/// It reads raw:
/// - the commissioning area
/// - the rows One ROM uses in page 1
/// - pages 59 and 60
/// - the LOCK1 words of the commissioning area's pages
/// - the LOCK1 words of pages 1, 59 and 60
///
/// Whatever `force` says it refuses:
/// - a board that isn't a Fire board
/// - a commissioning area holding an unknown version
/// - a current instance holding an unknown key
/// - a size other than the one [`board_size`] says OTP configures, where that
///   isn't M
///
/// Without `force` it refuses a current instance holding other values. Where
/// the date is today's and is the only value differing from the current
/// instance's, it takes that instance's date to finish it.
/// [`Prepared::date_from_current_instance`] then says so.
///
/// For an L board it refuses:
/// - a board without an external flash chip select pin
/// - FLASH_PARTITION_SLOT_SIZE that isn't an exact codeword
/// - FLASH_DEVINFO holding another value
///
/// For an M board without `force` it refuses FLASH_DEVINFO that is written
/// or enabled by a BOOT_FLAGS0 copy.
///
/// It also refuses:
/// - a white label row or USB_WHITE_LABEL_ADDR holding another value
/// - a USB_BOOT_FLAGS copy that is neither 0 nor One ROM's flags
/// - a row to write on page 1, 59 or 60 where the bootloader's lock on the
///   page refuses writes
/// - values [`CommissioningValues::new`] refuses
/// - values [`NewCommissioningInstance::new`] refuses at row `0x0c0`
pub async fn prepare<O: LocalOtpAccess>(
    otp: &mut O,
    request: &Request,
) -> Result<Prepared, CommissionError> {
    match request.board.model() {
        Model::Fire => {}
        Model::Ice => return Err(CommissionError::NotFire(request.board)),
    }
    let chip_id = read_chip_id(otp).await.map_err(at(CHIP_ID_ROW))?;
    let area = read_area(otp)
        .await
        .map_err(|(row, error)| CommissionError::Otp { row, error })?;
    let area_raw = read_raw_rows(otp, OTP_COMMISSIONING_AREA_FIRST_ROW, AREA_ROWS)
        .await
        .map_err(at(OTP_COMMISSIONING_AREA_FIRST_ROW))?;
    let boot = BootRows::read(otp).await.map_err(at(BootRows::FIRST_ROW))?;
    // The bootrom reads FLASH_DEVINFO with ECC.
    let flash_devinfo_ecc = read_ecc_rows(otp, OTP_FLASH_DEVINFO_ROW, 1)
        .await
        .map_err(at(OTP_FLASH_DEVINFO_ROW))?[0];
    let white_label_raw = read_raw_rows(otp, OTP_USB_WHITE_LABEL_ROW, WHITE_LABEL_PAGES_ROWS)
        .await
        .map_err(at(OTP_USB_WHITE_LABEL_ROW))?;
    let locks = read_locks(otp, AREA_FIRST_PAGE..=AREA_LAST_PAGE)
        .await
        .map_err(at(lock1_row(AREA_FIRST_PAGE)))?;
    let mut other_locks = read_locks(otp, BootRows::PAGE..=BootRows::PAGE)
        .await
        .map_err(at(lock1_row(BootRows::PAGE)))?;
    other_locks.extend(
        read_locks(otp, WHITE_LABEL_FIRST_PAGE..=WHITE_LABEL_LAST_PAGE)
            .await
            .map_err(at(lock1_row(WHITE_LABEL_FIRST_PAGE)))?,
    );

    check_area(&area)?;
    let current = area.current();
    let (date, date_from_current_instance) = choose_date(request, current);
    let values =
        CommissioningValues::new(request.board, &request.manufacturer, &date, request.signer)
            .map_err(CommissionError::Build)?;
    NewCommissioningInstance::new(&values, OTP_COMMISSIONING_AREA_FIRST_ROW)
        .map_err(CommissionError::Build)?;
    // Whether `force` let the request's values differ from the current
    // instance, so this run replaces it.
    let mut replaces_current = false;
    if let Some(current) = current {
        match check_current(current, request, &date) {
            Ok(()) => {}
            Err(e) if !request.force => return Err(e),
            Err(_) => replaces_current = true,
        }
    }

    let size = size_steps(request.board, request.size, &boot, flash_devinfo_ecc)?;
    match request.size {
        BoardSize::M if !request.force => check_second_chip(request.board, &boot)?,
        BoardSize::M | BoardSize::L => {}
    }
    let before: Vec<Step> = size.flash_devinfo.into_iter().collect();
    let mut after = white_label_steps(request.board, &white_label_raw, &boot)?;
    after.extend(size.boot_flags0);
    check_locks(before.iter().chain(&after), &other_locks)?;

    Ok(Prepared {
        chip_id,
        message: values.message(chip_id),
        date,
        date_from_current_instance,
        replaces_current,
        values,
        area,
        area_raw,
        locks,
        before,
        after,
    })
}

/// Refuses:
/// - an area holding an unknown version
/// - a current instance holding an unknown key
fn check_area(area: &CommissioningArea) -> Result<(), CommissionError> {
    check_version(area)?;
    let unknown = area.current().and_then(|current| {
        current.entries().iter().find_map(|entry| match entry {
            StoreEntry::Entry {
                row,
                entry: OneromOtpEntry::Unknown { key, .. },
            } => Some((*row, *key)),
            StoreEntry::Entry { .. }
            | StoreEntry::Deleted { .. }
            | StoreEntry::Unreadable { .. } => None,
        })
    });
    match unknown {
        // A key comes from a 16-bit row.
        Some((row, key)) => Err(CommissionError::UnknownKey {
            row,
            key: key as u16,
        }),
        None => Ok(()),
    }
}

/// Refuses an area holding an unknown version.
fn check_version(area: &CommissioningArea) -> Result<(), CommissionError> {
    for issue in area.issues() {
        match *issue {
            AreaIssue::UnknownVersion { row, version } => {
                return Err(CommissionError::NewerData { row, version });
            }
            AreaIssue::LostPlace { .. } => {}
        }
    }
    Ok(())
}

/// How an instance's values compare with a request's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparison {
    Same,
    OnlyDate,
    Other,
}

/// How `instance` compares with `request` when the request's date is `date`.
fn compare(instance: &CommissioningInstance, request: &Request, date: &str) -> Comparison {
    let others_match = instance.board() == Some(request.board.name())
        && instance.manufacturer() == Some(request.manufacturer.as_str())
        && instance.signer() == Some(request.signer);
    match (others_match, instance.date() == Some(date)) {
        (true, true) => Comparison::Same,
        (true, false) => Comparison::OnlyDate,
        (false, _) => Comparison::Other,
    }
}

/// The date the instance holds and whether it's the current instance's.
///
/// A run using today's date finishes a current instance whose date is the
/// only value that differs so it takes that instance's date.
fn choose_date(request: &Request, current: Option<&CommissioningInstance>) -> (String, bool) {
    let requested = request.date.as_str();
    if let (RequestDate::Today(_), Some(current)) = (&request.date, current)
        && compare(current, request, requested) == Comparison::OnlyDate
        && let Some(date) = current.date()
    {
        return (date.into(), true);
    }
    (requested.into(), false)
}

/// Refuses a current instance holding values other than `request`'s when the
/// request's date is `date`.
fn check_current(
    current: &CommissioningInstance,
    request: &Request,
    date: &str,
) -> Result<(), CommissionError> {
    let comparison = compare(current, request, date);
    if comparison == Comparison::Same {
        return Ok(());
    }
    // The current instance is valid so it has every value.
    Err(CommissionError::AlreadyCommissioned {
        row: current.first_row(),
        board: current.board().unwrap_or_default().into(),
        manufacturer: current.manufacturer().unwrap_or_default().into(),
        date: current.date().unwrap_or_default().into(),
        signer: current.signer().unwrap_or_default(),
        only_date_differs: comparison == Comparison::OnlyDate,
    })
}

/// The steps setting a board's size. [`prepare`] plans FLASH_DEVINFO before
/// its other steps and FLASH_DEVINFO_ENABLE after them.
struct SizeSteps {
    /// FLASH_DEVINFO. `None` for a size without a chip on chip select 1.
    flash_devinfo: Option<Step>,
    /// FLASH_DEVINFO_ENABLE in BOOT_FLAGS0 and its two copies. `None` for a
    /// size without a chip on chip select 1.
    boot_flags0: Option<Step>,
}

/// Checks `size` for `board` and builds the steps setting it. `boot` is the
/// rows One ROM uses in page 1 and `flash_devinfo_ecc` is FLASH_DEVINFO read
/// with ECC.
///
/// It refuses what [`check_size`] refuses and FLASH_DEVINFO containing another
/// value.
fn size_steps(
    board: Board,
    size: BoardSize,
    boot: &BootRows,
    flash_devinfo_ecc: u16,
) -> Result<SizeSteps, CommissionError> {
    let configured = board_size(boot.boot_flags0, flash_devinfo_ecc);
    let Some(devinfo) = check_size(board, size, boot, configured)? else {
        return Ok(SizeSteps {
            flash_devinfo: None,
            boot_flags0: None,
        });
    };
    let flash_devinfo = Step {
        kind: StepKind::FlashDevinfo,
        writes: vec![ecc_write(
            OTP_FLASH_DEVINFO_ROW,
            devinfo,
            boot.flash_devinfo,
        )?],
    };
    let boot_flags0 = Step {
        kind: StepKind::BootFlags0,
        writes: BOOT_FLAGS0_ROWS
            .iter()
            .zip(boot.boot_flags0)
            .map(|(&row, raw)| raw_write(row, raw | OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE, raw))
            .collect(),
    };
    Ok(SizeSteps {
        flash_devinfo: Some(flash_devinfo),
        boot_flags0: Some(boot_flags0),
    })
}

/// Checks `size` for `board`. `configured` is the size OTP configures. Returns
/// FLASH_DEVINFO's value, or `None` for a size without a chip on chip select
/// 1.
///
/// It refuses a size other than `configured` where that isn't M. For an L
/// request it also refuses:
/// - a board without an external flash chip select pin, before anything else
/// - FLASH_PARTITION_SLOT_SIZE that isn't an exact codeword
fn check_size(
    board: Board,
    size: BoardSize,
    boot: &BootRows,
    configured: OneromBoardSize,
) -> Result<Option<u16>, CommissionError> {
    match size {
        BoardSize::L => {
            let pin = board
                .external_flash_cs_pin()
                .ok_or(CommissionError::NoExternalFlash(board))?;
            check_configured(configured, size)?;
            // Once FLASH_DEVINFO_ENABLE is set the chip never boots again
            // unless this row is a valid ECC value. An ECC read corrects a
            // single wrong bit. A row with one is refused too.
            if codeword(boot.flash_partition_slot_size).is_none() {
                return Err(CommissionError::SlotSizeInvalid {
                    raw: boot.flash_partition_slot_size,
                });
            }
            Ok(flash_devinfo(size, pin))
        }
        BoardSize::M => {
            check_configured(configured, size)?;
            Ok(None)
        }
    }
}

/// Refuses `requested` where OTP configures another size that isn't M.
/// `configured` is the size OTP configures. A set bit can't be cleared so
/// nothing changes a size once it's set.
fn check_configured(
    configured: OneromBoardSize,
    requested: BoardSize,
) -> Result<(), CommissionError> {
    let same = match configured {
        OneromBoardSize::BoardSizeM => return Ok(()),
        OneromBoardSize::BoardSizeL => requested == BoardSize::L,
        OneromBoardSize::BoardSizeOther | OneromBoardSize::BoardSizeUnknown => false,
    };
    if same {
        Ok(())
    } else {
        Err(CommissionError::SizeAlreadySet {
            size: configured,
            requested,
        })
    }
}

/// Refuses an M request for `board` where FLASH_DEVINFO is written or a
/// BOOT_FLAGS0 copy enables it. `boot` is the rows One ROM uses in page 1.
/// Only [`prepare`] checks this, and its `force` overrides it.
fn check_second_chip(board: Board, boot: &BootRows) -> Result<(), CommissionError> {
    let enabled = boot
        .boot_flags0
        .iter()
        .any(|flags| flags & OTP_BOOT_FLAGS0_FLASH_DEVINFO_ENABLE != 0);
    if enabled || !is_unwritten(boot.flash_devinfo) {
        Err(CommissionError::SecondChipConfigured(board))
    } else {
        Ok(())
    }
}

/// The steps writing the bootloader's USB white label and enabling it. Refuses
/// a row holding another value.
fn white_label_steps(
    board: Board,
    raw: &[u32],
    boot: &BootRows,
) -> Result<Vec<Step>, CommissionError> {
    // onerom-metadata's tests convert every board's white label.
    let data = white_label(board)
        .to_otp_data_strict()
        .expect("pico-otp refuses a board's white label");
    let write = |i: usize, value: u16| ecc_write(OTP_USB_WHITE_LABEL_ROW + i as u16, value, raw[i]);
    let (table, strings) = data.rows().split_at(WHITE_LABEL_TABLE_ROWS);
    let strings = (WHITE_LABEL_TABLE_ROWS..)
        .zip(strings)
        .map(|(i, &value)| write(i, value))
        .collect::<Result<_, _>>()?;
    // A table row that is 0 isn't written so it stays free for its entry.
    let table = table
        .iter()
        .enumerate()
        .filter(|&(_, &value)| value != 0)
        .map(|(i, &value)| write(i, value))
        .collect::<Result<_, _>>()?;
    let addr = ecc_write(
        OTP_ROW_USB_WHITE_LABEL_DATA,
        OTP_USB_WHITE_LABEL_ROW,
        boot.usb_white_label_addr,
    )?;
    let flags = data.usb_boot_flags();
    let usb_boot_flags = USB_BOOT_FLAGS_ROWS
        .iter()
        .zip(boot.usb_boot_flags)
        .map(|(&row, raw)| {
            if raw == 0 || raw == flags {
                Ok(raw_write(row, flags, raw))
            } else {
                Err(CommissionError::RowWritten {
                    row,
                    raw,
                    value: RowValue::Raw(flags),
                })
            }
        })
        .collect::<Result<_, _>>()?;
    Ok(vec![
        Step {
            kind: StepKind::WhiteLabelStrings,
            writes: strings,
        },
        Step {
            kind: StepKind::WhiteLabelTable,
            writes: table,
        },
        Step {
            kind: StepKind::WhiteLabelAddr,
            writes: vec![addr],
        },
        Step {
            kind: StepKind::UsbBootFlags,
            writes: usb_boot_flags,
        },
    ])
}

/// Refuses a page where `steps` have a row still to write and the
/// bootloader's lock on the page refuses writes. `locks` are the LOCK1 words
/// of the pages the steps write.
fn check_locks<'a>(
    steps: impl Iterator<Item = &'a Step>,
    locks: &[PageLock],
) -> Result<(), CommissionError> {
    let to_write = steps
        .flat_map(|step| &step.writes)
        .filter(|write| !write.holds);
    for write in to_write {
        let page = write.row / OTP_PAGE_ROWS;
        let locked = locks
            .iter()
            .any(|lock| lock.page == page && bootloader_lock(lock.raw) >= LOCK_BL_READ_ONLY);
        if locked {
            return Err(CommissionError::PageLocked { page });
        }
    }
    Ok(())
}

/// An ECC write of `value` to `row`. `raw` is the row's raw value. Refuses a
/// row holding another value.
fn ecc_write(row: u16, value: u16, raw: u32) -> Result<PlannedWrite, CommissionError> {
    let holds = holds(raw, value);
    if holds || is_unwritten(raw) {
        Ok(PlannedWrite {
            row,
            value: RowValue::Ecc(value),
            holds,
        })
    } else {
        Err(CommissionError::RowWritten {
            row,
            raw,
            value: RowValue::Ecc(value),
        })
    }
}

/// A raw write of `value` to `row`. `raw` is the row's raw value.
fn raw_write(row: u16, value: u32, raw: u32) -> PlannedWrite {
    PlannedWrite {
        row,
        value: RowValue::Raw(value),
        holds: raw == value,
    }
}

// ---------------------------------------------------------------------------
// Setting a board's size
// ---------------------------------------------------------------------------

/// Reads the board and plans setting its size to `size` without commissioning
/// it. The plan writes the rows [`prepare`] plans for the size and nothing
/// else. For an L board that's FLASH_DEVINFO, then FLASH_DEVINFO_ENABLE in
/// BOOT_FLAGS0 and its two copies. An M board doesn't have any.
///
/// It reads with ECC:
/// - the commissioning area
/// - FLASH_DEVINFO
///
/// It reads raw:
/// - the rows One ROM uses in page 1
/// - page 1's LOCK1 word
///
/// It refuses:
/// - a board that isn't a Fire board
/// - a commissioning area containing an unknown version
/// - a last complete instance containing another board. It doesn't check the
///   instance's other values or its signature.
/// - a size other than the one [`board_size`] says OTP configures, where that
///   isn't M
///
/// For an L request it refuses:
/// - a board without an external flash chip select pin
/// - FLASH_PARTITION_SLOT_SIZE that isn't an exact codeword
/// - FLASH_DEVINFO containing another value
///
/// It also refuses a row to write on page 1 where the bootloader's lock on the
/// page refuses writes.
///
/// An M request for a board OTP configures as M goes ahead where FLASH_DEVINFO
/// is written or a BOOT_FLAGS0 copy enables it. Its plan doesn't write
/// anything.
pub async fn plan_size<O: LocalOtpAccess>(
    otp: &mut O,
    board: Board,
    size: BoardSize,
) -> Result<Plan, CommissionError> {
    match board.model() {
        Model::Fire => {}
        Model::Ice => return Err(CommissionError::NotFire(board)),
    }
    let area = read_area(otp)
        .await
        .map_err(|(row, error)| CommissionError::Otp { row, error })?;
    let boot = BootRows::read(otp).await.map_err(at(BootRows::FIRST_ROW))?;
    // The bootrom reads FLASH_DEVINFO with ECC.
    let flash_devinfo_ecc = read_ecc_rows(otp, OTP_FLASH_DEVINFO_ROW, 1)
        .await
        .map_err(at(OTP_FLASH_DEVINFO_ROW))?[0];
    let locks = read_locks(otp, BootRows::PAGE..=BootRows::PAGE)
        .await
        .map_err(at(lock1_row(BootRows::PAGE)))?;

    check_version(&area)?;
    // The firmware checks its board against the last complete instance's
    // without checking the instance's other values or its signature.
    let last_complete = area
        .instances()
        .iter()
        .rev()
        .find(|instance| instance.is_complete());
    if let Some(other) = last_complete.and_then(CommissioningInstance::board)
        && other != board.name()
    {
        return Err(CommissionError::CommissionedAsAnotherBoard {
            board: other.into(),
            requested: board,
        });
    }

    let size = size_steps(board, size, &boot, flash_devinfo_ecc)?;
    let steps: Vec<Step> = size
        .flash_devinfo
        .into_iter()
        .chain(size.boot_flags0)
        .collect();
    check_locks(steps.iter(), &locks)?;
    Ok(Plan { steps })
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// A request [`prepare`] has checked. [`plan`](Self::plan) takes the
/// instance's signature.
#[derive(Debug, Clone)]
pub struct Prepared {
    chip_id: [u16; 4],
    message: Vec<u8>,
    date: String,
    date_from_current_instance: bool,
    replaces_current: bool,
    values: CommissioningValues,
    area: CommissioningArea,
    /// The commissioning area's raw rows.
    area_raw: Vec<u32>,
    /// The LOCK1 words of the commissioning area's pages.
    locks: Vec<PageLock>,
    /// The steps before the instance's.
    before: Vec<Step>,
    /// The steps after the instance's locks.
    after: Vec<Step>,
}

impl Prepared {
    /// The board's CHIPID from rows `0x000`–`0x003` read with ECC. Row `0x000`
    /// comes first.
    pub fn chip_id(&self) -> [u16; 4] {
        self.chip_id
    }

    /// The `YYYYMMDD` date the instance holds.
    pub fn date(&self) -> &str {
        &self.date
    }

    /// Whether [`date`](Self::date) is the current instance's. [`prepare`]
    /// takes it in place of today's where the dates are the only values that
    /// differ.
    pub fn date_from_current_instance(&self) -> bool {
        self.date_from_current_instance
    }

    /// The current instance this run replaces. `Some` only where `force` let
    /// the request's values differ from it.
    pub fn replaced_instance(&self) -> Option<&CommissioningInstance> {
        self.area.current().filter(|_| self.replaces_current)
    }

    /// The message the instance's signature covers.
    pub fn message(&self) -> &[u8] {
        &self.message
    }

    /// Places the instance holding `signature` and lists every row to write.
    /// It doesn't read or write OTP.
    ///
    /// The instance goes at:
    /// - the last instance's first row if every row it needs there is
    ///   unwritten or holds its value
    /// - otherwise [`CommissioningArea::next_instance_row`]
    /// - otherwise the page after the area's last written row if the parser
    ///   lost its place and didn't find a later instance
    ///
    /// The first doesn't apply where the third does.
    ///
    /// It refuses:
    /// - an instance that doesn't fit
    /// - a row the instance needs that holds another value
    /// - a LOCK1 word that is neither 0 nor One ROM's lock
    /// - a locked page with a row still to write
    pub fn plan(self, signature: &[u8; 64]) -> Result<Plan, CommissionError> {
        let instance = self.place(signature)?;
        let locks = self.lock_steps(&instance)?;
        let mut steps = self.before;
        steps.push(Step {
            kind: StepKind::Instance,
            writes: instance,
        });
        steps.extend(locks);
        steps.extend(self.after);
        Ok(Plan { steps })
    }

    /// The instance's writes at the row [`plan`](Self::plan) chooses.
    fn place(&self, signature: &[u8; 64]) -> Result<Vec<PlannedWrite>, CommissionError> {
        // Where the parser can't find its way past the last instance, finishing
        // that instance wouldn't make it current. OTP.md puts the instance
        // after the last written row instead.
        let abandoned = is_abandoned(&self.area);
        if !abandoned
            && let Some(last) = self.area.instances().last()
            && let Some(writes) = self.instance_writes(last.first_row(), signature)
            && let Ok(writes) = self.check_instance(&writes)
        {
            return Ok(writes);
        }
        let first_row = match self.area.next_instance_row() {
            Some(row) => row,
            None if abandoned => self.after_last_written_row(),
            None => return Err(CommissionError::AreaFull),
        };
        let writes = self
            .instance_writes(first_row, signature)
            .ok_or(CommissionError::AreaFull)?;
        self.check_instance(&writes)
    }

    /// The writes of the instance at `first_row`. `None` where it doesn't fit
    /// there.
    fn instance_writes(&self, first_row: u16, signature: &[u8; 64]) -> Option<Vec<RowWrite>> {
        NewCommissioningInstance::new(&self.values, first_row)
            .ok()
            .map(|instance| instance.writes(signature))
    }

    /// `writes` as planned writes. Refuses a row holding another value.
    fn check_instance(&self, writes: &[RowWrite]) -> Result<Vec<PlannedWrite>, CommissionError> {
        writes
            .iter()
            .map(|write| {
                let raw = self.area_raw[usize::from(write.row - OTP_COMMISSIONING_AREA_FIRST_ROW)];
                ecc_write(write.row, write.value, raw)
            })
            .collect()
    }

    /// The page boundary after the area's last written row.
    fn after_last_written_row(&self) -> u16 {
        self.area_raw
            .iter()
            .rposition(|&raw| !is_unwritten(raw))
            .map_or(OTP_COMMISSIONING_AREA_FIRST_ROW, |i| {
                let row = OTP_COMMISSIONING_AREA_FIRST_ROW + i as u16;
                (row / OTP_PAGE_ROWS + 1) * OTP_PAGE_ROWS
            })
    }

    /// A lock step for each page `instance` takes. Refuses:
    /// - a LOCK1 word that is neither 0 nor One ROM's lock
    /// - a locked page with a row still to write
    fn lock_steps(&self, instance: &[PlannedWrite]) -> Result<Vec<Step>, CommissionError> {
        let pages = instance.iter().map(|write| write.row / OTP_PAGE_ROWS);
        let (Some(first), Some(last)) = (pages.clone().min(), pages.max()) else {
            return Ok(Vec::new());
        };
        (first..=last)
            .map(|page| {
                let raw = self.locks[usize::from(page - AREA_FIRST_PAGE)].raw;
                let to_write = instance
                    .iter()
                    .any(|write| write.row / OTP_PAGE_ROWS == page && !write.holds);
                match raw {
                    0 => {}
                    LOCK1_READ_ONLY if to_write => {
                        return Err(CommissionError::PageLocked { page });
                    }
                    LOCK1_READ_ONLY => {}
                    _ => {
                        return Err(CommissionError::RowWritten {
                            row: lock1_row(page),
                            raw,
                            value: RowValue::Raw(LOCK1_READ_ONLY),
                        });
                    }
                }
                Ok(Step {
                    kind: StepKind::Lock { page },
                    writes: vec![raw_write(lock1_row(page), LOCK1_READ_ONLY, raw)],
                })
            })
            .collect()
    }
}

/// Whether the parser lost its place and didn't find a later instance.
fn is_abandoned(area: &CommissioningArea) -> bool {
    match area.issues().last() {
        Some(&AreaIssue::LostPlace { row }) => area
            .instances()
            .last()
            .is_none_or(|last| last.first_row() <= row),
        Some(AreaIssue::UnknownVersion { .. }) | None => false,
    }
}

// ---------------------------------------------------------------------------
// Executing
// ---------------------------------------------------------------------------

/// The steps [`Prepared::plan`] or [`plan_size`] lists and the rows each
/// writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    steps: Vec<Step>,
}

/// A step of a [`Plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The part of OTP the step writes.
    pub kind: StepKind,
    /// The step's rows in the order it writes them.
    pub writes: Vec<PlannedWrite>,
}

/// The part of OTP a [`Step`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// FLASH_DEVINFO on an L board.
    FlashDevinfo,
    /// The commissioning instance.
    Instance,
    /// The LOCK1 word of one of the instance's pages.
    Lock {
        /// The page.
        page: u16,
    },
    /// The bootloader's USB strings.
    WhiteLabelStrings,
    /// The white label table's rows that aren't 0.
    WhiteLabelTable,
    /// USB_WHITE_LABEL_ADDR.
    WhiteLabelAddr,
    /// USB_BOOT_FLAGS and its two copies.
    UsbBootFlags,
    /// FLASH_DEVINFO_ENABLE in BOOT_FLAGS0 and its two copies on an L board.
    BootFlags0,
}

/// A row a [`Step`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedWrite {
    /// The row.
    pub row: u16,
    /// The value to write.
    pub value: RowValue,
    /// Whether the row already holds the value. A row that does isn't written.
    pub holds: bool,
}

impl Plan {
    /// The steps in the order [`execute`](Self::execute) takes them.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Writes each row that doesn't already hold its value and reads it back.
    /// It calls `done` after each step.
    ///
    /// Before setting bits that can't be cleared it checks the rows they
    /// enable:
    /// - USB_BOOT_FLAGS needs every white label row and USB_WHITE_LABEL_ADDR
    ///   to hold its value
    /// - BOOT_FLAGS0 needs FLASH_DEVINFO to hold its value and
    ///   FLASH_PARTITION_SLOT_SIZE to be an exact codeword
    pub async fn execute<O: LocalOtpAccess>(
        &self,
        otp: &mut O,
        mut done: impl FnMut(&Step),
    ) -> Result<(), CommissionError> {
        for step in &self.steps {
            if step.writes.iter().any(|write| !write.holds) {
                self.check_enabled(otp, step.kind).await?;
                for write in step.writes.iter().filter(|write| !write.holds) {
                    write_row(otp, write).await?;
                }
            }
            done(step);
        }
        Ok(())
    }

    /// Checks the rows a step of `kind` enables.
    async fn check_enabled<O: LocalOtpAccess>(
        &self,
        otp: &mut O,
        kind: StepKind,
    ) -> Result<(), CommissionError> {
        match kind {
            StepKind::UsbBootFlags => {
                self.read_back_steps(
                    otp,
                    &[
                        StepKind::WhiteLabelStrings,
                        StepKind::WhiteLabelTable,
                        StepKind::WhiteLabelAddr,
                    ],
                )
                .await
            }
            StepKind::BootFlags0 => {
                self.read_back_steps(otp, &[StepKind::FlashDevinfo]).await?;
                let raw = read_row(otp, OTP_FLASH_PARTITION_SLOT_SIZE_ROW).await?;
                match codeword(raw) {
                    Some(_) => Ok(()),
                    None => Err(CommissionError::SlotSizeInvalid { raw }),
                }
            }
            StepKind::FlashDevinfo
            | StepKind::Instance
            | StepKind::Lock { .. }
            | StepKind::WhiteLabelStrings
            | StepKind::WhiteLabelTable
            | StepKind::WhiteLabelAddr => Ok(()),
        }
    }

    /// Reads back every row of the steps of `kinds`.
    async fn read_back_steps<O: LocalOtpAccess>(
        &self,
        otp: &mut O,
        kinds: &[StepKind],
    ) -> Result<(), CommissionError> {
        let steps = self.steps.iter().filter(|step| kinds.contains(&step.kind));
        for write in steps.flat_map(|step| &step.writes) {
            read_back(otp, write).await?;
        }
        Ok(())
    }
}

/// Writes `write` and reads the row back.
async fn write_row<O: LocalOtpAccess>(
    otp: &mut O,
    write: &PlannedWrite,
) -> Result<(), CommissionError> {
    let written = match write.value {
        RowValue::Ecc(value) => otp.write_ecc(write.row, value).await,
        RowValue::Raw(value) => otp.write_raw(write.row, value).await,
    };
    written.map_err(at(write.row))?;
    read_back(otp, write).await
}

/// Refuses `write`'s row where it doesn't hold the value.
async fn read_back<O: LocalOtpAccess>(
    otp: &mut O,
    write: &PlannedWrite,
) -> Result<(), CommissionError> {
    let raw = read_row(otp, write.row).await?;
    let held = match write.value {
        RowValue::Ecc(value) => holds(raw, value),
        RowValue::Raw(value) => raw == value,
    };
    if held {
        Ok(())
    } else {
        Err(CommissionError::ReadBack {
            row: write.row,
            value: write.value,
            raw,
        })
    }
}

/// Reads `row` raw.
async fn read_row<O: LocalOtpAccess>(otp: &mut O, row: u16) -> Result<u32, CommissionError> {
    let rows = read_raw_rows(otp, row, 1).await.map_err(at(row))?;
    Ok(rows[0])
}
