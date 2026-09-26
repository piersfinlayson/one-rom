// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
// MIT License

//! The One ROM OTP store and the bootloader's USB white label, as
//! `docs/wip/OTP.md` specifies them.
//!
//! - [`CommissioningArea`] and [`GeneralStore`] parse the store's two areas.
//! - [`NewCommissioningInstance`] builds the rows a commissioning instance
//!   writes and the message its signature covers.
//! - [`white_label`] builds the bootloader's USB white label.

use alloc::vec;
use alloc::vec::Vec;

use onerom_config::hw::Board;
use pico_otp::{WhiteLabelError, WhiteLabelStruct};

use crate::{
    DeviceMemoryView, Generations, OTP_COMMISSIONING_AREA_FIRST_ROW,
    OTP_COMMISSIONING_AREA_LAST_ROW, OTP_COMMISSIONING_SIG_LEN, OTP_COMMISSIONING_SIGNER_LEN,
    OTP_GENERAL_STORE_FIRST_ROW, OTP_GENERAL_STORE_TERMINATOR_ROW,
    OTP_GENERAL_STORE_TERMINATOR_ROW_COUNT, OTP_KEY_NONE, OTP_PAGE_ROWS, OTP_STORE_MAGIC,
    OTP_STORE_VERSION, OneromOtpEntry, OneromOtpKey, SerializeContext,
};

pub use pico_otp;

/// The start of the message a commissioning signature covers.
const SIGNATURE_PREFIX: &[u8] = b"onerom-commissioning-sig-v1";

/// The row after the commissioning area's last.
const COMMISSIONING_AREA_END: u16 = OTP_COMMISSIONING_AREA_LAST_ROW + 1;

/// The row after the general store's terminator.
const GENERAL_STORE_END: u16 =
    OTP_GENERAL_STORE_TERMINATOR_ROW + OTP_GENERAL_STORE_TERMINATOR_ROW_COUNT;

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// A commissioning instance for `commission` to write, before it has a
/// signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCommissioningInstance {
    first_row: u16,
    /// Each entry's rows, key row first, without the signature's entry.
    entries: Vec<Vec<u16>>,
}

/// An ECC write of `value` to OTP row `row`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowWrite {
    /// The OTP row.
    pub row: u16,
    /// The value, written with ECC.
    pub value: u16,
}

/// Why [`NewCommissioningInstance::new`] refused an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    /// The manufacturer's name is empty.
    EmptyManufacturer,
    /// The date isn't 8 ASCII digits.
    BadDate,
    /// The signer is 0, which no signing key has.
    BadSigner,
    /// The first row isn't a page boundary in the commissioning area.
    BadFirstRow,
    /// The instance runs past the end of the commissioning area.
    DoesNotFit,
}

impl NewCommissioningInstance {
    /// Builds `board`'s commissioning instance at `first_row`.
    ///
    /// `date` is the UTC commissioning date as `YYYYMMDD`, and `signer` is the
    /// ID of the key that signs the instance.
    pub fn new(
        board: Board,
        manufacturer: &str,
        date: &str,
        signer: u16,
        first_row: u16,
    ) -> Result<Self, BuildError> {
        if manufacturer.is_empty() {
            return Err(BuildError::EmptyManufacturer);
        }
        if date.len() != 8 || !date.bytes().all(|b| b.is_ascii_digit()) {
            return Err(BuildError::BadDate);
        }
        if signer == 0 {
            return Err(BuildError::BadSigner);
        }
        if !first_row.is_multiple_of(OTP_PAGE_ROWS)
            || !(OTP_COMMISSIONING_AREA_FIRST_ROW..COMMISSIONING_AREA_END).contains(&first_row)
        {
            return Err(BuildError::BadFirstRow);
        }

        let entries = [
            OneromOtpEntry::OtpKeyCommissioningBoard {
                name: board.name().into(),
            },
            OneromOtpEntry::OtpKeyCommissioningManufacturer {
                name: manufacturer.into(),
            },
            OneromOtpEntry::OtpKeyCommissioningDate { date: date.into() },
            OneromOtpEntry::OtpKeyCommissioningSigner { id: signer },
        ];
        // The magic and version rows, the entries, then the signature's entry.
        // Any value that fits the area has a length the 16-bit length row can
        // hold.
        let rows = 2
            + entries
                .iter()
                .map(|entry| entry_row_count(value_len(entry)))
                .sum::<usize>()
            + entry_row_count(OTP_COMMISSIONING_SIG_LEN);
        if usize::from(first_row) + rows > usize::from(COMMISSIONING_AREA_END) {
            return Err(BuildError::DoesNotFit);
        }

        Ok(Self {
            first_row,
            entries: entries.iter().map(entry_rows).collect(),
        })
    }

