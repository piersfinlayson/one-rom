// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Access to an RP2350's OTP and the readers built on it.
//!
//! `docs/OTP.md` specifies the rows One ROM uses. A host reads and writes
//! them through [`LocalOtpAccess`]. [`MemoryOtp`] stands in for a chip in
//! tests.

mod memory;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::RangeInclusive;

use onerom_metadata::otp::pico_otp::whitelabel::{
    OTP_ROW_UNRESERVED_END, OTP_ROW_USB_BOOT_FLAGS, OTP_ROW_USB_BOOT_FLAGS_R1,
    OTP_ROW_USB_BOOT_FLAGS_R2, OTP_ROW_USB_WHITE_LABEL_DATA,
};
use onerom_metadata::otp::pico_otp::{OtpData, WhiteLabelStruct, ecc_encode};
use onerom_metadata::otp::{CommissioningArea, GeneralStore, board_size, format_chip_id};
use onerom_metadata::{
    OTP_BOOT_FLAGS0_R1_ROW, OTP_BOOT_FLAGS0_R2_ROW, OTP_BOOT_FLAGS0_ROW,
    OTP_COMMISSIONING_AREA_FIRST_ROW, OTP_COMMISSIONING_AREA_LAST_ROW,
    OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT, OTP_FLASH_DEVINFO_CS1_GPIO, OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT,
    OTP_FLASH_DEVINFO_D8H_ERASE_SUPPORTED, OTP_FLASH_DEVINFO_ROW, OTP_FLASH_DEVINFO_SIZE_2MB,
    OTP_FLASH_PARTITION_SLOT_SIZE_ROW, OTP_GENERAL_STORE_FIRST_ROW,
    OTP_GENERAL_STORE_TERMINATOR_ROW, OTP_GENERAL_STORE_TERMINATOR_ROW_COUNT, OTP_PAGE_ROWS,
    OTP_USB_WHITE_LABEL_ROW, OneromBoardSize,
};
use serde::Serialize;

use crate::commission::BoardSize;

pub use memory::{Interruption, MemoryOtp};

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// CHIPID's first row. CHIPID takes rows `0x000`–`0x003`.
pub(crate) const CHIP_ID_ROW: u16 = 0x000;

/// The 24 bits of a row.
pub(crate) const ROW_BITS: u32 = 0xff_ffff;

/// BOOT_FLAGS0 and its two copies.
pub(crate) const BOOT_FLAGS0_ROWS: [u16; 3] = [
    OTP_BOOT_FLAGS0_ROW,
    OTP_BOOT_FLAGS0_R1_ROW,
    OTP_BOOT_FLAGS0_R2_ROW,
];

/// USB_BOOT_FLAGS and its two copies.
pub(crate) const USB_BOOT_FLAGS_ROWS: [u16; 3] = [
    OTP_ROW_USB_BOOT_FLAGS,
    OTP_ROW_USB_BOOT_FLAGS_R1,
    OTP_ROW_USB_BOOT_FLAGS_R2,
];

/// Rows in the commissioning area.
pub(crate) const AREA_ROWS: u16 =
    OTP_COMMISSIONING_AREA_LAST_ROW - OTP_COMMISSIONING_AREA_FIRST_ROW + 1;

/// The commissioning area's first page.
pub(crate) const AREA_FIRST_PAGE: u16 = OTP_COMMISSIONING_AREA_FIRST_ROW / OTP_PAGE_ROWS;

/// The commissioning area's last page.
pub(crate) const AREA_LAST_PAGE: u16 = OTP_COMMISSIONING_AREA_LAST_ROW / OTP_PAGE_ROWS;

/// The first white label page. Pages 59 and 60 hold the white label table
/// and its strings.
pub(crate) const WHITE_LABEL_FIRST_PAGE: u16 = OTP_USB_WHITE_LABEL_ROW / OTP_PAGE_ROWS;

/// The last white label page.
pub(crate) const WHITE_LABEL_LAST_PAGE: u16 = (OTP_ROW_UNRESERVED_END - 1) / OTP_PAGE_ROWS;

/// Rows in pages 59 and 60.
pub(crate) const WHITE_LABEL_PAGES_ROWS: u16 = OTP_ROW_UNRESERVED_END - OTP_USB_WHITE_LABEL_ROW;

/// Rows in the white label table. Its strings follow it.
pub(crate) const WHITE_LABEL_TABLE_ROWS: usize = 16;

/// Rows in the general store and its terminator.
const GENERAL_STORE_ROWS: u16 = OTP_GENERAL_STORE_TERMINATOR_ROW
    + OTP_GENERAL_STORE_TERMINATOR_ROW_COUNT
    - OTP_GENERAL_STORE_FIRST_ROW;

/// Row of page 0's LOCK1 word. Page n's is 2n rows on.
const PAGE0_LOCK1_ROW: u16 = 0xf81;

/// A LOCK1 word whose three copies each make the page read-only for all
/// access. Commissioning writes it for each page of an instance.
pub const LOCK1_READ_ONLY: u32 = 0x15_1515;

/// LOCK_BL's position in each copy of a LOCK1 word.
const LOCK_BL_SHIFT: u32 = 4;

/// LOCK_BL's width.
const LOCK_BL_BITS: u32 = 0b11;

/// The LOCK_BL value making a page read-only. A lock from this value up
/// refuses writes.
pub(crate) const LOCK_BL_READ_ONLY: u8 = 1;

/// USB_BOOT_FLAGS' WHITE_LABEL_ADDR_VALID bit. The bootloader uses the white
/// label only where it's set.
const USB_BOOT_FLAGS_WHITE_LABEL_ADDR_VALID: u32 = 1 << 22;

/// The row of `page`'s LOCK1 word.
pub(crate) const fn lock1_row(page: u16) -> u16 {
    PAGE0_LOCK1_ROW + 2 * page
}

/// The bootloader's lock on a page whose LOCK1 word is `word`. It's LOCK_BL
/// with each bit's majority across the word's three copies.
pub(crate) fn bootloader_lock(word: u32) -> u8 {
    let copies = majority([word, word >> 8, word >> 16]);
    ((copies >> LOCK_BL_SHIFT) & LOCK_BL_BITS) as u8
}

/// Whether a row with raw value `raw` is unwritten. A row can leave the factory
/// with one bit set so a row with 0 or 1 bits set counts as unwritten.
pub(crate) fn is_unwritten(raw: u32) -> bool {
    raw.count_ones() <= 1
}

/// Whether an ECC row with raw value `raw` holds `value`. The bootrom writes
/// the value's encoding inverted where the plain encoding would clear a set bit
/// so either form counts.
pub(crate) fn holds(raw: u32, value: u16) -> bool {
    let encoding = ecc_encode(value);
    raw == encoding || raw == !encoding & ROW_BITS
}

/// The value an ECC row with raw value `raw` holds. `None` where `raw` isn't an
/// exact codeword in either form.
pub(crate) fn codeword(raw: u32) -> Option<u16> {
    // Bits 15:0 hold the value. The inverted form holds it inverted.
    [raw as u16, !raw as u16]
        .into_iter()
        .find(|&value| holds(raw, value))
}

/// Each bit's majority across three copies of a row.
pub(crate) fn majority(copies: [u32; 3]) -> u32 {
    let [a, b, c] = copies;
    (a & b) | (a & c) | (b & c)
}

/// FLASH_DEVINFO for a board of `size` with:
/// - 2MB of built-in flash on chip select 0
/// - the size's chip on chip select 1
/// - D8h block erase supported
/// - chip select 1 on GPIO `cs1_gpio`
///
/// `None` for a size without a chip on chip select 1.
pub(crate) fn flash_devinfo(size: BoardSize, cs1_gpio: u8) -> Option<u16> {
    let cs1_size = match size {
        BoardSize::M => return None,
        BoardSize::L => OTP_FLASH_DEVINFO_SIZE_2MB,
    };
    debug_assert!(u16::from(cs1_gpio) <= OTP_FLASH_DEVINFO_CS1_GPIO);
    Some(
        (cs1_size << OTP_FLASH_DEVINFO_CS1_SIZE_SHIFT)
            | (OTP_FLASH_DEVINFO_SIZE_2MB << OTP_FLASH_DEVINFO_CS0_SIZE_SHIFT)
            | OTP_FLASH_DEVINFO_D8H_ERASE_SUPPORTED
            | u16::from(cs1_gpio),
    )
}