    /// The message the instance's signature covers, for the chip whose CHIPID
    /// is `chip_id`: rows `0x000`–`0x003` read with ECC, row `0x000` first.
    pub fn message(&self, chip_id: [u16; 4]) -> Vec<u8> {
        let entries = self.entries.iter().flatten().copied();
        signed_message(
            chip_id,
            [OTP_STORE_MAGIC, OTP_STORE_VERSION]
                .into_iter()
                .chain(entries),
        )
    }

    /// The rows to write with ECC, in order.
    ///
    /// The magic and version rows come first. Each entry follows as its length
    /// row, its value rows and then its key row, with `signature`'s entry last.
    pub fn writes(&self, signature: &[u8; 64]) -> Vec<RowWrite> {
        let signature = entry_rows(&OneromOtpEntry::OtpKeyCommissioningSig {
            signature: *signature,
        });
        let mut writes = vec![
            RowWrite {
                row: self.first_row,
                value: OTP_STORE_MAGIC,
            },
            RowWrite {
                row: self.first_row + 1,
                value: OTP_STORE_VERSION,
            },
        ];
        let mut row = self.first_row + 2;
        for entry in self.entries.iter().chain([&signature]) {
            // The key row goes last, so the parser treats an entry cut short
            // as deleted.
            for i in (1..entry.len()).chain([0]) {
                writes.push(RowWrite {
                    row: row + i as u16,
                    value: entry[i],
                });
            }
            row += entry.len() as u16;
        }
        writes
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// An entry in one of the store's lists.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum StoreEntry {
    /// An entry whose value parsed. An unknown key's entry is
    /// [`OneromOtpEntry::Unknown`].
    Entry {
        /// The entry's key row.
        row: u16,
        /// The entry.
        entry: OneromOtpEntry,
    },
    /// Key 0 with a length other than 0, from an entry deleted or cut short
    /// before its key row was written.
    Deleted {
        /// The entry's key row.
        row: u16,
        /// The entry's length.
        len: u16,
    },
    /// A known key whose value doesn't parse: a string that isn't UTF-8, or a
    /// fixed-size value at another length.
    Unreadable {
        /// The entry's key row.
        row: u16,
        /// The entry's key.
        key: u16,
        /// The entry's length.
        len: u16,
    },
}

impl StoreEntry {
    /// The entry's key, or `None` for a deleted entry.
    fn key(&self) -> Option<u16> {
        match self {
            Self::Entry { entry, .. } => Some(entry_key(entry)),
            Self::Deleted { .. } => None,
            Self::Unreadable { key, .. } => Some(*key),
        }
    }
}

/// A problem the parser found in an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum AreaIssue {
    /// The commissioning instance or general store starting at `row` has a
    /// version other than [`OTP_STORE_VERSION`]. The parser stops there.
    UnknownVersion {
        /// The instance's or store's first row.
        row: u16,
        /// The version row's value.
        version: u16,
    },
    /// The parser lost its place at `row`. Either the entry whose key row is
    /// `row` runs past the end of the area, or `row` is the page boundary
    /// after a commissioning instance and holds neither the magic value nor 0.
    LostPlace {
        /// The row where the parser lost its place.
        row: u16,
    },
}

/// The commissioning area, parsed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CommissioningArea {
    instances: Vec<CommissioningInstance>,
    issues: Vec<AreaIssue>,
    /// The current instance's index in `instances`.
    current: Option<usize>,
    next_instance_row: Option<u16>,
}

impl CommissioningArea {
    /// Parses the commissioning area. `rows` holds its rows read with ECC,
    /// from row `0x0c0`. Rows past the area's end are ignored.
    pub fn parse(rows: &[u16]) -> Self {
        let area = Area::new(
            rows,
            OTP_COMMISSIONING_AREA_FIRST_ROW,
            COMMISSIONING_AREA_END,
        );
        let mut instances = Vec::new();
        let mut issues = Vec::new();
        let (current, next_instance_row) = match walk(&area, &mut instances, &mut issues) {
            Walk::Ended(next_instance_row) => {
                // The last complete instance is current, where it's valid.
                let current = instances
                    .iter()
                    .rposition(CommissioningInstance::is_complete)
                    .filter(|&i| instances[i].is_valid());
                (current, next_instance_row)
            }
            Walk::Abandoned => (None, None),
        };
        Self {
            instances,
            issues,
            current,
            next_instance_row,
        }
    }