// ---------------------------------------------------------------------------
// Access
// ---------------------------------------------------------------------------

/// Host-supplied access to an RP2350's OTP.
///
/// `onerom-app` doesn't perform I/O of its own. The readers and commissioning
/// read and write OTP through an implementation of this trait.
///
/// A row holds 24 bits. An ECC access moves the 16-bit value the row holds. A
/// raw access moves all 24 bits. A read can cross page boundaries. A write
/// takes one row so a caller can read each row back before writing the next.
///
/// # `Send` variants
///
/// [`trait_variant`] generates two forms of this trait:
///
/// - [`LocalOtpAccess`] for a single-threaded executor such as the browser's.
///   Its futures need not be `Send`.
/// - `OtpAccess` for a multi-threaded executor such as the CLI's Tokio
///   runtime. Its futures are `Send`.
///
/// A type that implements `OtpAccess` implements `LocalOtpAccess` too.
#[trait_variant::make(OtpAccess: Send)]
pub trait LocalOtpAccess {
    /// Reads `count` rows with ECC from `row` and returns their values in row
    /// order.
    async fn read_ecc(&mut self, row: u16, count: u16) -> Result<Vec<u16>, OtpError>;

    /// Reads `count` rows raw from `row` and returns them in row order.
    async fn read_raw(&mut self, row: u16, count: u16) -> Result<Vec<u32>, OtpError>;

    /// Writes `value` to `row` with ECC.
    async fn write_ecc(&mut self, row: u16, value: u16) -> Result<(), OtpError>;

    /// Writes bits 23–0 of `value` to `row` raw.
    async fn write_raw(&mut self, row: u16, value: u32) -> Result<(), OtpError>;
}

/// The reason an OTP access failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OtpError {
    /// The row is locked against the access. PICOBOOT reports this as
    /// `NotPermitted`.
    #[error("the row is locked")]
    NotPermitted,

    /// The write would clear a set bit. PICOBOOT reports this as
    /// `UnsupportedModification`.
    #[error("the write would clear a set bit")]
    UnsupportedModification,

    /// The host's transport failed. Carries its message.
    #[error("{0}")]
    Transport(String),
}

/// Reads `count` rows with ECC from `row`. Refuses a reply of another length.
pub(crate) async fn read_ecc_rows<O: LocalOtpAccess>(
    otp: &mut O,
    row: u16,
    count: u16,
) -> Result<Vec<u16>, OtpError> {
    let rows = otp.read_ecc(row, count).await?;
    exact(rows, count)
}

/// Reads `count` rows raw from `row`. Refuses a reply of another length.
pub(crate) async fn read_raw_rows<O: LocalOtpAccess>(
    otp: &mut O,
    row: u16,
    count: u16,
) -> Result<Vec<u32>, OtpError> {
    let rows = otp.read_raw(row, count).await?;
    exact(rows, count)
}

/// Refuses `rows` unless the read of `count` rows returned that many.
fn exact<T>(rows: Vec<T>, count: u16) -> Result<Vec<T>, OtpError> {
    if rows.len() == usize::from(count) {
        Ok(rows)
    } else {
        Err(OtpError::Transport(format!(
            "a read of {count} rows returned {}",
            rows.len()
        )))
    }
}

// ---------------------------------------------------------------------------
// Readers
// ---------------------------------------------------------------------------

/// Reads CHIPID from rows `0x000`–`0x003` with ECC. Row `0x000` comes first.
pub async fn read_chip_id<O: LocalOtpAccess>(otp: &mut O) -> Result<[u16; 4], OtpError> {
    let rows = read_ecc_rows(otp, CHIP_ID_ROW, 4).await?;
    Ok(core::array::from_fn(|i| rows[i]))
}

/// Reads the board size OTP configures, by [`board_size`]'s rule. It reads
/// BOOT_FLAGS0's three copies raw and FLASH_DEVINFO with ECC, as the bootrom
/// does.
pub async fn read_board_size<O: LocalOtpAccess>(otp: &mut O) -> Result<OneromBoardSize, OtpError> {
    let [first, .., last] = BOOT_FLAGS0_ROWS;
    let rows = read_raw_rows(otp, first, last - first + 1).await?;
    let boot_flags0 = BOOT_FLAGS0_ROWS.map(|row| rows[usize::from(row - first)]);
    let flash_devinfo = read_ecc_rows(otp, OTP_FLASH_DEVINFO_ROW, 1).await?[0];
    Ok(board_size(boot_flags0, flash_devinfo))
}

/// Reads and parses the commissioning area.
///
/// It reads a page at a time and stops once the parser has found where the
/// next instance goes.
pub async fn read_commissioning<O: LocalOtpAccess>(
    otp: &mut O,
) -> Result<CommissioningArea, OtpError> {
    read_area(otp).await.map_err(|(_, error)| error)
}

/// [`read_commissioning`] returning a failed read's first row with its error.
pub(crate) async fn read_area<O: LocalOtpAccess>(
    otp: &mut O,
) -> Result<CommissioningArea, (u16, OtpError)> {
    let mut rows = Vec::with_capacity(usize::from(AREA_ROWS));
    let mut page = OTP_COMMISSIONING_AREA_FIRST_ROW;
    loop {
        let read = read_ecc_rows(otp, page, OTP_PAGE_ROWS)
            .await
            .map_err(|error| (page, error))?;
        rows.extend(read);
        page += OTP_PAGE_ROWS;
        let area = CommissioningArea::parse(&rows);
        // A parser that loses its place searches only the rows read so far for
        // the next instance. So the read stops early only where the parse
        // didn't find any issues.
        let found = area.next_instance_row().is_some() && area.issues().is_empty();
        if found || page > OTP_COMMISSIONING_AREA_LAST_ROW {
            return Ok(area);
        }
    }
}

/// Reads the LOCK1 words of `pages`.
pub(crate) async fn read_locks<O: LocalOtpAccess>(
    otp: &mut O,
    pages: RangeInclusive<u16>,
) -> Result<Vec<PageLock>, OtpError> {
    let (first, last) = (*pages.start(), *pages.end());
    let rows = read_raw_rows(
        otp,
        lock1_row(first),
        lock1_row(last) - lock1_row(first) + 1,
    )
    .await?;
    Ok(pages
        .map(|page| PageLock {
            page,
            raw: rows[usize::from(lock1_row(page) - lock1_row(first))],
        })
        .collect())
}

/// The raw rows One ROM uses in page 1.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BootRows {
    pub(crate) boot_flags0: [u32; 3],
    pub(crate) flash_devinfo: u32,
    pub(crate) flash_partition_slot_size: u32,
    pub(crate) usb_boot_flags: [u32; 3],
    pub(crate) usb_white_label_addr: u32,
}

impl BootRows {
    /// The first row [`read`](Self::read) reads.
    pub(crate) const FIRST_ROW: u16 = OTP_BOOT_FLAGS0_ROW;

    /// The page holding the rows.
    pub(crate) const PAGE: u16 = Self::FIRST_ROW / OTP_PAGE_ROWS;

    /// Reads the rows from BOOT_FLAGS0 to USB_WHITE_LABEL_ADDR raw.
    pub(crate) async fn read<O: LocalOtpAccess>(otp: &mut O) -> Result<Self, OtpError> {
        let rows = read_raw_rows(
            otp,
            Self::FIRST_ROW,
            OTP_ROW_USB_WHITE_LABEL_DATA - Self::FIRST_ROW + 1,
        )
        .await?;
        let row = |row: u16| rows[usize::from(row - Self::FIRST_ROW)];
        Ok(Self {
            boot_flags0: BOOT_FLAGS0_ROWS.map(row),
            flash_devinfo: row(OTP_FLASH_DEVINFO_ROW),
            flash_partition_slot_size: row(OTP_FLASH_PARTITION_SLOT_SIZE_ROW),
            usb_boot_flags: USB_BOOT_FLAGS_ROWS.map(row),
            usb_white_label_addr: row(OTP_ROW_USB_WHITE_LABEL_DATA),
        })
    }
}