    /// Every instance in row order, complete or not.
    pub fn instances(&self) -> &[CommissioningInstance] {
        &self.instances
    }

    /// The last complete instance, where it's valid. `None` means the board
    /// doesn't have valid commissioning data.
    pub fn current(&self) -> Option<&CommissioningInstance> {
        self.current.map(|i| &self.instances[i])
    }

    /// The page boundary after the last instance, where the next one goes.
    /// `None` where the area is full, the parser lost its place, or it found
    /// an unknown version.
    pub fn next_instance_row(&self) -> Option<u16> {
        self.next_instance_row
    }

    /// The problems the parser found, in row order.
    pub fn issues(&self) -> &[AreaIssue] {
        &self.issues
    }
}

/// One commissioning instance.
///
/// Where a key appears more than once, its value comes from the last entry
/// with that key. An accessor returns `None` where that entry is missing or
/// unreadable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CommissioningInstance {
    first_row: u16,
    entries: Vec<StoreEntry>,
    /// The rows the signature covers, from the magic row to the row before
    /// the signature's key row. `None` where `COMMISSIONING_SIG` doesn't end
    /// the instance.
    #[serde(skip)]
    signed_rows: Option<Vec<u16>>,
}

impl CommissioningInstance {
    /// The instance's magic row.
    pub fn first_row(&self) -> u16 {
        self.first_row
    }

    /// The instance's entries in row order.
    pub fn entries(&self) -> &[StoreEntry] {
        &self.entries
    }

    /// `COMMISSIONING_BOARD`'s value.
    pub fn board(&self) -> Option<&str> {
        if let OneromOtpEntry::OtpKeyCommissioningBoard { name } =
            self.used(OneromOtpKey::OtpKeyCommissioningBoard)?
        {
            Some(name)
        } else {
            None
        }
    }

    /// `COMMISSIONING_MANUFACTURER`'s value.
    pub fn manufacturer(&self) -> Option<&str> {
        if let OneromOtpEntry::OtpKeyCommissioningManufacturer { name } =
            self.used(OneromOtpKey::OtpKeyCommissioningManufacturer)?
        {
            Some(name)
        } else {
            None
        }
    }

    /// `COMMISSIONING_DATE`'s value.
    pub fn date(&self) -> Option<&str> {
        if let OneromOtpEntry::OtpKeyCommissioningDate { date } =
            self.used(OneromOtpKey::OtpKeyCommissioningDate)?
        {
            Some(date)
        } else {
            None
        }
    }

    /// `COMMISSIONING_SIGNER`'s value.
    pub fn signer(&self) -> Option<u16> {
        if let OneromOtpEntry::OtpKeyCommissioningSigner { id } =
            self.used(OneromOtpKey::OtpKeyCommissioningSigner)?
        {
            Some(*id)
        } else {
            None
        }
    }

    /// `COMMISSIONING_SIG`'s value.
    pub fn signature(&self) -> Option<&[u8; 64]> {
        if let OneromOtpEntry::OtpKeyCommissioningSig { signature } =
            self.used(OneromOtpKey::OtpKeyCommissioningSig)?
        {
            Some(signature)
        } else {
            None
        }
    }

    /// Whether the instance has a value for each of the five commissioning
    /// keys.
    pub fn is_valid(&self) -> bool {
        self.board().is_some()
            && self.manufacturer().is_some()
            && self.date().is_some()
            && self.signer().is_some()
            && self.signature().is_some()
    }

    /// The message the instance's signature covers, for the chip whose CHIPID
    /// is `chip_id`: rows `0x000`–`0x003` read with ECC, row `0x000` first.
    /// `None` where `COMMISSIONING_SIG` doesn't end the instance.
    pub fn message(&self, chip_id: [u16; 4]) -> Option<Vec<u8>> {
        self.signed_rows
            .as_ref()
            .map(|rows| signed_message(chip_id, rows.iter().copied()))
    }

    /// Whether `COMMISSIONING_SIG` ends the instance.
    fn is_complete(&self) -> bool {
        self.signed_rows.is_some()
    }