/// Every OTP row One ROM uses.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OtpReport {
    /// CHIPID as the bootloader's USB serial number shows it.
    pub chip_id: String,
    /// The board size OTP configures. `None` where it configures neither M nor
    /// L, which serialises as `other`.
    #[serde(serialize_with = "size_or_other")]
    pub size: Option<BoardSize>,
    /// The raw BOOT_FLAGS0 and its two copies.
    pub boot_flags0: [u32; 3],
    /// FLASH_DEVINFO.
    pub flash_devinfo: EccRow,
    /// FLASH_PARTITION_SLOT_SIZE.
    pub flash_partition_slot_size: EccRow,
    /// The raw USB_BOOT_FLAGS and its two copies.
    pub usb_boot_flags: [u32; 3],
    /// USB_WHITE_LABEL_ADDR.
    pub usb_white_label_addr: EccRow,
    /// The white label in pages 59 and 60 as picotool's JSON. `None` where:
    /// - it's unwritten so it isn't decoded
    /// - pico-otp can't decode it
    pub white_label: Option<serde_json::Value>,
    /// pico-otp's reason it can't decode the white label. `None` where it can
    /// or the white label is unwritten.
    pub white_label_error: Option<String>,
    /// The warnings pico-otp raised while decoding the white label.
    pub white_label_warnings: Vec<String>,
    /// The commissioning area.
    pub commissioning: CommissioningArea,
    /// The general store. `None` where it hasn't been started.
    pub general_store: Option<GeneralStore>,
    /// The LOCK1 words of the commissioning area's pages and the white label's
    /// pages.
    pub locks: Vec<PageLock>,
}

/// Serialises `size`, or `other` where it's `None`.
fn size_or_other<S: serde::Serializer>(
    size: &Option<BoardSize>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match size {
        Some(size) => size.serialize(serializer),
        None => serializer.serialize_str("other"),
    }
}

/// An ECC row's raw value and the value it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EccRow {
    /// The row's 24 bits.
    pub raw: u32,
    /// The value the row holds. `None` where its raw value isn't an exact
    /// codeword.
    pub value: Option<u16>,
}

impl EccRow {
    fn new(raw: u32) -> Self {
        Self {
            raw,
            value: codeword(raw),
        }
    }
}

/// A page's LOCK1 word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PageLock {
    /// The page.
    pub page: u16,
    /// The raw LOCK1 word.
    pub raw: u32,
}

/// Reads every OTP row One ROM uses. It doesn't write anything.
///
/// It doesn't decode the white label where all of these hold:
/// - most USB_BOOT_FLAGS copies don't set WHITE_LABEL_ADDR_VALID
/// - USB_WHITE_LABEL_ADDR is unwritten
/// - pages 59 and 60 are unwritten
pub async fn read_report<O: LocalOtpAccess>(otp: &mut O) -> Result<OtpReport, OtpError> {
    let chip_id = read_chip_id(otp).await?;
    let boot = BootRows::read(otp).await?;
    let white_label_rows =
        read_ecc_rows(otp, OTP_USB_WHITE_LABEL_ROW, WHITE_LABEL_PAGES_ROWS).await?;
    let white_label_raw =
        read_raw_rows(otp, OTP_USB_WHITE_LABEL_ROW, WHITE_LABEL_PAGES_ROWS).await?;
    let commissioning = read_commissioning(otp).await?;
    let general_store = read_ecc_rows(otp, OTP_GENERAL_STORE_FIRST_ROW, GENERAL_STORE_ROWS).await?;
    // The board size comes from FLASH_DEVINFO read with ECC, as the bootrom
    // reads it.
    let flash_devinfo_ecc = read_ecc_rows(otp, OTP_FLASH_DEVINFO_ROW, 1).await?[0];
    let mut locks = read_locks(otp, AREA_FIRST_PAGE..=AREA_LAST_PAGE).await?;
    locks.extend(read_locks(otp, WHITE_LABEL_FIRST_PAGE..=WHITE_LABEL_LAST_PAGE).await?);

    let flash_devinfo = EccRow::new(boot.flash_devinfo);
    let usb_boot_flags = majority(boot.usb_boot_flags);
    let written = usb_boot_flags & USB_BOOT_FLAGS_WHITE_LABEL_ADDR_VALID != 0
        || !is_unwritten(boot.usb_white_label_addr)
        || !white_label_raw.iter().all(|&raw| is_unwritten(raw));
    let white_label = if written {
        DecodedWhiteLabel::new(usb_boot_flags, &white_label_rows)
    } else {
        DecodedWhiteLabel::NOT_SET
    };
    Ok(OtpReport {
        chip_id: format_chip_id(chip_id),
        size: configured_size(boot.boot_flags0, flash_devinfo_ecc),
        boot_flags0: boot.boot_flags0,
        flash_devinfo,
        flash_partition_slot_size: EccRow::new(boot.flash_partition_slot_size),
        usb_boot_flags: boot.usb_boot_flags,
        usb_white_label_addr: EccRow::new(boot.usb_white_label_addr),
        white_label: white_label.json,
        white_label_error: white_label.error,
        white_label_warnings: white_label.warnings,
        commissioning,
        general_store: GeneralStore::parse(&general_store),
        locks,
    })
}

/// The white label pico-otp decodes from pages 59 and 60. It holds pico-otp's
/// error and warnings too.
struct DecodedWhiteLabel {
    json: Option<serde_json::Value>,
    error: Option<String>,
    warnings: Vec<String>,
}

impl DecodedWhiteLabel {
    /// A white label that isn't decoded because it's unwritten.
    const NOT_SET: Self = Self {
        json: None,
        error: None,
        warnings: Vec::new(),
    };

    /// Decodes `rows` read with ECC from row `0xec0`. `usb_boot_flags` says
    /// which of their fields are valid.
    fn new(usb_boot_flags: u32, rows: &[u16]) -> Self {
        let decoded = OtpData::from_white_label_data(usb_boot_flags, rows, false)
            .and_then(|data| WhiteLabelStruct::try_from(&data));
        let white_label = match decoded {
            Ok(white_label) => white_label,
            Err(e) => {
                return Self {
                    json: None,
                    error: Some(e.to_string()),
                    warnings: Vec::new(),
                };
            }
        };
        let warnings = white_label.warnings().to_vec();
        match white_label.to_json() {
            Ok(json) => Self {
                json: Some(json),
                error: None,
                warnings,
            },
            Err(e) => Self {
                json: None,
                error: Some(e.to_string()),
                warnings,
            },
        }
    }
}