    /// The last entry with `key`, where its value parsed.
    fn used(&self, key: OneromOtpKey) -> Option<&OneromOtpEntry> {
        let key = key as u16;
        match self
            .entries
            .iter()
            .rev()
            .find(|entry| entry.key() == Some(key))?
        {
            StoreEntry::Entry { entry, .. } => Some(entry),
            StoreEntry::Deleted { .. } | StoreEntry::Unreadable { .. } => None,
        }
    }
}

/// The general store, parsed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GeneralStore {
    entries: Vec<StoreEntry>,
    issues: Vec<AreaIssue>,
}

impl GeneralStore {
    /// Parses the general store. `rows` holds its rows and its terminator's,
    /// read with ECC from row `0x4c0`. Rows past the terminator are ignored.
    ///
    /// `None` where the store hasn't been started: row `0x4c0` doesn't hold
    /// the magic value, or the version row is unwritten.
    pub fn parse(rows: &[u16]) -> Option<Self> {
        let area = Area::new(rows, OTP_GENERAL_STORE_FIRST_ROW, GENERAL_STORE_END);
        let first = area.first;
        if area.end < first + 2 || area.row(first) != OTP_STORE_MAGIC {
            return None;
        }
        let (entries, issues) = match area.row(first + 1) {
            0 => return None,
            OTP_STORE_VERSION => match parse_entries(&area, first + 2, false) {
                (entries, ListEnd::Lost { row }) => (entries, vec![AreaIssue::LostPlace { row }]),
                (entries, ListEnd::Terminated { .. } | ListEnd::Signature { .. }) => {
                    (entries, Vec::new())
                }
            },
            version => (
                Vec::new(),
                vec![AreaIssue::UnknownVersion {
                    row: first,
                    version,
                }],
            ),
        };
        Some(Self { entries, issues })
    }

    /// The store's entries in row order.
    pub fn entries(&self) -> &[StoreEntry] {
        &self.entries
    }

    /// The problems the parser found.
    pub fn issues(&self) -> &[AreaIssue] {
        &self.issues
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// An area's rows, read with ECC.
struct Area<'a> {
    rows: &'a [u16],
    /// The area's first row.
    first: u16,
    /// The row after the area's last.
    end: u16,
}

impl<'a> Area<'a> {
    /// The rows from `first` to the row before `end`, as far as `rows` goes.
    fn new(rows: &'a [u16], first: u16, end: u16) -> Self {
        let rows = &rows[..rows.len().min(usize::from(end - first))];
        Self {
            rows,
            first,
            end: first + rows.len() as u16,
        }
    }

    fn row(&self, row: u16) -> u16 {
        self.rows[usize::from(row - self.first)]
    }

    /// Rows `from` to the row before `to`.
    fn rows(&self, from: u16, to: u16) -> &'a [u16] {
        &self.rows[usize::from(from - self.first)..usize::from(to - self.first)]
    }

    /// The first page boundary after `row` holding the magic value.
    fn instance_after(&self, row: u16) -> Option<u16> {
        (page_after(row)..self.end)
            .step_by(usize::from(OTP_PAGE_ROWS))
            .find(|&boundary| self.row(boundary) == OTP_STORE_MAGIC)
    }
}

/// Where the parser left the commissioning area.
enum Walk {
    /// At the unwritten page boundary `Some(row)`, or at the area's end with
    /// `None`.
    Ended(Option<u16>),
    /// At an unknown version, or where it lost its place and found no
    /// instance after. The board then doesn't have valid commissioning data.
    Abandoned,
}

/// Parses each commissioning instance in `area` into `instances`.
fn walk(
    area: &Area,
    instances: &mut Vec<CommissioningInstance>,
    issues: &mut Vec<AreaIssue>,
) -> Walk {
    let mut boundary = area.first;
    while boundary < area.end {
        let lost = match area.row(boundary) {
            0 => return Walk::Ended(Some(boundary)),
            OTP_STORE_MAGIC => match parse_instance(area, boundary) {
                Instance::Ended { instance, last_row } => {
                    instances.push(instance);
                    boundary = page_after(last_row);
                    continue;
                }
                Instance::UnknownVersion(version) => {
                    issues.push(AreaIssue::UnknownVersion {
                        row: boundary,
                        version,
                    });
                    return Walk::Abandoned;
                }
                Instance::Lost { instance, row } => {
                    instances.push(instance);
                    row
                }
            },
            _ => boundary,
        };
        issues.push(AreaIssue::LostPlace { row: lost });
        match area.instance_after(lost) {
            Some(next) => boundary = next,
            None => return Walk::Abandoned,
        }
    }
    Walk::Ended(None)
}