/// The board size OTP configures, by [`board_size`]'s rule. `None` where it's
/// neither M nor L. `flash_devinfo` is FLASH_DEVINFO read with ECC.
fn configured_size(boot_flags0: [u32; 3], flash_devinfo: u16) -> Option<BoardSize> {
    match board_size(boot_flags0, flash_devinfo) {
        OneromBoardSize::BoardSizeM => Some(BoardSize::M),
        OneromBoardSize::BoardSizeL => Some(BoardSize::L),
        OneromBoardSize::BoardSizeUnknown | OneromBoardSize::BoardSizeOther => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use onerom_config::hw::BOARDS;

    /// `C_ROWS` from pico-otp's `src/ecc.rs`. The datasheet's and picotool's C
    /// encoders return these rows for every single-bit value and a few others.
    const C_ROWS: [(u16, u32); 21] = [
        (0x0000, 0x000000),
        (0x0001, 0x230001),
        (0x0002, 0x250002),
        (0x0004, 0x260004),
        (0x0008, 0x070008),
        (0x0010, 0x290010),
        (0x0020, 0x2a0020),
        (0x0040, 0x0b0040),
        (0x0080, 0x2c0080),
        (0x0100, 0x0d0100),
        (0x0200, 0x0e0200),
        (0x0400, 0x2f0400),
        (0x0800, 0x310800),
        (0x1000, 0x321000),
        (0x2000, 0x132000),
        (0x4000, 0x344000),
        (0x8000, 0x158000),
        (0xffff, 0x1effff),
        (0x1234, 0x191234),
        (0x5678, 0x285678),
        (0xabcd, 0x11abcd),
    ];

    #[test]
    fn a_row_holds_its_value_in_either_form() {
        for (value, row) in C_ROWS {
            let inverted = !row & ROW_BITS;
            assert!(holds(row, value), "{value:#06x}");
            assert!(holds(inverted, value), "{value:#06x}");
            assert_eq!(codeword(row), Some(value));
            assert_eq!(codeword(inverted), Some(value));
        }
    }

    /// ECC corrects a single wrong bit on a read. A row holding one still
    /// doesn't hold its value.
    #[test]
    fn a_row_with_a_wrong_bit_isnt_a_codeword() {
        for (value, row) in C_ROWS {
            for form in [row, !row & ROW_BITS] {
                for bit in 0..24 {
                    let damaged = form ^ (1 << bit);
                    assert!(!holds(damaged, value), "{value:#06x} bit {bit}");
                    assert_eq!(codeword(damaged), None, "{value:#06x} bit {bit}");
                }
            }
        }
    }

    #[test]
    fn a_row_with_0_or_1_bits_set_is_unwritten() {
        assert!(is_unwritten(0));
        for bit in 0..24 {
            assert!(is_unwritten(1 << bit));
        }
        for (value, row) in C_ROWS {
            assert!(!is_unwritten(!row & ROW_BITS), "{value:#06x}");
            if value != 0 {
                assert!(!is_unwritten(row), "{value:#06x}");
            }
        }
    }

    #[test]
    fn majority_takes_each_bit_from_two_copies() {
        assert_eq!(majority([0b0011, 0b0101, 0b0110]), 0b0111);
        assert_eq!(majority([0b0001, 0, 0]), 0);
        assert_eq!(
            majority([LOCK1_READ_ONLY, LOCK1_READ_ONLY, 0]),
            LOCK1_READ_ONLY
        );
    }

    #[test]
    fn the_bootloader_lock_is_lock_bl_in_most_copies() {
        assert_eq!(bootloader_lock(0), 0);
        assert_eq!(bootloader_lock(LOCK1_READ_ONLY), LOCK_BL_READ_ONLY);
        assert_eq!(bootloader_lock(0x30_3030), 3);
        // Every lock but LOCK_BL.
        assert_eq!(bootloader_lock(0x0f_0f0f), 0);
        // LOCK_BL in one copy or split across two.
        assert_eq!(bootloader_lock(0x00_0010), 0);
        assert_eq!(bootloader_lock(0x00_2010), 0);
        assert_eq!(bootloader_lock(0x10_0010), 1);
    }

    /// Fire-40-a boards sized L before `set-size` existed hold `0x99af` in
    /// `FLASH_DEVINFO`. A script wrote it as the raw row `0x3a99af`.
    /// `hardware commission` refuses a row holding another value, so it must
    /// write the same one for those boards to commission.
    #[test]
    fn fire_40_a_flash_devinfo_matches_boards_sized_before_set_size() {
        assert_eq!(flash_devinfo(BoardSize::L, 47), Some(0x99af));
        assert_eq!(ecc_encode(0x99af), 0x3a_99af);
    }

    #[test]
    fn every_l_board_has_2mb_on_each_chip_select_and_d8h_erase() {
        for board in BOARDS {
            let Some(pin) = board.external_flash_cs_pin() else {
                continue;
            };
            let devinfo = flash_devinfo(BoardSize::L, pin).unwrap();
            assert_eq!((devinfo >> 12) & 0xf, 9, "{board}");
            assert_eq!((devinfo >> 8) & 0xf, 9, "{board}");
            assert_eq!(devinfo & 0x80, 0x80, "{board}");
            assert_eq!(devinfo & 0x40, 0, "{board}");
            assert_eq!(devinfo & 0x3f, u16::from(pin), "{board}");
        }
    }

    #[test]
    fn lock1_rows_follow_the_datasheet() {
        assert_eq!(lock1_row(0), 0xf81);
        assert_eq!(lock1_row(3), 0xf87);
        assert_eq!(lock1_row(18), 0xfa5);
        assert_eq!(lock1_row(63), 0xfff);
    }
}