/// Where a commissioning instance ended.
enum Instance {
    /// At `last_row`, complete or not.
    Ended {
        instance: CommissioningInstance,
        last_row: u16,
    },
    /// At its version row, which holds an unknown version.
    UnknownVersion(u16),
    /// At the entry whose key row is `row`, which runs past the end of the
    /// area.
    Lost {
        instance: CommissioningInstance,
        row: u16,
    },
}

/// Parses the commissioning instance whose magic row is `first_row`.
fn parse_instance(area: &Area, first_row: u16) -> Instance {
    let instance =
        |entries: Vec<StoreEntry>, signed_rows: Option<Vec<u16>>| CommissioningInstance {
            first_row,
            entries,
            signed_rows,
        };
    let version_row = first_row + 1;
    if version_row == area.end {
        return Instance::Lost {
            instance: instance(Vec::new(), None),
            row: first_row,
        };
    }
    match area.row(version_row) {
        0 => {
            return Instance::Ended {
                instance: instance(Vec::new(), None),
                last_row: version_row,
            };
        }
        OTP_STORE_VERSION => {}
        version => return Instance::UnknownVersion(version),
    }
    match parse_entries(area, first_row + 2, true) {
        (entries, ListEnd::Terminated { last_row }) => Instance::Ended {
            instance: instance(entries, None),
            last_row,
        },
        (entries, ListEnd::Signature { key_row, last_row }) => Instance::Ended {
            instance: instance(entries, Some(area.rows(first_row, key_row).to_vec())),
            last_row,
        },
        (entries, ListEnd::Lost { row }) => Instance::Lost {
            instance: instance(entries, None),
            row,
        },
    }
}

/// Where a list of entries ended.
enum ListEnd {
    /// At key 0 with length 0, or at the end of the area. `last_row` is the
    /// list's last row.
    Terminated { last_row: u16 },
    /// At `COMMISSIONING_SIG`, whose key row is `key_row` and last row
    /// `last_row`.
    Signature { key_row: u16, last_row: u16 },
    /// At the entry whose key row is `row`, which runs past the end of the
    /// area.
    Lost { row: u16 },
}

/// Parses the list of entries starting at `row`. `COMMISSIONING_SIG` ends the
/// list where `signature_ends` is set.
fn parse_entries(area: &Area, mut row: u16, signature_ends: bool) -> (Vec<StoreEntry>, ListEnd) {
    let mut entries = Vec::new();
    loop {
        // An instance interrupted in the area's last page can fill the area
        // and leave no room for key 0 and length 0.
        if row == area.end {
            return (entries, ListEnd::Terminated { last_row: row - 1 });
        }
        if row + 1 == area.end {
            return (entries, ListEnd::Lost { row });
        }
        let (key, len) = (area.row(row), area.row(row + 1));
        if key == OTP_KEY_NONE && len == 0 {
            return (entries, ListEnd::Terminated { last_row: row + 1 });
        }
        let end = usize::from(row) + entry_row_count(usize::from(len));
        if end > usize::from(area.end) {
            return (entries, ListEnd::Lost { row });
        }
        let end = end as u16;
        entries.push(parse_entry(area.rows(row, end), row));
        if signature_ends && key == OneromOtpKey::OtpKeyCommissioningSig as u16 {
            return (
                entries,
                ListEnd::Signature {
                    key_row: row,
                    last_row: end - 1,
                },
            );
        }
        row = end;
    }
}

/// Parses the entry at `row` from its rows, key row first.
fn parse_entry(rows: &[u16], row: u16) -> StoreEntry {
    let (key, len) = (rows[0], rows[1]);
    if key == OTP_KEY_NONE {
        return StoreEntry::Deleted { row, len };
    }
    let bytes: Vec<u8> = rows.iter().flat_map(|r| r.to_le_bytes()).collect();
    let view = DeviceMemoryView::new(&bytes, 0);
    match OneromOtpEntry::parse(&view, 0, Generations::UNKNOWN) {
        // A variant never grows, so the parser treats a fixed-size value at
        // another length as unreadable.
        Ok(entry) if value_len(&entry) == usize::from(len) => StoreEntry::Entry { row, entry },
        Ok(_) | Err(_) => StoreEntry::Unreadable { row, key, len },
    }
}

/// The first page boundary after `row`.
fn page_after(row: u16) -> u16 {
    (row / OTP_PAGE_ROWS + 1) * OTP_PAGE_ROWS
}

// ---------------------------------------------------------------------------
// Entries
// ---------------------------------------------------------------------------

/// The rows an entry takes when its value is `len` bytes: a key row, a length
/// row, then two bytes of value per row.
fn entry_row_count(len: usize) -> usize {
    2 + len.div_ceil(2)
}

/// `entry`'s rows as the store holds them, key row first.
fn entry_rows(entry: &OneromOtpEntry) -> Vec<u16> {
    let mut bytes = vec![0; 2 * entry_row_count(value_len(entry))];
    let mut ctx = SerializeContext::new(0, 0, &mut bytes);
    // The context fills the buffer with 0xFF. An unwritten OTP row reads 0,
    // and the spare byte after an odd-length value must be 0.
    ctx.buf.fill(0);
    entry.write(&mut ctx, 0);
    let (rows, _) = bytes.as_chunks::<2>();
    rows.iter().map(|&row| u16::from_le_bytes(row)).collect()
}

/// The length of `entry`'s value in bytes.
fn value_len(entry: &OneromOtpEntry) -> usize {
    match entry {
        OneromOtpEntry::OtpKeyCommissioningBoard { name }
        | OneromOtpEntry::OtpKeyCommissioningManufacturer { name } => name.len(),
        OneromOtpEntry::OtpKeyCommissioningDate { date } => date.len(),
        OneromOtpEntry::OtpKeyCommissioningSig { .. } => OTP_COMMISSIONING_SIG_LEN,
        OneromOtpEntry::OtpKeyCommissioningSigner { .. } => OTP_COMMISSIONING_SIGNER_LEN,
        OneromOtpEntry::Unknown { params, .. } => params.len(),
    }
}

/// `entry`'s key.
fn entry_key(entry: &OneromOtpEntry) -> u16 {
    let key = match entry {
        OneromOtpEntry::OtpKeyCommissioningBoard { .. } => OneromOtpKey::OtpKeyCommissioningBoard,
        OneromOtpEntry::OtpKeyCommissioningSig { .. } => OneromOtpKey::OtpKeyCommissioningSig,
        OneromOtpEntry::OtpKeyCommissioningManufacturer { .. } => {
            OneromOtpKey::OtpKeyCommissioningManufacturer
        }
        OneromOtpEntry::OtpKeyCommissioningDate { .. } => OneromOtpKey::OtpKeyCommissioningDate,
        OneromOtpEntry::OtpKeyCommissioningSigner { .. } => OneromOtpKey::OtpKeyCommissioningSigner,
        OneromOtpEntry::Unknown { key, .. } => return *key as u16,
    };
    key as u16
}

/// [`SIGNATURE_PREFIX`], then CHIPID, then `rows`, each row low byte first.
fn signed_message(chip_id: [u16; 4], rows: impl Iterator<Item = u16>) -> Vec<u8> {
    let mut message = SIGNATURE_PREFIX.to_vec();
    message.extend(chip_id.into_iter().chain(rows).flat_map(u16::to_le_bytes));
    message
}

// ---------------------------------------------------------------------------
// USB white label
// ---------------------------------------------------------------------------

/// The bootloader's USB white label for `board`: One ROM's strings from
/// OTP.md's table, with the board's name as the INFO_UF2.TXT board ID.
pub fn white_label(board: Board) -> WhiteLabelStruct {
    let mut white_label = WhiteLabelStruct::default();
    let set = |wl: &mut WhiteLabelStruct| -> Result<(), WhiteLabelError> {
        wl.set_manufacturer("piers.rocks")?;
        wl.set_product("One ROM Bootloader")?;
        wl.set_volume_label("ONEROM")?;
        wl.set_redirect_url("https://onerom.org")?;
        wl.set_redirect_name("onerom.org")?;
        wl.set_uf2_model("One ROM")?;
        wl.set_uf2_board_id(board.name())
    };
    // pico-otp refuses only a string longer than 127 characters.
    set(&mut white_label).expect("a white label string is longer than 127 characters");
    white_label
}
